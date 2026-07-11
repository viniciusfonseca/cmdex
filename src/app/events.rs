use super::{event_types::UiEvent, *};

impl App {
    pub(super) fn handle_ui_event(&mut self, event: UiEvent) {
        match event {
            UiEvent::Chat(event) => self.handle_chat_event(event),
            UiEvent::Shell(event) => self.handle_shell_event(event),
            UiEvent::Git(event) => self.handle_git_event(event),
            UiEvent::Workspace(event) => self.handle_workspace_event(event),
            UiEvent::Lsp(event) => self.handle_lsp_event(event),
        }
    }
}
