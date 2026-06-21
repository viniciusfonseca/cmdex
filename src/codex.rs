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

    pub async fn start_thread(&self, cwd: &Path) -> Result<ThreadInfo> {
        let response: ThreadStartResponse = self
            .request(
                "thread/start",
                ThreadStartParams {
                    cwd: Some(cwd.to_string_lossy().to_string()),
                    ephemeral: Some(true),
                    service_name: Some("cmdex".to_string()),
                },
            )
            .await?;

        Ok(ThreadInfo {
            id: response.thread.id,
        })
    }

    pub async fn start_turn(&self, thread_id: &str, text: &str) -> Result<()> {
        let _: TurnStartResponse = self
            .request(
                "turn/start",
                TurnStartParams {
                    thread_id: thread_id.to_string(),
                    input: vec![UserInput::text(text)],
                },
            )
            .await?;

        Ok(())
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
}

#[derive(Debug, Deserialize)]
struct ThreadStartResponse {
    thread: ThreadResponse,
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
}

#[derive(Debug, Deserialize)]
struct TurnStartResponse {
    #[allow(dead_code)]
    turn: TurnResponse,
}

#[derive(Debug, Deserialize)]
struct TurnResponse {
    #[allow(dead_code)]
    id: String,
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
