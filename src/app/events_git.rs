use super::{components::GitDiffComponent, event_types::GitEvent, *};

impl App {
    pub(super) fn handle_git_event(&mut self, event: GitEvent) {
        match event {
            GitEvent::RemoteCompleted {
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
            GitEvent::MutationCompleted {
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
            GitEvent::Loaded {
                agent_index,
                generation,
                result,
                error,
            } => {
                if let Some(agent) = self.agents.get_mut(agent_index) {
                    agent.git_diff.apply_load_result(generation, result, error);
                }
            }
        }
    }
}
