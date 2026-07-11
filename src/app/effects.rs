use super::{
    ChatEvent, GitEvent, LspEvent, ShellEvent, UiEvent, WorkspaceEvent, chat::ChatSupport,
    event_types, *,
};
use crate::workspace::GitRepository;
use arboard::Clipboard;
use tokio::process::Command;

pub(super) enum AppEffect {
    StopWorkspaceWatcher {
        stop_tx: std::sync::mpsc::Sender<()>,
    },
    StartWorkspaceWatcher {
        agent_index: usize,
        root: PathBuf,
    },
    StopShellSession {
        pid: u32,
    },
    StopLspSession {
        command_tx: std::sync::mpsc::Sender<lsp::LspCommand>,
    },
    StartShellSession {
        agent_index: usize,
        session_id: usize,
        workspace: PathBuf,
    },
    SendShellCommand {
        agent_index: usize,
        session_id: usize,
        command_tx: std::sync::mpsc::Sender<String>,
        payload: String,
    },
    StartLspSession {
        agent_index: usize,
        server_index: usize,
        workspace: PathBuf,
        server: LspServerConfig,
        command: lsp::LspCommand,
    },
    SendLspCommand {
        agent_index: usize,
        server_index: usize,
        command_tx: std::sync::mpsc::Sender<lsp::LspCommand>,
        command: lsp::LspCommand,
    },
    RunShell {
        agent_index: usize,
        command: String,
        workspace: PathBuf,
    },
    RunGitRemote {
        agent_index: usize,
        root: PathBuf,
        action: GitRemoteAction,
    },
    RunGitMutation {
        agent_index: usize,
        root: PathBuf,
        mutation: GitMutation,
    },
    RefreshGitDiff {
        agent_index: usize,
        root: PathBuf,
        active_section: DiffSection,
        selected_path: Option<PathBuf>,
        generation: u64,
    },
    RefreshWorkspace {
        agent_index: usize,
        root: PathBuf,
    },
    SearchWorkspace {
        agent_index: usize,
        entries: Vec<crate::workspace::FileEntry>,
        query: String,
        generation: u64,
    },
    OpenWorkspaceEditor {
        agent_index: usize,
        path: PathBuf,
        position: Option<EditorPosition>,
    },
    LoadWorkspacePreview {
        agent_index: usize,
        path: PathBuf,
    },
    CopyToClipboard {
        agent_index: usize,
        path: PathBuf,
        text: String,
    },
    PasteFromClipboard {
        agent_index: usize,
        path: PathBuf,
    },
    StartChatTurn {
        agent_index: usize,
        text: String,
        existing_thread: Option<String>,
        thread_loaded: bool,
        selected_model: Option<String>,
        selected_effort: Option<String>,
        workspace: PathBuf,
    },
    ListModels {
        agent_index: usize,
    },
    InterruptTurn {
        agent_index: usize,
        thread_id: String,
        turn_id: String,
    },
}

pub(super) fn spawn(
    effect: AppEffect,
    codex: CodexAppServer,
    ui_tx: mpsc::UnboundedSender<UiEvent>,
) {
    match effect {
        AppEffect::StopWorkspaceWatcher { stop_tx } => {
            let _ = stop_tx.send(());
        }
        AppEffect::StartWorkspaceWatcher { agent_index, root } => {
            if let Err(error) =
                super::workspace_watcher::WorkspaceWatcher::spawn(agent_index, root, ui_tx.clone())
            {
                event_types::send(
                    &ui_tx,
                    WorkspaceEvent::WatcherFailed {
                        agent_index,
                        message: error.to_string(),
                    },
                );
            }
        }
        AppEffect::StopShellSession { pid } => {
            tokio::spawn(async move {
                let _ = tokio::task::spawn_blocking(move || {
                    std::process::Command::new("kill")
                        .args(["-TERM", &pid.to_string()])
                        .status()
                })
                .await;
            });
        }
        AppEffect::StopLspSession { command_tx } => {
            tokio::spawn(async move {
                let _ = command_tx.send(lsp::LspCommand::Shutdown);
            });
        }
        AppEffect::StartShellSession {
            agent_index,
            session_id,
            workspace,
        } => {
            tokio::spawn(async move {
                let shell_path = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
                let worker_tx = ui_tx.clone();
                let result = tokio::task::spawn_blocking(move || {
                    super::shell::ShellRuntimeFactory::spawn(
                        &shell_path,
                        &workspace,
                        agent_index,
                        session_id,
                        worker_tx,
                    )
                })
                .await;
                match result {
                    Ok(Ok((command_tx, pid))) => {
                        event_types::send(
                            &ui_tx,
                            ShellEvent::RuntimeReady {
                                agent_index,
                                session_id,
                                command_tx,
                                pid,
                            },
                        );
                    }
                    Ok(Err(error)) => {
                        event_types::send(
                            &ui_tx,
                            ShellEvent::SessionExited {
                                agent_index,
                                session_id,
                                message: error.to_string(),
                            },
                        );
                    }
                    Err(error) => {
                        event_types::send(
                            &ui_tx,
                            ShellEvent::SessionExited {
                                agent_index,
                                session_id,
                                message: format!("shell startup task failed: {error}"),
                            },
                        );
                    }
                }
            });
        }
        AppEffect::SendShellCommand {
            agent_index,
            session_id,
            command_tx,
            payload,
        } => {
            tokio::spawn(async move {
                if let Err(error) = command_tx.send(payload) {
                    event_types::send(
                        &ui_tx,
                        ShellEvent::SessionExited {
                            agent_index,
                            session_id,
                            message: format!("Failed to send command to shell: {error}"),
                        },
                    );
                }
            });
        }
        AppEffect::StartLspSession {
            agent_index,
            server_index,
            workspace,
            server,
            command,
        } => {
            tokio::spawn(async move {
                let worker_tx = ui_tx.clone();
                let server_name = server.name.clone();
                let result = tokio::task::spawn_blocking(move || {
                    lsp::LspRuntimeFactory::spawn(&workspace, server, agent_index, worker_tx)
                })
                .await;
                match result {
                    Ok(Ok(command_tx)) => {
                        if let Err(error) = command_tx.send(command) {
                            event_types::send(
                                &ui_tx,
                                LspEvent::RuntimeFailed {
                                    agent_index,
                                    server_index,
                                    message: format!("Failed to send LSP request: {error:?}"),
                                },
                            );
                        } else {
                            event_types::send(
                                &ui_tx,
                                LspEvent::RuntimeReady {
                                    agent_index,
                                    server_index,
                                    server_name,
                                    command_tx,
                                },
                            );
                        }
                    }
                    Ok(Err(error)) => {
                        event_types::send(
                            &ui_tx,
                            LspEvent::RuntimeFailed {
                                agent_index,
                                server_index,
                                message: error.to_string(),
                            },
                        );
                    }
                    Err(error) => {
                        event_types::send(
                            &ui_tx,
                            LspEvent::RuntimeFailed {
                                agent_index,
                                server_index,
                                message: format!("LSP startup task failed: {error}"),
                            },
                        );
                    }
                }
            });
        }
        AppEffect::SendLspCommand {
            agent_index,
            server_index,
            command_tx,
            command,
        } => {
            tokio::spawn(async move {
                if let Err(error) = command_tx.send(command) {
                    event_types::send(
                        &ui_tx,
                        LspEvent::RuntimeFailed {
                            agent_index,
                            server_index,
                            message: format!("Failed to send LSP request: {error:?}"),
                        },
                    );
                }
            });
        }
        AppEffect::RunShell {
            agent_index,
            command,
            workspace,
        } => {
            tokio::spawn(async move {
                let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
                let result = Command::new(shell)
                    .arg("-c")
                    .arg(&command)
                    .current_dir(&workspace)
                    .output()
                    .await;

                let (output, success) = match result {
                    Ok(output) => {
                        let stdout = String::from_utf8_lossy(&output.stdout);
                        let stderr = String::from_utf8_lossy(&output.stderr);
                        (
                            ChatSupport::format_shell_output(
                                &command,
                                &stdout,
                                &stderr,
                                output.status.code(),
                                output.status.success(),
                            ),
                            output.status.success(),
                        )
                    }
                    Err(error) => (
                        format!(
                            "```text\n{}\n```\n\nExit code: unavailable",
                            ChatSupport::truncate_shell_text(&error.to_string())
                        ),
                        false,
                    ),
                };

                event_types::send(
                    &ui_tx,
                    ShellEvent::CommandCompleted {
                        agent_index,
                        output,
                        success,
                    },
                );
            });
        }
        AppEffect::RunGitRemote {
            agent_index,
            root,
            action,
        } => {
            tokio::spawn(async move {
                let result = tokio::task::spawn_blocking(move || {
                    GitRepository::new(&root).run_remote_action(action)
                })
                .await;
                let (success, message) = match result {
                    Ok(Ok(output)) => (true, output),
                    Ok(Err(error)) => (false, error.to_string()),
                    Err(error) => (false, format!("git remote task failed: {error}")),
                };

                event_types::send(
                    &ui_tx,
                    GitEvent::RemoteCompleted {
                        agent_index,
                        action,
                        success,
                        message,
                    },
                );
            });
        }
        AppEffect::RunGitMutation {
            agent_index,
            root,
            mutation,
        } => {
            tokio::spawn(async move {
                let operation = mutation.clone();
                let result = tokio::task::spawn_blocking(move || {
                    GitRepository::new(&root).run_mutation(&mutation)
                })
                .await;
                let (success, message) = match result {
                    Ok(Ok(output)) => (true, output),
                    Ok(Err(error)) => (false, error.to_string()),
                    Err(error) => (false, format!("git mutation task failed: {error}")),
                };
                event_types::send(
                    &ui_tx,
                    GitEvent::MutationCompleted {
                        agent_index,
                        mutation: operation,
                        success,
                        message,
                    },
                );
            });
        }
        AppEffect::RefreshGitDiff {
            agent_index,
            root,
            active_section,
            selected_path,
            generation,
        } => {
            tokio::spawn(async move {
                let result = tokio::task::spawn_blocking(move || {
                    GitRepository::new(&root)
                        .load_snapshot(active_section, selected_path.as_deref())
                })
                .await;
                let (result, error) = match result {
                    Ok(Ok(result)) => (Some(result), None),
                    Ok(Err(error)) => (None, Some(error.to_string())),
                    Err(error) => (None, Some(format!("git refresh task failed: {error}"))),
                };
                event_types::send(
                    &ui_tx,
                    GitEvent::Loaded {
                        agent_index,
                        generation,
                        result,
                        error,
                    },
                );
            });
        }
        AppEffect::RefreshWorkspace { agent_index, root } => {
            tokio::spawn(async move {
                let result =
                    tokio::task::spawn_blocking(move || FileBrowserState::scan_entries(&root))
                        .await;
                let (entries, error) = match result {
                    Ok(Ok(entries)) => (entries, None),
                    Ok(Err(error)) => (Vec::new(), Some(error.to_string())),
                    Err(error) => (Vec::new(), Some(format!("workspace scan failed: {error}"))),
                };
                event_types::send(
                    &ui_tx,
                    WorkspaceEvent::EntriesLoaded {
                        agent_index,
                        entries,
                        error,
                    },
                );
            });
        }
        AppEffect::SearchWorkspace {
            agent_index,
            entries,
            query,
            generation,
        } => {
            tokio::spawn(async move {
                let requested_query = query.clone();
                let result = tokio::task::spawn_blocking(move || {
                    FileBrowserState::search_entries(&entries, &requested_query)
                })
                .await;
                let (snapshot, error) = match result {
                    Ok(Ok(snapshot)) => (snapshot, None),
                    Ok(Err(error)) => (WorkspaceSearchSnapshot::default(), Some(error.to_string())),
                    Err(error) => (
                        WorkspaceSearchSnapshot::default(),
                        Some(format!("workspace search failed: {error}")),
                    ),
                };
                event_types::send(
                    &ui_tx,
                    WorkspaceEvent::SearchCompleted {
                        agent_index,
                        generation,
                        query,
                        snapshot,
                        error,
                    },
                );
            });
        }
        AppEffect::OpenWorkspaceEditor {
            agent_index,
            path,
            position,
        } => {
            tokio::spawn(async move {
                let requested_path = path.clone();
                let result = tokio::task::spawn_blocking(move || {
                    crate::workspace::WorkspaceEditorState::read_source(&requested_path)
                })
                .await;
                let (source, error) = match result {
                    Ok(Ok(source)) => (Some(source), None),
                    Ok(Err(error)) => (None, Some(error.to_string())),
                    Err(error) => (None, Some(format!("editor load failed: {error}"))),
                };
                event_types::send(
                    &ui_tx,
                    WorkspaceEvent::EditorLoaded {
                        agent_index,
                        path,
                        position,
                        source,
                        error,
                    },
                );
            });
        }
        AppEffect::LoadWorkspacePreview { agent_index, path } => {
            tokio::spawn(async move {
                let requested_path = path.clone();
                let result = tokio::task::spawn_blocking(move || {
                    FileBrowserState::load_preview(&requested_path)
                })
                .await;
                let (preview, error) = match result {
                    Ok(Ok(preview)) => (preview, None),
                    Ok(Err(error)) => (Vec::new(), Some(error.to_string())),
                    Err(error) => (Vec::new(), Some(format!("preview load failed: {error}"))),
                };
                event_types::send(
                    &ui_tx,
                    WorkspaceEvent::PreviewLoaded {
                        agent_index,
                        path,
                        preview,
                        error,
                    },
                );
            });
        }
        AppEffect::CopyToClipboard {
            agent_index,
            path,
            text,
        } => {
            tokio::spawn(async move {
                let result = tokio::task::spawn_blocking(move || {
                    Clipboard::new().and_then(|mut clipboard| clipboard.set_text(text))
                })
                .await;
                let error = match result {
                    Ok(Ok(())) => None,
                    Ok(Err(error)) => Some(format!("Copy failed: {error}")),
                    Err(error) => Some(format!("clipboard task failed: {error}")),
                };
                event_types::send(
                    &ui_tx,
                    WorkspaceEvent::ClipboardCompleted {
                        agent_index,
                        path,
                        operation: ClipboardOperation::Copy,
                        text: None,
                        error,
                    },
                );
            });
        }
        AppEffect::PasteFromClipboard { agent_index, path } => {
            tokio::spawn(async move {
                let result = tokio::task::spawn_blocking(|| {
                    Clipboard::new().and_then(|mut clipboard| clipboard.get_text())
                })
                .await;
                let (text, error) = match result {
                    Ok(Ok(text)) if !text.is_empty() => (Some(text), None),
                    Ok(Ok(_)) => (None, Some("Clipboard is empty".to_string())),
                    Ok(Err(error)) => (None, Some(format!("Paste failed: {error}"))),
                    Err(error) => (None, Some(format!("clipboard task failed: {error}"))),
                };
                event_types::send(
                    &ui_tx,
                    WorkspaceEvent::ClipboardCompleted {
                        agent_index,
                        path,
                        operation: ClipboardOperation::Paste,
                        text,
                        error,
                    },
                );
            });
        }
        AppEffect::StartChatTurn {
            agent_index,
            text,
            existing_thread,
            thread_loaded,
            selected_model,
            selected_effort,
            workspace,
        } => {
            tokio::spawn(async move {
                let thread_id = match existing_thread {
                    Some(thread_id) => {
                        if !thread_loaded {
                            match codex
                                .resume_thread(&thread_id, selected_model.as_deref())
                                .await
                            {
                                Ok(thread) => {
                                    let id = thread.id.clone();
                                    event_types::send(
                                        &ui_tx,
                                        ChatEvent::ThreadReady {
                                            agent_index,
                                            thread,
                                        },
                                    );
                                    id
                                }
                                Err(error) => {
                                    event_types::send(
                                        &ui_tx,
                                        ChatEvent::SubmissionFailed {
                                            agent_index,
                                            message: error.to_string(),
                                        },
                                    );
                                    return;
                                }
                            }
                        } else {
                            thread_id
                        }
                    }
                    None => match codex
                        .start_thread(&workspace, selected_model.as_deref())
                        .await
                    {
                        Ok(thread) => {
                            let id = thread.id.clone();
                            event_types::send(
                                &ui_tx,
                                ChatEvent::ThreadReady {
                                    agent_index,
                                    thread,
                                },
                            );
                            id
                        }
                        Err(error) => {
                            event_types::send(
                                &ui_tx,
                                ChatEvent::SubmissionFailed {
                                    agent_index,
                                    message: error.to_string(),
                                },
                            );
                            return;
                        }
                    },
                };

                match codex
                    .start_turn(
                        &thread_id,
                        &text,
                        selected_model.as_deref(),
                        selected_effort.as_deref(),
                    )
                    .await
                {
                    Ok(turn_id) => {
                        event_types::send(
                            &ui_tx,
                            ChatEvent::TurnStartedLocal {
                                agent_index,
                                turn_id,
                            },
                        );
                    }
                    Err(error) => {
                        event_types::send(
                            &ui_tx,
                            ChatEvent::SubmissionFailed {
                                agent_index,
                                message: error.to_string(),
                            },
                        );
                    }
                }
            });
        }
        AppEffect::ListModels { agent_index } => {
            tokio::spawn(async move {
                match codex.list_models().await {
                    Ok(models) => {
                        event_types::send(
                            &ui_tx,
                            ChatEvent::ModelListLoaded {
                                agent_index,
                                models,
                            },
                        );
                    }
                    Err(error) => {
                        event_types::send(
                            &ui_tx,
                            ChatEvent::ModelCommandResult {
                                agent_index,
                                message: format!("Unable to load available models.\n\n{error}"),
                            },
                        );
                    }
                }
            });
        }
        AppEffect::InterruptTurn {
            agent_index,
            thread_id,
            turn_id,
        } => {
            tokio::spawn(async move {
                if let Err(error) = codex.interrupt_turn(&thread_id, &turn_id).await {
                    event_types::send(
                        &ui_tx,
                        ChatEvent::TurnInterruptFailed {
                            agent_index,
                            message: format!("Failed to cancel response: {error}"),
                        },
                    );
                }
            });
        }
    }
}
