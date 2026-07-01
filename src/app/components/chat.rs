use super::super::{
    chat::{ChatCommand, ChatSupport, ModelCommand},
    *,
};
use super::UiSupport;

pub(in crate::app) struct ChatComponent;

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

        let agent = &mut app.agents[agent_index];
        if agent.thinking || agent.shell_running {
            app.status_message = Some("Wait for the current response to finish.".to_string());
            return;
        }

        agent.push_message(ChatMessage::new(MessageRole::User, text.clone(), None));
        agent.thinking = true;
        agent.status = None;
        let existing_thread = agent.thread_id.clone();
        let thread_loaded = agent.thread_loaded;
        let selected_model = agent.chat_model.clone();
        let selected_effort = agent.chat_reasoning_effort.clone();
        let workspace = agent.definition.workspace.clone();
        app.chat_input.clear();

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
                let current_label = app.agents[agent_index].chat_model_label.clone();
                app.agents[agent_index].status = Some("Loading available models...".to_string());
                tokio::spawn(async move {
                    let message = match codex.list_models().await {
                        Ok(models) => Self::format_model_list_message(&current_label, &models),
                        Err(error) => format!("Unable to load available models.\n\n{error}"),
                    };
                    let _ = ui_tx.send(UiEvent::ModelCommandResult {
                        agent_index,
                        message,
                    });
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

    fn format_model_list_message(current_label: &str, models: &[ModelInfo]) -> String {
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
