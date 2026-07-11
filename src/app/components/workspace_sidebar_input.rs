use super::super::*;
use super::WorkspaceScreen;

impl WorkspaceScreen {
    pub(super) fn handle_search_key(
        workspace: &mut FileBrowserState,
        key: KeyEvent,
        _viewport: Rect,
    ) -> (bool, Option<EditorPosition>) {
        let mut open_request = None;
        match key.code {
            KeyCode::Esc => {
                workspace.set_sidebar_tab(WorkspaceSidebarTab::Files);
                workspace.focus_sidebar();
            }
            KeyCode::Up => workspace.search_move_up(),
            KeyCode::Down => workspace.search_move_down(),
            KeyCode::Enter => {
                open_request = workspace.open_selected_search_result_without_io();
            }
            KeyCode::Backspace => workspace.pop_search_char(),
            KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                workspace.push_search_char(character)
            }
            _ => return (false, None),
        }
        (true, open_request)
    }

    pub(super) fn open_selected_entry(app: &mut App, agent_index: usize, key: KeyEvent) -> bool {
        if key.code != KeyCode::Enter {
            return false;
        }

        if app.agents[agent_index].workspace.toggle_current_directory() {
            return true;
        }
        Self::request_open_editor(app, agent_index);
        true
    }

    pub(super) fn handle_sidebar_key(
        workspace: &mut FileBrowserState,
        key: KeyEvent,
        viewport: Rect,
    ) -> (bool, bool) {
        let (handled, request_open) = match key.code {
            KeyCode::Up => (true, workspace.move_up_without_io()),
            KeyCode::Down => (true, workspace.move_down_without_io()),
            KeyCode::Enter => {
                if workspace.toggle_current_directory() {
                    (true, false)
                } else {
                    (true, true)
                }
            }
            KeyCode::Esc => (true, false),
            _ => (false, false),
        };

        if handled && let Some(editor) = workspace.editor.as_mut() {
            editor.clear_hover();
            editor.ensure_visible(viewport.width, viewport.height);
        }
        (handled, request_open)
    }
}
