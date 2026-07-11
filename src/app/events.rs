use super::{
    chat::ChatSupport,
    components::{ChatComponent, GitDiffComponent, WorkspaceComponent},
    *,
};

impl App {
    pub(super) fn handle_server_event(&mut self, event: ServerEvent) {
        match event {
            ServerEvent::ThreadStatusChanged { thread_id, active } => {
                if let Some(agent) = self.find_agent_by_thread_mut(&thread_id) {
                    agent.chat.thinking = active;
                }
            }
            ServerEvent::TurnStarted { thread_id, turn_id } => {
                if let Some(agent) = self.find_agent_by_thread_mut(&thread_id) {
                    agent.chat.active_turn_id = Some(turn_id);
                    agent.chat.thinking = true;
                }
            }
            ServerEvent::ItemStarted { thread_id, item } => {
                if let Some(agent) = self.find_agent_by_thread_mut(&thread_id)
                    && let ThreadItem::AgentMessage { id, text } = item
                {
                    agent.chat.streaming_item_id = Some(id.clone());
                    agent.chat.push_message(ChatMessage::new(
                        MessageRole::Assistant,
                        text,
                        Some(id),
                    ));
                }
            }
            ServerEvent::ItemCompleted { thread_id, item } => {
                if let Some(agent) = self.find_agent_by_thread_mut(&thread_id) {
                    match item {
                        ThreadItem::AgentMessage { id, text } => {
                            MessageStore::upsert(agent, MessageRole::Assistant, &id, text);
                            agent.chat.streaming_item_id = None;
                        }
                        ThreadItem::UserMessage | ThreadItem::Other => {}
                    }
                }
            }
            ServerEvent::AgentMessageDelta {
                thread_id,
                item_id,
                delta,
            } => {
                if let Some(agent) = self.find_agent_by_thread_mut(&thread_id) {
                    if let Some(message) = agent
                        .chat
                        .messages
                        .iter_mut()
                        .find(|message| message.item_id.as_deref() == Some(item_id.as_str()))
                    {
                        message.append_text(&delta);
                        agent.chat.invalidate_chat_render_cache();
                    } else {
                        agent.chat.push_message(ChatMessage::new(
                            MessageRole::Assistant,
                            delta,
                            Some(item_id),
                        ));
                    }
                }
            }
            ServerEvent::TurnCompleted {
                thread_id,
                turn_id,
                interrupted,
            } => {
                if let Some(agent) = self.find_agent_by_thread_mut(&thread_id) {
                    if agent.chat.active_turn_id.as_deref() == Some(turn_id.as_str()) {
                        agent.chat.active_turn_id = None;
                    }
                    agent.chat.thinking = false;
                    agent.chat.streaming_item_id = None;
                    if interrupted {
                        agent.status = Some("Response canceled".to_string());
                    }
                }
                if interrupted {
                    self.status_message = Some("Response canceled".to_string());
                }
            }
            ServerEvent::Warning(message)
            | ServerEvent::Error(message)
            | ServerEvent::TransportError(message) => {
                self.status_message = Some(message);
            }
        }
    }

    pub(super) fn handle_ui_event(&mut self, event: UiEvent) {
        match event {
            UiEvent::ThreadReady {
                agent_index,
                thread,
            } => {
                if let Some(agent) = self.agents.get_mut(agent_index) {
                    agent.chat.thread_id = Some(thread.id);
                    agent.chat.thread_loaded = true;
                    if !agent.chat.chat_settings_explicit {
                        agent.chat.chat_model = thread.model;
                        agent.chat.chat_reasoning_effort = thread.reasoning_effort;
                    }
                    agent.chat.chat_model_label = ChatSupport::resolve_chat_model_label(
                        agent.chat.chat_model.as_deref(),
                        agent.chat.chat_reasoning_effort.as_deref(),
                        self.default_chat_model.as_deref(),
                        &self.chat_model_label,
                    );
                }
            }
            UiEvent::ModelCommandResult {
                agent_index,
                message,
            } => {
                if let Some(agent) = self.agents.get_mut(agent_index) {
                    agent.status = Some("Model command finished".to_string());
                    agent.chat.push_message(ChatMessage::new(
                        MessageRole::System,
                        message.clone(),
                        None,
                    ));
                }
                self.status_message = Some("Model command finished".to_string());
            }
            UiEvent::ModelListLoaded {
                agent_index,
                models,
            } => {
                let Some(agent) = self.agents.get_mut(agent_index) else {
                    return;
                };
                agent.status = Some("Select a model".to_string());

                if models.is_empty() {
                    let message = ChatComponent::format_model_list_message(
                        &agent.chat.chat_model_label,
                        &models,
                    );
                    agent
                        .chat
                        .push_message(ChatMessage::new(MessageRole::System, message, None));
                    self.status_message = Some("No models available".to_string());
                    return;
                }

                let selected = agent
                    .chat
                    .chat_model
                    .as_deref()
                    .and_then(|current| {
                        models
                            .iter()
                            .position(|model| model.id == current || model.model == current)
                    })
                    .or_else(|| models.iter().position(|model| model.is_default))
                    .unwrap_or(0);

                if self.current_agent == Some(agent_index) {
                    self.model_picker = Some(ModelPickerState {
                        agent_index,
                        models,
                        selected,
                        view: ModelPickerView::Models,
                    });
                    self.status_message = Some(
                        "Use Up/Down to select a model, Enter to apply, or Esc to cancel"
                            .to_string(),
                    );
                }
            }
            UiEvent::SessionLoaded {
                agent_index,
                session,
            } => {
                if let (Some(agent), Some(session)) = (self.agents.get_mut(agent_index), session)
                    && agent.chat.thread_id.is_none()
                    && agent.chat.messages.is_empty()
                {
                    let (thread_id, messages) = SessionLoader::session_messages(session);
                    agent.chat.thread_id = Some(thread_id);
                    agent.chat.thread_loaded = false;
                    agent.chat.replace_messages(messages);
                }
            }
            UiEvent::SubmissionFailed {
                agent_index,
                message,
            } => {
                if let Some(agent) = self.agents.get_mut(agent_index) {
                    agent.chat.thinking = false;
                    agent.chat.shell_running = false;
                    agent.chat.active_turn_id = None;
                    agent.chat.streaming_item_id = None;
                    agent.status = Some(message.clone());
                    agent.chat.push_message(ChatMessage::new(
                        MessageRole::System,
                        message.clone(),
                        None,
                    ));
                }
                self.status_message = Some(message);
            }
            UiEvent::TurnStartedLocal {
                agent_index,
                turn_id,
            } => {
                if let Some(agent) = self.agents.get_mut(agent_index) {
                    agent.chat.active_turn_id = Some(turn_id);
                    agent.chat.thinking = true;
                }
            }
            UiEvent::ShellCompleted {
                agent_index,
                output,
                success,
            } => {
                if let Some(agent) = self.agents.get_mut(agent_index) {
                    agent.chat.shell_running = false;
                    agent
                        .chat
                        .push_message(ChatMessage::new(MessageRole::Shell, output, None));
                    agent.status = Some(if success {
                        "Shell command finished".to_string()
                    } else {
                        "Shell command failed".to_string()
                    });
                }
                WorkspaceComponent::request_refresh_for_agent(self, agent_index);
                GitDiffComponent::request_refresh_for_agent(self, agent_index);
            }
            UiEvent::TurnInterruptFailed {
                agent_index,
                message,
            } => {
                if let Some(agent) = self.agents.get_mut(agent_index) {
                    agent.status = Some(message.clone());
                    agent.chat.push_message(ChatMessage::new(
                        MessageRole::System,
                        message.clone(),
                        None,
                    ));
                }
                self.status_message = Some(message);
            }
            UiEvent::GitDiffRemoteCompleted {
                agent_index,
                action,
                success,
                message,
            } => {
                let should_refresh = if let Some(agent) = self.agents.get_mut(agent_index) {
                    let root = agent.definition.workspace.clone();
                    agent
                        .git_diff
                        .complete_remote_action(&root, action, success, message.clone());
                    success
                } else {
                    false
                };
                if should_refresh {
                    GitDiffComponent::request_refresh(self);
                }
                self.status_message = Some(if success {
                    format!("Git {} finished", action.label().to_lowercase())
                } else {
                    message
                });
            }
            UiEvent::GitDiffMutationCompleted {
                agent_index,
                mutation,
                success,
                message,
            } => {
                let should_refresh = if let Some(agent) = self.agents.get_mut(agent_index) {
                    let root = agent.definition.workspace.clone();
                    agent
                        .git_diff
                        .complete_mutation(&root, &mutation, success, message.clone());
                    success
                } else {
                    false
                };
                if should_refresh {
                    GitDiffComponent::request_refresh(self);
                }
                self.status_message = Some(if success {
                    "Git operation finished".to_string()
                } else {
                    message
                });
            }
            UiEvent::WorkspaceEntriesLoaded {
                agent_index,
                entries,
                error,
            } => {
                self.workspace_refresh_in_flight = false;
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
                    WorkspaceComponent::request_open_editor(self, agent_index);
                }
            }
            UiEvent::GitDiffLoaded {
                agent_index,
                generation,
                result,
                error,
            } => {
                if let Some(agent) = self.agents.get_mut(agent_index) {
                    agent.git_diff.apply_load_result(generation, result, error);
                }
            }
            UiEvent::WorkspaceSearchCompleted {
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
            UiEvent::WorkspaceEditorLoaded {
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
            UiEvent::WorkspacePreviewLoaded {
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
            UiEvent::WorkspaceClipboardCompleted {
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
                        if editor.mode == EditorMode::Visual {
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
            UiEvent::ShellSessionReady {
                agent_index,
                session_id,
            } => {
                if let Some(agent) = self.agents.get_mut(agent_index)
                    && let Some(session) = agent.shell_tab.session_by_id_mut(session_id)
                {
                    session.mark_ready();
                }
            }
            UiEvent::ShellSessionOutput {
                agent_index,
                session_id,
                line,
                stderr,
            } => {
                if let Some(agent) = self.agents.get_mut(agent_index)
                    && let Some(session) = agent.shell_tab.session_by_id_mut(session_id)
                {
                    if stderr {
                        session.append_stderr_line(&line);
                    } else {
                        session.append_stdout_line(&line);
                    }
                }
            }
            UiEvent::ShellSessionCommandFinished {
                agent_index,
                session_id,
                exit_code,
            } => {
                if let Some(agent) = self.agents.get_mut(agent_index)
                    && let Some(session) = agent.shell_tab.session_by_id_mut(session_id)
                {
                    session.finish_command(exit_code);
                }
                WorkspaceComponent::request_refresh_for_agent(self, agent_index);
                GitDiffComponent::request_refresh_for_agent(self, agent_index);
            }
            UiEvent::ShellSessionExited {
                agent_index,
                session_id,
                message,
            } => {
                self.shell_runtimes.remove(&ShellSessionKey {
                    agent_index,
                    session_id,
                });
                if let Some(agent) = self.agents.get_mut(agent_index) {
                    agent.shell_tab.remove_session_by_id(session_id);
                    if agent.shell_tab.sessions.is_empty() {
                        agent.shell_tab.input.clear();
                    }
                }
                self.status_message = Some(message);
            }
            UiEvent::LspHoverResult {
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
            UiEvent::LspDefinitionResult {
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
                    .entries
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
                    WorkspaceComponent::request_open_editor_at(
                        self,
                        agent_index,
                        Some(target.position),
                    );
                }
            }
            UiEvent::LspCompletionResult {
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
            UiEvent::LspNotification {
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
