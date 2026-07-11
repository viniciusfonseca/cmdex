use super::{HistoryEntry, HistoryEntryKind, ThreadTurn, Value};

pub(super) struct CodexHistoryMapper;

impl CodexHistoryMapper {
    pub(super) fn entries_from_turns(turns: &[ThreadTurn]) -> Vec<HistoryEntry> {
        turns
            .iter()
            .flat_map(|turn| turn.items.iter())
            .filter_map(Self::entry_from_item)
            .collect()
    }

    fn entry_from_item(item: &Value) -> Option<HistoryEntry> {
        let item_type = item.get("type")?.as_str()?;

        match item_type {
            "userMessage" => {
                let text = Self::user_message_text(item);
                (!text.trim().is_empty()).then_some(HistoryEntry {
                    kind: HistoryEntryKind::User,
                    text,
                })
            }
            "agentMessage" => {
                let text = item.get("text")?.as_str()?.to_string();
                (!text.trim().is_empty()).then_some(HistoryEntry {
                    kind: HistoryEntryKind::Assistant,
                    text,
                })
            }
            "plan" => Self::event_with_text(item, "text", "[Plan]"),
            "reasoning" => {
                let text = item
                    .get("summary")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join("\n");
                Self::event_text("[Reasoning]", text)
            }
            "commandExecution" => Some(Self::event(Self::summarize_command_execution(item))),
            "fileChange" => Some(Self::event(Self::summarize_file_change(item))),
            "mcpToolCall" => Some(Self::event(Self::summarize_mcp_tool_call(item))),
            "dynamicToolCall" => Some(Self::event(Self::summarize_dynamic_tool_call(item))),
            "webSearch" => item
                .get("query")
                .and_then(Value::as_str)
                .map(|query| Self::event(format!("[Web Search] {query}"))),
            "contextCompaction" => Some(Self::event("[Context] Conversation compacted")),
            "enteredReviewMode" => {
                Self::event_from_field(item, "review", "[Review] Entered review mode: ")
            }
            "exitedReviewMode" => {
                Self::event_from_field(item, "review", "[Review] Exited review mode: ")
            }
            "imageView" => item
                .get("path")
                .and_then(Value::as_str)
                .map(|path| Self::event(format!("[Image] Viewed {path}"))),
            "imageGeneration" => Some(Self::event(Self::summarize_image_generation(item))),
            "subAgentActivity" => Some(Self::event(Self::summarize_subagent_activity(item))),
            "collabAgentToolCall" => Some(Self::event(Self::summarize_collab_tool_call(item))),
            "sleep" => item
                .get("durationMs")
                .and_then(Value::as_u64)
                .map(|duration| Self::event(format!("[Sleep] Waited {duration}ms"))),
            _ => None,
        }
    }

    fn event(text: impl Into<String>) -> HistoryEntry {
        HistoryEntry {
            kind: HistoryEntryKind::Event,
            text: text.into(),
        }
    }

    fn event_text(prefix: &str, text: String) -> Option<HistoryEntry> {
        let text = text.trim();
        (!text.is_empty()).then(|| Self::event(format!("{prefix}\n{text}")))
    }

    fn event_with_text(item: &Value, field: &str, prefix: &str) -> Option<HistoryEntry> {
        let text = item.get(field)?.as_str()?.trim();
        Self::event_text(prefix, text.to_string())
    }

    fn event_from_field(item: &Value, field: &str, prefix: &str) -> Option<HistoryEntry> {
        item.get(field)
            .and_then(Value::as_str)
            .map(|value| Self::event(format!("{prefix}{value}")))
    }

    fn user_message_text(item: &Value) -> String {
        item.get("content")
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
            .collect::<Vec<_>>()
            .join("")
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
        let changes = item
            .get("changes")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        if changes.is_empty() {
            return "[File Change] 0 files updated".to_string();
        }

        let count = changes.len();
        let noun = if count == 1 { "file" } else { "files" };
        let mut text = format!("[File Change] {count} {noun} updated");
        for change in changes {
            let path = change
                .get("path")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            let diff = change
                .get("diff")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim();
            text.push_str("\n\n- ");
            text.push_str(&Self::summarize_patch_change_kind(change.get("kind")));
            text.push_str(" `");
            text.push_str(path);
            text.push('`');
            if !diff.is_empty() {
                text.push_str("\n\n```diff\n");
                text.push_str(diff);
                text.push_str("\n```");
            }
        }
        text
    }

    fn summarize_patch_change_kind(kind: Option<&Value>) -> String {
        let Some(kind) = kind else {
            return "Updated".to_string();
        };
        match kind.get("type").and_then(Value::as_str).unwrap_or("update") {
            "add" => "Added".to_string(),
            "delete" => "Deleted".to_string(),
            "update" => kind
                .get("move_path")
                .and_then(Value::as_str)
                .map(|path| format!("Updated (moved to `{path}`)"))
                .unwrap_or_else(|| "Updated".to_string()),
            _ => "Updated".to_string(),
        }
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
            .map(Vec::len)
            .unwrap_or(0);
        format!("[Collab] {tool} • {status} • {receivers} agent(s)")
    }
}
