use super::super::*;
use super::{GitDiffComponent, WorkspaceScreen};
use crate::workspace::editor_keymap::{self, EditorKeyAction};

impl WorkspaceScreen {
    pub(super) fn handle_completion_key(
        workspace: &mut FileBrowserState,
        key: KeyEvent,
        viewport: Rect,
    ) -> bool {
        let Some(editor) = workspace.editor.as_mut() else {
            return false;
        };
        if editor.completion_popover().is_none() {
            return false;
        }

        match key.code {
            KeyCode::Esc => editor.clear_completion(),
            KeyCode::Up => editor.select_previous_completion(),
            KeyCode::Down => editor.select_next_completion(),
            KeyCode::Enter | KeyCode::Tab => {
                let _ = editor.apply_selected_completion();
            }
            _ => {
                editor.clear_completion();
                return false;
            }
        }
        editor.clear_hover();
        editor.ensure_visible(viewport.width, viewport.height);
        true
    }

    pub(super) fn handle_editor_key(
        app: &mut App,
        agent_index: usize,
        key: KeyEvent,
        page_step: usize,
        viewport: Rect,
    ) -> bool {
        let action = app.agents[agent_index]
            .workspace
            .editor
            .as_ref()
            .and_then(|editor| editor_keymap::map_key(&editor.mode, key, page_step));
        let Some(action) = action else {
            return false;
        };

        let mut command_result = None;
        let mut request_copy = false;
        let mut request_paste = false;
        {
            let editor = app.agents[agent_index]
                .workspace
                .editor
                .as_mut()
                .expect("editor action requires an open editor");
            match action {
                EditorKeyAction::Apply(command) => editor.apply(command),
                EditorKeyAction::Copy => request_copy = true,
                EditorKeyAction::Paste => request_paste = true,
                EditorKeyAction::CommandCancel => editor.cancel_command(),
                EditorKeyAction::CommandBackspace => editor.pop_command_char(),
                EditorKeyAction::CommandSubmit => match editor.execute_command() {
                    Ok(result) => command_result = Some(result),
                    Err(error) => editor.status = Some(error.to_string()),
                },
                EditorKeyAction::CommandInsert(character) => editor.push_command_char(character),
                EditorKeyAction::Consume => {}
            }
            editor.clear_hover();
            editor.clear_completion();
            editor.ensure_visible(viewport.width, viewport.height);
        }

        if request_copy {
            Self::request_copy_editor(app, agent_index);
        }
        if request_paste {
            Self::request_paste_editor(app, agent_index);
        }

        if command_result.is_some_and(|result| result.saved) {
            GitDiffComponent::request_refresh(app);
        }
        if command_result.is_some_and(|result| result.close)
            && let Err(error) = app.agents[agent_index].workspace.close_editor()
        {
            app.agents[agent_index].workspace.error = Some(error.to_string());
        } else if command_result.is_some_and(|result| result.close) {
            Self::request_preview(app, agent_index);
        }

        true
    }
}
