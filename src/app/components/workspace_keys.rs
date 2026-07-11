use super::super::*;
use super::{WorkspaceComponent, WorkspaceEditorComponent};

impl WorkspaceComponent {
    pub(in crate::app) fn handle_key(app: &mut App, key: KeyEvent, area: Rect) -> bool {
        if app.current_tab != AppTab::Workspace {
            return false;
        }

        let Some(agent_index) = app.current_agent else {
            return false;
        };
        let viewport = WorkspaceEditorComponent::viewport(App::compute_layout(app, area).body);
        let page_step = usize::from(CONTENT_SCROLL_STEP);

        if app.agents[agent_index].workspace.sidebar_focused()
            && app.agents[agent_index].workspace.sidebar_tab == WorkspaceSidebarTab::Search
        {
            let (handled, open_request) = {
                let workspace = &mut app.agents[agent_index].workspace;
                Self::handle_search_key(workspace, key, viewport)
            };
            if let Some(position) = open_request {
                Self::request_open_editor_at(app, agent_index, Some(position));
            }
            if handled {
                return true;
            }
        }

        if app.agents[agent_index].workspace.editor.is_none() {
            return Self::open_selected_entry(app, agent_index, key);
        }

        let mut sidebar_result = None;
        {
            let workspace = &mut app.agents[agent_index].workspace;
            let editor_mode = workspace.editor.as_ref().map(|editor| editor.mode);
            let completion_visible = workspace
                .editor
                .as_ref()
                .is_some_and(|editor| editor.completion_popover().is_some());

            if workspace.editor.is_some()
                && key.code == KeyCode::Tab
                && !completion_visible
                && matches!(editor_mode, Some(EditorMode::Normal | EditorMode::Visual))
            {
                workspace.toggle_focus();
                return true;
            }

            if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('h') {
                if let Some(editor) = workspace.editor.as_mut() {
                    editor.clear_hover();
                    editor.clear_completion();
                    editor.toggle_shortcuts_help();
                }
                return true;
            }

            if workspace
                .editor
                .as_ref()
                .is_some_and(WorkspaceEditorState::shortcuts_help_open)
            {
                if key.code == KeyCode::Esc
                    && let Some(editor) = workspace.editor.as_mut()
                {
                    editor.close_shortcuts_help();
                }
                return true;
            }

            if workspace.sidebar_focused() {
                sidebar_result = Some(Self::handle_sidebar_key(workspace, key, viewport));
            } else if Self::handle_completion_key(workspace, key, viewport) {
                return true;
            }
        }

        if let Some((handled, request_open)) = sidebar_result {
            if request_open {
                Self::request_open_editor(app, agent_index);
            }
            return handled;
        }

        Self::handle_editor_key(app, agent_index, key, page_step, viewport)
    }
}
