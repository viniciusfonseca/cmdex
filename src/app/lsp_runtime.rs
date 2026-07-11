use std::{
    path::{Path, PathBuf},
    sync::mpsc as std_mpsc,
    thread,
};

use anyhow::{Context, Result};
use tokio::sync::mpsc;

use super::{
    LspSession, UiEvent,
    event_types::{self, LspEvent},
};
use crate::config::LspServerConfig;
use crate::workspace::EditorPosition;

#[derive(Debug)]
pub(crate) enum LspCommand {
    Hover {
        agent_index: usize,
        path: PathBuf,
        source: String,
        position: EditorPosition,
    },
    Definition {
        agent_index: usize,
        path: PathBuf,
        source: String,
        position: EditorPosition,
    },
    Completion {
        agent_index: usize,
        path: PathBuf,
        source: String,
        position: EditorPosition,
    },
    Shutdown,
}

pub(crate) struct LspRuntimeFactory;

impl LspRuntimeFactory {
    pub(crate) fn spawn(
        workspace_root: &Path,
        server: LspServerConfig,
        agent_index: usize,
        ui_tx: mpsc::UnboundedSender<UiEvent>,
    ) -> Result<std_mpsc::Sender<LspCommand>> {
        let workspace_root = workspace_root.to_path_buf();
        let (command_tx, command_rx) = std_mpsc::channel::<LspCommand>();

        thread::Builder::new()
            .name(format!(
                "cmdex-lsp-{}-{}",
                server.name.clone(),
                workspace_root
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("workspace")
            ))
            .spawn(move || {
                let mut session =
                    LspSession::new(workspace_root, server, agent_index, ui_tx.clone());

                while let Ok(command) = command_rx.recv() {
                    match command {
                        LspCommand::Hover {
                            agent_index,
                            path,
                            source,
                            position,
                        } => match session.hover(&path, &source, position) {
                            Ok(contents) => event_types::send(
                                &ui_tx,
                                LspEvent::HoverResult {
                                    agent_index,
                                    path,
                                    position,
                                    contents,
                                    error: None,
                                },
                            ),
                            Err(error) => event_types::send(
                                &ui_tx,
                                LspEvent::HoverResult {
                                    agent_index,
                                    path,
                                    position,
                                    contents: None,
                                    error: Some(error.to_string()),
                                },
                            ),
                        },
                        LspCommand::Definition {
                            agent_index,
                            path,
                            source,
                            position,
                        } => match session.definition(&path, &source, position) {
                            Ok(target) => event_types::send(
                                &ui_tx,
                                LspEvent::DefinitionResult {
                                    agent_index,
                                    source_path: path,
                                    _source_position: position,
                                    target,
                                    error: None,
                                },
                            ),
                            Err(error) => event_types::send(
                                &ui_tx,
                                LspEvent::DefinitionResult {
                                    agent_index,
                                    source_path: path,
                                    _source_position: position,
                                    target: None,
                                    error: Some(error.to_string()),
                                },
                            ),
                        },
                        LspCommand::Completion {
                            agent_index,
                            path,
                            source,
                            position,
                        } => match session.completion(&path, &source, position) {
                            Ok(items) => event_types::send(
                                &ui_tx,
                                LspEvent::CompletionResult {
                                    agent_index,
                                    path,
                                    position,
                                    items,
                                    error: None,
                                },
                            ),
                            Err(error) => event_types::send(
                                &ui_tx,
                                LspEvent::CompletionResult {
                                    agent_index,
                                    path,
                                    position,
                                    items: Vec::new(),
                                    error: Some(error.to_string()),
                                },
                            ),
                        },
                        LspCommand::Shutdown => break,
                    }
                }

                session.shutdown();
            })
            .context("failed to spawn LSP worker thread")?;

        Ok(command_tx)
    }
}
