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

#[path = "codex_history.rs"]
mod codex_history;

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
    TurnStarted {
        thread_id: String,
        turn_id: String,
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
        turn_id: String,
        interrupted: bool,
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
    pub supported_reasoning_efforts: Vec<ModelReasoningEffort>,
    pub default_reasoning_effort: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ModelReasoningEffort {
    #[serde(rename = "reasoningEffort")]
    pub reasoning_effort: String,
    #[serde(default)]
    pub description: Option<String>,
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
                                            Some(error) => Err(anyhow!(
                                                CodexResponseMapper::format_jsonrpc_error(error)
                                            )),
                                            None => Ok(value
                                                .get("result")
                                                .cloned()
                                                .unwrap_or(Value::Null)),
                                        };
                                        let _ = sender.send(result);
                                    }
                                } else if let Some(event) =
                                    CodexResponseMapper::parse_server_event(value)
                                {
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
    ) -> Result<String> {
        let response: TurnStartResponse = self
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

        Ok(response.turn.id)
    }

    pub async fn interrupt_turn(&self, thread_id: &str, turn_id: &str) -> Result<()> {
        let _: TurnInterruptResponse = self
            .request(
                "turn/interrupt",
                TurnInterruptParams {
                    thread_id: thread_id.to_string(),
                    turn_id: turn_id.to_string(),
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
            entries: CodexResponseMapper::history_entries_from_turns(&response.thread.turns),
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
                supported_reasoning_efforts: model.supported_reasoning_efforts,
                default_reasoning_effort: model.default_reasoning_effort,
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
struct TurnInterruptParams {
    #[serde(rename = "threadId")]
    thread_id: String,
    #[serde(rename = "turnId")]
    turn_id: String,
}

#[derive(Debug, Deserialize)]
struct TurnInterruptResponse {}

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
    #[serde(rename = "supportedReasoningEfforts", default)]
    supported_reasoning_efforts: Vec<ModelReasoningEffort>,
    #[serde(rename = "defaultReasoningEffort", default)]
    default_reasoning_effort: Option<String>,
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
    turn: TurnCompletedTurn,
}

#[derive(Debug, Deserialize)]
struct TurnStartedParams {
    #[serde(rename = "threadId")]
    thread_id: String,
    turn: TurnResponse,
}

#[derive(Debug, Deserialize)]
struct TurnCompletedTurn {
    id: String,
    status: String,
}

struct CodexResponseMapper;

impl CodexResponseMapper {
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
            "turn/started" => {
                let params: TurnStartedParams = serde_json::from_value(params).ok()?;
                Some(ServerEvent::TurnStarted {
                    thread_id: params.thread_id,
                    turn_id: params.turn.id,
                })
            }
            "item/started" => {
                let params: ItemNotificationParams = serde_json::from_value(params).ok()?;
                Some(ServerEvent::ItemStarted {
                    thread_id: params.thread_id,
                    item: Self::map_thread_item(params.item),
                })
            }
            "item/completed" => {
                let params: ItemNotificationParams = serde_json::from_value(params).ok()?;
                Some(ServerEvent::ItemCompleted {
                    thread_id: params.thread_id,
                    item: Self::map_thread_item(params.item),
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
                    turn_id: params.turn.id,
                    interrupted: params.turn.status == "interrupted",
                })
            }
            "warning" => Some(ServerEvent::Warning(Self::extract_message(&params))),
            "error" => Some(ServerEvent::Error(Self::extract_message(&params))),
            _ => None,
        }
    }

    fn map_thread_item(item: RawThreadItem) -> ThreadItem {
        match item {
            RawThreadItem::AgentMessage { id, text } => ThreadItem::AgentMessage { id, text },
            RawThreadItem::UserMessage => ThreadItem::UserMessage,
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
        codex_history::CodexHistoryMapper::entries_from_turns(turns)
    }
}

#[cfg(test)]
#[path = "codex_tests.rs"]
mod tests;
