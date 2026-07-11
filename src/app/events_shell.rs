use super::{
    components::{GitDiffComponent, WorkspaceScreen},
    event_types::ShellEvent,
    *,
};

impl App {
    pub(super) fn handle_shell_event(&mut self, event: ShellEvent) {
        match event {
            ShellEvent::RuntimeReady {
                agent_index,
                session_id,
                command_tx,
                pid,
            } => {
                if !self.agents.get(agent_index).is_some_and(|agent| {
                    agent
                        .shell_tab
                        .sessions
                        .iter()
                        .any(|session| session.id == session_id)
                }) {
                    return;
                }
                self.shell_runtimes.insert(
                    ShellSessionKey {
                        agent_index,
                        session_id,
                    },
                    ShellSessionRuntime { command_tx, pid },
                );
            }
            ShellEvent::CommandCompleted {
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
                WorkspaceScreen::request_refresh_for_agent(self, agent_index);
                GitDiffComponent::request_refresh_for_agent(self, agent_index);
            }
            ShellEvent::SessionReady {
                agent_index,
                session_id,
            } => {
                if let Some(agent) = self.agents.get_mut(agent_index)
                    && let Some(session) = agent.shell_tab.session_by_id_mut(session_id)
                {
                    session.mark_ready();
                }
            }
            ShellEvent::SessionOutput {
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
            ShellEvent::SessionCommandFinished {
                agent_index,
                session_id,
                exit_code,
            } => {
                if let Some(agent) = self.agents.get_mut(agent_index)
                    && let Some(session) = agent.shell_tab.session_by_id_mut(session_id)
                {
                    session.finish_command(exit_code);
                }
                WorkspaceScreen::request_refresh_for_agent(self, agent_index);
                GitDiffComponent::request_refresh_for_agent(self, agent_index);
            }
            ShellEvent::SessionExited {
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
        }
    }
}
