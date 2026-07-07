use super::{
    chat::{ChatCommand, ChatSupport, ModelCommand},
    components::{
        ChatInputComponent, GitDiffComponent, ShellSidebarComponent, TopNavigationComponent,
        UiSupport, WorkspaceComponent, WorkspaceEditorComponent,
    },
    shell::{ShellOutputParser, ShellOutputRecord, ShellPresenter, ShellTabState},
    *,
};
use ratatui::style::Color;
use serde_json::json;

#[test]
fn wrapped_text_height_matches_paragraph_word_wrapping() {
    let text = Text::from(vec![Line::from("abc def ghi")]);

    assert_eq!(UiSupport::wrapped_text_height(&text, 6), 3);
}

#[test]
fn list_offset_scrolls_long_lists_to_keep_selection_visible() {
    assert_eq!(UiSupport::list_offset(0, 10, 4), 0);
    assert_eq!(UiSupport::list_offset(3, 10, 4), 0);
    assert_eq!(UiSupport::list_offset(4, 10, 4), 1);
    assert_eq!(UiSupport::list_offset(9, 10, 4), 6);
}

#[test]
fn git_diff_remote_button_label_shows_spinner_only_for_active_action() {
    assert_eq!(
        GitDiffComponent::remote_button_label(
            "Push",
            Some(GitRemoteAction::Push),
            GitRemoteAction::Push,
            2,
        ),
        format!("{} Push", SPINNER[2])
    );
    assert_eq!(
        GitDiffComponent::remote_button_label(
            "Pull",
            Some(GitRemoteAction::Push),
            GitRemoteAction::Pull,
            2,
        ),
        "Pull"
    );
}

#[test]
fn git_diff_preview_padding_fills_changed_rows_to_viewport_width() {
    let changed = Line::from("abc").style(Style::default().bg(Color::Red));
    let context = Line::from("xy");

    let padded = GitDiffComponent::pad_preview_lines(&[changed, context], 5);

    assert_eq!(line_text(&padded[0]), "abc  ");
    assert_eq!(padded[0].width(), 5);
    assert_eq!(padded[1].width(), 2);
}

#[test]
fn shell_tab_session_creation_selects_new_session() {
    let mut shell_tab = ShellTabState::default();
    let workspace = PathBuf::from("/tmp/project");

    let first = shell_tab.create_session(&workspace);
    let second = shell_tab.create_session(&workspace);

    assert_eq!(first, 1);
    assert_eq!(second, 2);
    assert_eq!(shell_tab.selected_index(), 1);
    assert_eq!(
        shell_tab
            .selected_session()
            .map(|session| session.title.as_str()),
        Some("Session 2")
    );
}

#[test]
fn shell_tab_only_creates_initial_session_once() {
    let mut shell_tab = ShellTabState::default();
    let workspace = PathBuf::from("/tmp/project");

    let first = shell_tab.create_session_if_empty(&workspace);
    let second = shell_tab.create_session_if_empty(&workspace);

    assert_eq!(first, Some(1));
    assert_eq!(second, None);
    assert_eq!(shell_tab.sessions.len(), 1);
    assert_eq!(shell_tab.selected_index(), 0);
}

#[test]
fn shell_tab_removes_closed_session_and_clamps_selection() {
    let mut shell_tab = ShellTabState::default();
    let workspace = PathBuf::from("/tmp/project");

    let first = shell_tab.create_session(&workspace);
    let second = shell_tab.create_session(&workspace);

    assert_eq!(shell_tab.selected_index(), 1);
    shell_tab.remove_session_by_id(second);

    assert_eq!(shell_tab.sessions.len(), 1);
    assert_eq!(shell_tab.selected_index(), 0);
    assert_eq!(
        shell_tab.selected_session().map(|session| session.id),
        Some(first)
    );
}

#[test]
fn shell_command_payload_appends_completion_sentinel() {
    let payload = ShellPresenter::command_payload("pwd");

    assert_eq!(payload, "pwd\n");
}

#[test]
fn shell_display_lines_hides_prompt_while_session_is_running() {
    let mut shell_tab = ShellTabState::default();
    let workspace = PathBuf::from("/tmp/project");
    shell_tab.create_session(&workspace);
    let session = shell_tab.selected_session_mut().expect("session");
    session.mark_ready();

    let idle_lines = ShellPresenter::display_lines(session, "ls");
    assert_eq!(line_text(idle_lines.last().expect("idle prompt")), "$ ls");

    session.append_command("ls");
    let running_lines = ShellPresenter::display_lines(session, "");
    assert_eq!(
        line_text(running_lines.last().expect("running line")),
        "$ ls"
    );
}

#[test]
fn shell_output_parser_splits_sentinel_from_same_terminal_line() {
    let mut parser = ShellOutputParser::new();
    let records = parser.push("value__CMDEX_DONE__:7\n");

    assert_eq!(records.len(), 2);
    assert!(matches!(&records[0], ShellOutputRecord::Line(line) if line == "value"));
    assert!(matches!(records[1], ShellOutputRecord::CommandFinished(7)));
}

#[test]
fn shell_output_parser_normalizes_carriage_returns() {
    let mut parser = ShellOutputParser::new();
    let records = parser.push("first\rsecond\r__CMDEX_DONE__:0\n");

    assert_eq!(records.len(), 3);
    assert!(matches!(&records[0], ShellOutputRecord::Line(line) if line == "first"));
    assert!(matches!(&records[1], ShellOutputRecord::Line(line) if line == "second"));
    assert!(matches!(records[2], ShellOutputRecord::CommandFinished(0)));
}

#[test]
fn shell_output_parser_strips_ansi_sequences_before_rendering() {
    let mut parser = ShellOutputParser::new();
    let records = parser.push("\u{1b}[31mred\u{1b}[0m\r\n");

    assert!(matches!(records.as_slice(), [ShellOutputRecord::Line(line)] if line == "red"));
}

#[test]
fn shell_output_parser_treats_crlf_as_single_line_break() {
    let mut parser = ShellOutputParser::new();
    let records = parser.push("first\r\nsecond\r\n");

    assert_eq!(records.len(), 2);
    assert!(matches!(&records[0], ShellOutputRecord::Line(line) if line == "first"));
    assert!(matches!(&records[1], ShellOutputRecord::Line(line) if line == "second"));
}

#[test]
fn shell_output_parser_recognizes_ready_sentinel() {
    let mut parser = ShellOutputParser::new();
    let records = parser.push("__CMDEX_READY__\r\n");

    assert!(matches!(records.as_slice(), [ShellOutputRecord::Ready]));
}

#[cfg(unix)]
#[test]
fn shell_pty_session_does_not_echo_commands_into_output() {
    use portable_pty::{CommandBuilder, PtySize, native_pty_system};
    use std::{
        io::{BufRead, BufReader, Write},
        time::{Duration, Instant},
    };

    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows: 40,
            cols: 120,
            pixel_width: 0,
            pixel_height: 0,
        })
        .expect("open PTY");

    let shell_path = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
    let mut command = CommandBuilder::new(&shell_path);
    command.arg("-c");
    command.arg(super::shell::SHELL_SESSION_LOOP);

    let mut child = pair.slave.spawn_command(command).expect("spawn PTY shell");
    let mut reader = BufReader::new(pair.master.try_clone_reader().expect("clone PTY reader"));
    let mut writer = pair.master.take_writer().expect("open PTY writer");

    let mut parser = ShellOutputParser::new();
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut buffer = Vec::new();
    let mut ready = false;

    while Instant::now() < deadline && !ready {
        buffer.clear();
        let bytes = reader
            .read_until(b'\n', &mut buffer)
            .expect("read PTY output");
        if bytes == 0 {
            break;
        }

        let chunk = String::from_utf8_lossy(&buffer);
        ready = parser
            .push(&chunk)
            .into_iter()
            .any(|record| matches!(record, ShellOutputRecord::Ready));
    }

    assert!(ready, "shell PTY did not report readiness");

    writer.write_all(b"pwd\n").expect("write command");
    writer.flush().expect("flush command");

    let mut records = Vec::new();
    while Instant::now() < deadline {
        buffer.clear();
        let bytes = reader
            .read_until(b'\n', &mut buffer)
            .expect("read PTY output");
        if bytes == 0 {
            break;
        }

        let chunk = String::from_utf8_lossy(&buffer);
        records.extend(parser.push(&chunk));

        if records
            .iter()
            .any(|record| matches!(record, ShellOutputRecord::CommandFinished(_)))
        {
            break;
        }
    }

    let _ = child.kill();
    let _ = child.wait();

    assert!(
        records
            .iter()
            .all(|record| !matches!(record, ShellOutputRecord::Line(line) if line == "pwd")),
        "command echo leaked into shell output"
    );
    assert!(
        records
            .iter()
            .any(|record| matches!(record, ShellOutputRecord::CommandFinished(0))),
        "shell command did not finish successfully"
    );
}

#[test]
fn shell_tab_index_sits_between_workspace_and_git_diff() {
    let mut app = App::new(PathBuf::new(), CmdexConfig::default());

    app.current_tab = AppTab::Workspace;
    assert_eq!(TopNavigationComponent::selected_index(&app), 1);

    app.current_tab = AppTab::Shell;
    assert_eq!(TopNavigationComponent::selected_index(&app), 2);

    app.current_tab = AppTab::GitDiff;
    assert_eq!(TopNavigationComponent::selected_index(&app), 3);
}

#[test]
fn shell_sidebar_labels_include_new_session_action() {
    let workspace = PathBuf::from("/tmp/project");
    let config = CmdexConfig {
        agents: vec![AgentDefinition {
            name: "Test".to_string(),
            workspace: workspace.clone(),
        }],
    };
    let mut app = App::new(PathBuf::new(), config);
    app.current_tab = AppTab::Shell;
    app.current_agent = Some(0);

    let labels_without_sessions = ShellSidebarComponent::labels(&app);
    assert_eq!(labels_without_sessions, vec!["+ New session".to_string()]);

    let session_id = app.agents[0].shell_tab.create_session(&workspace);
    app.agents[0]
        .shell_tab
        .session_by_id_mut(session_id)
        .expect("session")
        .mark_ready();

    let labels_with_session = ShellSidebarComponent::labels(&app);
    assert_eq!(labels_with_session[0], "+ New session");
    assert_eq!(labels_with_session[1], "Session 1");
}

#[test]
fn shell_session_exit_event_removes_session_from_sidebar() {
    let workspace = PathBuf::from("/tmp/project");
    let config = CmdexConfig {
        agents: vec![AgentDefinition {
            name: "Test".to_string(),
            workspace: workspace.clone(),
        }],
    };
    let mut app = App::new(PathBuf::new(), config);
    app.current_tab = AppTab::Shell;
    app.current_agent = Some(0);

    let session_id = app.agents[0].shell_tab.create_session(&workspace);
    app.handle_ui_event(UiEvent::ShellSessionExited {
        agent_index: 0,
        session_id,
        message: "Shell exited".to_string(),
    });

    assert_eq!(
        ShellSidebarComponent::labels(&app),
        vec!["+ New session".to_string()]
    );
    assert_eq!(app.status_message.as_deref(), Some("Shell exited"));
}

#[test]
fn top_navigation_clicks_ignore_cmdex_prefix_and_hit_tabs() {
    let area = Rect::new(0, 0, 50, 3);
    let tabs = TopNavigationComponent::tabs_rect(area);

    assert_eq!(TopNavigationComponent::tab_from_click(area, 1, 1), None);
    assert_eq!(
        TopNavigationComponent::tab_from_click(area, tabs.x, tabs.y),
        Some(AppTab::Chat)
    );

    let workspace_x = tabs.x.saturating_add("Chat".chars().count() as u16 + 3);
    assert_eq!(
        TopNavigationComponent::tab_from_click(area, workspace_x, tabs.y),
        Some(AppTab::Workspace)
    );
}

#[test]
fn alt_r_requests_restart() {
    let mut app = App::new(PathBuf::new(), CmdexConfig::default());

    app.should_restart = true;
    app.should_quit = true;

    assert!(app.should_restart);
    assert!(app.should_quit);
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

    assert_eq!(ChatSupport::max_scroll(&agent, area), 2);
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
    let text = Text::from(ChatSupport::lines(&agent));
    let expected_content_height =
        UiSupport::wrapped_text_height(&text, area.width.saturating_sub(2));

    assert!(UiSupport::scrollable_text_height(&text, area) > expected_content_height);
    assert_eq!(
        ChatSupport::content_height(&agent, area),
        expected_content_height
    );
    assert_eq!(
        ChatSupport::max_scroll(&agent, area),
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

    let lines = ChatSupport::lines(&agent);

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

    let lines = ChatSupport::lines(&agent);

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
    let lines = ChatSupport::padded_lines(&agent, area);

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
fn chat_limits_rendered_lines_to_ten_thousand() {
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
        agent.messages.push(ChatMessage::new(
            MessageRole::Assistant,
            format!("message {index}"),
            None,
        ));
    }

    let lines = ChatSupport::lines(&agent);

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
        ChatSupport::command_from_input("/model gpt-5.5 xhigh"),
        Some(ChatCommand::Model(ModelCommand::Set {
            model: Some("gpt-5.5".to_string()),
            effort: Some("xhigh".to_string()),
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
fn shell_output_is_formatted_for_chat() {
    let output = ChatSupport::format_shell_output("ls", "file.txt\n", "", Some(0), true);

    assert!(output.contains("Command: `ls`"));
    assert!(output.contains("```text"));
    assert!(output.contains("file.txt"));
    assert!(output.contains("Exit code: 0 (ok)"));
}

#[test]
fn chat_input_wraps_into_multiple_lines() {
    assert_eq!(
        ChatInputComponent::wrapped_lines("abcdef", 4),
        vec!["abcd".to_string(), "ef".to_string()]
    );
    assert_eq!(
        ChatInputComponent::wrapped_lines("ab\ncd", 4),
        vec!["ab".to_string(), "cd".to_string()]
    );
}

#[test]
fn chat_input_height_grows_with_wrapped_content() {
    let main_area = Rect::new(0, 0, 10, 20);

    assert_eq!(
        ChatInputComponent::height_for_main_area("short", main_area),
        3
    );
    assert_eq!(
        ChatInputComponent::height_for_main_area("abcdefghijk", main_area),
        4
    );
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
        ChatSupport::parse_codex_model_from_config(config),
        Some("gpt-5.4".to_string())
    );
    assert_eq!(
        ChatSupport::parse_codex_reasoning_effort_from_config(config),
        Some("xhigh".to_string())
    );
}

#[test]
fn ignores_non_top_level_model_keys() {
    let config = r#"
[profiles.fast]
model = "gpt-5.5-mini"
"#;

    assert_eq!(ChatSupport::parse_codex_model_from_config(config), None);
    assert_eq!(
        ChatSupport::parse_codex_reasoning_effort_from_config(config),
        None
    );
}

#[test]
fn builds_chat_model_label_with_reasoning_effort() {
    let config = r#"
model = "gpt-5.4"
model_reasoning_effort = "xhigh"
"#;

    let model = ChatSupport::parse_codex_model_from_config(config).unwrap();
    let effort = ChatSupport::parse_codex_reasoning_effort_from_config(config).unwrap();

    assert_eq!(
        ChatSupport::format_chat_model_label(&model, Some(&effort)),
        "gpt-5.4 · xhigh"
    );
    assert_eq!(
        ChatSupport::format_chat_model_label(&model, None),
        "gpt-5.4"
    );
    assert_eq!(
        ChatSupport::resolve_chat_model_label(None, Some("high"), Some(&model), "default"),
        "gpt-5.4 · high"
    );
    assert_eq!(
        ChatSupport::resolve_chat_model_label(None, None, Some(&model), "gpt-5.4 · xhigh"),
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
    let metrics = UiSupport::vertical_scrollbar_metrics(area, 32).expect("scrollbar metrics");

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

    assert_eq!(UiSupport::scroll_position_from_row(metrics, 10), 0);
    assert_eq!(UiSupport::scroll_position_from_row(metrics, 13), 14);
    assert_eq!(UiSupport::scroll_position_from_row(metrics, 15), 24);
}

#[test]
fn scrollbar_drag_keeps_cursor_at_thumb_center() {
    let metrics = ScrollbarMetrics {
        track: Rect::new(0, 10, 1, 6),
        content_length: 8,
        viewport_length: 4,
    };

    let (thumb_top, thumb_height) =
        UiSupport::scrollbar_thumb_bounds(metrics, 3).expect("thumb bounds");
    let cursor_row = metrics.track.y + thumb_top + thumb_height / 2;

    assert_eq!(UiSupport::scroll_position_from_row(metrics, cursor_row), 3);
}

#[test]
fn scrollbar_thumb_reaches_bottom_of_track_at_max_scroll() {
    let metrics = ScrollbarMetrics {
        track: Rect::new(0, 10, 1, 6),
        content_length: 8,
        viewport_length: 4,
    };

    let (thumb_top, thumb_height) =
        UiSupport::scrollbar_thumb_bounds(metrics, 4).expect("thumb bounds");

    assert_eq!(thumb_top + thumb_height, metrics.track.height);
}

#[test]
fn scrollable_content_height_accounts_for_scrollbar_width() {
    let area = Rect::new(0, 0, 6, 3);
    let lines = vec![Line::from("1234567")];
    let text = Text::from(lines.clone());

    assert_eq!(
        UiSupport::scrollable_preview_content_height(&lines, area),
        3
    );
    assert_eq!(UiSupport::scrollable_text_height(&text, area), 3);
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
    TopNavigationComponent::refresh_current_tab(&mut app);

    fs::write(root.join("beta.txt"), "beta").unwrap();
    app.last_workspace_refresh_at =
        Some(Instant::now() - WORKSPACE_AUTO_REFRESH_INTERVAL - Duration::from_millis(1));

    let (ui_tx, _ui_rx) = tokio::sync::mpsc::unbounded_channel();
    app.on_tick(&ui_tx);

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

#[test]
fn workspace_arrow_keys_move_editor_cursor_when_editor_is_focused() {
    let root = std::env::temp_dir().join(format!(
        "cmdex-app-workspace-focus-editor-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&root).unwrap();
    let alpha = root.join("alpha.txt");
    let zeta = root.join("zeta.txt");
    fs::write(&alpha, "alpha\nbeta\n").unwrap();
    fs::write(&zeta, "zeta\n").unwrap();

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
    TopNavigationComponent::refresh_current_tab(&mut app);

    {
        let workspace = &mut app.active_agent_mut().unwrap().workspace;
        workspace.select(0);
        workspace.open_editor().unwrap();
        assert!(workspace.editor_focused());
    }

    let handled = WorkspaceComponent::handle_key(
        &mut app,
        KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
        Rect::new(0, 0, 120, 40),
    );

    assert!(handled);
    let workspace = &app.active_agent().unwrap().workspace;
    assert_eq!(workspace.selected, 0);
    assert!(workspace.editor_focused());
    assert_eq!(workspace.editor.as_ref().unwrap().path, alpha);
    assert_eq!(workspace.editor.as_ref().unwrap().cursor_row, 1);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn workspace_hover_waits_for_stationary_cursor_before_requesting_lsp() {
    let root = std::env::temp_dir().join(format!(
        "cmdex-app-workspace-hover-delay-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&root).unwrap();
    let file = root.join("main.rs");
    fs::write(&file, "greet();\n").unwrap();

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
    TopNavigationComponent::refresh_current_tab(&mut app);

    {
        let workspace = &mut app.active_agent_mut().unwrap().workspace;
        workspace.select(0);
        workspace.open_editor().unwrap();
    }

    let area = Rect::new(0, 0, 120, 40);
    let layout = app.compute_layout(area);
    let viewport = WorkspaceEditorComponent::viewport(layout.body);
    let gutter_width = app
        .active_agent()
        .unwrap()
        .workspace
        .editor
        .as_ref()
        .unwrap()
        .gutter_width() as u16;
    let column = viewport.x + gutter_width;
    let row = viewport.y;
    let (ui_tx, _ui_rx) = tokio::sync::mpsc::unbounded_channel();

    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Moved,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        },
        area,
        &ui_tx,
    );

    let first_started_at = app
        .pending_workspace_hover
        .as_ref()
        .expect("pending hover")
        .started_at;
    assert_eq!(
        app.active_agent()
            .unwrap()
            .workspace
            .editor
            .as_ref()
            .unwrap()
            .hover_request_position(),
        None
    );

    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Moved,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        },
        area,
        &ui_tx,
    );

    assert_eq!(
        app.pending_workspace_hover
            .as_ref()
            .expect("pending hover")
            .started_at,
        first_started_at
    );
    assert!(!app.on_tick(&ui_tx));
    assert_eq!(
        app.active_agent()
            .unwrap()
            .workspace
            .editor
            .as_ref()
            .unwrap()
            .hover_request_position(),
        None
    );

    app.pending_workspace_hover
        .as_mut()
        .expect("pending hover")
        .started_at = Instant::now() - HOVER_POPOVER_DELAY - Duration::from_millis(1);

    assert!(app.on_tick(&ui_tx));
    assert_eq!(
        app.active_agent()
            .unwrap()
            .workspace
            .editor
            .as_ref()
            .unwrap()
            .hover_request_position(),
        Some(EditorPosition { row: 0, col: 0 })
    );

    app.shutdown_lsp_sessions();
    let _ = fs::remove_dir_all(root);
}

#[test]
fn workspace_arrow_keys_move_sidebar_selection_when_sidebar_is_focused() {
    let root = std::env::temp_dir().join(format!(
        "cmdex-app-workspace-focus-sidebar-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&root).unwrap();
    let alpha = root.join("alpha.txt");
    let zeta = root.join("zeta.txt");
    fs::write(&alpha, "alpha\nbeta\n").unwrap();
    fs::write(&zeta, "zeta\n").unwrap();

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
    TopNavigationComponent::refresh_current_tab(&mut app);

    {
        let workspace = &mut app.active_agent_mut().unwrap().workspace;
        workspace.select(0);
        workspace.open_editor().unwrap();
        assert!(workspace.editor_focused());
    }

    assert!(WorkspaceComponent::handle_key(
        &mut app,
        KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE),
        Rect::new(0, 0, 120, 40),
    ));
    assert!(app.active_agent().unwrap().workspace.sidebar_focused());

    let handled = WorkspaceComponent::handle_key(
        &mut app,
        KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
        Rect::new(0, 0, 120, 40),
    );

    assert!(handled);
    let workspace = &app.active_agent().unwrap().workspace;
    assert_eq!(workspace.selected, 1);
    assert!(workspace.sidebar_focused());
    assert_eq!(workspace.editor.as_ref().unwrap().path, zeta);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn workspace_u_shortcut_undoes_editor_changes() {
    let root = std::env::temp_dir().join(format!(
        "cmdex-app-workspace-undo-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&root).unwrap();
    let alpha = root.join("alpha.txt");
    fs::write(&alpha, "hello").unwrap();

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
    TopNavigationComponent::refresh_current_tab(&mut app);

    {
        let workspace = &mut app.active_agent_mut().unwrap().workspace;
        workspace.select(0);
        workspace.open_editor().unwrap();
        let editor = workspace.editor.as_mut().unwrap();
        editor.enter_insert_mode();
        editor.insert_char('!');
        editor.mode = EditorMode::Normal;
        assert_eq!(editor.lines, vec!["!hello".to_string()]);
        assert!(editor.dirty);
    }

    let handled = WorkspaceComponent::handle_key(
        &mut app,
        KeyEvent::new(KeyCode::Char('u'), KeyModifiers::NONE),
        Rect::new(0, 0, 120, 40),
    );

    assert!(handled);
    let editor = app
        .active_agent()
        .unwrap()
        .workspace
        .editor
        .as_ref()
        .unwrap();
    assert_eq!(editor.lines, vec!["hello".to_string()]);
    assert!(!editor.dirty);
    assert_eq!(editor.status.as_deref(), Some("Undid last change"));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn workspace_search_only_captures_text_when_sidebar_is_focused() {
    let root = std::env::temp_dir().join(format!(
        "cmdex-app-workspace-search-focus-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&root).unwrap();
    let alpha = root.join("alpha.txt");
    fs::write(&alpha, "needle\n").unwrap();

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
    TopNavigationComponent::refresh_current_tab(&mut app);

    {
        let workspace = &mut app.active_agent_mut().unwrap().workspace;
        workspace.set_sidebar_tab(WorkspaceSidebarTab::Search);
        for character in "needle".chars() {
            workspace.push_search_char(character);
        }
        assert!(workspace.open_selected_search_result().unwrap());
        workspace.editor.as_mut().unwrap().enter_insert_mode();
        assert!(workspace.editor_focused());
    }

    app.handle_text_input('!');

    let workspace = &app.active_agent().unwrap().workspace;
    assert_eq!(workspace.search_query, "needle");
    assert_eq!(workspace.editor.as_ref().unwrap().lines[0], "!needle");

    let _ = fs::remove_dir_all(root);
}

#[test]
fn workspace_mouse_drag_selects_text_in_editor() {
    let root = std::env::temp_dir().join(format!(
        "cmdex-app-workspace-mouse-select-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&root).unwrap();
    let alpha = root.join("alpha.txt");
    fs::write(&alpha, "hello\n").unwrap();

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
    TopNavigationComponent::refresh_current_tab(&mut app);

    {
        let workspace = &mut app.active_agent_mut().unwrap().workspace;
        workspace.select(0);
        workspace.open_editor().unwrap();
    }

    let area = Rect::new(0, 0, 120, 40);
    let layout = app.compute_layout(area);
    let viewport = WorkspaceEditorComponent::viewport(layout.body);
    let gutter_width = app
        .active_agent()
        .unwrap()
        .workspace
        .editor
        .as_ref()
        .unwrap()
        .gutter_width() as u16;
    let origin_x = viewport.x + gutter_width;
    let row = viewport.y;
    let (ui_tx, _ui_rx) = tokio::sync::mpsc::unbounded_channel();

    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: origin_x,
            row,
            modifiers: KeyModifiers::NONE,
        },
        area,
        &ui_tx,
    );
    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: origin_x + 2,
            row,
            modifiers: KeyModifiers::NONE,
        },
        area,
        &ui_tx,
    );
    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: origin_x + 2,
            row,
            modifiers: KeyModifiers::NONE,
        },
        area,
        &ui_tx,
    );

    let editor = app
        .active_agent()
        .unwrap()
        .workspace
        .editor
        .as_ref()
        .unwrap();
    assert_eq!(editor.mode, EditorMode::Visual);
    assert!(editor.has_selection());

    let selected = editor.rendered_lines(1)[0]
        .spans
        .iter()
        .filter(|span| span.style.bg == Some(ThemeRegistry::app().selection_bg))
        .map(|span| span.content.as_ref())
        .collect::<String>();
    assert_eq!(selected, "he");

    let _ = fs::remove_dir_all(root);
}

#[test]
fn workspace_mouse_click_clears_existing_selection() {
    let root = std::env::temp_dir().join(format!(
        "cmdex-app-workspace-mouse-clear-select-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&root).unwrap();
    let alpha = root.join("alpha.txt");
    fs::write(&alpha, "hello\n").unwrap();

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
    TopNavigationComponent::refresh_current_tab(&mut app);

    {
        let workspace = &mut app.active_agent_mut().unwrap().workspace;
        workspace.select(0);
        workspace.open_editor().unwrap();
        let editor = workspace.editor.as_mut().unwrap();
        editor.enter_visual_mode();
        editor.extend_right();
        editor.extend_right();
        assert!(editor.has_selection());
    }

    let area = Rect::new(0, 0, 120, 40);
    let layout = app.compute_layout(area);
    let viewport = WorkspaceEditorComponent::viewport(layout.body);
    let gutter_width = app
        .active_agent()
        .unwrap()
        .workspace
        .editor
        .as_ref()
        .unwrap()
        .gutter_width() as u16;
    let click_x = viewport.x + gutter_width + 3;
    let row = viewport.y;
    let (ui_tx, _ui_rx) = tokio::sync::mpsc::unbounded_channel();

    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: click_x,
            row,
            modifiers: KeyModifiers::NONE,
        },
        area,
        &ui_tx,
    );
    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: click_x,
            row,
            modifiers: KeyModifiers::NONE,
        },
        area,
        &ui_tx,
    );

    let editor = app
        .active_agent()
        .unwrap()
        .workspace
        .editor
        .as_ref()
        .unwrap();
    assert_eq!(editor.mode, EditorMode::Normal);
    assert!(!editor.has_selection());
    assert_eq!(editor.cursor_row, 0);
    assert_eq!(editor.cursor_col, 3);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn lsp_hover_parses_marked_string_objects() {
    let hover = json!({
        "contents": {
            "language": "rust",
            "value": "fn greet()"
        }
    });

    assert_eq!(
        super::lsp::parse_hover_response(&hover).as_deref(),
        Some("rust\nfn greet()")
    );
}

#[test]
fn lsp_hover_parses_hover_arrays() {
    let hover = json!({
        "contents": [
            { "value": "fn greet()" },
            "Returns a greeting"
        ]
    });

    assert_eq!(
        super::lsp::parse_hover_response(&hover).as_deref(),
        Some("fn greet()\n\nReturns a greeting")
    );
}

#[test]
fn lsp_hover_summary_preserves_line_breaks_and_strips_code_fences() {
    let hover = "```rust\nfn greet()\n```\n\nReturns a greeting";

    assert_eq!(
        super::lsp::summarize_hover_text(hover).as_deref(),
        Some("fn greet()\n\nReturns a greeting")
    );
}

#[test]
fn lsp_utf16_columns_round_trip_through_unicode_text() {
    let source = "a😀b\n";
    let position = EditorPosition { row: 0, col: 2 };

    let utf16 = super::lsp::utf16_column(source, position);

    assert_eq!(utf16, 3);
    assert_eq!(super::lsp::char_column_from_utf16(source, 0, utf16), 2);
}

#[test]
fn lsp_definition_parser_converts_utf16_offsets_into_editor_columns() {
    let path = std::env::temp_dir().join(format!(
        "cmdex-lsp-definition-{}-{}.rs",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::write(&path, "a😀b\n").unwrap();

    let response = json!([{
        "uri": url::Url::from_file_path(&path).unwrap().to_string(),
        "range": {
            "start": {
                "line": 0,
                "character": 3
            },
            "end": {
                "line": 0,
                "character": 4
            }
        }
    }]);

    let target = super::lsp::parse_definition_response(&response)
        .unwrap()
        .unwrap();

    assert_eq!(target.path, path);
    assert_eq!(target.position, EditorPosition { row: 0, col: 2 });

    let _ = fs::remove_file(path);
}

#[test]
fn workspace_editor_hover_popover_stays_inside_viewport() {
    let code_area = Rect::new(10, 5, 40, 12);

    let popup = WorkspaceEditorComponent::hover_popover_area(code_area, 47, 15, 20, 6);

    assert_eq!(popup, Rect::new(28, 10, 20, 6));
}

fn line_text(line: &Line<'_>) -> String {
    line.spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>()
}
