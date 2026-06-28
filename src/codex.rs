use std::{
    collections::HashMap,
    path::Path,
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{ChildStdin, Command},
    sync::{Mutex, mpsc, oneshot},
};

#[derive(Debug, Clone)]
pub struct ThreadInfo {
    pub id: String,
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
}

#[derive(Debug, Clone)]
pub struct WorkspaceSession {
    pub thread: ThreadInfo,
    pub entries: Vec<HistoryEntry>,
}

#[derive(Debug, Clone)]
pub struct HistoryEntry {
    pub kind: HistoryEntryKind,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryEntryKind {
    User,
    Assistant,
    Event,
}

#[derive(Debug, Clone)]
pub enum ThreadItem {
    AgentMessage { id: String, text: String },
    UserMessage,
    Other,
}

#[derive(Debug, Clone)]
pub enum ServerEvent {
    ThreadStatusChanged {
        thread_id: String,
        active: bool,
    },
    ItemStarted {
        thread_id: String,
        item: ThreadItem,
    },
    ItemCompleted {
        thread_id: String,
        item: ThreadItem,
    },
    AgentMessageDelta {
        thread_id: String,
        item_id: String,
        delta: String,
    },
    TurnCompleted {
        thread_id: String,
    },
    Warning(String),
    Error(String),
    TransportError(String),
}

#[derive(Debug, Clone)]
pub struct ModelInfo {
    pub id: String,
    pub model: String,
    pub display_name: String,
    pub is_default: bool,
}

#[derive(Clone)]
pub struct CodexAppServer {
    stdin: Arc<Mutex<ChildStdin>>,
    next_id: Arc<AtomicU64>,
    pending: Arc<Mutex<HashMap<u64, oneshot::Sender<Result<Value>>>>>,
}

impl CodexAppServer {
    pub async fn spawn(event_tx: mpsc::UnboundedSender<ServerEvent>) -> Result<Self> {
        let mut command = Command::new("codex");
        command
            .args(["app-server", "--stdio"])
            .kill_on_drop(true)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());

        let mut child = command
            .spawn()
            .context("failed to start codex app-server")?;
        let stdin = child.stdin.take().context("missing app-server stdin")?;
        let stdout = child.stdout.take().context("missing app-server stdout")?;

        let pending = Arc::new(Mutex::new(
            HashMap::<u64, oneshot::Sender<Result<Value>>>::new(),
        ));
        let pending_reader = Arc::clone(&pending);
        tokio::spawn(async move {
            let mut reader = BufReader::new(stdout).lines();
            loop {
                match reader.next_line().await {
                    Ok(Some(line)) => {
                        if line.trim().is_empty() {
                            continue;
                        }
                        match serde_json::from_str::<Value>(&line) {
                            Ok(value) => {
                                if let Some(id) = value.get("id").and_then(Value::as_u64) {
                                    let sender = pending_reader.lock().await.remove(&id);
                                    if let Some(sender) = sender {
                                        let result = match value.get("error") {
                                            Some(error) => {
                                                Err(anyhow!(format_jsonrpc_error(error)))
                                            }
                                            None => Ok(value
                                                .get("result")
                                                .cloned()
                                                .unwrap_or(Value::Null)),
                                        };
                                        let _ = sender.send(result);
                                    }
                                } else if let Some(event) = parse_server_event(value) {
                                    let _ = event_tx.send(event);
                                }
                            }
                            Err(error) => {
                                let _ =
                                    event_tx.send(ServerEvent::TransportError(error.to_string()));
                            }
                        }
                    }
                    Ok(None) => {
                        let _ = event_tx.send(ServerEvent::TransportError(
                            "Codex app-server closed the connection.".to_string(),
                        ));
                        break;
                    }
                    Err(error) => {
                        let _ = event_tx.send(ServerEvent::TransportError(error.to_string()));
                        break;
                    }
                }
            }
        });

        let client = Self {
            stdin: Arc::new(Mutex::new(stdin)),
            next_id: Arc::new(AtomicU64::new(1)),
            pending,
        };

        let _: InitializeResponse = client
            .request(
                "initialize",
                InitializeParams {
                    client_info: ClientInfo {
                        name: "cmdex".to_string(),
                        version: env!("CARGO_PKG_VERSION").to_string(),
                    },
                    capabilities: None,
                },
            )
            .await?;
        client.notify("initialized", None::<Value>).await?;

        Ok(client)
    }

    pub async fn start_thread(&self, cwd: &Path, model: Option<&str>) -> Result<ThreadInfo> {
        let response: ThreadStartResponse = self
            .request(
                "thread/start",
                ThreadStartParams {
                    cwd: Some(cwd.to_string_lossy().to_string()),
                    ephemeral: Some(false),
                    service_name: Some("cmdex".to_string()),
                    model: model.map(ToString::to_string),
                },
            )
            .await?;

        Ok(ThreadInfo {
            id: response.thread.id,
            model: Some(response.model),
            reasoning_effort: response.reasoning_effort,
        })
    }

    pub async fn start_turn(
        &self,
        thread_id: &str,
        text: &str,
        model: Option<&str>,
        effort: Option<&str>,
    ) -> Result<()> {
        let _: TurnStartResponse = self
            .request(
                "turn/start",
                TurnStartParams {
                    thread_id: thread_id.to_string(),
                    input: vec![UserInput::text(text)],
                    model: model.map(ToString::to_string),
                    effort: effort.map(ToString::to_string),
                },
            )
            .await?;

        Ok(())
    }

    pub async fn resume_thread(&self, thread_id: &str, model: Option<&str>) -> Result<ThreadInfo> {
        let response: ThreadResumeResponse = self
            .request(
                "thread/resume",
                ThreadResumeParams {
                    thread_id: thread_id.to_string(),
                    model: model.map(ToString::to_string),
                },
            )
            .await?;

        Ok(ThreadInfo {
            id: response.thread.id,
            model: Some(response.model),
            reasoning_effort: response.reasoning_effort,
        })
    }

    pub async fn load_latest_workspace_session(
        &self,
        cwd: &Path,
    ) -> Result<Option<WorkspaceSession>> {
        let response: ThreadListResponse = self
            .request(
                "thread/list",
                ThreadListParams {
                    limit: Some(1),
                    sort_key: Some("updated_at".to_string()),
                    sort_direction: Some("desc".to_string()),
                    cwd: Some(cwd.to_string_lossy().to_string()),
                },
            )
            .await?;

        let Some(thread) = response.data.into_iter().next() else {
            return Ok(None);
        };

        let response: ThreadReadResponse = self
            .request(
                "thread/read",
                ThreadReadParams {
                    thread_id: thread.id.clone(),
                    include_turns: Some(true),
                },
            )
            .await?;

        Ok(Some(WorkspaceSession {
            thread: ThreadInfo {
                id: response.thread.id,
                model: None,
                reasoning_effort: None,
            },
            entries: history_entries_from_turns(&response.thread.turns),
        }))
    }

    pub async fn list_models(&self) -> Result<Vec<ModelInfo>> {
        let mut models = Vec::new();
        let mut cursor = None;

        loop {
            let response: ModelListResponse = self
                .request(
                    "model/list",
                    ModelListParams {
                        cursor: cursor.clone(),
                        include_hidden: Some(false),
                        limit: Some(100),
                    },
                )
                .await?;

            models.extend(response.data.into_iter().map(|model| ModelInfo {
                id: model.id,
                model: model.model,
                display_name: model.display_name,
                is_default: model.is_default,
            }));

            let Some(next_cursor) = response.next_cursor else {
                break;
            };
            cursor = Some(next_cursor);
        }

        Ok(models)
    }

    async fn request<T>(&self, method: &str, params: impl Serialize) -> Result<T>
    where
        T: for<'de> Deserialize<'de>,
    {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (sender, receiver) = oneshot::channel();
        self.pending.lock().await.insert(id, sender);

        let payload = JsonRpcRequest {
            jsonrpc: "2.0",
            id,
            method,
            params,
        };
        self.write_payload(&payload).await?;

        let value = receiver
            .await
            .context("app-server dropped the response channel")??;
        serde_json::from_value(value).context("failed to decode app-server response")
    }

    async fn notify<T>(&self, method: &str, params: Option<T>) -> Result<()>
    where
        T: Serialize,
    {
        let payload = JsonRpcNotification {
            jsonrpc: "2.0",
            method,
            params,
        };
        self.write_payload(&payload).await
    }

    async fn write_payload(&self, payload: &impl Serialize) -> Result<()> {
        let mut stdin = self.stdin.lock().await;
        let line = serde_json::to_string(payload).context("failed to serialize request")?;
        stdin
            .write_all(line.as_bytes())
            .await
            .context("failed to write request")?;
        stdin
            .write_all(b"\n")
            .await
            .context("failed to terminate request line")?;
        stdin.flush().await.context("failed to flush request")?;
        Ok(())
    }
}

#[derive(Debug, Serialize)]
struct JsonRpcRequest<'a, T> {
    jsonrpc: &'static str,
    id: u64,
    method: &'a str,
    params: T,
}

#[derive(Debug, Serialize)]
struct JsonRpcNotification<'a, T> {
    jsonrpc: &'static str,
    method: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    params: Option<T>,
}

#[derive(Debug, Serialize)]
struct InitializeParams {
    #[serde(rename = "clientInfo")]
    client_info: ClientInfo,
    capabilities: Option<Value>,
}

#[derive(Debug, Serialize)]
struct ClientInfo {
    name: String,
    version: String,
}

#[derive(Debug, Deserialize)]
struct InitializeResponse {
    #[allow(dead_code)]
    #[serde(rename = "userAgent")]
    user_agent: String,
}

#[derive(Debug, Serialize)]
struct ThreadStartParams {
    #[serde(rename = "cwd")]
    cwd: Option<String>,
    ephemeral: Option<bool>,
    #[serde(rename = "serviceName")]
    service_name: Option<String>,
    model: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ThreadStartResponse {
    thread: ThreadResponse,
    model: String,
    #[serde(rename = "reasoningEffort")]
    reasoning_effort: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ThreadResponse {
    id: String,
}

#[derive(Debug, Serialize)]
struct TurnStartParams {
    #[serde(rename = "threadId")]
    thread_id: String,
    input: Vec<UserInput>,
    model: Option<String>,
    effort: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TurnStartResponse {
    #[allow(dead_code)]
    turn: TurnResponse,
}

#[derive(Debug, Serialize)]
struct ThreadResumeParams {
    #[serde(rename = "threadId")]
    thread_id: String,
    model: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ThreadResumeResponse {
    thread: ThreadResponse,
    model: String,
    #[serde(rename = "reasoningEffort")]
    reasoning_effort: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TurnResponse {
    #[allow(dead_code)]
    id: String,
}

#[derive(Debug, Serialize)]
struct ThreadListParams {
    limit: Option<u64>,
    #[serde(rename = "sortKey")]
    sort_key: Option<String>,
    #[serde(rename = "sortDirection")]
    sort_direction: Option<String>,
    cwd: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ThreadListResponse {
    data: Vec<ThreadListItem>,
}

#[derive(Debug, Deserialize)]
struct ThreadListItem {
    id: String,
}

#[derive(Debug, Serialize)]
struct ThreadReadParams {
    #[serde(rename = "threadId")]
    thread_id: String,
    #[serde(rename = "includeTurns")]
    include_turns: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct ThreadReadResponse {
    thread: ThreadReadThread,
}

#[derive(Debug, Deserialize)]
struct ThreadReadThread {
    id: String,
    #[serde(default)]
    turns: Vec<ThreadTurn>,
}

#[derive(Debug, Serialize)]
struct ModelListParams {
    cursor: Option<String>,
    #[serde(rename = "includeHidden")]
    include_hidden: Option<bool>,
    limit: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct ModelListResponse {
    data: Vec<ModelListItem>,
    #[serde(rename = "nextCursor")]
    next_cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ModelListItem {
    id: String,
    model: String,
    #[serde(rename = "displayName")]
    display_name: String,
    #[serde(rename = "isDefault")]
    is_default: bool,
}

#[derive(Debug, Deserialize)]
struct ThreadTurn {
    #[serde(default)]
    items: Vec<Value>,
}

#[derive(Debug, Serialize)]
struct UserInput {
    #[serde(rename = "type")]
    kind: &'static str,
    text: String,
    text_elements: Vec<Value>,
}

impl UserInput {
    fn text(text: &str) -> Self {
        Self {
            kind: "text",
            text: text.to_string(),
            text_elements: Vec::new(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct ThreadStatusChangedParams {
    #[serde(rename = "threadId")]
    thread_id: String,
    status: ThreadStatus,
}

#[derive(Debug, Deserialize)]
struct ThreadStatus {
    #[serde(rename = "type")]
    kind: String,
}

#[derive(Debug, Deserialize)]
struct ItemNotificationParams {
    #[serde(rename = "threadId")]
    thread_id: String,
    item: RawThreadItem,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum RawThreadItem {
    #[serde(rename = "agentMessage")]
    AgentMessage { id: String, text: String },
    #[serde(rename = "userMessage")]
    UserMessage,
    #[serde(other)]
    Other,
}

#[derive(Debug, Deserialize)]
struct AgentMessageDeltaParams {
    #[serde(rename = "threadId")]
    thread_id: String,
    #[serde(rename = "itemId")]
    item_id: String,
    delta: String,
}

#[derive(Debug, Deserialize)]
struct TurnCompletedParams {
    #[serde(rename = "threadId")]
    thread_id: String,
}

fn parse_server_event(value: Value) -> Option<ServerEvent> {
    let method = value.get("method")?.as_str()?;
    let params = value.get("params").cloned().unwrap_or(Value::Null);

    match method {
        "thread/status/changed" => {
            let params: ThreadStatusChangedParams = serde_json::from_value(params).ok()?;
            Some(ServerEvent::ThreadStatusChanged {
                thread_id: params.thread_id,
                active: params.status.kind == "active",
            })
        }
        "item/started" => {
            let params: ItemNotificationParams = serde_json::from_value(params).ok()?;
            Some(ServerEvent::ItemStarted {
                thread_id: params.thread_id,
                item: map_thread_item(params.item),
            })
        }
        "item/completed" => {
            let params: ItemNotificationParams = serde_json::from_value(params).ok()?;
            Some(ServerEvent::ItemCompleted {
                thread_id: params.thread_id,
                item: map_thread_item(params.item),
            })
        }
        "item/agentMessage/delta" => {
            let params: AgentMessageDeltaParams = serde_json::from_value(params).ok()?;
            Some(ServerEvent::AgentMessageDelta {
                thread_id: params.thread_id,
                item_id: params.item_id,
                delta: params.delta,
            })
        }
        "turn/completed" => {
            let params: TurnCompletedParams = serde_json::from_value(params).ok()?;
            Some(ServerEvent::TurnCompleted {
                thread_id: params.thread_id,
            })
        }
        "warning" => Some(ServerEvent::Warning(extract_message(&params))),
        "error" => Some(ServerEvent::Error(extract_message(&params))),
        _ => None,
    }
}

fn map_thread_item(item: RawThreadItem) -> ThreadItem {
    match item {
        RawThreadItem::AgentMessage { id, text } => ThreadItem::AgentMessage { id, text },
        RawThreadItem::UserMessage { .. } => ThreadItem::UserMessage,
        RawThreadItem::Other => ThreadItem::Other,
    }
}

fn extract_message(params: &Value) -> String {
    params
        .get("message")
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .unwrap_or_else(|| params.to_string())
}

fn format_jsonrpc_error(error: &Value) -> String {
    let code = error
        .get("code")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("unknown error");
    format!("{message} (code {code})")
}

fn history_entries_from_turns(turns: &[ThreadTurn]) -> Vec<HistoryEntry> {
    let mut entries = Vec::new();

    for turn in turns {
        for item in &turn.items {
            if let Some(entry) = history_entry_from_item(item) {
                entries.push(entry);
            }
        }
    }

    entries
}

fn history_entry_from_item(item: &Value) -> Option<HistoryEntry> {
    let item_type = item.get("type")?.as_str()?;

    match item_type {
        "userMessage" => {
            let text = user_message_text(item);
            if text.trim().is_empty() {
                None
            } else {
                Some(HistoryEntry {
                    kind: HistoryEntryKind::User,
                    text,
                })
            }
        }
        "agentMessage" => {
            let text = item.get("text")?.as_str()?.to_string();
            if text.trim().is_empty() {
                None
            } else {
                Some(HistoryEntry {
                    kind: HistoryEntryKind::Assistant,
                    text,
                })
            }
        }
        "plan" => {
            let text = item.get("text")?.as_str()?.trim().to_string();
            (!text.is_empty()).then(|| HistoryEntry {
                kind: HistoryEntryKind::Event,
                text: format!("[Plan]\n{text}"),
            })
        }
        "reasoning" => {
            let summary = item
                .get("summary")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join("\n");

            let text = summary.trim();
            (!text.is_empty()).then(|| HistoryEntry {
                kind: HistoryEntryKind::Event,
                text: format!("[Reasoning]\n{text}"),
            })
        }
        "commandExecution" => Some(HistoryEntry {
            kind: HistoryEntryKind::Event,
            text: summarize_command_execution(item),
        }),
        "fileChange" => Some(HistoryEntry {
            kind: HistoryEntryKind::Event,
            text: summarize_file_change(item),
        }),
        "mcpToolCall" => Some(HistoryEntry {
            kind: HistoryEntryKind::Event,
            text: summarize_mcp_tool_call(item),
        }),
        "dynamicToolCall" => Some(HistoryEntry {
            kind: HistoryEntryKind::Event,
            text: summarize_dynamic_tool_call(item),
        }),
        "webSearch" => item
            .get("query")
            .and_then(Value::as_str)
            .map(|query| HistoryEntry {
                kind: HistoryEntryKind::Event,
                text: format!("[Web Search] {query}"),
            }),
        "contextCompaction" => Some(HistoryEntry {
            kind: HistoryEntryKind::Event,
            text: "[Context] Conversation compacted".to_string(),
        }),
        "enteredReviewMode" => {
            item.get("review")
                .and_then(Value::as_str)
                .map(|review| HistoryEntry {
                    kind: HistoryEntryKind::Event,
                    text: format!("[Review] Entered review mode: {review}"),
                })
        }
        "exitedReviewMode" => {
            item.get("review")
                .and_then(Value::as_str)
                .map(|review| HistoryEntry {
                    kind: HistoryEntryKind::Event,
                    text: format!("[Review] Exited review mode: {review}"),
                })
        }
        "imageView" => item
            .get("path")
            .and_then(Value::as_str)
            .map(|path| HistoryEntry {
                kind: HistoryEntryKind::Event,
                text: format!("[Image] Viewed {path}"),
            }),
        "imageGeneration" => Some(HistoryEntry {
            kind: HistoryEntryKind::Event,
            text: summarize_image_generation(item),
        }),
        "subAgentActivity" => Some(HistoryEntry {
            kind: HistoryEntryKind::Event,
            text: summarize_subagent_activity(item),
        }),
        "collabAgentToolCall" => Some(HistoryEntry {
            kind: HistoryEntryKind::Event,
            text: summarize_collab_tool_call(item),
        }),
        "sleep" => item
            .get("durationMs")
            .and_then(Value::as_u64)
            .map(|duration| HistoryEntry {
                kind: HistoryEntryKind::Event,
                text: format!("[Sleep] Waited {duration}ms"),
            }),
        _ => None,
    }
}

fn user_message_text(item: &Value) -> String {
    let parts = item
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|part| {
            let part_type = part.get("type")?.as_str()?;
            match part_type {
                "text" => Some(part.get("text")?.as_str()?.to_string()),
                "localImage" => Some(format!(
                    "[local image: {}]",
                    part.get("path")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown")
                )),
                "image" => Some(format!(
                    "[image: {}]",
                    part.get("url").and_then(Value::as_str).unwrap_or("unknown")
                )),
                "skill" | "mention" => Some(
                    part.get("name")
                        .and_then(Value::as_str)
                        .map(|name| format!("@{name}"))
                        .unwrap_or_default(),
                ),
                _ => None,
            }
        })
        .collect::<Vec<_>>();

    parts.join("")
}

fn summarize_command_execution(item: &Value) -> String {
    let command = item
        .get("command")
        .and_then(Value::as_str)
        .unwrap_or("unknown command");
    let status = item
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let exit_code = item
        .get("exitCode")
        .and_then(Value::as_i64)
        .map(|code| format!(" • exit {code}"))
        .unwrap_or_default();

    format!("[Command] {command} • {status}{exit_code}")
}

fn summarize_file_change(item: &Value) -> String {
    let count = item
        .get("changes")
        .and_then(Value::as_array)
        .map(|changes| changes.len())
        .unwrap_or(0);
    let noun = if count == 1 { "file" } else { "files" };
    format!("[File Change] {count} {noun} updated")
}

fn summarize_mcp_tool_call(item: &Value) -> String {
    let server = item
        .get("server")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let tool = item.get("tool").and_then(Value::as_str).unwrap_or("tool");
    let status = item
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    format!("[MCP] {server}/{tool} • {status}")
}

fn summarize_dynamic_tool_call(item: &Value) -> String {
    let namespace = item
        .get("namespace")
        .and_then(Value::as_str)
        .map(|value| format!("{value}/"))
        .unwrap_or_default();
    let tool = item.get("tool").and_then(Value::as_str).unwrap_or("tool");
    let status = item
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    format!("[Tool] {namespace}{tool} • {status}")
}

fn summarize_image_generation(item: &Value) -> String {
    let status = item
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let saved_path = item
        .get("savedPath")
        .and_then(Value::as_str)
        .map(|path| format!(" • {path}"))
        .unwrap_or_default();
    format!("[Image Generation] {status}{saved_path}")
}

fn summarize_subagent_activity(item: &Value) -> String {
    let kind = item
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or("activity");
    let path = item
        .get("agentPath")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    format!("[Sub-agent] {kind} • {path}")
}

fn summarize_collab_tool_call(item: &Value) -> String {
    let tool = item.get("tool").and_then(Value::as_str).unwrap_or("tool");
    let status = item
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let receivers = item
        .get("receiverThreadIds")
        .and_then(Value::as_array)
        .map(|value| value.len())
        .unwrap_or(0);
    format!("[Collab] {tool} • {status} • {receivers} agent(s)")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn turns_are_converted_to_history_entries() {
        let turns = vec![ThreadTurn {
            items: vec![
                json!({
                    "type": "userMessage",
                    "content": [{"type":"text","text":"hello"}]
                }),
                json!({
                    "type": "commandExecution",
                    "command": "cargo check",
                    "status": "completed",
                    "exitCode": 0
                }),
                json!({
                    "type": "agentMessage",
                    "text": "done"
                }),
            ],
        }];

        let entries = history_entries_from_turns(&turns);
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].kind, HistoryEntryKind::User);
        assert_eq!(entries[0].text, "hello");
        assert_eq!(entries[1].kind, HistoryEntryKind::Event);
        assert!(entries[1].text.contains("cargo check"));
        assert_eq!(entries[2].kind, HistoryEntryKind::Assistant);
        assert_eq!(entries[2].text, "done");
    }
}
