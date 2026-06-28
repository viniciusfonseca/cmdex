use super::{
    chat::{
        ChatCommand, ModelCommand, chat_command_from_input, chat_content_height, chat_lines,
        chat_max_scroll, format_chat_model_label, format_shell_output, padded_chat_lines,
        parse_codex_model_from_config, parse_codex_reasoning_effort_from_config,
        resolve_chat_model_label, shell_command_from_input,
    },
    ui::{
        chat_input_height_for_main_area, list_offset, scroll_position_from_row,
        scrollable_preview_content_height, scrollable_text_height, scrollbar_thumb_bounds,
        vertical_scrollbar_metrics, wrapped_chat_input_lines, wrapped_text_height,
    },
    *,
};

#[test]
fn wrapped_text_height_matches_paragraph_word_wrapping() {
    let text = Text::from(vec![Line::from("abc def ghi")]);

    assert_eq!(wrapped_text_height(&text, 6), 3);
}

#[test]
fn list_offset_scrolls_long_lists_to_keep_selection_visible() {
    assert_eq!(list_offset(0, 10, 4), 0);
    assert_eq!(list_offset(3, 10, 4), 0);
    assert_eq!(list_offset(4, 10, 4), 1);
    assert_eq!(list_offset(9, 10, 4), 6);
}

#[test]
fn chat_max_scroll_uses_wrapped_height_for_last_message() {
    let mut agent = AgentState::new(
        AgentDefinition {
            name: "Test".to_string(),
            workspace: PathBuf::from("/tmp"),
        },
        None,
        None,
        "default",
    );
    agent.messages.push(ChatMessage::new(
        MessageRole::Assistant,
        "abc def ghi",
        None,
    ));

    let area = Rect::new(0, 0, 8, 5);

    assert_eq!(chat_max_scroll(&agent, area), 2);
}

#[test]
fn chat_scroll_height_matches_rendered_width_without_extra_tail_space() {
    let mut agent = AgentState::new(
        AgentDefinition {
            name: "Test".to_string(),
            workspace: PathBuf::from("/tmp"),
        },
        None,
        None,
        "default",
    );
    agent.messages.push(ChatMessage::new(
        MessageRole::Assistant,
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        None,
    ));

    let area = Rect::new(0, 0, 10, 5);
    let text = Text::from(chat_lines(&agent));
    let expected_content_height = wrapped_text_height(&text, area.width.saturating_sub(2));

    assert!(scrollable_text_height(&text, area) > expected_content_height);
    assert_eq!(chat_content_height(&agent, area), expected_content_height);
    assert_eq!(
        chat_max_scroll(&agent, area),
        expected_content_height.saturating_sub(area.height.saturating_sub(2) as usize) as u16
    );
}

#[test]
fn chat_appends_single_blank_line_after_final_message() {
    let mut agent = AgentState::new(
        AgentDefinition {
            name: "Test".to_string(),
            workspace: PathBuf::from("/tmp"),
        },
        None,
        None,
        "default",
    );
    agent.messages.push(ChatMessage::new(
        MessageRole::Assistant,
        "final message",
        None,
    ));

    let lines = chat_lines(&agent);

    assert_eq!(lines.len(), 3);
    assert_eq!(line_text(&lines[0]), "Test:");
    assert_eq!(line_text(&lines[1]), "final message");
    assert!(line_text(&lines[2]).is_empty());
}

#[test]
fn chat_keeps_a_single_blank_line_between_messages() {
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
        .messages
        .push(ChatMessage::new(MessageRole::User, "one", None));
    agent
        .messages
        .push(ChatMessage::new(MessageRole::Assistant, "two", None));

    let lines = chat_lines(&agent);

    assert_eq!(line_text(&lines[0]), "You:");
    assert_eq!(line_text(&lines[1]), "one");
    assert!(line_text(&lines[2]).is_empty());
    assert_eq!(line_text(&lines[3]), "Test:");
    assert_eq!(line_text(&lines[4]), "two");
    assert!(line_text(&lines[5]).is_empty());
}

#[test]
fn chat_bottom_aligns_short_content_with_one_blank_line_after_last_message() {
    let mut agent = AgentState::new(
        AgentDefinition {
            name: "Test".to_string(),
            workspace: PathBuf::from("/tmp"),
        },
        None,
        None,
        "default",
    );
    agent.messages.push(ChatMessage::new(
        MessageRole::Assistant,
        "final message",
        None,
    ));

    let area = Rect::new(0, 0, 20, 8);
    let lines = padded_chat_lines(&agent, area);

    assert_eq!(lines.len(), 6);
    assert!(line_text(&lines[0]).is_empty());
    assert!(line_text(&lines[1]).is_empty());
    assert!(line_text(&lines[2]).is_empty());
    assert_eq!(line_text(&lines[3]), "Test:");
    assert_eq!(line_text(&lines[4]), "final message");
    assert!(line_text(&lines[5]).is_empty());
}

#[test]
fn cached_chat_message_lines_refresh_after_text_updates() {
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
fn shell_command_is_detected_from_chat_input() {
    assert_eq!(
        shell_command_from_input("> cargo test"),
        Some("cargo test".to_string())
    );
    assert_eq!(
        shell_command_from_input(">   ls -la"),
        Some("ls -la".to_string())
    );
    assert_eq!(shell_command_from_input("hello > world"), None);
    assert_eq!(shell_command_from_input(">"), None);
}

#[test]
fn model_command_is_detected_from_chat_input() {
    assert_eq!(
        chat_command_from_input("/model"),
        Some(ChatCommand::Model(ModelCommand::List))
    );
    assert_eq!(
        chat_command_from_input("/model gpt-5.5"),
        Some(ChatCommand::Model(ModelCommand::Set {
            model: Some("gpt-5.5".to_string()),
            effort: None,
        }))
    );
    assert_eq!(
        chat_command_from_input("/model gpt-5.5 xhigh"),
        Some(ChatCommand::Model(ModelCommand::Set {
            model: Some("gpt-5.5".to_string()),
            effort: Some("xhigh".to_string()),
        }))
    );
    assert_eq!(
        chat_command_from_input("/model high"),
        Some(ChatCommand::Model(ModelCommand::Set {
            model: None,
            effort: Some("high".to_string()),
        }))
    );
    assert_eq!(
        chat_command_from_input("  /model default  "),
        Some(ChatCommand::Model(ModelCommand::ResetDefault))
    );
    assert_eq!(chat_command_from_input("/modelx"), None);
}

#[test]
fn shell_output_is_formatted_for_chat() {
    let output = format_shell_output("ls", "file.txt\n", "", Some(0), true);

    assert!(output.contains("Command: `ls`"));
    assert!(output.contains("```text"));
    assert!(output.contains("file.txt"));
    assert!(output.contains("Exit code: 0 (ok)"));
}

#[test]
fn chat_input_wraps_into_multiple_lines() {
    assert_eq!(
        wrapped_chat_input_lines("abcdef", 4),
        vec!["abcd".to_string(), "ef".to_string()]
    );
    assert_eq!(
        wrapped_chat_input_lines("ab\ncd", 4),
        vec!["ab".to_string(), "cd".to_string()]
    );
}

#[test]
fn chat_input_height_grows_with_wrapped_content() {
    let main_area = Rect::new(0, 0, 10, 20);

    assert_eq!(chat_input_height_for_main_area("short", main_area), 3);
    assert_eq!(chat_input_height_for_main_area("abcdefghijk", main_area), 4);
}

#[test]
fn turn_events_track_active_turn_and_interruption_status() {
    let config = CmdexConfig {
        agents: vec![AgentDefinition {
            name: "Test".to_string(),
            workspace: PathBuf::from("/tmp"),
        }],
    };
    let mut app = App::new(PathBuf::new(), config);
    app.current_agent = Some(0);
    app.chat_sidebar_index = 1;
    app.agents[0].thread_id = Some("thread-1".to_string());

    app.handle_server_event(ServerEvent::TurnStarted {
        thread_id: "thread-1".to_string(),
        turn_id: "turn-1".to_string(),
    });

    assert_eq!(app.agents[0].active_turn_id.as_deref(), Some("turn-1"));
    assert!(app.agents[0].thinking);

    app.handle_server_event(ServerEvent::TurnCompleted {
        thread_id: "thread-1".to_string(),
        turn_id: "turn-1".to_string(),
        interrupted: true,
    });

    assert_eq!(app.agents[0].active_turn_id, None);
    assert!(!app.agents[0].thinking);
    assert_eq!(app.agents[0].status.as_deref(), Some("Response canceled"));
}

#[test]
fn parses_codex_model_from_top_level_config() {
    let config = r#"
model = "gpt-5.4"
model_reasoning_effort = "xhigh"

[projects."/tmp/example"]
trust_level = "trusted"
"#;

    assert_eq!(
        parse_codex_model_from_config(config),
        Some("gpt-5.4".to_string())
    );
    assert_eq!(
        parse_codex_reasoning_effort_from_config(config),
        Some("xhigh".to_string())
    );
}

#[test]
fn ignores_non_top_level_model_keys() {
    let config = r#"
[profiles.fast]
model = "gpt-5.5-mini"
"#;

    assert_eq!(parse_codex_model_from_config(config), None);
    assert_eq!(parse_codex_reasoning_effort_from_config(config), None);
}

#[test]
fn builds_chat_model_label_with_reasoning_effort() {
    let config = r#"
model = "gpt-5.4"
model_reasoning_effort = "xhigh"
"#;

    let model = parse_codex_model_from_config(config).unwrap();
    let effort = parse_codex_reasoning_effort_from_config(config).unwrap();

    assert_eq!(
        format_chat_model_label(&model, Some(&effort)),
        "gpt-5.4 · xhigh"
    );
    assert_eq!(format_chat_model_label(&model, None), "gpt-5.4");
    assert_eq!(
        resolve_chat_model_label(None, Some("high"), Some(&model), "default"),
        "gpt-5.4 · high"
    );
    assert_eq!(
        resolve_chat_model_label(None, None, Some(&model), "gpt-5.4 · xhigh"),
        "gpt-5.4 · xhigh"
    );
}

#[test]
fn debounces_repeated_mouse_scroll_events() {
    let mut app = App::new(PathBuf::new(), CmdexConfig::default());
    let now = Instant::now();

    assert!(app.should_handle_mouse_scroll_at(ScrollDirection::Down, now));
    assert!(
        !app.should_handle_mouse_scroll_at(ScrollDirection::Down, now + Duration::from_millis(10))
    );
    assert!(
        app.should_handle_mouse_scroll_at(ScrollDirection::Down, now + Duration::from_millis(25))
    );

    let mut other_direction = App::new(PathBuf::new(), CmdexConfig::default());
    assert!(other_direction.should_handle_mouse_scroll_at(ScrollDirection::Down, now));
    assert!(
        other_direction
            .should_handle_mouse_scroll_at(ScrollDirection::Up, now + Duration::from_millis(10))
    );
}

#[test]
fn chat_and_workspace_share_same_mouse_scroll_debounce() {
    let mut chat_app = App::new(PathBuf::new(), CmdexConfig::default());
    let mut workspace_app = App::new(PathBuf::new(), CmdexConfig::default());
    let now = Instant::now();
    workspace_app.current_tab = AppTab::Workspace;

    assert!(chat_app.should_handle_mouse_scroll_at(ScrollDirection::Down, now));
    assert!(
        !chat_app
            .should_handle_mouse_scroll_at(ScrollDirection::Down, now + Duration::from_millis(10))
    );
    assert!(
        chat_app
            .should_handle_mouse_scroll_at(ScrollDirection::Down, now + Duration::from_millis(25))
    );

    assert!(workspace_app.should_handle_mouse_scroll_at(ScrollDirection::Down, now));
    assert!(
        !workspace_app
            .should_handle_mouse_scroll_at(ScrollDirection::Down, now + Duration::from_millis(10))
    );
    assert!(
        workspace_app
            .should_handle_mouse_scroll_at(ScrollDirection::Down, now + Duration::from_millis(25))
    );
}

#[test]
fn vertical_scrollbar_track_stays_inside_container_border() {
    let area = Rect::new(10, 5, 20, 8);
    let metrics = vertical_scrollbar_metrics(area, 32).expect("scrollbar metrics");

    assert_eq!(metrics.track.x, 28);
    assert_eq!(metrics.track.y, 6);
    assert_eq!(metrics.track.width, 1);
    assert_eq!(metrics.track.height, 6);
}

#[test]
fn scrollbar_drag_maps_mouse_row_to_scroll_position() {
    let metrics = ScrollbarMetrics {
        track: Rect::new(0, 10, 1, 6),
        content_length: 30,
        viewport_length: 6,
    };

    assert_eq!(scroll_position_from_row(metrics, 10), 0);
    assert_eq!(scroll_position_from_row(metrics, 13), 14);
    assert_eq!(scroll_position_from_row(metrics, 15), 24);
}

#[test]
fn scrollbar_drag_keeps_cursor_at_thumb_center() {
    let metrics = ScrollbarMetrics {
        track: Rect::new(0, 10, 1, 6),
        content_length: 8,
        viewport_length: 4,
    };

    let (thumb_top, thumb_height) = scrollbar_thumb_bounds(metrics, 3).expect("thumb bounds");
    let cursor_row = metrics.track.y + thumb_top + thumb_height / 2;

    assert_eq!(scroll_position_from_row(metrics, cursor_row), 3);
}

#[test]
fn scrollbar_thumb_reaches_bottom_of_track_at_max_scroll() {
    let metrics = ScrollbarMetrics {
        track: Rect::new(0, 10, 1, 6),
        content_length: 8,
        viewport_length: 4,
    };

    let (thumb_top, thumb_height) = scrollbar_thumb_bounds(metrics, 4).expect("thumb bounds");

    assert_eq!(thumb_top + thumb_height, metrics.track.height);
}

#[test]
fn scrollable_content_height_accounts_for_scrollbar_width() {
    let area = Rect::new(0, 0, 6, 3);
    let lines = vec![Line::from("1234567")];
    let text = Text::from(lines.clone());

    assert_eq!(scrollable_preview_content_height(&lines, area), 3);
    assert_eq!(scrollable_text_height(&text, area), 3);
}

#[test]
fn workspace_tree_refreshes_on_tick_after_filesystem_changes() {
    let root = std::env::temp_dir().join(format!(
        "cmdex-app-workspace-refresh-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("alpha.txt"), "alpha").unwrap();

    let config = CmdexConfig {
        agents: vec![AgentDefinition {
            name: "Test".to_string(),
            workspace: root.clone(),
        }],
    };
    let mut app = App::new(PathBuf::new(), config);
    app.current_tab = AppTab::Workspace;
    app.current_agent = Some(0);
    app.chat_sidebar_index = 1;
    app.refresh_current_tab();

    fs::write(root.join("beta.txt"), "beta").unwrap();
    app.last_workspace_refresh_at =
        Some(Instant::now() - WORKSPACE_AUTO_REFRESH_INTERVAL - Duration::from_millis(1));

    app.on_tick();

    assert!(
        app.active_agent()
            .unwrap()
            .workspace
            .sidebar_labels()
            .iter()
            .any(|label| label.contains("beta.txt"))
    );

    let _ = fs::remove_dir_all(root);
}

fn line_text(line: &Line<'_>) -> String {
    line.spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>()
}
