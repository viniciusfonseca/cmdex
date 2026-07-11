use super::{
    components::WorkspaceScreen,
    event_types::{ClipboardOperation, WorkspaceEvent},
    *,
};

impl App {
    pub(super) fn handle_workspace_event(&mut self, event: WorkspaceEvent) {
        match event {
            WorkspaceEvent::WatcherReady {
                agent_index,
                stop_tx,
            } => {
                if self.agents.get(agent_index).is_none() {
                    let _ = stop_tx.send(());
                    return;
                }
                if let Some(previous) = self.workspace_watchers.insert(agent_index, stop_tx) {
                    let _ = previous.send(());
                }
            }
            WorkspaceEvent::WatcherFailed {
                agent_index,
                message,
            } => {
                self.workspace_watchers.remove(&agent_index);
                if let Some(agent) = self.agents.get_mut(agent_index) {
                    agent.workspace.error = Some(message.clone());
                }
                self.status_message = Some(message);
            }
            WorkspaceEvent::WatcherError {
                agent_index,
                message,
            } => {
                if let Some(agent) = self.agents.get_mut(agent_index) {
                    agent.workspace.error = Some(message.clone());
                }
                self.status_message = Some(message);
            }
            WorkspaceEvent::FilesystemChanged { agent_index } => {
                if self.workspace_refresh_in_flight.contains(&agent_index) {
                    return;
                }
                WorkspaceScreen::request_refresh_for_agent(self, agent_index);
            }
            WorkspaceEvent::EntriesLoaded {
                agent_index,
                entries,
                error,
            } => {
                self.workspace_refresh_in_flight.remove(&agent_index);
                let Some(agent) = self.agents.get_mut(agent_index) else {
                    return;
                };
                if let Some(error) = error {
                    agent.workspace.error = Some(error);
                    return;
                }
                let should_open_editor =
                    match agent.workspace.apply_scanned_entries_without_io(entries) {
                        Ok(changed) => changed && agent.workspace.editor.is_none(),
                        Err(error) => {
                            agent.workspace.error = Some(error.to_string());
                            false
                        }
                    };
                if should_open_editor {
                    WorkspaceScreen::request_open_editor(self, agent_index);
                }
            }
            WorkspaceEvent::SearchCompleted {
                agent_index,
                generation,
                query,
                snapshot,
                error,
            } => {
                let Some(agent) = self.agents.get_mut(agent_index) else {
                    return;
                };
                if agent
                    .workspace
                    .apply_search_snapshot(generation, &query, snapshot)
                {
                    agent.workspace.error = error;
                }
            }
            WorkspaceEvent::EditorLoaded {
                agent_index,
                path,
                position,
                source,
                error,
            } => {
                let Some(agent) = self.agents.get_mut(agent_index) else {
                    return;
                };
                match (source, error) {
                    (Some(source), None) => {
                        if let Err(error) =
                            agent.workspace.apply_loaded_editor(&path, source, position)
                        {
                            agent.workspace.error = Some(error.to_string());
                        }
                    }
                    (_, Some(error)) => {
                        agent.workspace.finish_editor_load(&path);
                        agent.workspace.error = Some(error);
                    }
                    _ => {}
                }
            }
            WorkspaceEvent::PreviewLoaded {
                agent_index,
                path,
                preview,
                error,
            } => {
                let Some(agent) = self.agents.get_mut(agent_index) else {
                    return;
                };
                if let Some(error) = error {
                    agent.workspace.error = Some(error);
                } else {
                    let _ = agent.workspace.apply_loaded_preview(&path, preview);
                }
            }
            WorkspaceEvent::ClipboardCompleted {
                agent_index,
                path,
                operation,
                text,
                error,
            } => {
                let Some(agent) = self.agents.get_mut(agent_index) else {
                    return;
                };
                let Some(editor) = agent.workspace.editor.as_mut() else {
                    return;
                };
                if editor.path != path {
                    return;
                }
                if let Some(error) = error {
                    editor.status = Some(error);
                    return;
                }
                match operation {
                    ClipboardOperation::Copy => {
                        if editor.mode.is_visual() {
                            editor.exit_visual_mode();
                        }
                        editor.status = Some("Copied selection".to_string());
                    }
                    ClipboardOperation::Paste => {
                        if let Some(text) = text {
                            let pasted_chars = text.chars().count();
                            if editor.paste_text(&text) {
                                editor.status = Some(format!("Pasted {pasted_chars} chars"));
                            } else {
                                editor.status = Some("Clipboard is empty".to_string());
                            }
                        }
                    }
                }
            }
        }
    }
}
