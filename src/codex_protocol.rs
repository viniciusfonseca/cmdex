use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{HistoryEntry, ModelReasoningEffort, ServerEvent, ThreadItem};

#[derive(Debug, Serialize)]
pub(super) struct JsonRpcRequest<'a, T> {
    pub(super) jsonrpc: &'static str,
    pub(super) id: u64,
    pub(super) method: &'a str,
    pub(super) params: T,
}

#[derive(Debug, Serialize)]
pub(super) struct JsonRpcNotification<'a, T> {
    pub(super) jsonrpc: &'static str,
    pub(super) method: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) params: Option<T>,
}

#[derive(Debug, Serialize)]
pub(super) struct InitializeParams {
    #[serde(rename = "clientInfo")]
    pub(super) client_info: ClientInfo,
    pub(super) capabilities: Option<Value>,
}

#[derive(Debug, Serialize)]
pub(super) struct ClientInfo {
    pub(super) name: String,
    pub(super) version: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct InitializeResponse {
    #[allow(dead_code)]
    #[serde(rename = "userAgent")]
    pub(super) user_agent: String,
}

#[derive(Debug, Serialize)]
pub(super) struct ThreadStartParams {
    #[serde(rename = "cwd")]
    pub(super) cwd: Option<String>,
    pub(super) ephemeral: Option<bool>,
    #[serde(rename = "serviceName")]
    pub(super) service_name: Option<String>,
    pub(super) model: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct ThreadStartResponse {
    pub(super) thread: ThreadResponse,
    pub(super) model: String,
    #[serde(rename = "reasoningEffort")]
    pub(super) reasoning_effort: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct ThreadResponse {
    pub(super) id: String,
}

#[derive(Debug, Serialize)]
pub(super) struct TurnStartParams {
    #[serde(rename = "threadId")]
    pub(super) thread_id: String,
    pub(super) input: Vec<UserInput>,
    pub(super) model: Option<String>,
    pub(super) effort: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct TurnStartResponse {
    #[allow(dead_code)]
    pub(super) turn: TurnResponse,
}

#[derive(Debug, Serialize)]
pub(super) struct TurnInterruptParams {
    #[serde(rename = "threadId")]
    pub(super) thread_id: String,
    #[serde(rename = "turnId")]
    pub(super) turn_id: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct TurnInterruptResponse {}

#[derive(Debug, Serialize)]
pub(super) struct ThreadResumeParams {
    #[serde(rename = "threadId")]
    pub(super) thread_id: String,
    pub(super) model: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct ThreadResumeResponse {
    pub(super) thread: ThreadResponse,
    pub(super) model: String,
    #[serde(rename = "reasoningEffort")]
    pub(super) reasoning_effort: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct TurnResponse {
    #[allow(dead_code)]
    pub(super) id: String,
}

#[derive(Debug, Serialize)]
pub(super) struct ThreadListParams {
    pub(super) limit: Option<u64>,
    #[serde(rename = "sortKey")]
    pub(super) sort_key: Option<String>,
    #[serde(rename = "sortDirection")]
    pub(super) sort_direction: Option<String>,
    pub(super) cwd: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct ThreadListResponse {
    pub(super) data: Vec<ThreadListItem>,
}

#[derive(Debug, Deserialize)]
pub(super) struct ThreadListItem {
    pub(super) id: String,
}

#[derive(Debug, Serialize)]
pub(super) struct ThreadReadParams {
    #[serde(rename = "threadId")]
    pub(super) thread_id: String,
    #[serde(rename = "includeTurns")]
    pub(super) include_turns: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub(super) struct ThreadReadResponse {
    pub(super) thread: ThreadReadThread,
}

#[derive(Debug, Deserialize)]
pub(super) struct ThreadReadThread {
    pub(super) id: String,
    #[serde(default)]
    pub(super) turns: Vec<ThreadTurn>,
}

#[derive(Debug, Serialize)]
pub(super) struct ModelListParams {
    pub(super) cursor: Option<String>,
    #[serde(rename = "includeHidden")]
    pub(super) include_hidden: Option<bool>,
    pub(super) limit: Option<u32>,
}

#[derive(Debug, Deserialize)]
pub(super) struct ModelListResponse {
    pub(super) data: Vec<ModelListItem>,
    #[serde(rename = "nextCursor")]
    pub(super) next_cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct ModelListItem {
    pub(super) id: String,
    pub(super) model: String,
    #[serde(rename = "displayName")]
    pub(super) display_name: String,
    #[serde(rename = "isDefault")]
    pub(super) is_default: bool,
    #[serde(rename = "supportedReasoningEfforts", default)]
    pub(super) supported_reasoning_efforts: Vec<ModelReasoningEffort>,
    #[serde(rename = "defaultReasoningEffort", default)]
    pub(super) default_reasoning_effort: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct ThreadTurn {
    #[serde(default)]
    pub(super) items: Vec<Value>,
}

#[derive(Debug, Serialize)]
pub(super) struct UserInput {
    #[serde(rename = "type")]
    pub(super) kind: &'static str,
    pub(super) text: String,
    pub(super) text_elements: Vec<Value>,
}

impl UserInput {
    pub(super) fn text(text: &str) -> Self {
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

pub(super) struct CodexResponseMapper;

impl CodexResponseMapper {
    pub(super) fn parse_server_event(value: Value) -> Option<ServerEvent> {
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

    pub(super) fn format_jsonrpc_error(error: &Value) -> String {
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

    pub(super) fn history_entries_from_turns(turns: &[ThreadTurn]) -> Vec<HistoryEntry> {
        super::codex_history::CodexHistoryMapper::entries_from_turns(turns)
    }
}
