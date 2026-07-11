use super::chat::{ChatCommand, ChatSupport, ModelCommand};
use super::components::UiSupport;
use super::*;
use ratatui::{layout::Rect, text::Line};

fn line_text(line: &Line<'_>) -> String {
    line.spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect()
}

#[test]
fn max_scroll_uses_wrapped_height_for_last_message() {
    let mut agent = AgentState::new(
        AgentDefinition {
            name: "Test".to_string(),
            workspace: PathBuf::from("/tmp"),
        },
        None,
        None,
        "default",
    );
    agent.chat.messages.push(ChatMessage::new(
        MessageRole::Assistant,
        "abc def ghi",
        None,
    ));

    assert_eq!(
        ChatSupport::max_scroll(&agent.chat, Rect::new(0, 0, 8, 5)),
        2
    );
}

#[test]
fn scroll_height_matches_rendered_width_without_extra_tail_space() {
    let mut agent = AgentState::new(
        AgentDefinition {
            name: "Test".to_string(),
            workspace: PathBuf::from("/tmp"),
        },
        None,
        None,
        "default",
    );
    agent.chat.messages.push(ChatMessage::new(
        MessageRole::Assistant,
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        None,
    ));

    let area = Rect::new(0, 0, 10, 5);
    let text = ChatSupport::build_text(&agent.chat);
    let expected = UiSupport::wrapped_text_height(&text, area.width.saturating_sub(2));

    assert!(UiSupport::scrollable_text_height(&text, area) > expected);
    assert_eq!(ChatSupport::content_height(&agent.chat, area), expected);
    assert_eq!(
        ChatSupport::max_scroll(&agent.chat, area),
        expected.saturating_sub(area.height.saturating_sub(2) as usize) as u16
    );
}

#[test]
fn build_text_adds_one_blank_line_after_final_message() {
    let mut agent = AgentState::new(
        AgentDefinition {
            name: "Test".to_string(),
            workspace: PathBuf::from("/tmp"),
        },
        None,
        None,
        "default",
    );
    agent.chat.messages.push(ChatMessage::new(
        MessageRole::Assistant,
        "final message",
        None,
    ));

    let lines = ChatSupport::build_text(&agent.chat).lines;
    assert_eq!(lines.len(), 3);
    assert_eq!(line_text(&lines[0]), "Test:");
    assert_eq!(line_text(&lines[1]), "final message");
    assert!(line_text(&lines[2]).is_empty());
}

#[test]
fn build_text_keeps_one_blank_line_between_messages() {
    let mut agent = AgentState::new(
        AgentDefinition {
            name: "Test".to_string(),
            workspace: PathBuf::from("/tmp"),
        },
        None,
        None,
        "default",
    );
    agent
        .chat
        .messages
        .push(ChatMessage::new(MessageRole::User, "one", None));
    agent
        .chat
        .messages
        .push(ChatMessage::new(MessageRole::Assistant, "two", None));

    let lines = ChatSupport::build_text(&agent.chat).lines;
    assert_eq!(line_text(&lines[0]), "You:");
    assert_eq!(line_text(&lines[1]), "one");
    assert!(line_text(&lines[2]).is_empty());
    assert_eq!(line_text(&lines[3]), "Test:");
    assert_eq!(line_text(&lines[4]), "two");
    assert!(line_text(&lines[5]).is_empty());
}

#[test]
fn render_state_bottom_aligns_short_content() {
    let mut agent = AgentState::new(
        AgentDefinition {
            name: "Test".to_string(),
            workspace: PathBuf::from("/tmp"),
        },
        None,
        None,
        "default",
    );
    agent.chat.messages.push(ChatMessage::new(
        MessageRole::Assistant,
        "final message",
        None,
    ));

    let lines = ChatSupport::render_state(&agent.chat, Rect::new(0, 0, 20, 8))
        .text
        .lines;
    assert_eq!(lines.len(), 6);
    assert!(line_text(&lines[0]).is_empty());
    assert!(line_text(&lines[1]).is_empty());
    assert!(line_text(&lines[2]).is_empty());
    assert_eq!(line_text(&lines[3]), "Test:");
    assert_eq!(line_text(&lines[4]), "final message");
    assert!(line_text(&lines[5]).is_empty());
}

#[test]
fn cached_message_lines_refresh_after_text_updates() {
    let mut message = ChatMessage::new(MessageRole::Assistant, "one", Some("item".to_string()));

    assert_eq!(line_text(&message.rendered_lines[0]), "one");
    message.append_text("\n\ntwo");
    assert_eq!(line_text(&message.rendered_lines[0]), "one");
    assert_eq!(
        line_text(message.rendered_lines.last().expect("rendered lines")),
        "two"
    );

    message.set_text("three".to_string());
    assert_eq!(message.rendered_lines.len(), 1);
    assert_eq!(line_text(&message.rendered_lines[0]), "three");
}

#[test]
fn build_text_limits_rendered_lines_to_ten_thousand() {
    let mut agent = AgentState::new(
        AgentDefinition {
            name: "Test".to_string(),
            workspace: PathBuf::from("/tmp"),
        },
        None,
        None,
        "default",
    );
    for index in 0..4_000 {
        agent.chat.messages.push(ChatMessage::new(
            MessageRole::Assistant,
            format!("message {index}"),
            None,
        ));
    }

    let lines = ChatSupport::build_text(&agent.chat).lines;
    assert_eq!(lines.len(), 9_999);
    assert_eq!(line_text(&lines[0]), "Test:");
    assert_eq!(line_text(&lines[1]), "message 667");
    assert!(line_text(lines.last().expect("last line")).is_empty());
}

#[test]
fn shell_command_is_detected_from_chat_input() {
    assert_eq!(
        ChatSupport::shell_command_from_input("> cargo test"),
        Some("cargo test".to_string())
    );
    assert_eq!(
        ChatSupport::shell_command_from_input(">   ls -la"),
        Some("ls -la".to_string())
    );
    assert_eq!(ChatSupport::shell_command_from_input("hello > world"), None);
    assert_eq!(ChatSupport::shell_command_from_input(">"), None);
}

#[test]
fn model_command_is_detected_from_chat_input() {
    assert_eq!(
        ChatSupport::command_from_input("/model"),
        Some(ChatCommand::Model(ModelCommand::List))
    );
    assert_eq!(
        ChatSupport::command_from_input("/model gpt-5.5"),
        Some(ChatCommand::Model(ModelCommand::Set {
            model: Some("gpt-5.5".to_string()),
            effort: None,
        }))
    );
    assert_eq!(
        ChatSupport::command_from_input("/model high"),
        Some(ChatCommand::Model(ModelCommand::Set {
            model: None,
            effort: Some("high".to_string()),
        }))
    );
    assert_eq!(
        ChatSupport::command_from_input("  /model default  "),
        Some(ChatCommand::Model(ModelCommand::ResetDefault))
    );
    assert_eq!(ChatSupport::command_from_input("/modelx"), None);
}

#[test]
fn parses_only_top_level_codex_model_configuration() {
    let config = r#"
model = "gpt-5.4"
model_reasoning_effort = "xhigh"

[projects."/tmp/example"]
trust_level = "trusted"
"#;

    assert_eq!(
        ChatSupport::parse_codex_model_from_config(config),
        Some("gpt-5.4".to_string())
    );
    assert_eq!(
        ChatSupport::parse_codex_reasoning_effort_from_config(config),
        Some("xhigh".to_string())
    );

    let nested = r#"
[profiles.fast]
model = "gpt-5.5-mini"
"#;
    assert_eq!(ChatSupport::parse_codex_model_from_config(nested), None);
    assert_eq!(
        ChatSupport::parse_codex_reasoning_effort_from_config(nested),
        None
    );
}

#[test]
fn formats_chat_model_label_with_reasoning_effort() {
    assert_eq!(
        ChatSupport::format_chat_model_label("gpt-5.4", Some("xhigh")),
        "gpt-5.4 · xhigh"
    );
    assert_eq!(
        ChatSupport::resolve_chat_model_label(None, Some("high"), Some("gpt-5.4"), "default"),
        "gpt-5.4 · high"
    );
    assert_eq!(
        ChatSupport::resolve_chat_model_label(None, None, Some("gpt-5.4"), "default"),
        "default"
    );
}
