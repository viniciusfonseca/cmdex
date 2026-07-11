use super::*;

impl FileBrowserState {
    pub(crate) fn load_preview(path: &Path) -> Result<Vec<Line<'static>>> {
        WorkspaceRenderer::read_text_preview(path)
    }

    pub(crate) fn apply_loaded_editor(
        &mut self,
        path: &Path,
        source: String,
        position: Option<EditorPosition>,
    ) -> Result<bool> {
        self.finish_editor_load(path);
        let selected_path = self
            .entries()
            .get(self.selected)
            .map(|entry| entry.path.as_path());
        if selected_path != Some(path) || self.editor.as_ref().is_some_and(|editor| editor.dirty) {
            return Ok(false);
        }

        let mut editor = WorkspaceEditorState::from_source(path, source)?;
        if let Some(position) = position {
            editor.set_cursor(position.row, position.col);
            editor.vertical_scroll = position.row.saturating_sub(3) as u16;
            editor.horizontal_scroll = position.col.saturating_sub(8) as u16;
        }
        self.editor = Some(editor);
        self.focus_editor();
        self.error = None;
        Ok(true)
    }

    pub(crate) fn begin_editor_load(&mut self, path: &Path) -> bool {
        if self
            .editor
            .as_ref()
            .is_some_and(|editor| editor.path == path)
            || self.editor_load_in_flight.as_deref() == Some(path)
        {
            return false;
        }
        self.editor_load_in_flight = Some(path.to_path_buf());
        true
    }

    pub(crate) fn finish_editor_load(&mut self, path: &Path) {
        if self.editor_load_in_flight.as_deref() == Some(path) {
            self.editor_load_in_flight = None;
        }
    }

    pub fn close_editor(&mut self) -> Result<()> {
        self.editor = None;
        self.focus_sidebar();
        self.content_scroll = 0;
        self.preview_title = self
            .entries()
            .get(self.selected)
            .map(|entry| entry.path.display().to_string())
            .unwrap_or_else(|| "Workspace".to_string());
        self.preview = WorkspaceRenderer::plain_preview_lines("Loading preview...");
        Ok(())
    }

    pub(crate) fn apply_loaded_preview(
        &mut self,
        path: &Path,
        preview: Vec<Line<'static>>,
    ) -> bool {
        if self.editor.is_some()
            || self
                .entries()
                .get(self.selected)
                .is_none_or(|entry| entry.path != path)
        {
            return false;
        }
        self.preview_title = path.display().to_string();
        self.preview = preview;
        self.error = None;
        true
    }
}
