use super::{UiEvent, chat::ChatSupport, *};
use crate::workspace::GitRepository;
use arboard::Clipboard;
use tokio::process::Command;

pub(super) enum AppEffect {
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

                let _ = ui_tx.send(UiEvent::ShellCompleted {
                    agent_index,
                    output,
                    success,
                });
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

                let _ = ui_tx.send(UiEvent::GitDiffRemoteCompleted {
                    agent_index,
                    action,
                    success,
                    message,
                });
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
                let _ = ui_tx.send(UiEvent::GitDiffMutationCompleted {
                    agent_index,
                    mutation: operation,
                    success,
                    message,
                });
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
                let _ = ui_tx.send(UiEvent::GitDiffLoaded {
                    agent_index,
                    generation,
                    result,
                    error,
                });
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
                let _ = ui_tx.send(UiEvent::WorkspaceEntriesLoaded {
                    agent_index,
                    entries,
                    error,
                });
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
                let _ = ui_tx.send(UiEvent::WorkspaceSearchCompleted {
                    agent_index,
                    generation,
                    query,
                    snapshot,
                    error,
                });
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
                let _ = ui_tx.send(UiEvent::WorkspaceEditorLoaded {
                    agent_index,
                    path,
                    position,
                    source,
                    error,
                });
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
                let _ = ui_tx.send(UiEvent::WorkspacePreviewLoaded {
                    agent_index,
                    path,
                    preview,
                    error,
                });
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
                let _ = ui_tx.send(UiEvent::WorkspaceClipboardCompleted {
                    agent_index,
                    path,
                    operation: ClipboardOperation::Copy,
                    text: None,
                    error,
                });
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
                let _ = ui_tx.send(UiEvent::WorkspaceClipboardCompleted {
                    agent_index,
                    path,
                    operation: ClipboardOperation::Paste,
                    text,
                    error,
                });
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
                                    let _ = ui_tx.send(UiEvent::ThreadReady {
                                        agent_index,
                                        thread,
                                    });
                                    id
                                }
                                Err(error) => {
                                    let _ = ui_tx.send(UiEvent::SubmissionFailed {
                                        agent_index,
                                        message: error.to_string(),
                                    });
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
                            let _ = ui_tx.send(UiEvent::ThreadReady {
                                agent_index,
                                thread,
                            });
                            id
                        }
                        Err(error) => {
                            let _ = ui_tx.send(UiEvent::SubmissionFailed {
                                agent_index,
                                message: error.to_string(),
                            });
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
                        let _ = ui_tx.send(UiEvent::TurnStartedLocal {
                            agent_index,
                            turn_id,
                        });
                    }
                    Err(error) => {
                        let _ = ui_tx.send(UiEvent::SubmissionFailed {
                            agent_index,
                            message: error.to_string(),
                        });
                    }
                }
            });
        }
        AppEffect::ListModels { agent_index } => {
            tokio::spawn(async move {
                match codex.list_models().await {
                    Ok(models) => {
                        let _ = ui_tx.send(UiEvent::ModelListLoaded {
                            agent_index,
                            models,
                        });
                    }
                    Err(error) => {
                        let _ = ui_tx.send(UiEvent::ModelCommandResult {
                            agent_index,
                            message: format!("Unable to load available models.\n\n{error}"),
                        });
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
                    let _ = ui_tx.send(UiEvent::TurnInterruptFailed {
                        agent_index,
                        message: format!("Failed to cancel response: {error}"),
                    });
                }
            });
        }
    }
}
