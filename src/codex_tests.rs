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

    let entries = CodexResponseMapper::history_entries_from_turns(&turns);
    assert_eq!(entries.len(), 3);
    assert_eq!(entries[0].kind, HistoryEntryKind::User);
    assert_eq!(entries[0].text, "hello");
    assert_eq!(entries[1].kind, HistoryEntryKind::Event);
    assert!(entries[1].text.contains("cargo check"));
    assert_eq!(entries[2].kind, HistoryEntryKind::Assistant);
    assert_eq!(entries[2].text, "done");
}

#[test]
fn file_change_entries_include_paths_and_diffs() {
    let turns = vec![ThreadTurn {
        items: vec![json!({
            "type": "fileChange",
            "changes": [
                {
                    "path": "src/main.rs",
                    "diff": "@@ -1 +1 @@\n-old\n+new",
                    "kind": { "type": "update", "move_path": null }
                },
                {
                    "path": "README.md",
                    "diff": "+ hello",
                    "kind": { "type": "add" }
                }
            ]
        })],
    }];

    let entries = CodexResponseMapper::history_entries_from_turns(&turns);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].kind, HistoryEntryKind::Event);
    assert!(entries[0].text.contains("[File Change] 2 files updated"));
    assert!(entries[0].text.contains("- Updated `src/main.rs`"));
    assert!(entries[0].text.contains("```diff"));
    assert!(entries[0].text.contains("+new"));
    assert!(entries[0].text.contains("- Added `README.md`"));
}

#[test]
fn model_list_preserves_supported_reasoning_efforts() {
    let model: ModelListItem = serde_json::from_value(json!({
        "id": "gpt-5.5",
        "model": "gpt-5.5",
        "displayName": "GPT-5.5",
        "isDefault": true,
        "supportedReasoningEfforts": [
            {"reasoningEffort": "low", "description": "Fast"},
            {"reasoningEffort": "high", "description": "Deep"}
        ],
        "defaultReasoningEffort": "high"
    }))
    .unwrap();

    assert_eq!(model.supported_reasoning_efforts.len(), 2);
    assert_eq!(model.supported_reasoning_efforts[0].reasoning_effort, "low");
    assert_eq!(model.default_reasoning_effort.as_deref(), Some("high"));
}

#[test]
fn parses_turn_started_and_completed_notifications() {
    let started = CodexResponseMapper::parse_server_event(json!({
        "method": "turn/started",
        "params": {
            "threadId": "thread-1",
            "turn": {
                "id": "turn-1",
                "items": [],
                "status": "inProgress"
            }
        }
    }));
    assert!(matches!(
        started,
        Some(ServerEvent::TurnStarted {
            thread_id,
            turn_id
        }) if thread_id == "thread-1" && turn_id == "turn-1"
    ));

    let completed = CodexResponseMapper::parse_server_event(json!({
        "method": "turn/completed",
        "params": {
            "threadId": "thread-1",
            "turn": {
                "id": "turn-1",
                "items": [],
                "status": "interrupted"
            }
        }
    }));
    assert!(matches!(
        completed,
        Some(ServerEvent::TurnCompleted {
            thread_id,
            turn_id,
            interrupted
        }) if thread_id == "thread-1" && turn_id == "turn-1" && interrupted
    ));
}
