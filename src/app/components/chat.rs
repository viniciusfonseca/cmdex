use super::super::{
    chat::{ChatCommand, ChatSupport, ModelCommand},
    *,
};
use super::UiSupport;

pub(in crate::app) struct ChatComponent;

#[derive(Debug, PartialEq, Eq)]
pub(in crate::app) enum ModelPickerAction {
    NotOpen,
    Handled,
    Apply {
        agent_index: usize,
        model: String,
        effort: Option<String>,
    },
}

impl ChatComponent {
    pub(in crate::app) fn draw(frame: &mut Frame, app: &App, area: Rect) {
        let Some(agent) = app.active_agent() else {
            let empty = Paragraph::new("Add an agent from the sidebar to start chatting.")
                .block(UiSupport::panel_block().title("Chat"))
                .style(UiSupport::panel_style());
            frame.render_widget(empty, area);
            return;
        };

        let title = if let Some(status) = &agent.status {
            format!("Chat - {} ({status})", agent.definition.name)
        } else {
            format!("Chat - {}", agent.definition.name)
        };

        let render_state = ChatSupport::render_state(agent, area);
        let inner_height = area.height.saturating_sub(2);
        let max_scroll = render_state
            .content_height
            .saturating_sub(inner_height as usize) as u16;
        let scroll = if agent.chat_follow_output {
            max_scroll
        } else {
            agent.chat_scroll.min(max_scroll)
        };

        let chat = Paragraph::new(render_state.text)
            .block(UiSupport::panel_block().title(title))
            .style(UiSupport::panel_style())
            .scroll((scroll, 0))
            .wrap(Wrap { trim: false });
        frame.render_widget(chat, area);
        UiSupport::render_vertical_scrollbar(frame, area, render_state.content_height, scroll);
    }

    pub(in crate::app) fn submit_message(
        app: &mut App,
        codex: CodexAppServer,
        ui_tx: mpsc::UnboundedSender<UiEvent>,
    ) {
        if let Some(command) = ChatSupport::command_from_input(&app.chat_input) {
            Self::submit_chat_command(app, command, codex, ui_tx);
            return;
        }

        if let Some(command) = ChatSupport::shell_command_from_input(&app.chat_input) {
            Self::submit_shell_command(app, command, ui_tx);
            return;
        }

        let text = app.chat_input.trim().to_string();
        if text.is_empty() {
            return;
        }

        let Some(agent_index) = app.current_agent else {
            app.status_message = Some("Add an agent before sending messages.".to_string());
            return;
        };

        if app.agents[agent_index].thinking || app.agents[agent_index].shell_running {
            Self::enqueue_message(app, agent_index, text);
            return;
        }

        app.chat_input.clear();
        Self::dispatch_message(app, agent_index, text, codex, ui_tx);
    }

    pub(in crate::app) fn handle_model_picker_key(
        app: &mut App,
        key: KeyEvent,
    ) -> ModelPickerAction {
        if app.current_tab != AppTab::Chat {
            return ModelPickerAction::NotOpen;
        }

        let Some(mut picker) = app.model_picker.take() else {
            return ModelPickerAction::NotOpen;
        };

        let action = match &mut picker.view {
            super::super::ModelPickerView::Models => match key.code {
                KeyCode::Up => {
                    picker.selected = picker.selected.saturating_sub(1);
                    ModelPickerAction::Handled
                }
                KeyCode::Down => {
                    picker.selected = picker
                        .selected
                        .saturating_add(1)
                        .min(picker.models.len().saturating_sub(1));
                    ModelPickerAction::Handled
                }
                KeyCode::Esc => {
                    app.status_message = Some("Model selection canceled".to_string());
                    return ModelPickerAction::Handled;
                }
                KeyCode::Enter => {
                    let model_index = picker.selected;
                    let Some(model) = picker.models.get(model_index).cloned() else {
                        app.model_picker = Some(picker);
                        return ModelPickerAction::Handled;
                    };

                    if model.supported_reasoning_efforts.is_empty() {
                        ModelPickerAction::Apply {
                            agent_index: picker.agent_index,
                            model: model.id,
                            effort: model.default_reasoning_effort,
                        }
                    } else {
                        let current_effort = app
                            .agents
                            .get(picker.agent_index)
                            .and_then(|agent| agent.chat_reasoning_effort.as_deref());
                        let selected_effort = current_effort
                            .and_then(|current| {
                                model
                                    .supported_reasoning_efforts
                                    .iter()
                                    .position(|effort| effort.reasoning_effort == current)
                            })
                            .or_else(|| {
                                model
                                    .default_reasoning_effort
                                    .as_deref()
                                    .and_then(|default| {
                                        model
                                            .supported_reasoning_efforts
                                            .iter()
                                            .position(|effort| effort.reasoning_effort == default)
                                    })
                            })
                            .unwrap_or(0);
                        picker.view = super::super::ModelPickerView::Efforts {
                            model_index,
                            selected: selected_effort,
                        };
                        if let Some(agent) = app.agents.get_mut(picker.agent_index) {
                            agent.status = Some("Select an effort".to_string());
                        }
                        app.status_message = Some(
                            "Use Up/Down to select an effort, Enter to apply, or Esc to return"
                                .to_string(),
                        );
                        ModelPickerAction::Handled
                    }
                }
                _ => ModelPickerAction::Handled,
            },
            super::super::ModelPickerView::Efforts {
                model_index,
                selected,
            } => {
                let model = &picker.models[*model_index];
                match key.code {
                    KeyCode::Up => {
                        *selected = selected.saturating_sub(1);
                        ModelPickerAction::Handled
                    }
                    KeyCode::Down => {
                        *selected = selected
                            .saturating_add(1)
                            .min(model.supported_reasoning_efforts.len().saturating_sub(1));
                        ModelPickerAction::Handled
                    }
                    KeyCode::Esc => {
                        picker.view = super::super::ModelPickerView::Models;
                        if let Some(agent) = app.agents.get_mut(picker.agent_index) {
                            agent.status = Some("Select a model".to_string());
                        }
                        app.status_message = Some("Select a model".to_string());
                        ModelPickerAction::Handled
                    }
                    KeyCode::Enter => {
                        let effort = model.supported_reasoning_efforts[*selected]
                            .reasoning_effort
                            .clone();
                        ModelPickerAction::Apply {
                            agent_index: picker.agent_index,
                            model: picker.models[*model_index].id.clone(),
                            effort: Some(effort),
                        }
                    }
                    _ => ModelPickerAction::Handled,
                }
            }
        };

        if !matches!(action, ModelPickerAction::Apply { .. }) {
            app.model_picker = Some(picker);
        }
        action
    }

    pub(in crate::app) fn handle_queue_key(app: &mut App, key: KeyEvent) -> bool {
        if app.current_tab != AppTab::Chat || app.add_agent_selected() {
            return false;
        }

        let Some(agent) = app.active_agent_mut() else {
            return false;
        };
        if !agent.has_queued_chat_messages() {
            return false;
        }

        match key.code {
            KeyCode::Up if key.modifiers.contains(KeyModifiers::ALT) => {
                agent.select_previous_queued_chat_message();
                true
            }
            KeyCode::Down if key.modifiers.contains(KeyModifiers::ALT) => {
                agent.select_next_queued_chat_message();
                true
            }
            KeyCode::Backspace | KeyCode::Delete if key.modifiers.contains(KeyModifiers::ALT) => {
                let _ = agent.cancel_selected_queued_chat_message();
                let remaining = agent.queued_chat_count();
                agent.status = if remaining == 0 {
                    Some("Queue is empty".to_string())
                } else {
                    Some(format!("{remaining} queued message(s) remaining"))
                };
                true
            }
            KeyCode::Char('x') if key.modifiers.contains(KeyModifiers::ALT) => {
                let _ = agent.cancel_selected_queued_chat_message();
                let remaining = agent.queued_chat_count();
                agent.status = if remaining == 0 {
                    Some("Queue is empty".to_string())
                } else {
                    Some(format!("{remaining} queued message(s) remaining"))
                };
                true
            }
            _ => false,
        }
    }

    pub(in crate::app) fn maybe_dispatch_queued_messages(
        app: &mut App,
        codex: CodexAppServer,
        ui_tx: mpsc::UnboundedSender<UiEvent>,
    ) {
        let queued_agents = app
            .agents
            .iter()
            .enumerate()
            .filter_map(|(agent_index, agent)| {
                (!agent.thinking
                    && !agent.shell_running
                    && agent.active_turn_id.is_none()
                    && agent.has_queued_chat_messages())
                .then_some(agent_index)
            })
            .collect::<Vec<_>>();

        for agent_index in queued_agents {
            let Some(text) = app.agents[agent_index].pop_next_queued_chat_message() else {
                continue;
            };
            Self::dispatch_message(app, agent_index, text, codex.clone(), ui_tx.clone());
        }
    }

    fn enqueue_message(app: &mut App, agent_index: usize, text: String) {
        let agent = &mut app.agents[agent_index];
        agent.enqueue_chat_message(text);
        let queued = agent.queued_chat_count();
        agent.status = Some(format!("{queued} queued message(s)"));
        app.status_message = Some(format!("Queued {queued} message(s)"));
        app.chat_input.clear();
    }

    fn dispatch_message(
        app: &mut App,
        agent_index: usize,
        text: String,
        codex: CodexAppServer,
        ui_tx: mpsc::UnboundedSender<UiEvent>,
    ) {
        let agent = &mut app.agents[agent_index];
        agent.push_message(ChatMessage::new(MessageRole::User, text.clone(), None));
        agent.thinking = true;
        agent.status = None;
        let existing_thread = agent.thread_id.clone();
        let thread_loaded = agent.thread_loaded;
        let selected_model = agent.chat_model.clone();
        let selected_effort = agent.chat_reasoning_effort.clone();
        let workspace = agent.definition.workspace.clone();

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

    pub(in crate::app) fn submit_chat_command(
        app: &mut App,
        command: ChatCommand,
        codex: CodexAppServer,
        ui_tx: mpsc::UnboundedSender<UiEvent>,
    ) {
        match command {
            ChatCommand::Model(command) => Self::submit_model_command(app, command, codex, ui_tx),
        }
    }

    pub(in crate::app) fn submit_model_command(
        app: &mut App,
        command: ModelCommand,
        codex: CodexAppServer,
        ui_tx: mpsc::UnboundedSender<UiEvent>,
    ) {
        let Some(agent_index) = app.current_agent else {
            app.status_message = Some("Add an agent before changing the model.".to_string());
            return;
        };

        app.chat_input.clear();

        match command {
            ModelCommand::List => {
                app.agents[agent_index].status = Some("Loading available models...".to_string());
                tokio::spawn(async move {
                    match codex.list_models().await {
                        Ok(models) => {
                            let _ = ui_tx.send(UiEvent::ModelListLoaded {
                                agent_index,
                                models,
                            });
                        }
                        Err(error) => {
                            let message = format!("Unable to load available models.\n\n{error}");
                            let _ = ui_tx.send(UiEvent::ModelCommandResult {
                                agent_index,
                                message,
                            });
                        }
                    }
                });
            }
            ModelCommand::ResetDefault => {
                let agent = &mut app.agents[agent_index];
                agent.chat_model = app.default_chat_model.clone();
                agent.chat_reasoning_effort = app.default_chat_reasoning_effort.clone();
                agent.chat_model_label = app.chat_model_label.clone();
                agent.chat_settings_explicit = false;
                let message = format!("Model set to `{}`.", agent.chat_model_label);
                agent.status = Some(message.clone());
                agent.push_message(ChatMessage::new(MessageRole::System, message, None));
                app.status_message = Some("Model updated".to_string());
            }
            ModelCommand::Set { model, effort } => {
                let default_model = app.default_chat_model.clone();
                let default_label = app.chat_model_label.clone();
                let agent = &mut app.agents[agent_index];
                if let Some(model) = model {
                    agent.chat_model = Some(model);
                }
                if let Some(effort) = effort {
                    agent.chat_reasoning_effort = Some(effort);
                }
                agent.chat_settings_explicit = true;
                agent.chat_model_label = ChatSupport::resolve_chat_model_label(
                    agent.chat_model.as_deref(),
                    agent.chat_reasoning_effort.as_deref(),
                    default_model.as_deref(),
                    &default_label,
                );
                let message = format!("Model set to `{}`.", agent.chat_model_label);
                agent.status = Some(message.clone());
                agent.push_message(ChatMessage::new(MessageRole::System, message, None));
                app.status_message = Some("Model updated".to_string());
            }
        }
    }

    pub(in crate::app) fn submit_shell_command(
        app: &mut App,
        command: String,
        ui_tx: mpsc::UnboundedSender<UiEvent>,
    ) {
        let Some(agent_index) = app.current_agent else {
            app.status_message = Some("Add an agent before running shell commands.".to_string());
            return;
        };

        let agent = &mut app.agents[agent_index];
        if agent.thinking || agent.shell_running {
            app.status_message = Some("Wait for the current response to finish.".to_string());
            return;
        }

        agent.push_message(ChatMessage::new(
            MessageRole::Shell,
            format!("> {command}"),
            None,
        ));
        agent.shell_running = true;
        agent.status = None;
        let workspace = agent.definition.workspace.clone();
        app.chat_input.clear();

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

    pub(in crate::app) fn interrupt_active_turn(
        app: &mut App,
        codex: CodexAppServer,
        ui_tx: mpsc::UnboundedSender<UiEvent>,
    ) {
        if app.add_agent_selected() {
            return;
        }

        let Some(agent_index) = app.current_agent else {
            return;
        };
        let agent = &mut app.agents[agent_index];

        if !agent.thinking || agent.shell_running {
            return;
        }

        let (Some(thread_id), Some(turn_id)) =
            (agent.thread_id.clone(), agent.active_turn_id.clone())
        else {
            return;
        };

        agent.status = Some("Canceling response...".to_string());
        app.status_message = Some("Canceling response...".to_string());

        tokio::spawn(async move {
            if let Err(error) = codex.interrupt_turn(&thread_id, &turn_id).await {
                let _ = ui_tx.send(UiEvent::TurnInterruptFailed {
                    agent_index,
                    message: format!("Failed to cancel response: {error}"),
                });
            }
        });
    }

    pub(in crate::app) fn format_model_list_message(
        current_label: &str,
        models: &[ModelInfo],
    ) -> String {
        if models.is_empty() {
            return format!(
                "Current model: `{current_label}`\n\nNo visible models were returned by the app server."
            );
        }

        let mut lines = vec![
            format!("Current model: `{current_label}`"),
            String::new(),
            "Available models:".to_string(),
        ];

        for model in models {
            let mut line = format!("- `{}`", model.model);
            if model.id != model.model {
                line.push_str(&format!(" [{}]", model.id));
            }
            if model.display_name != model.model {
                line.push_str(&format!(" - {}", model.display_name));
            }
            if model.is_default {
                line.push_str(" (default)");
            }
            lines.push(line);
        }

        lines.push(String::new());
        lines.push("Use `/model <id>` to switch models.".to_string());
        lines.push("Use `/model <id> <effort>` to switch model and effort together.".to_string());
        lines.push("Use `/model <effort>` to change only the effort.".to_string());
        lines.push("Use `/model default` to go back to the configured default.".to_string());
        lines.join("\n")
    }
}
