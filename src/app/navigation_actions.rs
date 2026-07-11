use super::{chat::ChatSupport, components::*, *};

impl App {
    pub(super) fn move_selection_up(&mut self) {
        match self.current_tab {
            AppTab::Chat => {
                AgentsSidebarComponent::select_index(
                    self,
                    self.chat_sidebar_index.saturating_sub(1),
                );
            }
            AppTab::Workspace => {
                if let Some(agent) = self.active_agent_mut() {
                    if agent.workspace.editor_focused() {
                        return;
                    }
                    if agent.workspace.sidebar_tab == WorkspaceSidebarTab::Search {
                        agent.workspace.search_move_up();
                    } else {
                        agent.workspace.move_up_without_io();
                    }
                }
            }
            AppTab::Shell => {
                if let Some(agent) = self.active_agent_mut() {
                    agent.shell_tab.move_up();
                }
            }
            AppTab::GitDiff => {
                if let Some(agent) = self.active_agent_mut() {
                    let root = agent.definition.workspace.clone();
                    agent.git_diff.move_up(&root);
                }
            }
        }
        if self.current_tab == AppTab::GitDiff {
            GitDiffComponent::request_refresh(self);
        } else if self.current_tab == AppTab::Workspace
            && let Some(agent_index) = self.current_agent
        {
            WorkspaceComponent::request_open_editor(self, agent_index);
        }
    }

    pub(super) fn move_selection_down(&mut self) {
        match self.current_tab {
            AppTab::Chat => {
                let max_index = self.agents.len();
                AgentsSidebarComponent::select_index(
                    self,
                    (self.chat_sidebar_index + 1).min(max_index),
                );
            }
            AppTab::Workspace => {
                if let Some(agent) = self.active_agent_mut() {
                    if agent.workspace.editor_focused() {
                        return;
                    }
                    if agent.workspace.sidebar_tab == WorkspaceSidebarTab::Search {
                        agent.workspace.search_move_down();
                    } else {
                        agent.workspace.move_down_without_io();
                    }
                }
            }
            AppTab::Shell => {
                if let Some(agent) = self.active_agent_mut() {
                    agent.shell_tab.move_down();
                }
            }
            AppTab::GitDiff => {
                if let Some(agent) = self.active_agent_mut() {
                    let root = agent.definition.workspace.clone();
                    agent.git_diff.move_down(&root);
                }
            }
        }
        if self.current_tab == AppTab::GitDiff {
            GitDiffComponent::request_refresh(self);
        } else if self.current_tab == AppTab::Workspace
            && let Some(agent_index) = self.current_agent
        {
            WorkspaceComponent::request_open_editor(self, agent_index);
        }
    }
    pub(super) fn scroll_content_up(&mut self, area: Rect) {
        self.scroll_content_up_by(area, CONTENT_SCROLL_STEP);
    }

    pub(super) fn scroll_content_up_by(&mut self, _area: Rect, lines: u16) {
        match self.current_tab {
            AppTab::Workspace => {
                let has_editor = self
                    .active_agent()
                    .and_then(|agent| agent.workspace.editor.as_ref())
                    .is_some();
                if has_editor {
                    self.clear_workspace_hover();
                    if let Some(agent) = self.active_agent_mut()
                        && let Some(editor) = agent.workspace.editor.as_mut()
                    {
                        editor.scroll_up(lines);
                        return;
                    }
                }
                if let Some(agent) = self.active_agent_mut() {
                    agent.workspace.scroll_up(lines);
                }
            }
            AppTab::Shell => {
                if let Some(agent) = self.active_agent_mut()
                    && let Some(session) = agent.shell_tab.selected_session_mut()
                {
                    session.scroll = session.scroll.saturating_sub(lines);
                }
            }
            AppTab::GitDiff => {
                if let Some(agent) = self.active_agent_mut() {
                    agent.git_diff.scroll_up(lines);
                }
            }
            AppTab::Chat => {}
        }
    }
    pub(super) fn should_handle_mouse_scroll(
        &mut self,
        direction: ScrollDirection,
        horizontal: bool,
    ) -> bool {
        let axis = if horizontal {
            ScrollAxis::Horizontal
        } else {
            ScrollAxis::Vertical
        };
        self.should_handle_mouse_scroll_at_axis(axis, direction, Instant::now())
    }

    pub(super) fn should_handle_mouse_scroll_at_axis(
        &mut self,
        axis: ScrollAxis,
        direction: ScrollDirection,
        now: Instant,
    ) -> bool {
        if self
            .last_mouse_scroll
            .is_some_and(|(last_axis, last_direction, last_at)| {
                last_axis == axis
                    && last_direction == direction
                    && now
                        .checked_duration_since(last_at)
                        .is_some_and(|elapsed| elapsed < MOUSE_SCROLL_DEBOUNCE)
            })
        {
            return false;
        }

        self.last_mouse_scroll = Some((axis, direction, now));
        true
    }

    pub(super) fn scroll_content_down(&mut self, area: Rect) {
        self.scroll_content_down_by(area, CONTENT_SCROLL_STEP);
    }

    pub(super) fn scroll_content_down_by(&mut self, area: Rect, lines: u16) {
        match self.current_tab {
            AppTab::Workspace => {
                let has_editor = self
                    .active_agent()
                    .and_then(|agent| agent.workspace.editor.as_ref())
                    .is_some();
                if has_editor {
                    self.clear_workspace_hover();
                    if let Some(agent) = self.active_agent_mut()
                        && let Some(editor) = agent.workspace.editor.as_mut()
                    {
                        let viewport = WorkspaceEditorComponent::viewport(area);
                        editor.scroll_down(lines, viewport.height);
                        return;
                    }
                }
                if let Some(agent) = self.active_agent_mut() {
                    agent.workspace.scroll_down(lines);
                }
            }
            AppTab::Shell => {
                if let Some(agent) = self.active_agent_mut()
                    && let Some(session) = agent.shell_tab.selected_session_mut()
                {
                    session.scroll = session.scroll.saturating_add(lines);
                }
            }
            AppTab::GitDiff => {
                if let Some(agent) = self.active_agent_mut() {
                    agent.git_diff.scroll_down(lines);
                }
            }
            AppTab::Chat => {}
        }
    }

    pub(super) fn scroll_chat_up(&mut self, area: Rect, lines: u16) {
        let Some(agent) = self.active_agent_mut() else {
            return;
        };

        let max_scroll = ChatSupport::max_scroll(&agent.chat, area);
        let current = if agent.chat.chat_follow_output {
            max_scroll
        } else {
            agent.chat.chat_scroll.min(max_scroll)
        };
        let next = current.saturating_sub(lines);

        agent.chat.chat_scroll = next;
        agent.chat.chat_follow_output = next >= max_scroll;
    }

    pub(super) fn scroll_chat_down(&mut self, area: Rect, lines: u16) {
        let Some(agent) = self.active_agent_mut() else {
            return;
        };

        let max_scroll = ChatSupport::max_scroll(&agent.chat, area);
        let current = if agent.chat.chat_follow_output {
            max_scroll
        } else {
            agent.chat.chat_scroll.min(max_scroll)
        };
        let next = current.saturating_add(lines).min(max_scroll);

        agent.chat.chat_scroll = next;
        agent.chat.chat_follow_output = next >= max_scroll;
    }
}
