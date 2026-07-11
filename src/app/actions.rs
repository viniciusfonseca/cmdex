use super::{
    chat::ModelCommand,
    components::*,
    input::{AppInput, AppOutcome},
    lsp, *,
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

        let search_requested = WorkspaceComponent::maybe_search(self);
        WorkspaceComponent::maybe_refresh(self) || search_requested || needs_redraw
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

    pub(super) fn shutdown_shell_sessions(&mut self) {
        let pids = self
            .shell_runtimes
            .drain()
            .map(|(_, runtime)| runtime.pid)
            .filter(|pid| *pid != 0)
            .collect::<Vec<_>>();

        for pid in pids {
            let _ = std::process::Command::new("kill")
                .args(["-TERM", &pid.to_string()])
                .status();
        }
    }

    pub(super) fn shutdown_lsp_sessions(&mut self) {
        for (_, runtime) in self.lsp_runtimes.drain() {
            let _ = runtime.command_tx.send(lsp::LspCommand::Shutdown);
        }
    }

    pub(super) fn handle_key(
        &mut self,
        key: KeyEvent,
        codex: &CodexAppServer,
        ui_tx: &mpsc::UnboundedSender<UiEvent>,
        area: Rect,
    ) -> Option<AppExit> {
        match ChatComponent::handle_model_picker_key(self, key) {
            ModelPickerAction::NotOpen => {}
            ModelPickerAction::Handled => return None,
            ModelPickerAction::Apply {
                agent_index,
                model,
                effort,
            } => {
                if self.current_agent == Some(agent_index) {
                    ChatComponent::submit_model_command(
                        self,
                        ModelCommand::Set {
                            model: Some(model),
                            effort,
                        },
                    );
                }
                return None;
            }
        }
        if self.current_tab == AppTab::Workspace {
            self.clear_workspace_hover();
            if WorkspaceComponent::handle_completion_request(self, key, ui_tx) {
                return None;
            }
        }
        if WorkspaceComponent::handle_key(self, key, area) {
            return None;
        }
        if GitDiffComponent::handle_key(self, key) {
            return None;
        }
        if self.current_tab == AppTab::Chat && ChatComponent::handle_queue_key(self, key) {
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
            KeyCode::Enter
                if self.current_tab == AppTab::Chat
                    && !self.add_agent_selected()
                    && key.modifiers.contains(KeyModifiers::SHIFT) =>
            {
                self.chat_input.push('\n');
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
                    self.scroll_chat_up(self.compute_layout(area).body, CONTENT_SCROLL_STEP);
                } else {
                    self.scroll_content_up(self.compute_layout(area).body);
                }
                None
            }
            KeyCode::PageDown => {
                if self.current_tab == AppTab::Chat && !self.add_agent_selected() {
                    self.scroll_chat_down(self.compute_layout(area).body, CONTENT_SCROLL_STEP);
                } else {
                    self.scroll_content_down(self.compute_layout(area).body);
                }
                None
            }
            KeyCode::F(5) => {
                TopNavigationComponent::refresh_current_tab(self);
                None
            }
            KeyCode::Esc if self.add_agent_selected() => {
                self.add_form = AddAgentForm::default();
                if !self.agents.is_empty() {
                    self.chat_sidebar_index =
                        self.current_agent.map(|index| index + 1).unwrap_or(0);
                }
                None
            }
            KeyCode::Esc if self.current_tab == AppTab::Chat => {
                ChatComponent::interrupt_active_turn(self);
                None
            }
            KeyCode::Tab if self.add_agent_selected() => {
                self.add_form.active_field = match self.add_form.active_field {
                    AddAgentField::Name => AddAgentField::Workspace,
                    AddAgentField::Workspace => AddAgentField::Name,
                };
                None
            }
            KeyCode::Enter if self.add_agent_selected() => {
                AddAgentDialogComponent::submit(self, codex.clone(), ui_tx.clone());
                None
            }
            KeyCode::Enter if self.current_tab == AppTab::Chat => {
                ChatComponent::submit_message(self);
                None
            }
            KeyCode::Enter if self.current_tab == AppTab::Shell => {
                ShellComponent::submit_command(self, ui_tx.clone());
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
        if self.add_agent_selected() {
            self.add_form.error = None;
            match self.add_form.active_field {
                AddAgentField::Name => self.add_form.name.push(character),
                AddAgentField::Workspace => self.add_form.workspace.push(character),
            }
        } else if self.current_tab == AppTab::Workspace {
            self.clear_workspace_hover();
            if let Some(agent) = self.active_agent_mut() {
                if agent.workspace.sidebar_focused()
                    && agent.workspace.sidebar_tab == WorkspaceSidebarTab::Search
                {
                    agent.workspace.push_search_char(character);
                    return;
                }
                if let Some(editor) = agent.workspace.editor.as_mut() {
                    editor.clear_completion();
                    match editor.mode {
                        EditorMode::Insert => editor.insert_char(character),
                        EditorMode::Command => editor.command.push(character),
                        EditorMode::Normal | EditorMode::Visual => {}
                    }
                }
            }
        } else if self.current_tab == AppTab::Shell {
            if let Some(agent) = self.active_agent_mut() {
                agent.shell_tab.input.push(character);
                if let Some(session) = agent.shell_tab.selected_session_mut() {
                    session.scroll = u16::MAX;
                }
            }
        } else if self.current_tab == AppTab::GitDiff {
            if let Some(agent) = self.active_agent_mut() {
                agent.git_diff.commit_message.push(character);
                agent.git_diff.error = None;
            }
        } else if self.current_tab == AppTab::Chat {
            self.chat_input.push(character);
        }
    }

    fn handle_backspace(&mut self) {
        if self.add_agent_selected() {
            match self.add_form.active_field {
                AddAgentField::Name => {
                    self.add_form.name.pop();
                }
                AddAgentField::Workspace => {
                    self.add_form.workspace.pop();
                }
            }
        } else if self.current_tab == AppTab::Workspace {
            self.clear_workspace_hover();
            if let Some(agent) = self.active_agent_mut() {
                if agent.workspace.sidebar_focused()
                    && agent.workspace.sidebar_tab == WorkspaceSidebarTab::Search
                {
                    agent.workspace.pop_search_char();
                    return;
                }
                if let Some(editor) = agent.workspace.editor.as_mut() {
                    editor.clear_completion();
                    match editor.mode {
                        EditorMode::Insert => editor.backspace(),
                        EditorMode::Command => {
                            editor.command.pop();
                        }
                        EditorMode::Normal | EditorMode::Visual => {}
                    }
                }
            }
        } else if self.current_tab == AppTab::Shell {
            if let Some(agent) = self.active_agent_mut() {
                agent.shell_tab.input.pop();
                if let Some(session) = agent.shell_tab.selected_session_mut() {
                    session.scroll = u16::MAX;
                }
            }
        } else if self.current_tab == AppTab::GitDiff {
            if let Some(agent) = self.active_agent_mut() {
                agent.git_diff.commit_message.pop();
                agent.git_diff.error = None;
            }
        } else if self.current_tab == AppTab::Chat {
            self.chat_input.pop();
        }
    }
}
