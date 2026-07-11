use super::{
    components::*,
    input::{AppInput, AppOutcome},
    *,
};

impl App {
    pub(super) fn handle_input(
        &mut self,
        input: AppInput,
        codex: &CodexAppServer,
        ui_tx: &mpsc::UnboundedSender<UiEvent>,
        area: Rect,
    ) -> AppOutcome {
        match input {
            AppInput::Key(key) => self
                .handle_key(key, codex, ui_tx, area)
                .map_or(AppOutcome::Redraw, AppOutcome::Exit),
            AppInput::Mouse(mouse) => {
                self.handle_mouse(mouse, area, ui_tx);
                AppOutcome::Redraw
            }
            AppInput::Paste(text) => {
                for character in text.chars() {
                    self.handle_text_input(character);
                }
                AppOutcome::Redraw
            }
            AppInput::Tick => {
                if self.on_tick(ui_tx) {
                    AppOutcome::Redraw
                } else {
                    AppOutcome::Handled
                }
            }
        }
    }

    pub(super) fn on_tick(&mut self, ui_tx: &mpsc::UnboundedSender<UiEvent>) -> bool {
        let mut needs_redraw = false;

        if self.has_active_animation() {
            self.spinner_index = (self.spinner_index + 1) % SPINNER.len();
            needs_redraw = true;
        }

        if self.dispatch_pending_workspace_hover(ui_tx) {
            needs_redraw = true;
        }

        let search_requested = WorkspaceScreen::maybe_search(self);
        search_requested || needs_redraw
    }

    pub(super) fn tick_interval(&self) -> Duration {
        if self.has_active_animation() {
            FAST_TICK_INTERVAL
        } else if self.current_tab == AppTab::Workspace {
            WORKSPACE_TICK_INTERVAL
        } else {
            IDLE_TICK_INTERVAL
        }
    }

    fn has_active_animation(&self) -> bool {
        let Some(agent) = self.active_agent() else {
            return false;
        };

        match self.current_tab {
            AppTab::Chat => agent.chat.thinking || agent.chat.shell_running,
            AppTab::Workspace => self.has_active_workspace_lsp_startup(),
            AppTab::Shell => agent
                .shell_tab
                .sessions
                .iter()
                .any(|session| !session.ready || session.running),
            AppTab::GitDiff => {
                agent.git_diff.remote_action.is_some() || agent.git_diff.mutation_running
            }
        }
    }

    pub(super) fn shutdown_shell_sessions(&mut self) -> Vec<effects::AppEffect> {
        self.shell_runtimes
            .drain()
            .map(|(_, runtime)| runtime.pid)
            .filter(|pid| *pid != 0)
            .map(|pid| effects::AppEffect::StopShellSession { pid })
            .collect()
    }

    pub(super) fn shutdown_lsp_sessions(&mut self) -> Vec<effects::AppEffect> {
        self.lsp_starting.clear();
        self.pending_lsp_commands.clear();
        self.lsp_runtimes
            .drain()
            .map(|(_, runtime)| effects::AppEffect::StopLspSession {
                command_tx: runtime.command_tx,
            })
            .collect()
    }

    pub(super) fn shutdown_workspace_watchers(&mut self) -> Vec<effects::AppEffect> {
        self.workspace_watchers
            .drain()
            .map(|(_, stop_tx)| effects::AppEffect::StopWorkspaceWatcher { stop_tx })
            .collect()
    }

    pub(super) fn handle_key(
        &mut self,
        key: KeyEvent,
        codex: &CodexAppServer,
        ui_tx: &mpsc::UnboundedSender<UiEvent>,
        area: Rect,
    ) -> Option<AppExit> {
        if ChatComponent::handle_key(self, key) {
            return None;
        }
        if AddAgentDialogComponent::handle_key(self, key, codex, ui_tx.clone()) {
            return None;
        }
        if WorkspaceScreen::handle_key_with_context(self, key, ui_tx, area) {
            return None;
        }
        if GitDiffComponent::handle_key(self, key) {
            return None;
        }
        if ShellComponent::handle_key(self, key, ui_tx.clone()) {
            return None;
        }
        match key.code {
            KeyCode::Char('q') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                Some(AppExit::Quit)
            }
            KeyCode::Char('r') if key.modifiers.contains(KeyModifiers::ALT) => {
                Some(AppExit::Restart)
            }
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                Some(AppExit::Quit)
            }
            KeyCode::Char('t') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                ShellComponent::open_tab_and_create_session(self, ui_tx.clone());
                None
            }
            KeyCode::Left if key.modifiers.contains(KeyModifiers::CONTROL) => {
                let tab = match self.current_tab {
                    AppTab::Chat => AppTab::GitDiff,
                    AppTab::Workspace => AppTab::Chat,
                    AppTab::Shell => AppTab::Workspace,
                    AppTab::GitDiff => AppTab::Shell,
                };
                TopNavigationComponent::activate_tab(self, tab, ui_tx.clone());
                None
            }
            KeyCode::Right if key.modifiers.contains(KeyModifiers::CONTROL) => {
                let tab = match self.current_tab {
                    AppTab::Chat => AppTab::Workspace,
                    AppTab::Workspace => AppTab::Shell,
                    AppTab::Shell => AppTab::GitDiff,
                    AppTab::GitDiff => AppTab::Chat,
                };
                TopNavigationComponent::activate_tab(self, tab, ui_tx.clone());
                None
            }
            KeyCode::Up => {
                self.move_selection_up();
                None
            }
            KeyCode::Down => {
                self.move_selection_down();
                None
            }
            KeyCode::PageUp => {
                if self.current_tab == AppTab::Chat && !self.add_agent_selected() {
                    ChatComponent::scroll_up(
                        self,
                        self.compute_layout(area).body,
                        CONTENT_SCROLL_STEP,
                    );
                } else {
                    self.scroll_content_up(self.compute_layout(area).body);
                }
                None
            }
            KeyCode::PageDown => {
                if self.current_tab == AppTab::Chat && !self.add_agent_selected() {
                    ChatComponent::scroll_down(
                        self,
                        self.compute_layout(area).body,
                        CONTENT_SCROLL_STEP,
                    );
                } else {
                    self.scroll_content_down(self.compute_layout(area).body);
                }
                None
            }
            KeyCode::F(5) => {
                TopNavigationComponent::refresh_current_tab(self);
                None
            }
            KeyCode::Backspace => {
                self.handle_backspace();
                None
            }
            KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.handle_text_input(character);
                None
            }
            _ => None,
        }
    }

    pub(super) fn handle_text_input(&mut self, character: char) {
        if AddAgentDialogComponent::handle_text_input(self, character) {
            return;
        }
        let _ = WorkspaceScreen::handle_text_input(self, character)
            || ShellComponent::handle_text_input(self, character)
            || GitDiffComponent::handle_text_input(self, character)
            || ChatComponent::handle_text_input(self, character);
    }

    fn handle_backspace(&mut self) {
        if AddAgentDialogComponent::handle_backspace(self) {
            return;
        }
        let _ = WorkspaceScreen::handle_backspace(self)
            || ShellComponent::handle_backspace(self)
            || GitDiffComponent::handle_backspace(self)
            || ChatComponent::handle_backspace(self);
    }
}
