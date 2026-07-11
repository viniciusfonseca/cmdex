use super::super::effects::AppEffect;
use super::super::{shell::ShellPresenter, *};
use super::{ChatInputComponent, TopNavigationComponent, UiSupport};

pub(in crate::app) struct ShellComponent;

impl ShellComponent {
    pub(in crate::app) fn move_selection_up(app: &mut App) -> bool {
        if app.current_tab != AppTab::Shell {
            return false;
        }
        if let Some(agent) = app.active_agent_mut() {
            agent.shell_tab.move_up();
        }
        true
    }

    pub(in crate::app) fn move_selection_down(app: &mut App) -> bool {
        if app.current_tab != AppTab::Shell {
            return false;
        }
        if let Some(agent) = app.active_agent_mut() {
            agent.shell_tab.move_down();
        }
        true
    }

    pub(in crate::app) fn handle_key(
        app: &mut App,
        key: KeyEvent,
        ui_tx: mpsc::UnboundedSender<UiEvent>,
    ) -> bool {
        if app.current_tab != AppTab::Shell || key.code != KeyCode::Enter {
            return false;
        }
        Self::submit_command(app, ui_tx);
        true
    }

    pub(in crate::app) fn scroll_output(app: &mut App, lines: u16, up: bool) -> bool {
        if app.current_tab != AppTab::Shell {
            return false;
        }
        if let Some(agent) = app.active_agent_mut()
            && let Some(session) = agent.shell_tab.selected_session_mut()
        {
            if up {
                session.scroll = session.scroll.saturating_sub(lines);
            } else {
                session.scroll = session.scroll.saturating_add(lines);
            }
        }
        true
    }

    pub(in crate::app) fn handle_text_input(app: &mut App, character: char) -> bool {
        if app.current_tab != AppTab::Shell {
            return false;
        }
        if let Some(agent) = app.active_agent_mut() {
            agent.shell_tab.input.push(character);
            if let Some(session) = agent.shell_tab.selected_session_mut() {
                session.scroll = u16::MAX;
            }
        }
        true
    }

    pub(in crate::app) fn handle_backspace(app: &mut App) -> bool {
        if app.current_tab != AppTab::Shell {
            return false;
        }
        if let Some(agent) = app.active_agent_mut() {
            agent.shell_tab.input.pop();
            if let Some(session) = agent.shell_tab.selected_session_mut() {
                session.scroll = u16::MAX;
            }
        }
        true
    }

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
        } else if !session.ready {
            format!(
                "{} · {} Starting...",
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
        if inner.width == 0 || inner.height == 0 || session.running || !session.ready {
            return;
        }

        let prompt_text = ShellPresenter::prompt_text(&agent.shell_tab.input);
        let prompt_lines = ChatInputComponent::wrapped_lines(&prompt_text, inner.width);
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

    pub(in crate::app) fn open_tab(app: &mut App, _ui_tx: mpsc::UnboundedSender<UiEvent>) {
        let Some(agent_index) = app.current_agent else {
            app.status_message = Some("Add an agent before creating a shell session.".to_string());
            return;
        };

        app.current_tab = AppTab::Shell;
        TopNavigationComponent::refresh_current_tab(app);
        let workspace = app.agents[agent_index].definition.workspace.clone();
        let Some(session_id) = app.agents[agent_index]
            .shell_tab
            .create_session_if_empty(&workspace)
        else {
            return;
        };
        Self::start_session(app, agent_index, session_id, workspace);
    }

    pub(in crate::app) fn open_tab_and_create_session(
        app: &mut App,
        _ui_tx: mpsc::UnboundedSender<UiEvent>,
    ) {
        let Some(agent_index) = app.current_agent else {
            app.status_message = Some("Add an agent before creating a shell session.".to_string());
            return;
        };

        app.current_tab = AppTab::Shell;
        TopNavigationComponent::refresh_current_tab(app);
        let workspace = app.agents[agent_index].definition.workspace.clone();
        let session_id = app.agents[agent_index].shell_tab.create_session(&workspace);
        Self::start_session(app, agent_index, session_id, workspace);
    }

    pub(in crate::app) fn submit_command(app: &mut App, _ui_tx: mpsc::UnboundedSender<UiEvent>) {
        let Some(agent_index) = app.current_agent else {
            app.status_message = Some("Add an agent before running shell commands.".to_string());
            return;
        };

        let Some(session_id) = app.agents[agent_index]
            .shell_tab
            .selected_session()
            .map(|session| session.id)
        else {
            app.status_message = Some("Click + New session or press Ctrl+T.".to_string());
            return;
        };

        let command = app.agents[agent_index].shell_tab.input.trim().to_string();
        if command.is_empty() {
            return;
        }

        let key = ShellSessionKey {
            agent_index,
            session_id,
        };
        let Some(runtime) = app.shell_runtimes.get(&key) else {
            app.status_message =
                Some("The selected shell session is no longer available.".to_string());
            app.agents[agent_index]
                .shell_tab
                .remove_session_by_id(session_id);
            return;
        };
        let command_tx = runtime.command_tx.clone();

        let Some(session) = app.agents[agent_index]
            .shell_tab
            .session_by_id_mut(session_id)
        else {
            return;
        };
        if session.running {
            app.status_message = Some("Wait for the current shell command to finish.".to_string());
            return;
        }
        if !session.ready {
            app.status_message = Some("Shell session is still starting.".to_string());
            return;
        }

        session.append_command(&command);
        app.agents[agent_index].shell_tab.input.clear();

        app.enqueue_effect(AppEffect::SendShellCommand {
            agent_index,
            session_id,
            command_tx,
            payload: ShellPresenter::command_payload(&command),
        });
    }

    fn start_session(app: &mut App, agent_index: usize, session_id: usize, workspace: PathBuf) {
        app.enqueue_effect(AppEffect::StartShellSession {
            agent_index,
            session_id,
            workspace,
        });
    }
}
