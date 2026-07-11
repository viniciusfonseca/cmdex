use super::{components::WorkspaceScreen, event_types::LspEvent, *};

impl App {
    pub(super) fn handle_lsp_event(&mut self, event: LspEvent) {
        match event {
            LspEvent::RuntimeReady {
                agent_index,
                server_index,
                server_name,
                command_tx,
            } => {
                let key = LspRuntimeKey {
                    agent_index,
                    server_index,
                };
                self.lsp_starting.remove(&key);
                self.lsp_runtimes.insert(
                    key,
                    LspRuntime {
                        command_tx: command_tx.clone(),
                        server_name,
                        starting: true,
                    },
                );
                if let Some(commands) = self.pending_lsp_commands.remove(&key) {
                    for command in commands {
                        self.enqueue_effect(super::effects::AppEffect::SendLspCommand {
                            agent_index,
                            server_index,
                            command_tx: command_tx.clone(),
                            command,
                        });
                    }
                }
            }
            LspEvent::RuntimeFailed {
                agent_index,
                server_index,
                message,
            } => {
                let key = LspRuntimeKey {
                    agent_index,
                    server_index,
                };
                self.lsp_starting.remove(&key);
                self.pending_lsp_commands.remove(&key);
                self.lsp_runtimes.remove(&key);
                self.set_lsp_editor_status(agent_index, message);
            }
            LspEvent::HoverResult {
                agent_index,
                path,
                position,
                contents,
                error,
            } => {
                self.mark_lsp_runtime_ready_for_path(agent_index, &path);
                let Some(agent) = self.agents.get_mut(agent_index) else {
                    return;
                };
                let Some(editor) = agent.workspace.editor.as_mut() else {
                    return;
                };
                if editor.path != path {
                    return;
                }
                if !editor.resolve_hover(position, contents) {
                    return;
                }
                if let Some(error) = error {
                    editor.status = Some(error);
                }
            }
            LspEvent::DefinitionResult {
                agent_index,
                source_path,
                _source_position: _,
                target,
                error,
            } => {
                self.mark_lsp_runtime_ready_for_path(agent_index, &source_path);
                let Some(agent) = self.agents.get_mut(agent_index) else {
                    return;
                };
                let Some(editor) = agent.workspace.editor.as_ref() else {
                    return;
                };
                if editor.path != source_path {
                    return;
                }

                if let Some(error) = error {
                    if let Some(editor) = agent.workspace.editor.as_mut() {
                        editor.status = Some(error);
                    }
                    return;
                }

                let Some(target) = target else {
                    if let Some(editor) = agent.workspace.editor.as_mut() {
                        editor.status = Some("Definition not found".to_string());
                    }
                    return;
                };

                if !agent
                    .workspace
                    .entries()
                    .iter()
                    .any(|entry| entry.path == target.path)
                {
                    if let Some(editor) = agent.workspace.editor.as_mut() {
                        editor.status = Some("Definition is outside the workspace.".to_string());
                    }
                    return;
                }

                if agent
                    .workspace
                    .open_path_at_position_without_io(&target.path)
                    .is_none()
                {
                    if let Some(editor) = agent.workspace.editor.as_mut() {
                        editor.status = Some("Definition lookup failed".to_string());
                    }
                } else {
                    WorkspaceScreen::request_open_editor_at(
                        self,
                        agent_index,
                        Some(target.position),
                    );
                }
            }
            LspEvent::CompletionResult {
                agent_index,
                path,
                position,
                items,
                error,
            } => {
                self.mark_lsp_runtime_ready_for_path(agent_index, &path);
                let Some(agent) = self.agents.get_mut(agent_index) else {
                    return;
                };
                let Some(editor) = agent.workspace.editor.as_mut() else {
                    return;
                };
                if editor.path != path {
                    return;
                }
                if !editor.resolve_completion(position, items) {
                    return;
                }
                if let Some(error) = error {
                    editor.status = Some(error);
                    editor.clear_completion();
                } else if editor.completion_popover().is_none() {
                    editor.status = Some("No completion suggestions".to_string());
                } else {
                    editor.status = None;
                }
            }
            LspEvent::Notification {
                agent_index,
                server_name,
                method,
                params,
            } => {
                let message = params
                    .get("message")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string);
                if let Some(agent) = self.agents.get_mut(agent_index)
                    && matches!(method.as_str(), "window/showMessage" | "window/logMessage")
                    && let Some(message) = message
                {
                    agent.status = Some(format!("{server_name}: {message}"));
                }
            }
        }
    }
}
