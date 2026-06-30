use super::super::{
    shell::{self, ShellPresenter},
    *,
};
use super::{chat::wrapped_chat_input_lines, shared::UiSupport};
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

pub(in crate::app) struct ShellView;

impl ShellView {
    pub(in crate::app) fn draw(frame: &mut Frame, app: &App, area: Rect) {
        let Some(agent) = app.active_agent() else {
            let empty = Paragraph::new("Add an agent from the sidebar to start a shell session.")
                .block(UiSupport::panel_block().title("Shell"))
                .style(UiSupport::panel_style());
            frame.render_widget(empty, area);
            return;
        };

        let Some(session) = agent.shell_tab.selected_session() else {
            let empty =
                Paragraph::new("Click + New session or press Ctrl+T to start a shell session.")
                    .block(UiSupport::panel_block().title("Shell"))
                    .style(UiSupport::panel_style());
            frame.render_widget(empty, area);
            return;
        };

        let title = if session.running {
            format!(
                "{} · {} Running...",
                session.title, SPINNER[app.spinner_index]
            )
        } else {
            session.title.clone()
        };
        let lines = ShellPresenter::display_lines(session, &agent.shell_tab.input);
        let content_length = UiSupport::scrollable_preview_content_height(&lines, area);
        let max_scroll =
            content_length.saturating_sub(area.height.saturating_sub(2) as usize) as u16;
        let scroll = session.scroll.min(max_scroll);
        let shell = Paragraph::new(Text::from(lines))
            .block(UiSupport::panel_block().title(title))
            .style(UiSupport::panel_style())
            .scroll((scroll, 0))
            .wrap(Wrap { trim: false });
        frame.render_widget(shell, area);
        UiSupport::render_vertical_scrollbar(frame, area, content_length, scroll);

        let inner = UiSupport::inner_rect(area);
        if inner.width == 0 || inner.height == 0 || session.running {
            return;
        }

        let prompt_text = ShellPresenter::prompt_text(&agent.shell_tab.input);
        let prompt_lines = wrapped_chat_input_lines(&prompt_text, inner.width);
        let prompt_last_line = prompt_lines
            .last()
            .map(|line| line.chars().count())
            .unwrap_or(0) as u16;
        let content_before_prompt =
            UiSupport::scrollable_preview_content_height(&session.lines, area);
        let prompt_row = content_before_prompt
            .saturating_add(prompt_lines.len().saturating_sub(1))
            .saturating_sub(scroll as usize) as u16;
        if prompt_row >= inner.height {
            return;
        }

        let x = inner
            .x
            .saturating_add(prompt_last_line)
            .min(inner.x + inner.width.saturating_sub(1));
        let y = inner.y.saturating_add(prompt_row);
        frame.set_cursor_position((x, y));
    }
}

impl App {
    pub(in crate::app) fn handle_shell_sidebar_click(
        &mut self,
        column: u16,
        row: u16,
        sidebar_list: Rect,
        ui_tx: &mpsc::UnboundedSender<UiEvent>,
    ) {
        let inner = UiSupport::inner_rect(sidebar_list);
        if inner.height == 0 || !UiSupport::rect_contains(inner, column, row) {
            return;
        }

        let visible_row = row.saturating_sub(inner.y) as usize;
        let clicked_index = self.active_agent().map(|agent| {
            let total = agent.shell_tab.sessions.len() + 1;
            let selected = if agent.shell_tab.sessions.is_empty() {
                0
            } else {
                agent.shell_tab.selected_index() + 1
            };
            let offset = UiSupport::list_offset(selected, total, inner.height as usize);
            (offset + visible_row).min(total.saturating_sub(1))
        });

        match clicked_index {
            Some(0) => self.open_shell_tab_and_create_session(ui_tx.clone()),
            Some(index) => {
                if let Some(agent) = self.active_agent_mut() {
                    agent.shell_tab.select(index - 1);
                }
            }
            None => {}
        }
    }

    pub(in crate::app) fn open_shell_tab(&mut self, ui_tx: mpsc::UnboundedSender<UiEvent>) {
        let Some(agent_index) = self.current_agent else {
            self.status_message = Some("Add an agent before creating a shell session.".to_string());
            return;
        };

        self.current_tab = AppTab::Shell;
        self.refresh_current_tab();
        let workspace = self.agents[agent_index].definition.workspace.clone();
        let Some(session_id) = self.agents[agent_index]
            .shell_tab
            .create_session_if_empty(&workspace)
        else {
            return;
        };
        self.start_shell_session(agent_index, session_id, workspace, ui_tx);
    }

    pub(in crate::app) fn open_shell_tab_and_create_session(
        &mut self,
        ui_tx: mpsc::UnboundedSender<UiEvent>,
    ) {
        let Some(agent_index) = self.current_agent else {
            self.status_message = Some("Add an agent before creating a shell session.".to_string());
            return;
        };

        self.current_tab = AppTab::Shell;
        self.refresh_current_tab();
        let workspace = self.agents[agent_index].definition.workspace.clone();
        let session_id = self.agents[agent_index]
            .shell_tab
            .create_session(&workspace);
        self.start_shell_session(agent_index, session_id, workspace, ui_tx);
    }

    fn start_shell_session(
        &mut self,
        agent_index: usize,
        session_id: usize,
        workspace: PathBuf,
        ui_tx: mpsc::UnboundedSender<UiEvent>,
    ) {
        if let Err(error) =
            self.spawn_shell_session_runtime(agent_index, session_id, workspace, ui_tx)
        {
            self.agents[agent_index]
                .shell_tab
                .remove_session_by_id(session_id);
            self.status_message = Some(error.to_string());
        }
    }

    pub(in crate::app) fn submit_shell_session_command(
        &mut self,
        _ui_tx: mpsc::UnboundedSender<UiEvent>,
    ) {
        let Some(agent_index) = self.current_agent else {
            self.status_message = Some("Add an agent before running shell commands.".to_string());
            return;
        };

        let Some(session_id) = self.agents[agent_index]
            .shell_tab
            .selected_session()
            .map(|session| session.id)
        else {
            self.status_message = Some("Click + New session or press Ctrl+T.".to_string());
            return;
        };

        let command = self.agents[agent_index].shell_tab.input.trim().to_string();
        if command.is_empty() {
            return;
        }

        let key = ShellSessionKey {
            agent_index,
            session_id,
        };
        let Some(runtime) = self.shell_runtimes.get(&key) else {
            self.status_message =
                Some("The selected shell session is no longer available.".to_string());
            self.agents[agent_index]
                .shell_tab
                .remove_session_by_id(session_id);
            return;
        };

        let Some(session) = self.agents[agent_index]
            .shell_tab
            .session_by_id_mut(session_id)
        else {
            return;
        };
        if session.running {
            self.status_message = Some("Wait for the current shell command to finish.".to_string());
            return;
        }

        session.append_command(&command);
        self.agents[agent_index].shell_tab.input.clear();

        if runtime
            .command_tx
            .send(ShellPresenter::command_payload(&command))
            .is_err()
        {
            self.agents[agent_index]
                .shell_tab
                .remove_session_by_id(session_id);
            self.shell_runtimes.remove(&key);
            self.status_message = Some("Failed to send command to shell.".to_string());
        }
    }

    fn spawn_shell_session_runtime(
        &mut self,
        agent_index: usize,
        session_id: usize,
        workspace: PathBuf,
        ui_tx: mpsc::UnboundedSender<UiEvent>,
    ) -> Result<()> {
        let shell_path = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
        let mut child = Command::new(&shell_path)
            .current_dir(&workspace)
            .env("PS1", "")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow::anyhow!("failed to open shell stdin"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow::anyhow!("failed to open shell stdout"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| anyhow::anyhow!("failed to open shell stderr"))?;
        let pid = child.id().unwrap_or_default();

        let (command_tx, mut command_rx) = mpsc::unbounded_channel::<String>();
        let writer_tx = ui_tx.clone();
        tokio::spawn(async move {
            let mut stdin = stdin;
            while let Some(command) = command_rx.recv().await {
                if let Err(error) = stdin.write_all(command.as_bytes()).await {
                    let _ = writer_tx.send(UiEvent::ShellSessionExited {
                        agent_index,
                        session_id,
                        message: format!("Shell stdin write failed: {error}"),
                    });
                    break;
                }
                if let Err(error) = stdin.flush().await {
                    let _ = writer_tx.send(UiEvent::ShellSessionExited {
                        agent_index,
                        session_id,
                        message: format!("Shell stdin flush failed: {error}"),
                    });
                    break;
                }
            }
        });

        let stdout_tx = ui_tx.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stdout).lines();
            loop {
                match lines.next_line().await {
                    Ok(Some(line)) => {
                        if let Some(code) = line.strip_prefix(shell::SHELL_COMMAND_SENTINEL) {
                            let exit_code = code.parse::<i32>().unwrap_or(-1);
                            let _ = stdout_tx.send(UiEvent::ShellSessionCommandFinished {
                                agent_index,
                                session_id,
                                exit_code,
                            });
                        } else {
                            let _ = stdout_tx.send(UiEvent::ShellSessionOutput {
                                agent_index,
                                session_id,
                                line,
                                stderr: false,
                            });
                        }
                    }
                    Ok(None) => break,
                    Err(error) => {
                        let _ = stdout_tx.send(UiEvent::ShellSessionExited {
                            agent_index,
                            session_id,
                            message: format!("Shell stdout read failed: {error}"),
                        });
                        break;
                    }
                }
            }
        });

        let stderr_tx = ui_tx.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            loop {
                match lines.next_line().await {
                    Ok(Some(line)) => {
                        let _ = stderr_tx.send(UiEvent::ShellSessionOutput {
                            agent_index,
                            session_id,
                            line,
                            stderr: true,
                        });
                    }
                    Ok(None) => break,
                    Err(error) => {
                        let _ = stderr_tx.send(UiEvent::ShellSessionExited {
                            agent_index,
                            session_id,
                            message: format!("Shell stderr read failed: {error}"),
                        });
                        break;
                    }
                }
            }
        });

        let exit_tx = ui_tx.clone();
        tokio::spawn(async move {
            let message = match child.wait().await {
                Ok(status) => format!("Shell exited with status {}", status),
                Err(error) => format!("Shell wait failed: {error}"),
            };
            let _ = exit_tx.send(UiEvent::ShellSessionExited {
                agent_index,
                session_id,
                message,
            });
        });

        self.shell_runtimes.insert(
            ShellSessionKey {
                agent_index,
                session_id,
            },
            ShellSessionRuntime { command_tx, pid },
        );
        Ok(())
    }
}
