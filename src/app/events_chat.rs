use super::{chat::ChatSupport, components::ChatComponent, event_types::ChatEvent, *};

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

    pub(super) fn handle_chat_event(&mut self, event: ChatEvent) {
        match event {
            ChatEvent::ThreadReady {
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
            ChatEvent::ModelCommandResult {
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
            ChatEvent::ModelListLoaded {
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
            ChatEvent::SessionLoaded {
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
            ChatEvent::SubmissionFailed {
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
            ChatEvent::TurnStartedLocal {
                agent_index,
                turn_id,
            } => {
                if let Some(agent) = self.agents.get_mut(agent_index) {
                    agent.chat.active_turn_id = Some(turn_id);
                    agent.chat.thinking = true;
                }
            }
            ChatEvent::TurnInterruptFailed {
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
        }
    }
}
