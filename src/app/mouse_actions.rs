use super::shell::ShellPresenter;
use super::{chat::ChatSupport, components::*, *};

impl App {
    pub(super) fn handle_mouse(
        &mut self,
        mouse: MouseEvent,
        area: Rect,
        ui_tx: &mpsc::UnboundedSender<UiEvent>,
    ) {
        let layout = self.compute_layout(area);

        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                self.active_workspace_selection_drag = false;
                if self.handle_scrollbar_press(mouse.column, mouse.row, layout) {
                    return;
                }
                self.handle_left_click(mouse.column, mouse.row, mouse.modifiers, layout, ui_tx);
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                if self.handle_scrollbar_drag(mouse.row, layout) {
                    return;
                }
                WorkspaceScreen::handle_editor_drag_if_active(
                    self,
                    mouse.column,
                    mouse.row,
                    layout.body,
                );
            }
            MouseEventKind::Up(MouseButton::Left) => {
                self.active_scrollbar_drag = None;
                WorkspaceScreen::finish_editor_drag(self);
                self.active_workspace_selection_drag = false;
            }
            MouseEventKind::Moved if !self.active_workspace_selection_drag => {
                self.handle_mouse_move(mouse.column, mouse.row, layout)
            }
            MouseEventKind::ScrollUp => {
                let horizontal = mouse.modifiers.contains(KeyModifiers::SHIFT);
                if self.should_handle_mouse_scroll(ScrollDirection::Up, horizontal) {
                    self.handle_scroll(mouse.column, mouse.row, layout, true, horizontal);
                }
            }
            MouseEventKind::ScrollDown => {
                let horizontal = mouse.modifiers.contains(KeyModifiers::SHIFT);
                if self.should_handle_mouse_scroll(ScrollDirection::Down, horizontal) {
                    self.handle_scroll(mouse.column, mouse.row, layout, false, horizontal);
                }
            }
            _ => {}
        }
    }

    fn handle_mouse_move(&mut self, column: u16, row: u16, layout: UiLayout) {
        if WorkspaceScreen::handle_mouse_move(self, column, row, layout.body) {
            return;
        }

        self.clear_workspace_hover();
    }

    fn handle_scrollbar_press(&mut self, column: u16, row: u16, layout: UiLayout) -> bool {
        self.active_scrollbar_drag = None;

        let Some(target) = self.scrollbar_drag_target_at(column, row, layout) else {
            return false;
        };

        self.active_scrollbar_drag = Some(target);
        self.update_scrollbar_drag(target, row, layout)
    }

    fn handle_scrollbar_drag(&mut self, row: u16, layout: UiLayout) -> bool {
        let Some(target) = self.active_scrollbar_drag else {
            return false;
        };

        if self.update_scrollbar_drag(target, row, layout) {
            true
        } else {
            self.active_scrollbar_drag = None;
            false
        }
    }

    fn scrollbar_drag_target_at(
        &self,
        column: u16,
        row: u16,
        layout: UiLayout,
    ) -> Option<ScrollbarDragTarget> {
        if let Some(target) = WorkspaceScreen::scrollbar_drag_target_at(self, column, row, layout) {
            return Some(target);
        }

        let target = match self.current_tab {
            AppTab::Chat if !self.add_agent_selected() => ScrollbarDragTarget::Chat,
            AppTab::Shell => ScrollbarDragTarget::ShellOutput,
            AppTab::GitDiff => ScrollbarDragTarget::GitDiffPreview,
            AppTab::Chat | AppTab::Workspace => return None,
        };

        let metrics = self.scrollbar_metrics(target, layout)?;
        UiSupport::rect_contains(metrics.track, column, row).then_some(target)
    }

    fn update_scrollbar_drag(
        &mut self,
        target: ScrollbarDragTarget,
        row: u16,
        layout: UiLayout,
    ) -> bool {
        let Some(metrics) = self.scrollbar_metrics(target, layout) else {
            return false;
        };
        let scroll = UiSupport::scroll_position_from_row(metrics, row);

        if WorkspaceScreen::update_scrollbar_drag(self, target, scroll, metrics) {
            return true;
        }

        match target {
            ScrollbarDragTarget::Chat => {
                let Some(agent) = self.active_agent_mut() else {
                    return false;
                };
                agent.chat.chat_follow_output = false;
                agent.chat.chat_scroll = scroll;
            }
            ScrollbarDragTarget::ShellOutput => {
                let Some(agent) = self.active_agent_mut() else {
                    return false;
                };
                let Some(session) = agent.shell_tab.selected_session_mut() else {
                    return false;
                };
                session.scroll = scroll;
            }
            ScrollbarDragTarget::GitDiffPreview => {
                let Some(agent) = self.active_agent_mut() else {
                    return false;
                };
                agent.git_diff.content_scroll = scroll;
            }
            ScrollbarDragTarget::WorkspacePreview
            | ScrollbarDragTarget::WorkspaceEditor
            | ScrollbarDragTarget::WorkspaceCompletionPopover => return false,
        }

        true
    }

    fn scrollbar_metrics(
        &self,
        target: ScrollbarDragTarget,
        layout: UiLayout,
    ) -> Option<ScrollbarMetrics> {
        if let Some(metrics) = WorkspaceScreen::scrollbar_metrics(self, target, layout) {
            return Some(metrics);
        }
        let agent = self.active_agent()?;

        match target {
            ScrollbarDragTarget::Chat => {
                let content_length = ChatSupport::content_height(&agent.chat, layout.body);
                UiSupport::vertical_scrollbar_metrics(layout.body, content_length)
            }
            ScrollbarDragTarget::ShellOutput => {
                let session = agent.shell_tab.selected_session()?;
                let lines = ShellPresenter::display_lines(session, &agent.shell_tab.input);
                let content_length =
                    UiSupport::scrollable_preview_content_height(&lines, layout.body);
                UiSupport::vertical_scrollbar_metrics(layout.body, content_length)
            }
            ScrollbarDragTarget::GitDiffPreview => {
                let diff_layout = GitDiffComponent::layout(layout.body);
                let content_length = UiSupport::scrollable_preview_content_height(
                    &agent.git_diff.preview,
                    diff_layout.preview,
                );
                UiSupport::vertical_scrollbar_metrics(diff_layout.preview, content_length)
            }
            ScrollbarDragTarget::WorkspacePreview
            | ScrollbarDragTarget::WorkspaceEditor
            | ScrollbarDragTarget::WorkspaceCompletionPopover => None,
        }
    }

    fn handle_left_click(
        &mut self,
        column: u16,
        row: u16,
        modifiers: KeyModifiers,
        layout: UiLayout,
        ui_tx: &mpsc::UnboundedSender<UiEvent>,
    ) {
        if WorkspaceScreen::handle_popup_click_or_close(self, column, row, layout.body) {
            return;
        }

        if UiSupport::rect_contains(layout.tabs, column, row) {
            if let Some(tab) = TopNavigationComponent::tab_from_click(layout.tabs, column, row) {
                TopNavigationComponent::activate_tab(self, tab, ui_tx.clone());
            }
            return;
        }

        if UiSupport::rect_contains(layout.sidebar_list, column, row) {
            self.handle_sidebar_click(column, row, layout.sidebar_list, ui_tx);
            return;
        }

        if self.current_tab == AppTab::GitDiff
            && GitDiffComponent::handle_click(self, column, row, layout.body)
        {
            return;
        }

        if WorkspaceScreen::handle_editor_click_with_context(
            self,
            column,
            row,
            modifiers,
            layout.body,
            ui_tx,
        ) {
            return;
        }

        AddAgentDialogComponent::handle_click(
            self,
            column,
            row,
            layout.add_name,
            layout.add_workspace,
        );
    }

    fn handle_scroll(
        &mut self,
        column: u16,
        row: u16,
        layout: UiLayout,
        up: bool,
        horizontal: bool,
    ) {
        if UiSupport::rect_contains(layout.sidebar_list, column, row) {
            if up {
                self.move_selection_up();
            } else {
                self.move_selection_down();
            }
            return;
        }

        if WorkspaceScreen::handle_editor_scroll(
            self,
            column,
            row,
            layout.body,
            layout.footer,
            up,
            horizontal,
        ) {
            return;
        }

        if UiSupport::rect_contains(layout.body, column, row)
            || layout
                .footer
                .is_some_and(|rect| UiSupport::rect_contains(rect, column, row))
        {
            if ChatComponent::handle_scroll(self, layout.body, up) {
                return;
            }
            if up {
                self.scroll_content_up_by(layout.body, MOUSE_SCROLL_STEP);
            } else {
                self.scroll_content_down_by(layout.body, MOUSE_SCROLL_STEP);
            }
        }
    }

    fn handle_sidebar_click(
        &mut self,
        column: u16,
        row: u16,
        sidebar_list: Rect,
        ui_tx: &mpsc::UnboundedSender<UiEvent>,
    ) {
        match self.current_tab {
            AppTab::Chat => AgentsSidebarComponent::handle_click(self, column, row, sidebar_list),
            AppTab::Workspace => {
                WorkspaceSidebarComponent::handle_click(self, column, row, sidebar_list)
            }
            AppTab::Shell => {
                ShellSidebarComponent::handle_click(self, column, row, sidebar_list, ui_tx)
            }
            AppTab::GitDiff => {
                GitDiffSidebarComponent::handle_click(self, column, row, sidebar_list)
            }
        }
    }
}
