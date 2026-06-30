use std::path::Path;

use super::*;

pub(super) const SHELL_COMMAND_SENTINEL: &str = "__CMDEX_DONE__:";
pub(super) struct ShellPresenter;

#[derive(Debug, Clone, Default)]
pub(super) struct ShellTabState {
    pub(super) sessions: Vec<ShellSessionState>,
    pub(super) selected: usize,
    pub(super) input: String,
    next_session_id: usize,
}

#[derive(Debug, Clone)]
pub(super) struct ShellSessionState {
    pub(super) id: usize,
    pub(super) title: String,
    pub(super) lines: Vec<Line<'static>>,
    pub(super) scroll: u16,
    pub(super) running: bool,
}

impl ShellTabState {
    pub(super) fn create_session(&mut self, workspace: &Path) -> usize {
        self.next_session_id += 1;
        let id = self.next_session_id;
        self.sessions.push(ShellSessionState::new(id, workspace));
        self.selected = self.sessions.len().saturating_sub(1);
        id
    }

    pub(super) fn create_session_if_empty(&mut self, workspace: &Path) -> Option<usize> {
        if self.sessions.is_empty() {
            Some(self.create_session(workspace))
        } else {
            None
        }
    }

    pub(super) fn selected_index(&self) -> usize {
        self.selected.min(self.sessions.len().saturating_sub(1))
    }

    pub(super) fn selected_session(&self) -> Option<&ShellSessionState> {
        self.sessions.get(self.selected_index())
    }

    pub(super) fn selected_session_mut(&mut self) -> Option<&mut ShellSessionState> {
        let index = self.selected_index();
        self.sessions.get_mut(index)
    }

    pub(super) fn session_by_id_mut(
        &mut self,
        session_id: usize,
    ) -> Option<&mut ShellSessionState> {
        self.sessions
            .iter_mut()
            .find(|session| session.id == session_id)
    }

    pub(super) fn remove_session_by_id(&mut self, session_id: usize) -> Option<ShellSessionState> {
        let index = self
            .sessions
            .iter()
            .position(|session| session.id == session_id)?;
        let removed = self.sessions.remove(index);

        if self.sessions.is_empty() {
            self.selected = 0;
        } else if self.selected > index {
            self.selected -= 1;
        } else {
            self.selected = self.selected.min(self.sessions.len().saturating_sub(1));
        }

        Some(removed)
    }

    pub(super) fn select(&mut self, index: usize) {
        if self.sessions.is_empty() {
            return;
        }
        self.selected = index.min(self.sessions.len().saturating_sub(1));
    }

    pub(super) fn move_up(&mut self) {
        self.select(self.selected_index().saturating_sub(1));
    }

    pub(super) fn move_down(&mut self) {
        if self.sessions.is_empty() {
            return;
        }
        self.select((self.selected_index() + 1).min(self.sessions.len().saturating_sub(1)));
    }

    pub(super) fn labels(&self, spinner_index: usize) -> Vec<String> {
        self.sessions
            .iter()
            .map(|session| session.label(spinner_index))
            .collect()
    }
}

impl ShellSessionState {
    fn new(id: usize, workspace: &Path) -> Self {
        let mut lines = Vec::new();
        lines.push(Line::from(Span::styled(
            format!(
                "Session started in {}",
                ConfigStore::compact_home(workspace)
            ),
            Style::default().fg(ThemeRegistry::app().muted),
        )));
        lines.push(Line::default());

        Self {
            id,
            title: format!("Session {id}"),
            lines,
            scroll: 0,
            running: false,
        }
    }

    pub(super) fn label(&self, spinner_index: usize) -> String {
        if self.running {
            format!("{} {}", SPINNER[spinner_index % SPINNER.len()], self.title)
        } else {
            self.title.clone()
        }
    }

    pub(super) fn append_command(&mut self, command: &str) {
        self.lines.push(Line::from(Span::styled(
            format!("$ {command}"),
            Style::default().fg(ThemeRegistry::app().accent),
        )));
        self.running = true;
        self.scroll = u16::MAX;
    }

    pub(super) fn append_stdout_line(&mut self, line: &str) {
        self.lines.push(Line::from(line.to_string()));
        self.scroll = u16::MAX;
    }

    pub(super) fn append_stderr_line(&mut self, line: &str) {
        self.lines.push(Line::from(Span::styled(
            line.to_string(),
            Style::default().fg(ThemeRegistry::app().error),
        )));
        self.scroll = u16::MAX;
    }

    pub(super) fn finish_command(&mut self, exit_code: i32) {
        self.running = false;
        self.lines.push(Line::from(Span::styled(
            format!("[exit {exit_code}]"),
            Style::default().fg(ThemeRegistry::app().muted),
        )));
        self.scroll = u16::MAX;
    }
}

impl ShellPresenter {
    pub(super) fn command_payload(command: &str) -> String {
        format!("{command}\nprintf '{SHELL_COMMAND_SENTINEL}%s\\n' \"$?\"\n")
    }

    pub(super) fn prompt_text(input: &str) -> String {
        format!("$ {input}")
    }

    pub(super) fn display_lines(session: &ShellSessionState, input: &str) -> Vec<Line<'static>> {
        let mut lines = session.lines.clone();
        if !session.running {
            lines.push(Line::from(vec![
                Span::styled("$ ", Style::default().fg(ThemeRegistry::app().accent)),
                Span::raw(input.to_string()),
            ]));
        }
        lines
    }
}
