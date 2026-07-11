use super::test_support::{app_with_agent, model};
use super::{
    chat::ChatSupport,
    components::{
        ChatComponent, ChatInputComponent, GitDiffComponent, ModelPickerAction,
        ShellSidebarComponent, TopNavigationComponent, WorkspaceEditorComponent, WorkspaceScreen,
    },
    effects::AppEffect,
    shell::{ShellOutputParser, ShellOutputRecord},
    *,
};
use crossterm::event::{Event, KeyEventKind, KeyEventState};
use ratatui::style::Color;
use ratatui::{Terminal, backend::TestBackend};

#[test]
fn app_input_normalizes_terminal_events_and_filters_non_input_events() {
    let key = KeyEvent {
        code: KeyCode::Char('x'),
        modifiers: KeyModifiers::NONE,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    };

    assert!(matches!(
        AppInput::from_terminal_event(Event::Key(key)),
        Some(AppInput::Key(_))
    ));
    assert!(matches!(
        AppInput::from_terminal_event(Event::Paste("text".to_string())),
        Some(AppInput::Paste(text)) if text == "text"
    ));
    assert!(AppInput::from_terminal_event(Event::Resize(80, 24)).is_none());
}

#[test]
fn git_mutations_are_enqueued_instead_of_running_in_the_input_handler() {
    let config = CmdexConfig {
        agents: vec![AgentDefinition {
            name: "Test".to_string(),
            workspace: PathBuf::from("/tmp"),
        }],
        ..CmdexConfig::default()
    };
    let mut app = App::new(PathBuf::new(), config);
    app.current_agent = Some(0);
    app.current_tab = AppTab::GitDiff;
    app.agents[0]
        .git_diff
        .changes
        .push(crate::workspace::DiffEntry {
            label: "[M] file.rs".to_string(),
            path: PathBuf::from("/tmp/file.rs"),
            status: "M".to_string(),
        });

    GitDiffComponent::handle_key(
        &mut app,
        KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL),
    );

    assert!(app.agents[0].git_diff.mutation_running);
    assert!(matches!(
        app.take_effects().as_slice(),
        [AppEffect::RunGitMutation {
            agent_index: 0,
            root,
            mutation: GitMutation::Stage(path),
        }] if root == &PathBuf::from("/tmp") && path == "file.rs"
    ));
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
    let mut app = app_with_agent(PathBuf::from("/tmp"));

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
        ..CmdexConfig::default()
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
        ..CmdexConfig::default()
    };
    let mut app = App::new(PathBuf::new(), config);
    app.current_tab = AppTab::Shell;
    app.current_agent = Some(0);

    let session_id = app.agents[0].shell_tab.create_session(&workspace);
    app.handle_ui_event(UiEvent::Shell(ShellEvent::SessionExited {
        agent_index: 0,
        session_id,
        message: "Shell exited".to_string(),
    }));

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
fn app_outcome_carries_restart_exit() {
    assert_eq!(
        super::input::AppOutcome::Exit(AppExit::Restart).exit(),
        Some(AppExit::Restart)
    );
}

#[test]
fn chat_shell_submission_enqueues_effect_without_spawning_runtime_work() {
    let config = CmdexConfig {
        agents: vec![AgentDefinition {
            name: "Test".to_string(),
            workspace: PathBuf::from("/tmp"),
        }],
        ..CmdexConfig::default()
    };
    let mut app = App::new(PathBuf::new(), config);
    app.current_agent = Some(0);
    app.chat_sidebar_index = 1;

    ChatComponent::submit_shell_command(&mut app, "printf hello".to_string());

    assert!(app.agents[0].chat.shell_running);
    assert!(matches!(
        app.take_effects().as_slice(),
        [AppEffect::RunShell {
            agent_index: 0,
            command,
            workspace,
        }] if command == "printf hello" && workspace == &PathBuf::from("/tmp")
    ));
}

#[test]
fn chat_codex_commands_enqueue_effects_without_spawning_runtime_work() {
    let config = CmdexConfig {
        agents: vec![AgentDefinition {
            name: "Test".to_string(),
            workspace: PathBuf::from("/tmp"),
        }],
        ..CmdexConfig::default()
    };
    let mut app = App::new(PathBuf::new(), config);
    app.current_agent = Some(0);
    app.chat_sidebar_index = 1;
    app.chat_input = "hello".to_string();

    ChatComponent::submit_message(&mut app);

    assert!(app.agents[0].chat.thinking);
    assert!(matches!(
        app.take_effects().as_slice(),
        [AppEffect::StartChatTurn {
            agent_index: 0,
            text,
            workspace,
            ..
        }] if text == "hello" && workspace == &PathBuf::from("/tmp")
    ));

    app.chat_input = "/model".to_string();
    ChatComponent::submit_message(&mut app);
    assert!(matches!(
        app.take_effects().as_slice(),
        [AppEffect::ListModels { agent_index: 0 }]
    ));
}

#[test]
fn model_list_opens_picker_with_current_model_selected() {
    let mut app = app_with_agent(PathBuf::from("/tmp"));
    app.current_agent = Some(0);
    app.chat_sidebar_index = 1;
    app.agents[0].chat.chat_model = Some("gpt-5.5".to_string());

    app.handle_ui_event(UiEvent::Chat(ChatEvent::ModelListLoaded {
        agent_index: 0,
        models: vec![
            model("gpt-5.4", "GPT-5.4", true, &[]),
            model("gpt-5.5", "GPT-5.5", false, &[]),
        ],
    }));

    let picker = app.model_picker.as_ref().expect("model picker");
    assert_eq!(picker.selected, 1);
    assert_eq!(picker.models.len(), 2);
}

#[test]
fn model_picker_keyboard_navigation_returns_selected_model() {
    let config = CmdexConfig {
        agents: vec![AgentDefinition {
            name: "Test".to_string(),
            workspace: PathBuf::from("/tmp"),
        }],
        ..CmdexConfig::default()
    };
    let mut app = App::new(PathBuf::new(), config);
    app.current_agent = Some(0);
    app.chat_sidebar_index = 1;
    app.model_picker = Some(ModelPickerState {
        agent_index: 0,
        models: vec![
            ModelInfo {
                id: "gpt-5.4".to_string(),
                model: "gpt-5.4".to_string(),
                display_name: "GPT-5.4".to_string(),
                is_default: true,
                supported_reasoning_efforts: Vec::new(),
                default_reasoning_effort: None,
            },
            ModelInfo {
                id: "gpt-5.5".to_string(),
                model: "gpt-5.5".to_string(),
                display_name: "GPT-5.5".to_string(),
                is_default: false,
                supported_reasoning_efforts: Vec::new(),
                default_reasoning_effort: None,
            },
        ],
        selected: 0,
        view: ModelPickerView::Models,
    });

    assert_eq!(
        ChatComponent::handle_model_picker_key(
            &mut app,
            KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
        ),
        ModelPickerAction::Handled
    );
    assert_eq!(
        ChatComponent::handle_model_picker_key(
            &mut app,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        ),
        ModelPickerAction::Apply {
            agent_index: 0,
            model: "gpt-5.5".to_string(),
            effort: None,
        }
    );
    assert!(app.model_picker.is_none());
}

#[test]
fn model_picker_selects_effort_supported_by_selected_model() {
    let config = CmdexConfig {
        agents: vec![AgentDefinition {
            name: "Test".to_string(),
            workspace: PathBuf::from("/tmp"),
        }],
        ..CmdexConfig::default()
    };
    let mut app = App::new(PathBuf::new(), config);
    app.current_agent = Some(0);
    app.chat_sidebar_index = 1;
    app.agents[0].chat.chat_reasoning_effort = Some("low".to_string());
    app.model_picker = Some(ModelPickerState {
        agent_index: 0,
        models: vec![ModelInfo {
            id: "gpt-5.5".to_string(),
            model: "gpt-5.5".to_string(),
            display_name: "GPT-5.5".to_string(),
            is_default: true,
            supported_reasoning_efforts: vec![
                ModelReasoningEffort {
                    reasoning_effort: "low".to_string(),
                    description: Some("Fast".to_string()),
                },
                ModelReasoningEffort {
                    reasoning_effort: "high".to_string(),
                    description: Some("Deep".to_string()),
                },
            ],
            default_reasoning_effort: Some("high".to_string()),
        }],
        selected: 0,
        view: ModelPickerView::Models,
    });

    assert_eq!(
        ChatComponent::handle_model_picker_key(
            &mut app,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        ),
        ModelPickerAction::Handled
    );
    assert!(matches!(
        app.model_picker.as_ref().map(|picker| &picker.view),
        Some(ModelPickerView::Efforts {
            model_index: 0,
            selected: 0
        })
    ));

    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| {
            ChatInputComponent::draw(frame, &app, Rect::new(0, 20, 80, 4));
        })
        .unwrap();
    let buffer = terminal.backend().buffer();
    assert!(buffer_cell_for_text(buffer, "Select effort").is_some());
    assert!(buffer_cell_for_text(buffer, "low - Fast").is_some());

    assert_eq!(
        ChatComponent::handle_model_picker_key(
            &mut app,
            KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
        ),
        ModelPickerAction::Handled
    );
    assert_eq!(
        ChatComponent::handle_model_picker_key(
            &mut app,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        ),
        ModelPickerAction::Apply {
            agent_index: 0,
            model: "gpt-5.5".to_string(),
            effort: Some("high".to_string()),
        }
    );
    assert!(app.model_picker.is_none());
}

#[test]
fn model_picker_renders_model_names() {
    let config = CmdexConfig {
        agents: vec![AgentDefinition {
            name: "Test".to_string(),
            workspace: PathBuf::from("/tmp"),
        }],
        ..CmdexConfig::default()
    };
    let mut app = App::new(PathBuf::new(), config);
    app.current_agent = Some(0);
    app.chat_sidebar_index = 1;
    app.model_picker = Some(ModelPickerState {
        agent_index: 0,
        models: vec![ModelInfo {
            id: "gpt-5.5".to_string(),
            model: "gpt-5.5".to_string(),
            display_name: "GPT-5.5".to_string(),
            is_default: true,
            supported_reasoning_efforts: Vec::new(),
            default_reasoning_effort: None,
        }],
        selected: 0,
        view: ModelPickerView::Models,
    });

    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| {
            ChatInputComponent::draw(frame, &app, Rect::new(0, 20, 80, 4));
        })
        .unwrap();

    let buffer = terminal.backend().buffer();
    assert!(buffer_cell_for_text(buffer, "Select model").is_some());
    assert!(buffer_cell_for_text(buffer, "gpt-5.5").is_some());
    assert!(buffer_cell_for_text(buffer, "GPT-5.5").is_some());
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
fn queued_chat_messages_can_be_iterated_and_canceled() {
    let config = CmdexConfig {
        agents: vec![AgentDefinition {
            name: "Test".to_string(),
            workspace: PathBuf::from("/tmp"),
        }],
        ..CmdexConfig::default()
    };
    let mut app = App::new(PathBuf::new(), config);
    app.current_tab = AppTab::Chat;
    app.current_agent = Some(0);
    app.chat_sidebar_index = 1;

    {
        let agent = app.active_agent_mut().unwrap();
        agent
            .chat
            .enqueue_chat_message("primeira mensagem".to_string());
        agent
            .chat
            .enqueue_chat_message("segunda mensagem".to_string());
        agent
            .chat
            .enqueue_chat_message("terceira mensagem".to_string());
        assert_eq!(agent.chat.queued_chat_count(), 3);
        assert_eq!(agent.chat.selected_queued_chat_index(), Some(0));
    }

    assert!(ChatComponent::handle_queue_key(
        &mut app,
        KeyEvent::new(KeyCode::Down, KeyModifiers::ALT)
    ));
    assert_eq!(
        app.active_agent()
            .unwrap()
            .chat
            .selected_queued_chat_index(),
        Some(1)
    );

    assert!(ChatComponent::handle_queue_key(
        &mut app,
        KeyEvent::new(KeyCode::Down, KeyModifiers::ALT)
    ));
    assert_eq!(
        app.active_agent()
            .unwrap()
            .chat
            .selected_queued_chat_index(),
        Some(2)
    );

    assert!(ChatComponent::handle_queue_key(
        &mut app,
        KeyEvent::new(KeyCode::Backspace, KeyModifiers::ALT)
    ));

    let agent = app.active_agent().unwrap();
    assert_eq!(agent.chat.queued_chat_count(), 2);
    assert_eq!(agent.chat.selected_queued_chat_index(), Some(1));
    assert_eq!(
        agent
            .chat
            .queued_chat_messages()
            .iter()
            .map(|message| message.text.as_str())
            .collect::<Vec<_>>(),
        vec!["primeira mensagem", "segunda mensagem"]
    );
}

#[test]
fn turn_events_track_active_turn_and_interruption_status() {
    let config = CmdexConfig {
        agents: vec![AgentDefinition {
            name: "Test".to_string(),
            workspace: PathBuf::from("/tmp"),
        }],
        ..CmdexConfig::default()
    };
    let mut app = App::new(PathBuf::new(), config);
    app.current_agent = Some(0);
    app.chat_sidebar_index = 1;
    app.agents[0].chat.thread_id = Some("thread-1".to_string());

    app.handle_server_event(ServerEvent::TurnStarted {
        thread_id: "thread-1".to_string(),
        turn_id: "turn-1".to_string(),
    });

    assert_eq!(app.agents[0].chat.active_turn_id.as_deref(), Some("turn-1"));
    assert!(app.agents[0].chat.thinking);

    app.handle_server_event(ServerEvent::TurnCompleted {
        thread_id: "thread-1".to_string(),
        turn_id: "turn-1".to_string(),
        interrupted: true,
    });

    assert_eq!(app.agents[0].chat.active_turn_id, None);
    assert!(!app.agents[0].chat.thinking);
    assert_eq!(app.agents[0].status.as_deref(), Some("Response canceled"));
}

#[test]
fn debounces_repeated_mouse_scroll_events() {
    let mut app = App::new(PathBuf::new(), CmdexConfig::default());
    let now = Instant::now();

    assert!(app.should_handle_mouse_scroll_at_axis(
        ScrollAxis::Vertical,
        ScrollDirection::Down,
        now,
    ));
    assert!(!app.should_handle_mouse_scroll_at_axis(
        ScrollAxis::Vertical,
        ScrollDirection::Down,
        now + Duration::from_millis(10),
    ));
    assert!(app.should_handle_mouse_scroll_at_axis(
        ScrollAxis::Vertical,
        ScrollDirection::Down,
        now + Duration::from_millis(25),
    ));

    let mut other_direction = App::new(PathBuf::new(), CmdexConfig::default());
    assert!(other_direction.should_handle_mouse_scroll_at_axis(
        ScrollAxis::Vertical,
        ScrollDirection::Down,
        now,
    ));
    assert!(other_direction.should_handle_mouse_scroll_at_axis(
        ScrollAxis::Vertical,
        ScrollDirection::Up,
        now + Duration::from_millis(10),
    ));
}

#[test]
fn chat_and_workspace_share_same_mouse_scroll_debounce() {
    let mut chat_app = App::new(PathBuf::new(), CmdexConfig::default());
    let mut workspace_app = App::new(PathBuf::new(), CmdexConfig::default());
    let now = Instant::now();
    workspace_app.current_tab = AppTab::Workspace;

    assert!(chat_app.should_handle_mouse_scroll_at_axis(
        ScrollAxis::Vertical,
        ScrollDirection::Down,
        now,
    ));
    assert!(!chat_app.should_handle_mouse_scroll_at_axis(
        ScrollAxis::Vertical,
        ScrollDirection::Down,
        now + Duration::from_millis(10),
    ));
    assert!(chat_app.should_handle_mouse_scroll_at_axis(
        ScrollAxis::Vertical,
        ScrollDirection::Down,
        now + Duration::from_millis(25),
    ));

    assert!(workspace_app.should_handle_mouse_scroll_at_axis(
        ScrollAxis::Vertical,
        ScrollDirection::Down,
        now,
    ));
    assert!(!workspace_app.should_handle_mouse_scroll_at_axis(
        ScrollAxis::Vertical,
        ScrollDirection::Down,
        now + Duration::from_millis(10),
    ));
    assert!(workspace_app.should_handle_mouse_scroll_at_axis(
        ScrollAxis::Vertical,
        ScrollDirection::Down,
        now + Duration::from_millis(25),
    ));
}

#[test]
fn horizontal_mouse_scroll_uses_independent_debounce_axis() {
    let mut app = App::new(PathBuf::new(), CmdexConfig::default());
    let now = Instant::now();

    assert!(app.should_handle_mouse_scroll_at_axis(
        ScrollAxis::Vertical,
        ScrollDirection::Down,
        now,
    ));
    assert!(app.should_handle_mouse_scroll_at_axis(
        ScrollAxis::Horizontal,
        ScrollDirection::Down,
        now + Duration::from_millis(10)
    ));
    assert!(!app.should_handle_mouse_scroll_at_axis(
        ScrollAxis::Horizontal,
        ScrollDirection::Down,
        now + Duration::from_millis(15)
    ));
}

#[test]
fn workspace_tree_refreshes_after_filesystem_notification() {
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
        ..CmdexConfig::default()
    };
    let mut app = App::new(PathBuf::new(), config);
    app.current_tab = AppTab::Workspace;
    app.current_agent = Some(0);
    app.chat_sidebar_index = 1;
    TopNavigationComponent::refresh_current_tab(&mut app);

    fs::write(root.join("beta.txt"), "beta").unwrap();
    app.workspace_refresh_in_flight.remove(&0);

    app.handle_ui_event(UiEvent::Workspace(WorkspaceEvent::FilesystemChanged {
        agent_index: 0,
    }));
    assert!(app.workspace_refresh_in_flight.contains(&0));
    let entries = FileBrowserState::scan_entries(&root).unwrap();
    app.handle_ui_event(UiEvent::Workspace(WorkspaceEvent::EntriesLoaded {
        agent_index: 0,
        entries,
        error: None,
    }));

    assert!(
        app.active_agent()
            .unwrap()
            .workspace
            .tree_rows
            .iter()
            .map(|row| row.label.as_str())
            .any(|label| label.contains("beta.txt"))
    );
    assert!(!app.workspace_refresh_in_flight.contains(&0));

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
        ..CmdexConfig::default()
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

    let handled = WorkspaceScreen::handle_key(
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
        ..CmdexConfig::default()
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
        ..CmdexConfig::default()
    };
    let mut app = App::new(PathBuf::new(), config);
    app.current_tab = AppTab::Workspace;
    app.current_agent = Some(0);
    app.chat_sidebar_index = 1;
    TopNavigationComponent::refresh_current_tab(&mut app);
    let _ = app.take_effects();

    {
        let workspace = &mut app.active_agent_mut().unwrap().workspace;
        workspace.select(0);
        workspace.open_editor().unwrap();
        assert!(workspace.editor_focused());
    }

    assert!(WorkspaceScreen::handle_key(
        &mut app,
        KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE),
        Rect::new(0, 0, 120, 40),
    ));
    assert!(app.active_agent().unwrap().workspace.sidebar_focused());

    let handled = WorkspaceScreen::handle_key(
        &mut app,
        KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
        Rect::new(0, 0, 120, 40),
    );

    assert!(handled);
    let workspace = &app.active_agent().unwrap().workspace;
    assert_eq!(workspace.selected, 1);
    assert!(workspace.sidebar_focused());
    assert!(workspace.editor.is_none());
    assert!(matches!(
        app.take_effects().as_slice(),
        [AppEffect::OpenWorkspaceEditor {
            path,
            position: None,
            ..
        }] if path == &zeta
    ));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn workspace_ctrl_space_requests_editor_completion() {
    let root = std::env::temp_dir().join(format!(
        "cmdex-app-workspace-completion-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&root).unwrap();
    let main = root.join("main.rs");
    fs::write(&main, "gre\n").unwrap();

    let config = CmdexConfig {
        agents: vec![AgentDefinition {
            name: "Test".to_string(),
            workspace: root.clone(),
        }],
        ..CmdexConfig::default()
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
        editor.move_right();
        editor.move_right();
        editor.move_right();
    }

    let (ui_tx, _ui_rx) = tokio::sync::mpsc::unbounded_channel();
    let handled = WorkspaceScreen::handle_completion_request(
        &mut app,
        KeyEvent::new(KeyCode::Char(' '), KeyModifiers::CONTROL),
        &ui_tx,
    );

    assert!(handled);
    assert!(
        app.active_agent_mut()
            .unwrap()
            .workspace
            .editor
            .as_mut()
            .unwrap()
            .resolve_completion(EditorPosition { row: 0, col: 3 }, Vec::new())
    );

    app.shutdown_lsp_sessions();
    let _ = fs::remove_dir_all(root);
}

#[test]
fn workspace_ctrl_h_toggles_shortcuts_popup() {
    let root = std::env::temp_dir().join(format!(
        "cmdex-app-workspace-shortcuts-toggle-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&root).unwrap();
    let alpha = root.join("alpha.rs");
    fs::write(&alpha, "fn main() {}\n").unwrap();

    let config = CmdexConfig {
        agents: vec![AgentDefinition {
            name: "Test".to_string(),
            workspace: root.clone(),
        }],
        ..CmdexConfig::default()
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

    assert!(WorkspaceScreen::handle_key(
        &mut app,
        KeyEvent::new(KeyCode::Char('h'), KeyModifiers::CONTROL),
        Rect::new(0, 0, 120, 40),
    ));
    assert!(
        app.active_agent()
            .unwrap()
            .workspace
            .editor
            .as_ref()
            .unwrap()
            .shortcuts_help_open()
    );

    assert!(WorkspaceScreen::handle_key(
        &mut app,
        KeyEvent::new(KeyCode::Char('h'), KeyModifiers::CONTROL),
        Rect::new(0, 0, 120, 40),
    ));
    assert!(
        !app.active_agent()
            .unwrap()
            .workspace
            .editor
            .as_ref()
            .unwrap()
            .shortcuts_help_open()
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn workspace_lsp_startup_activates_spinner_and_status_label() {
    let root = std::env::temp_dir().join(format!(
        "cmdex-app-workspace-lsp-loading-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&root).unwrap();
    let alpha = root.join("alpha.rs");
    fs::write(&alpha, "fn main() {}\n").unwrap();

    let config = CmdexConfig {
        agents: vec![AgentDefinition {
            name: "Test".to_string(),
            workspace: root.clone(),
        }],
        ..CmdexConfig::default()
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

    let (command_tx, _command_rx) = std::sync::mpsc::channel();
    app.lsp_runtimes.insert(
        LspRuntimeKey {
            agent_index: 0,
            server_index: 0,
        },
        LspRuntime {
            command_tx,
            server_name: "rust-analyzer".to_string(),
            starting: true,
        },
    );

    assert_eq!(app.tick_interval(), FAST_TICK_INTERVAL);
    assert_eq!(
        app.active_workspace_lsp_loading_label().as_deref(),
        Some("⠏ rust-analyzer")
    );

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
        ..CmdexConfig::default()
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
        editor.enter_normal_mode();
        assert_eq!(editor.lines, vec!["!hello".to_string()]);
        assert!(editor.dirty);
    }

    let handled = WorkspaceScreen::handle_key(
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
        ..CmdexConfig::default()
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
        let snapshot = FileBrowserState::search_entries(workspace.entries(), "needle").unwrap();
        let generation = workspace.search_generation();
        assert!(workspace.apply_search_snapshot(generation, "needle", snapshot));
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
        ..CmdexConfig::default()
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
    assert!(matches!(editor.mode, EditorMode::Visual { .. }));
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
        ..CmdexConfig::default()
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
    assert!(matches!(editor.mode, EditorMode::Normal));
    assert!(!editor.has_selection());
    assert_eq!(editor.cursor_row, 0);
    assert_eq!(editor.cursor_col, 3);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn workspace_mouse_scroll_over_completion_popover_moves_completion_selection() {
    let root = std::env::temp_dir().join(format!(
        "cmdex-app-workspace-completion-scroll-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&root).unwrap();
    let alpha = root.join("alpha.rs");
    fs::write(&alpha, "gre\n").unwrap();

    let config = CmdexConfig {
        agents: vec![AgentDefinition {
            name: "Test".to_string(),
            workspace: root.clone(),
        }],
        ..CmdexConfig::default()
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
        let position = EditorPosition { row: 0, col: 3 };
        editor.request_completion(position);
        assert!(
            editor.resolve_completion(
                position,
                (0..12)
                    .map(|index| EditorCompletionItem {
                        label: format!("item-{index:02}"),
                        detail: None,
                        insert_text: format!("item_{index:02}"),
                        replace_start: EditorPosition { row: 0, col: 0 },
                        replace_end: EditorPosition { row: 0, col: 3 },
                        preselected: index == 0,
                    })
                    .collect()
            )
        );
    }

    let area = Rect::new(0, 0, 80, 20);
    let layout = app.compute_layout(area);
    let popup_area = WorkspaceEditorComponent::completion_popover_area(
        app.active_agent()
            .unwrap()
            .workspace
            .editor
            .as_ref()
            .unwrap(),
        layout.body,
    )
    .expect("completion popover area");
    let (ui_tx, _ui_rx) = tokio::sync::mpsc::unbounded_channel();

    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: popup_area.x.saturating_add(1),
            row: popup_area.y.saturating_add(1),
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
    let (_, selected, _) = editor.completion_popover().expect("completion popover");
    assert_eq!(selected, 1);
    assert_eq!(editor.vertical_scroll, 0);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn workspace_mouse_clicks_shortcuts_popup_close_button() {
    let root = std::env::temp_dir().join(format!(
        "cmdex-app-workspace-shortcuts-close-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&root).unwrap();
    let alpha = root.join("alpha.rs");
    fs::write(&alpha, "fn main() {}\n").unwrap();

    let config = CmdexConfig {
        agents: vec![AgentDefinition {
            name: "Test".to_string(),
            workspace: root.clone(),
        }],
        ..CmdexConfig::default()
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
        workspace.editor.as_mut().unwrap().toggle_shortcuts_help();
    }

    let area = Rect::new(0, 0, 120, 40);
    let layout = app.compute_layout(area);
    let close_button = WorkspaceEditorComponent::shortcuts_popup_close_button_area(layout.body);
    let (ui_tx, _ui_rx) = tokio::sync::mpsc::unbounded_channel();

    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: close_button.x.saturating_add(1),
            row: close_button.y.saturating_add(1),
            modifiers: KeyModifiers::NONE,
        },
        area,
        &ui_tx,
    );

    assert!(
        !app.active_agent()
            .unwrap()
            .workspace
            .editor
            .as_ref()
            .unwrap()
            .shortcuts_help_open()
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn workspace_mouse_drag_on_completion_popover_scrollbar_updates_window() {
    let root = std::env::temp_dir().join(format!(
        "cmdex-app-workspace-completion-drag-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&root).unwrap();
    let alpha = root.join("alpha.rs");
    fs::write(&alpha, "gre\n").unwrap();

    let config = CmdexConfig {
        agents: vec![AgentDefinition {
            name: "Test".to_string(),
            workspace: root.clone(),
        }],
        ..CmdexConfig::default()
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
        let position = EditorPosition { row: 0, col: 3 };
        editor.request_completion(position);
        assert!(
            editor.resolve_completion(
                position,
                (0..12)
                    .map(|index| EditorCompletionItem {
                        label: format!("item-{index:02}"),
                        detail: None,
                        insert_text: format!("item_{index:02}"),
                        replace_start: EditorPosition { row: 0, col: 0 },
                        replace_end: EditorPosition { row: 0, col: 3 },
                        preselected: index == 0,
                    })
                    .collect()
            )
        );
    }

    let area = Rect::new(0, 0, 80, 20);
    let layout = app.compute_layout(area);
    let metrics = WorkspaceEditorComponent::completion_popover_scrollbar_metrics(
        app.active_agent()
            .unwrap()
            .workspace
            .editor
            .as_ref()
            .unwrap(),
        layout.body,
    )
    .expect("completion popover scrollbar metrics");
    let track_x = metrics.track.x;
    let track_top = metrics.track.y;
    let track_bottom = metrics.track.y + metrics.track.height.saturating_sub(1);
    let (ui_tx, _ui_rx) = tokio::sync::mpsc::unbounded_channel();

    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: track_x,
            row: track_top,
            modifiers: KeyModifiers::NONE,
        },
        area,
        &ui_tx,
    );
    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: track_x,
            row: track_bottom,
            modifiers: KeyModifiers::NONE,
        },
        area,
        &ui_tx,
    );
    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: track_x,
            row: track_bottom,
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
    let (_, selected, _) = editor.completion_popover().expect("completion popover");
    assert_eq!(editor.completion_window_start(8), 4);
    assert_eq!(selected, 4);
    assert_eq!(editor.vertical_scroll, 0);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn workspace_shift_scroll_moves_editor_horizontally() {
    let root = std::env::temp_dir().join(format!(
        "cmdex-app-workspace-shift-scroll-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&root).unwrap();
    let alpha = root.join("alpha.txt");
    fs::write(
        &alpha,
        "0123456789abcdefghijklmnopqrstuvwxyz0123456789abcdefghijklmnopqrstuvwxyz\n",
    )
    .unwrap();

    let config = CmdexConfig {
        agents: vec![AgentDefinition {
            name: "Test".to_string(),
            workspace: root.clone(),
        }],
        ..CmdexConfig::default()
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

    let area = Rect::new(0, 0, 40, 20);
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
            kind: MouseEventKind::ScrollDown,
            column,
            row,
            modifiers: KeyModifiers::SHIFT,
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
    assert_eq!(editor.vertical_scroll, 0);
    assert!(editor.horizontal_scroll > 0);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn workspace_editor_hover_popover_stays_inside_viewport() {
    let code_area = Rect::new(10, 5, 40, 12);

    let popup = WorkspaceEditorComponent::hover_popover_area(code_area, 47, 15, 20, 6);

    assert_eq!(popup, Rect::new(28, 10, 20, 6));
}

#[test]
fn workspace_editor_shortcuts_popup_renders_content_and_close_button() {
    let root = std::env::temp_dir().join(format!(
        "cmdex-workspace-editor-shortcuts-popup-{}-{}.rs",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::write(&root, "fn main() {}\n").unwrap();
    let mut editor = WorkspaceEditorState::open(&root).unwrap();
    editor.toggle_shortcuts_help();

    let backend = TestBackend::new(100, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| {
            WorkspaceEditorComponent::draw(frame, &editor, Rect::new(0, 0, 100, 24), true, None);
        })
        .unwrap();

    let buffer = terminal.backend().buffer();
    assert!(buffer_cell_for_text(buffer, "Shortcuts").is_some());
    assert!(buffer_cell_for_text(buffer, "Ctrl+H").is_some());
    assert!(buffer_cell_for_text(buffer, "Close").is_some());

    let _ = fs::remove_file(root);
}

#[test]
fn workspace_editor_renders_lsp_loading_status_in_bottom_right() {
    let root = std::env::temp_dir().join(format!(
        "cmdex-workspace-editor-lsp-status-{}-{}.rs",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::write(&root, "fn main() {}\n").unwrap();
    let editor = WorkspaceEditorState::open(&root).unwrap();

    let backend = TestBackend::new(80, 20);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| {
            WorkspaceEditorComponent::draw(
                frame,
                &editor,
                Rect::new(0, 0, 80, 20),
                true,
                Some("⠏ rust-analyzer"),
            );
        })
        .unwrap();

    let buffer = terminal.backend().buffer();
    let (x, y) = buffer_text_position(buffer, "⠏ rust-analyzer").expect("loading status");

    assert!(x > 55);
    assert!(y > 15);

    let _ = fs::remove_file(root);
}

#[test]
fn workspace_editor_footer_no_longer_renders_shortcut_hints() {
    let root = std::env::temp_dir().join(format!(
        "cmdex-workspace-editor-footer-status-{}-{}.rs",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::write(&root, "fn main() {}\n").unwrap();
    let editor = WorkspaceEditorState::open(&root).unwrap();

    let backend = TestBackend::new(100, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| {
            WorkspaceEditorComponent::draw(frame, &editor, Rect::new(0, 0, 100, 24), true, None);
        })
        .unwrap();

    let buffer = terminal.backend().buffer();
    assert!(buffer_cell_for_text(buffer, "NORMAL").is_some());
    assert!(buffer_cell_for_text(buffer, "Ctrl+Space autocomplete").is_none());

    let _ = fs::remove_file(root);
}

#[test]
fn workspace_editor_completion_popover_renders_scrollbar_for_long_lists() {
    let root = std::env::temp_dir().join(format!(
        "cmdex-completion-popover-scrollbar-{}-{}.rs",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::write(&root, "gre\n").unwrap();
    let mut editor = WorkspaceEditorState::open(&root).unwrap();
    let position = EditorPosition { row: 0, col: 3 };
    editor.request_completion(position);
    assert!(
        editor.resolve_completion(
            position,
            (0..12)
                .map(|index| EditorCompletionItem {
                    label: format!("item-{index:02}"),
                    detail: None,
                    insert_text: format!("item_{index:02}"),
                    replace_start: EditorPosition { row: 0, col: 0 },
                    replace_end: EditorPosition { row: 0, col: 3 },
                    preselected: index == 9,
                })
                .collect()
        )
    );
    let backend = TestBackend::new(80, 20);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| {
            WorkspaceEditorComponent::draw(frame, &editor, Rect::new(0, 0, 80, 20), true, None);
        })
        .unwrap();

    let buffer = terminal.backend().buffer();
    assert!(buffer_cell_for_text(buffer, "item-09").is_some());
    assert!(buffer_cell_for_text(buffer, "item-00").is_none());
    assert!(buffer_cell_for_text(buffer, "█").is_some());

    let _ = fs::remove_file(root);
}

#[test]
fn workspace_editor_hover_popover_preserves_syntax_highlighting() {
    let root = std::env::temp_dir().join(format!(
        "cmdex-hover-popover-highlight-{}-{}.rs",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::write(&root, "let placeholder = 1;\n").unwrap();
    let mut editor = WorkspaceEditorState::open(&root).unwrap();
    let hover = "```rust\nfn greet(name: &str) -> String\n```";
    let position = EditorPosition { row: 0, col: 3 };
    assert!(editor.request_hover(position));
    assert!(editor.resolve_hover(position, Some(hover.to_string())));

    let backend = TestBackend::new(80, 20);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| {
            WorkspaceEditorComponent::draw(frame, &editor, Rect::new(0, 0, 80, 20), true, None);
        })
        .unwrap();

    let buffer = terminal.backend().buffer();
    let found_highlighted_cell =
        buffer_contains_highlighted_text(buffer, "fn greet", ThemeRegistry::app().foreground);

    assert!(found_highlighted_cell);

    let _ = fs::remove_file(root);
}

#[test]
fn workspace_editor_hover_popover_highlights_indented_markdown_code_blocks() {
    let root = std::env::temp_dir().join(format!(
        "cmdex-hover-popover-indented-{}-{}.rs",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::write(&root, "let placeholder = 1;\n").unwrap();
    let mut editor = WorkspaceEditorState::open(&root).unwrap();
    let hover =
        super::lsp::summarize_hover_text("Example:\n\n    fn greet(name: &str) -> String\n")
            .expect("hover summary");
    let position = EditorPosition { row: 0, col: 3 };
    assert!(editor.request_hover(position));
    assert!(editor.resolve_hover(position, Some(hover)));

    let backend = TestBackend::new(80, 20);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| {
            WorkspaceEditorComponent::draw(frame, &editor, Rect::new(0, 0, 80, 20), true, None);
        })
        .unwrap();

    let buffer = terminal.backend().buffer();
    let found_highlighted_cell =
        buffer_contains_highlighted_text(buffer, "fn greet", ThemeRegistry::app().foreground);

    assert!(found_highlighted_cell);

    let _ = fs::remove_file(root);
}

#[test]
fn workspace_editor_hover_popover_uses_editor_syntax_for_unlabeled_code_blocks() {
    let root = std::env::temp_dir().join(format!(
        "cmdex-hover-popover-unlabeled-{}-{}.rs",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::write(&root, "let placeholder = 1;\n").unwrap();
    let mut editor = WorkspaceEditorState::open(&root).unwrap();
    let hover = "```\nfn greet(name: &str) -> String\n```";
    let position = EditorPosition { row: 0, col: 3 };
    assert!(editor.request_hover(position));
    assert!(editor.resolve_hover(position, Some(hover.to_string())));

    let backend = TestBackend::new(80, 20);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| {
            WorkspaceEditorComponent::draw(frame, &editor, Rect::new(0, 0, 80, 20), true, None);
        })
        .unwrap();

    let buffer = terminal.backend().buffer();
    let found_highlighted_cell =
        buffer_contains_highlighted_text(buffer, "fn greet", ThemeRegistry::app().foreground);

    assert!(found_highlighted_cell);

    let _ = fs::remove_file(root);
}

#[test]
fn workspace_editor_hover_popover_prioritizes_editor_file_extension() {
    let root = std::env::temp_dir().join(format!(
        "cmdex-hover-popover-extension-priority-{}-{}.rs",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::write(&root, "let placeholder = 1;\n").unwrap();
    let mut editor = WorkspaceEditorState::open(&root).unwrap();
    let hover = "```typescript\nfn greet(name: &str) -> String\n```";
    let position = EditorPosition { row: 0, col: 3 };
    assert!(editor.request_hover(position));
    assert!(editor.resolve_hover(position, Some(hover.to_string())));

    let backend = TestBackend::new(80, 20);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| {
            WorkspaceEditorComponent::draw(frame, &editor, Rect::new(0, 0, 80, 20), true, None);
        })
        .unwrap();

    let buffer = terminal.backend().buffer();
    let keyword_color = ThemeRegistry::app().accent;
    let keyword_cell = buffer_cell_for_text(buffer, "fn greet").expect("hover text should render");

    assert_eq!(keyword_cell.fg, keyword_color);

    let _ = fs::remove_file(root);
}

fn buffer_contains_highlighted_text(
    buffer: &ratatui::buffer::Buffer,
    needle: &str,
    default_fg: Color,
) -> bool {
    buffer_cell_for_text(buffer, needle).is_some_and(|cell| cell.fg != default_fg)
}

fn buffer_text_position(buffer: &ratatui::buffer::Buffer, needle: &str) -> Option<(u16, u16)> {
    let needle = needle.chars().collect::<Vec<_>>();
    for y in 0..buffer.area.height {
        let mut row = Vec::with_capacity(buffer.area.width as usize);
        for x in 0..buffer.area.width {
            row.push(buffer[(x, y)].symbol().chars().next().unwrap_or(' '));
        }

        let Some(start) = row
            .windows(needle.len())
            .position(|window| window == needle.as_slice())
        else {
            continue;
        };

        return Some((start as u16, y));
    }

    None
}

fn buffer_cell_for_text<'a>(
    buffer: &'a ratatui::buffer::Buffer,
    needle: &str,
) -> Option<&'a ratatui::buffer::Cell> {
    let (x, y) = buffer_text_position(buffer, needle)?;
    Some(&buffer[(x, y)])
}
