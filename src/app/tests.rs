use super::{
    chat::{
        chat_max_scroll, format_shell_output, parse_codex_model_from_config,
        parse_codex_reasoning_effort_from_config, shell_command_from_input,
    },
    ui::{
        chat_input_height_for_main_area, scroll_position_from_row, vertical_scrollbar_metrics,
        wrapped_chat_input_lines, wrapped_text_height,
    },
    *,
};

#[test]
fn wrapped_text_height_matches_paragraph_word_wrapping() {
    let text = Text::from(vec![Line::from("abc def ghi")]);

    assert_eq!(wrapped_text_height(&text, 6), 3);
}

#[test]
fn chat_max_scroll_uses_wrapped_height_for_last_message() {
    let mut agent = AgentState::new(AgentDefinition {
        name: "Test".to_string(),
        workspace: PathBuf::from("/tmp"),
    });
    agent.messages.push(ChatMessage {
        role: MessageRole::Assistant,
        text: "abc def ghi".to_string(),
        item_id: None,
    });

    let area = Rect::new(0, 0, 8, 5);

    assert_eq!(chat_max_scroll(&agent, area), 2);
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

    assert_eq!(format!("{model} · {effort}"), "gpt-5.4 · xhigh");
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
        app.should_handle_mouse_scroll_at(ScrollDirection::Up, now + Duration::from_millis(10))
    );
    assert!(
        app.should_handle_mouse_scroll_at(ScrollDirection::Down, now + Duration::from_millis(40))
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
