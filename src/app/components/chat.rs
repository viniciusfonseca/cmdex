use super::super::{
    chat::{ChatCommand, ChatSupport, ModelCommand},
    *,
};
use super::shared::UiSupport;

pub(in crate::app) fn draw_chat(frame: &mut Frame, app: &App, area: Rect) {
    let Some(agent) = app.active_agent() else {
        let empty = Paragraph::new("Add an agent from the sidebar to start chatting.")
            .block(UiSupport::panel_block().title("Chat"))
            .style(UiSupport::panel_style());
        frame.render_widget(empty, area);
        return;
    };

    let lines = ChatSupport::padded_lines(agent, area);
    let title = if let Some(status) = &agent.status {
        format!("Chat - {} ({status})", agent.definition.name)
    } else {
        format!("Chat - {}", agent.definition.name)
    };

    let inner_height = area.height.saturating_sub(2);
    let text = Text::from(lines);
    let content_height = ChatSupport::content_height(agent, area);
    let max_scroll = content_height.saturating_sub(inner_height as usize) as u16;
    let scroll = if agent.chat_follow_output {
        max_scroll
    } else {
        agent.chat_scroll.min(max_scroll)
    };

    let chat = Paragraph::new(text)
        .block(UiSupport::panel_block().title(title))
        .style(UiSupport::panel_style())
        .scroll((scroll, 0))
        .wrap(Wrap { trim: false });
    frame.render_widget(chat, area);
    UiSupport::render_vertical_scrollbar(frame, area, content_height, scroll);
}

pub(in crate::app) fn draw_chat_input(frame: &mut Frame, app: &App, area: Rect) {
    let shell_mode = ChatSupport::input_is_shell(&app.chat_input);
    let thinking = app.active_agent().is_some_and(|agent| agent.thinking);
    let shell_running = app.active_agent().is_some_and(|agent| agent.shell_running);
    let title = if shell_running {
        format!(
            "Shell · {}  {} Running...",
            app.active_chat_model_label(),
            SPINNER[app.spinner_index]
        )
    } else if shell_mode {
        format!("Shell · {}", app.active_chat_model_label())
    } else if thinking {
        format!(
            "Message · {}  {} Thinking...",
            app.active_chat_model_label(),
            SPINNER[app.spinner_index]
        )
    } else {
        format!("Message · {}", app.active_chat_model_label())
    };

    let wrapped_lines = wrapped_chat_input_lines(&app.chat_input, area.width.saturating_sub(2));
    let input = Paragraph::new(Text::from(
        wrapped_lines
            .iter()
            .cloned()
            .map(Line::from)
            .collect::<Vec<_>>(),
    ))
    .block(UiSupport::panel_block().title(title))
    .style(UiSupport::panel_style())
    .wrap(Wrap { trim: false });
    frame.render_widget(input, area);

    let last_line = wrapped_lines
        .last()
        .map(|line| line.chars().count())
        .unwrap_or(0) as u16;
    let cursor_row = wrapped_lines.len().saturating_sub(1) as u16;
    let x = area
        .x
        .saturating_add(1 + last_line)
        .min(area.x + area.width.saturating_sub(2));
    let y = area
        .y
        .saturating_add(1 + cursor_row)
        .min(area.y + area.height.saturating_sub(2));
    frame.set_cursor_position((x, y));
}

pub(in crate::app) fn draw_add_agent_form(frame: &mut Frame, app: &App, area: Rect) {
    frame.render_widget(Clear, area);
    let outer = UiSupport::panel_block().title("New Agent");
    let inner = outer.inner(area);
    frame.render_widget(outer, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(3),
        ])
        .margin(1)
        .split(inner);

    let help = Paragraph::new("Use Tab to switch fields and Enter to save the agent.")
        .style(UiSupport::muted_panel_style());
    frame.render_widget(help, chunks[0]);

    let name_title = if app.add_form.active_field == AddAgentField::Name {
        "Name *"
    } else {
        "Name"
    };
    let workspace_title = if app.add_form.active_field == AddAgentField::Workspace {
        "Workspace *"
    } else {
        "Workspace"
    };

    let name = Paragraph::new(app.add_form.name.as_str())
        .block(UiSupport::input_block().title(name_title))
        .style(UiSupport::input_style());
    let workspace = Paragraph::new(app.add_form.workspace.as_str())
        .block(UiSupport::input_block().title(workspace_title))
        .style(UiSupport::input_style());
    frame.render_widget(name, chunks[1]);
    frame.render_widget(workspace, chunks[2]);

    let status = app
        .add_form
        .error
        .clone()
        .unwrap_or_else(|| "Saved agents live in ~/.cmdex.yml".to_string());
    let status = Paragraph::new(status).style(Style::default().bg(UiSupport::theme().panel_bg).fg(
        if app.add_form.error.is_some() {
            UiSupport::theme().error
        } else {
            UiSupport::theme().muted
        },
    ));
    frame.render_widget(status, chunks[3]);

    let target = match app.add_form.active_field {
        AddAgentField::Name => chunks[1],
        AddAgentField::Workspace => chunks[2],
    };
    let content = match app.add_form.active_field {
        AddAgentField::Name => &app.add_form.name,
        AddAgentField::Workspace => &app.add_form.workspace,
    };
    let cursor_x = target
        .x
        .saturating_add(1 + content.chars().count() as u16)
        .min(target.x + target.width.saturating_sub(2));
    frame.set_cursor_position((cursor_x, target.y + 1));
}

pub(in crate::app) fn chat_input_height_for_main_area(input: &str, main_area: Rect) -> u16 {
    let available = main_area.height.saturating_sub(3);
    if available == 0 {
        return 0;
    }

    let desired = wrapped_chat_input_lines(input, main_area.width.saturating_sub(2))
        .len()
        .saturating_add(2) as u16;
    let min_height = available.min(3);
    let max_height = available.saturating_sub(1).max(min_height);

    desired.clamp(min_height, max_height)
}

pub(in crate::app) fn wrapped_chat_input_lines(input: &str, width: u16) -> Vec<String> {
    let width = usize::from(width.max(1));
    if input.is_empty() {
        return vec![String::new()];
    }

    let mut wrapped = Vec::new();
    for raw_line in input.split('\n') {
        let mut current = String::new();
        let mut count = 0usize;

        for character in raw_line.chars() {
            if count == width {
                wrapped.push(current);
                current = String::new();
                count = 0;
            }

            current.push(character);
            count += 1;
        }

        wrapped.push(current);
    }

    if wrapped.is_empty() {
        wrapped.push(String::new());
    }

    wrapped
}

impl App {
    pub(in crate::app) fn handle_chat_sidebar_click(
        &mut self,
        column: u16,
        row: u16,
        sidebar_list: Rect,
    ) {
        let inner = UiSupport::inner_rect(sidebar_list);
        if inner.height == 0 || !UiSupport::rect_contains(inner, column, row) {
            return;
        }

        let visible_row = row.saturating_sub(inner.y) as usize;
        let total = self.agents.len() + 1;
        let offset = UiSupport::list_offset(self.chat_sidebar_index, total, inner.height as usize);
        let index = (offset + visible_row).min(total.saturating_sub(1));
        self.select_chat_sidebar_index(index);
    }

    pub(in crate::app) fn select_chat_sidebar_index(&mut self, index: usize) {
        self.chat_sidebar_index = index.min(self.agents.len());
        if self.chat_sidebar_index > 0 {
            self.current_agent = Some(self.chat_sidebar_index - 1);
            self.add_form.error = None;
        }
    }

    pub(in crate::app) fn submit_new_agent(
        &mut self,
        codex: CodexAppServer,
        ui_tx: mpsc::UnboundedSender<UiEvent>,
    ) {
        match ConfigStore::validate_agent_input(&self.add_form.name, &self.add_form.workspace) {
            Ok(agent) => {
                self.config.agents.push(agent.clone());
                if let Err(error) = ConfigStore::save(&self.config_path, &self.config) {
                    self.add_form.error = Some(error.to_string());
                    return;
                }

                self.agents.push(AgentState::new(
                    agent,
                    self.default_chat_model.clone(),
                    self.default_chat_reasoning_effort.clone(),
                    &self.chat_model_label,
                ));
                let new_index = self.agents.len().saturating_sub(1);
                self.current_agent = Some(new_index);
                self.chat_sidebar_index = new_index + 1;
                self.add_form = AddAgentForm::default();
                self.status_message = Some("Agent saved to ~/.cmdex.yml".to_string());
                SessionLoader::spawn(
                    codex,
                    ui_tx,
                    new_index,
                    self.agents[new_index].definition.workspace.clone(),
                );
            }
            Err(error) => {
                self.add_form.error = Some(error.to_string());
            }
        }
    }

    pub(in crate::app) fn submit_message(
        &mut self,
        codex: CodexAppServer,
        ui_tx: mpsc::UnboundedSender<UiEvent>,
    ) {
        if let Some(command) = ChatSupport::command_from_input(&self.chat_input) {
            self.submit_chat_command(command, codex, ui_tx);
            return;
        }

        if let Some(command) = ChatSupport::shell_command_from_input(&self.chat_input) {
            self.submit_shell_command(command, ui_tx);
            return;
        }

        let text = self.chat_input.trim().to_string();
        if text.is_empty() {
            return;
        }

        let Some(agent_index) = self.current_agent else {
            self.status_message = Some("Add an agent before sending messages.".to_string());
            return;
        };

        let agent = &mut self.agents[agent_index];
        if agent.thinking || agent.shell_running {
            self.status_message = Some("Wait for the current response to finish.".to_string());
            return;
        }

        agent
            .messages
            .push(ChatMessage::new(MessageRole::User, text.clone(), None));
        agent.thinking = true;
        agent.status = None;
        let existing_thread = agent.thread_id.clone();
        let thread_loaded = agent.thread_loaded;
        let selected_model = agent.chat_model.clone();
        let selected_effort = agent.chat_reasoning_effort.clone();
        let workspace = agent.definition.workspace.clone();
        self.chat_input.clear();

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
        &mut self,
        command: ChatCommand,
        codex: CodexAppServer,
        ui_tx: mpsc::UnboundedSender<UiEvent>,
    ) {
        match command {
            ChatCommand::Model(command) => self.submit_model_command(command, codex, ui_tx),
        }
    }

    pub(in crate::app) fn submit_model_command(
        &mut self,
        command: ModelCommand,
        codex: CodexAppServer,
        ui_tx: mpsc::UnboundedSender<UiEvent>,
    ) {
        let Some(agent_index) = self.current_agent else {
            self.status_message = Some("Add an agent before changing the model.".to_string());
            return;
        };

        self.chat_input.clear();

        match command {
            ModelCommand::List => {
                let current_label = self.agents[agent_index].chat_model_label.clone();
                self.agents[agent_index].status = Some("Loading available models...".to_string());
                tokio::spawn(async move {
                    let message = match codex.list_models().await {
                        Ok(models) => format_model_list_message(&current_label, &models),
                        Err(error) => format!("Unable to load available models.\n\n{error}"),
                    };
                    let _ = ui_tx.send(UiEvent::ModelCommandResult {
                        agent_index,
                        message,
                    });
                });
            }
            ModelCommand::ResetDefault => {
                let agent = &mut self.agents[agent_index];
                agent.chat_model = self.default_chat_model.clone();
                agent.chat_reasoning_effort = self.default_chat_reasoning_effort.clone();
                agent.chat_model_label = self.chat_model_label.clone();
                agent.chat_settings_explicit = false;
                let message = format!("Model set to `{}`.", agent.chat_model_label);
                agent.status = Some(message.clone());
                agent
                    .messages
                    .push(ChatMessage::new(MessageRole::System, message, None));
                self.status_message = Some("Model updated".to_string());
            }
            ModelCommand::Set { model, effort } => {
                let default_model = self.default_chat_model.clone();
                let default_label = self.chat_model_label.clone();
                let agent = &mut self.agents[agent_index];
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
                agent
                    .messages
                    .push(ChatMessage::new(MessageRole::System, message, None));
                self.status_message = Some("Model updated".to_string());
            }
        }
    }

    pub(in crate::app) fn submit_shell_command(
        &mut self,
        command: String,
        ui_tx: mpsc::UnboundedSender<UiEvent>,
    ) {
        let Some(agent_index) = self.current_agent else {
            self.status_message = Some("Add an agent before running shell commands.".to_string());
            return;
        };

        let agent = &mut self.agents[agent_index];
        if agent.thinking || agent.shell_running {
            self.status_message = Some("Wait for the current response to finish.".to_string());
            return;
        }

        agent.messages.push(ChatMessage::new(
            MessageRole::Shell,
            format!("> {command}"),
            None,
        ));
        agent.shell_running = true;
        agent.status = None;
        let workspace = agent.definition.workspace.clone();
        self.chat_input.clear();

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

    pub(in crate::app) fn interrupt_active_chat_turn(
        &mut self,
        codex: CodexAppServer,
        ui_tx: mpsc::UnboundedSender<UiEvent>,
    ) {
        if self.add_agent_selected() {
            return;
        }

        let Some(agent_index) = self.current_agent else {
            return;
        };
        let agent = &mut self.agents[agent_index];

        if !agent.thinking || agent.shell_running {
            return;
        }

        let (Some(thread_id), Some(turn_id)) =
            (agent.thread_id.clone(), agent.active_turn_id.clone())
        else {
            return;
        };

        agent.status = Some("Canceling response...".to_string());
        self.status_message = Some("Canceling response...".to_string());

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
