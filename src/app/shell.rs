use std::{
    io::{BufRead, BufReader, Write},
    path::Path,
    sync::mpsc as std_mpsc,
    thread,
};

use super::*;
use anyhow::{Context, Result};
use portable_pty::{CommandBuilder, PtySize, native_pty_system};

pub(super) const SHELL_COMMAND_SENTINEL: &str = "__CMDEX_DONE__:";
pub(super) const SHELL_READY_SENTINEL: &str = "__CMDEX_READY__";
pub(super) const SHELL_SESSION_LOOP: &str = "stty -echo; printf '__CMDEX_READY__\\n'; while IFS= read -r cmd; do eval \"$cmd\"; printf '__CMDEX_DONE__:%s\\n' \"$?\"; done";
pub(super) struct ShellPresenter;
pub(super) struct ShellRuntimeFactory;

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
    pub(super) ready: bool,
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
            ready: false,
            running: false,
        }
    }

    pub(super) fn label(&self, spinner_index: usize) -> String {
        if !self.ready || self.running {
            format!("{} {}", SPINNER[spinner_index % SPINNER.len()], self.title)
        } else {
            self.title.clone()
        }
    }

    pub(super) fn mark_ready(&mut self) {
        self.ready = true;
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
        format!("{command}\n")
    }

    pub(super) fn prompt_text(input: &str) -> String {
        format!("$ {input}")
    }

    pub(super) fn display_lines(session: &ShellSessionState, input: &str) -> Vec<Line<'static>> {
        let mut lines = session.lines.clone();
        if session.ready && !session.running {
            lines.push(Line::from(vec![
                Span::styled("$ ", Style::default().fg(ThemeRegistry::app().accent)),
                Span::raw(input.to_string()),
            ]));
        }
        lines
    }
}

pub(super) struct ShellOutputParser {
    pending: String,
    escape_state: ShellEscapeState,
}

pub(super) enum ShellOutputRecord {
    Ready,
    Line(String),
    CommandFinished(i32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShellEscapeState {
    Ground,
    Escape,
    Csi,
    Osc,
    OscEscape,
}

impl ShellRuntimeFactory {
    pub(super) fn spawn(
        shell_path: &str,
        workspace: &Path,
        agent_index: usize,
        session_id: usize,
        ui_tx: mpsc::UnboundedSender<UiEvent>,
    ) -> Result<(std_mpsc::Sender<String>, u32)> {
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows: 40,
                cols: 120,
                pixel_width: 0,
                pixel_height: 0,
            })
            .context("failed to open PTY for shell session")?;

        let mut command = CommandBuilder::new(shell_path);
        command.arg("-c");
        command.arg(SHELL_SESSION_LOOP);
        command.cwd(workspace);

        let child = pair
            .slave
            .spawn_command(command)
            .context("failed to spawn shell PTY session")?;
        let pid = child.process_id().unwrap_or_default();

        let reader = pair
            .master
            .try_clone_reader()
            .context("failed to clone PTY reader")?;
        let writer = pair
            .master
            .take_writer()
            .context("failed to open PTY writer")?;

        let (command_tx, command_rx) = std_mpsc::channel::<String>();
        Self::spawn_writer(writer, command_rx, agent_index, session_id, ui_tx.clone())?;
        Self::spawn_reader(reader, agent_index, session_id, ui_tx.clone())?;
        Self::spawn_waiter(child, agent_index, session_id, ui_tx)?;

        Ok((command_tx, pid))
    }

    fn spawn_writer(
        mut writer: Box<dyn Write + Send>,
        command_rx: std_mpsc::Receiver<String>,
        agent_index: usize,
        session_id: usize,
        ui_tx: mpsc::UnboundedSender<UiEvent>,
    ) -> Result<()> {
        thread::Builder::new()
            .name(format!("cmdex-shell-writer-{agent_index}-{session_id}"))
            .spawn(move || {
                for command in command_rx {
                    if let Err(error) = writer.write_all(command.as_bytes()) {
                        let _ = ui_tx.send(UiEvent::ShellSessionExited {
                            agent_index,
                            session_id,
                            message: format!("Shell PTY write failed: {error}"),
                        });
                        break;
                    }
                    if let Err(error) = writer.flush() {
                        let _ = ui_tx.send(UiEvent::ShellSessionExited {
                            agent_index,
                            session_id,
                            message: format!("Shell PTY flush failed: {error}"),
                        });
                        break;
                    }
                }
            })
            .context("failed to spawn PTY shell writer thread")?;
        Ok(())
    }

    fn spawn_reader(
        reader: Box<dyn std::io::Read + Send>,
        agent_index: usize,
        session_id: usize,
        ui_tx: mpsc::UnboundedSender<UiEvent>,
    ) -> Result<()> {
        thread::Builder::new()
            .name(format!("cmdex-shell-reader-{agent_index}-{session_id}"))
            .spawn(move || {
                let mut parser = ShellOutputParser::new();
                let mut reader = BufReader::new(reader);
                let mut buffer = Vec::new();

                loop {
                    buffer.clear();
                    match reader.read_until(b'\n', &mut buffer) {
                        Ok(0) => {
                            for record in parser.finish() {
                                Self::dispatch_record(record, agent_index, session_id, &ui_tx);
                            }
                            break;
                        }
                        Ok(_) => {
                            let chunk = String::from_utf8_lossy(&buffer);
                            for record in parser.push(&chunk) {
                                Self::dispatch_record(record, agent_index, session_id, &ui_tx);
                            }
                        }
                        Err(error) => {
                            let _ = ui_tx.send(UiEvent::ShellSessionExited {
                                agent_index,
                                session_id,
                                message: format!("Shell PTY read failed: {error}"),
                            });
                            break;
                        }
                    }
                }
            })
            .context("failed to spawn PTY shell reader thread")?;
        Ok(())
    }

    fn spawn_waiter(
        mut child: Box<dyn portable_pty::Child + Send>,
        agent_index: usize,
        session_id: usize,
        ui_tx: mpsc::UnboundedSender<UiEvent>,
    ) -> Result<()> {
        thread::Builder::new()
            .name(format!("cmdex-shell-wait-{agent_index}-{session_id}"))
            .spawn(move || {
                let message = match child.wait() {
                    Ok(status) => format!("Shell exited with status {}", status),
                    Err(error) => format!("Shell wait failed: {error}"),
                };
                let _ = ui_tx.send(UiEvent::ShellSessionExited {
                    agent_index,
                    session_id,
                    message,
                });
            })
            .context("failed to spawn PTY shell wait thread")?;
        Ok(())
    }

    fn dispatch_record(
        record: ShellOutputRecord,
        agent_index: usize,
        session_id: usize,
        ui_tx: &mpsc::UnboundedSender<UiEvent>,
    ) {
        match record {
            ShellOutputRecord::Ready => {
                let _ = ui_tx.send(UiEvent::ShellSessionReady {
                    agent_index,
                    session_id,
                });
            }
            ShellOutputRecord::Line(line) => {
                let _ = ui_tx.send(UiEvent::ShellSessionOutput {
                    agent_index,
                    session_id,
                    line,
                    stderr: false,
                });
            }
            ShellOutputRecord::CommandFinished(exit_code) => {
                let _ = ui_tx.send(UiEvent::ShellSessionCommandFinished {
                    agent_index,
                    session_id,
                    exit_code,
                });
            }
        }
    }
}

impl ShellOutputParser {
    pub(super) fn new() -> Self {
        Self {
            pending: String::new(),
            escape_state: ShellEscapeState::Ground,
        }
    }

    pub(super) fn push(&mut self, chunk: &str) -> Vec<ShellOutputRecord> {
        for character in chunk.chars() {
            self.consume_char(character);
        }
        let mut records = Vec::new();

        while let Some(index) = self.pending.find('\n') {
            let line = self.pending[..index].to_string();
            self.pending.drain(..=index);
            self.process_record(&line, &mut records);
        }

        records
    }

    fn finish(&mut self) -> Vec<ShellOutputRecord> {
        let mut records = Vec::new();
        if !self.pending.is_empty() {
            let line = std::mem::take(&mut self.pending);
            self.process_record(&line, &mut records);
        }
        records
    }

    fn process_record(&mut self, raw: &str, records: &mut Vec<ShellOutputRecord>) {
        let trailing_crlf = raw.ends_with('\r');
        for (index, segment) in raw.split('\r').enumerate() {
            if trailing_crlf && index > 0 && segment.is_empty() {
                continue;
            }
            self.process_segment(segment, records);
        }
    }

    fn process_segment(&mut self, segment: &str, records: &mut Vec<ShellOutputRecord>) {
        if segment.trim() == SHELL_READY_SENTINEL {
            records.push(ShellOutputRecord::Ready);
            return;
        }

        if let Some(position) = segment.find(SHELL_COMMAND_SENTINEL) {
            let before = &segment[..position];
            if !before.is_empty() {
                records.push(ShellOutputRecord::Line(before.to_string()));
            }

            let code = segment[position + SHELL_COMMAND_SENTINEL.len()..]
                .trim()
                .parse::<i32>()
                .unwrap_or(-1);
            records.push(ShellOutputRecord::CommandFinished(code));
            return;
        }

        records.push(ShellOutputRecord::Line(segment.to_string()));
    }

    fn consume_char(&mut self, character: char) {
        match self.escape_state {
            ShellEscapeState::Ground => match character {
                '\u{1b}' => self.escape_state = ShellEscapeState::Escape,
                '\u{08}' => {
                    self.pending.pop();
                }
                '\n' | '\r' => self.pending.push(character),
                '\t' => self.pending.push(' '),
                control if control.is_control() => {}
                _ => self.pending.push(character),
            },
            ShellEscapeState::Escape => match character {
                '[' => self.escape_state = ShellEscapeState::Csi,
                ']' => self.escape_state = ShellEscapeState::Osc,
                _ => self.escape_state = ShellEscapeState::Ground,
            },
            ShellEscapeState::Csi => {
                if ('@'..='~').contains(&character) {
                    self.escape_state = ShellEscapeState::Ground;
                }
            }
            ShellEscapeState::Osc => match character {
                '\u{07}' => self.escape_state = ShellEscapeState::Ground,
                '\u{1b}' => self.escape_state = ShellEscapeState::OscEscape,
                _ => {}
            },
            ShellEscapeState::OscEscape => {
                self.escape_state = if character == '\\' {
                    ShellEscapeState::Ground
                } else {
                    ShellEscapeState::Osc
                };
            }
        }
    }
}
