use super::{render::*, *};

const UNDO_LIMIT: usize = 256;

impl WorkspaceEditorState {
    pub fn open(path: &Path) -> Result<Self> {
        let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
        if bytes.contains(&0) {
            return Err(anyhow::anyhow!("Binary files cannot be edited in-app."));
        }
        if bytes.len() > PREVIEW_LIMIT {
            return Err(anyhow::anyhow!(
                "Files larger than {} bytes cannot be edited in-app.",
                PREVIEW_LIMIT
            ));
        }

        let source = WorkspaceRenderer::normalize_newlines(&String::from_utf8_lossy(&bytes));
        let mut lines = WorkspaceRenderer::split_preserving_lines(&source);
        if lines.is_empty() {
            lines.push(String::new());
        }

        let mut editor = Self {
            path: path.to_path_buf(),
            saved_lines: lines.clone(),
            lines,
            cursor_row: 0,
            cursor_col: 0,
            vertical_scroll: 0,
            horizontal_scroll: 0,
            mode: EditorMode::Normal,
            command: String::new(),
            dirty: false,
            status: None,
            hover: None,
            undo_stack: Vec::new(),
            preferred_col: 0,
            selection_anchor: None,
            hover_request: None,
            render_cache: EditorRenderCache::default(),
        };
        editor.rebuild_render_cache();
        Ok(editor)
    }

    pub fn rendered_lines(&self, viewport_height: u16) -> Vec<Line<'static>> {
        let start = self.clamped_vertical_scroll(viewport_height) as usize;
        let end = start
            .saturating_add(usize::from(viewport_height.max(1)))
            .min(self.render_cache.lines.len());
        let mut lines = self.render_cache.lines[start..end].to_vec();
        let gutter_width = self.gutter_width();

        for (visible_row, line) in lines.iter_mut().enumerate() {
            let row = start + visible_row;
            if row == self.cursor_row {
                WorkspaceRenderer::highlight_editor_line(line);
            }
            if let Some((selection_start, selection_end)) = self.selection_range_for_row(row) {
                WorkspaceRenderer::highlight_editor_selection(
                    line,
                    gutter_width,
                    selection_start,
                    selection_end,
                );
            }
        }
        lines
    }

    pub fn content_height(&self) -> usize {
        self.render_cache.lines.len().max(1)
    }

    pub fn clamped_vertical_scroll(&self, viewport_height: u16) -> u16 {
        self.vertical_scroll
            .min(self.max_vertical_scroll(viewport_height))
    }

    pub fn scroll_up(&mut self, lines: u16) {
        self.vertical_scroll = self.vertical_scroll.saturating_sub(lines);
    }

    pub fn scroll_down(&mut self, lines: u16, viewport_height: u16) {
        self.vertical_scroll = self
            .vertical_scroll
            .saturating_add(lines)
            .min(self.max_vertical_scroll(viewport_height));
    }

    pub fn scroll_left(&mut self, columns: u16) {
        self.horizontal_scroll = self.horizontal_scroll.saturating_sub(columns);
    }

    pub fn scroll_right(&mut self, columns: u16, viewport_width: u16) {
        self.horizontal_scroll = self
            .horizontal_scroll
            .saturating_add(columns)
            .min(self.max_horizontal_scroll(viewport_width));
    }

    pub fn set_vertical_scroll(&mut self, scroll: u16, viewport_height: u16) {
        self.vertical_scroll = scroll.min(self.max_vertical_scroll(viewport_height));
    }

    pub fn gutter_width(&self) -> usize {
        self.lines.len().max(1).to_string().len() + 3
    }

    pub fn ensure_visible(&mut self, viewport_width: u16, viewport_height: u16) {
        let viewport_height = usize::from(viewport_height.max(1));
        if self.cursor_row < self.vertical_scroll as usize {
            self.vertical_scroll = self.cursor_row as u16;
        } else if self.cursor_row >= self.vertical_scroll as usize + viewport_height {
            self.vertical_scroll =
                self.cursor_row
                    .saturating_sub(viewport_height.saturating_sub(1)) as u16;
        }

        let content_width = usize::from(
            viewport_width
                .saturating_sub(self.gutter_width() as u16)
                .saturating_sub(1)
                .max(1),
        );
        if self.cursor_col < self.horizontal_scroll as usize {
            self.horizontal_scroll = self.cursor_col as u16;
        } else if self.cursor_col >= self.horizontal_scroll as usize + content_width {
            self.horizontal_scroll =
                self.cursor_col
                    .saturating_sub(content_width.saturating_sub(1)) as u16;
        }
    }

    pub fn move_left(&mut self) {
        if self.cursor_col > 0 {
            self.cursor_col -= 1;
        } else if self.cursor_row > 0 {
            self.cursor_row -= 1;
            self.cursor_col = self.line_len(self.cursor_row);
        }
        self.preferred_col = self.cursor_col;
        self.status = None;
    }

    pub fn move_right(&mut self) {
        let line_len = self.line_len(self.cursor_row);
        if self.cursor_col < line_len {
            self.cursor_col += 1;
        } else if self.cursor_row + 1 < self.lines.len() {
            self.cursor_row += 1;
            self.cursor_col = 0;
        }
        self.preferred_col = self.cursor_col;
        self.status = None;
    }

    pub fn move_up(&mut self) {
        if self.cursor_row > 0 {
            self.cursor_row -= 1;
            self.cursor_col = self.preferred_col.min(self.line_len(self.cursor_row));
        }
        self.status = None;
    }

    pub fn move_down(&mut self) {
        if self.cursor_row + 1 < self.lines.len() {
            self.cursor_row += 1;
            self.cursor_col = self.preferred_col.min(self.line_len(self.cursor_row));
        }
        self.status = None;
    }

    pub fn move_page_up(&mut self, lines: usize) {
        self.cursor_row = self.cursor_row.saturating_sub(lines);
        self.cursor_col = self.preferred_col.min(self.line_len(self.cursor_row));
        self.status = None;
    }

    pub fn move_page_down(&mut self, lines: usize) {
        self.cursor_row = (self.cursor_row + lines).min(self.lines.len().saturating_sub(1));
        self.cursor_col = self.preferred_col.min(self.line_len(self.cursor_row));
        self.status = None;
    }

    pub fn move_line_start(&mut self) {
        self.cursor_col = 0;
        self.preferred_col = 0;
        self.status = None;
    }

    pub fn move_line_end(&mut self) {
        self.cursor_col = self.line_len(self.cursor_row);
        self.preferred_col = self.cursor_col;
        self.status = None;
    }

    pub fn enter_visual_mode(&mut self) {
        self.mode = EditorMode::Visual;
        self.command.clear();
        if self.selection_anchor.is_none() {
            self.selection_anchor = Some(self.cursor_position());
        }
        self.status = None;
    }

    pub fn exit_visual_mode(&mut self) {
        self.mode = EditorMode::Normal;
        self.clear_selection();
        self.status = None;
    }

    pub fn extend_left(&mut self) {
        self.enter_visual_mode();
        self.move_left();
    }

    pub fn extend_right(&mut self) {
        self.enter_visual_mode();
        self.move_right();
    }

    pub fn extend_up(&mut self) {
        self.enter_visual_mode();
        self.move_up();
    }

    pub fn extend_down(&mut self) {
        self.enter_visual_mode();
        self.move_down();
    }

    pub fn extend_page_up(&mut self, lines: usize) {
        self.enter_visual_mode();
        self.move_page_up(lines);
    }

    pub fn extend_page_down(&mut self, lines: usize) {
        self.enter_visual_mode();
        self.move_page_down(lines);
    }

    pub fn extend_line_start(&mut self) {
        self.enter_visual_mode();
        self.move_line_start();
    }

    pub fn extend_line_end(&mut self) {
        self.enter_visual_mode();
        self.move_line_end();
    }

    pub fn enter_insert_mode(&mut self) {
        self.clear_selection();
        self.mode = EditorMode::Insert;
        self.command.clear();
        self.status = None;
    }

    pub fn enter_insert_after(&mut self) {
        let line_len = self.line_len(self.cursor_row);
        if self.cursor_col < line_len {
            self.cursor_col += 1;
        }
        self.preferred_col = self.cursor_col;
        self.enter_insert_mode();
    }

    pub fn open_below(&mut self) {
        self.push_undo_state();
        let next_row = self.cursor_row + 1;
        self.lines.insert(next_row, String::new());
        self.cursor_row = next_row;
        self.cursor_col = 0;
        self.preferred_col = 0;
        self.sync_dirty_state();
        self.status = None;
        self.rebuild_render_cache();
        self.enter_insert_mode();
    }

    pub fn delete_char(&mut self) {
        if self.has_selection() {
            self.push_undo_state();
            self.delete_selection_internal(false);
            return;
        }

        let line_len = self.line_len(self.cursor_row);
        if self.cursor_col < line_len {
            self.push_undo_state();
            let byte_index =
                Self::byte_index_for_char(&self.lines[self.cursor_row], self.cursor_col);
            self.lines[self.cursor_row].remove(byte_index);
        } else if self.cursor_row + 1 < self.lines.len() {
            self.push_undo_state();
            let next = self.lines.remove(self.cursor_row + 1);
            self.lines[self.cursor_row].push_str(&next);
        } else {
            return;
        }
        self.clamp_cursor();
        self.sync_dirty_state();
        self.status = None;
        self.rebuild_render_cache();
    }

    pub fn insert_char(&mut self, character: char) {
        self.push_undo_state();
        let _ = self.delete_selection_internal(false);
        let byte_index = Self::byte_index_for_char(&self.lines[self.cursor_row], self.cursor_col);
        self.lines[self.cursor_row].insert(byte_index, character);
        self.cursor_col += 1;
        self.preferred_col = self.cursor_col;
        self.sync_dirty_state();
        self.status = None;
        self.rebuild_render_cache();
    }

    pub fn insert_newline(&mut self) {
        self.push_undo_state();
        let _ = self.delete_selection_internal(false);
        let byte_index = Self::byte_index_for_char(&self.lines[self.cursor_row], self.cursor_col);
        let tail = self.lines[self.cursor_row].split_off(byte_index);
        self.cursor_row += 1;
        self.lines.insert(self.cursor_row, tail);
        self.cursor_col = 0;
        self.preferred_col = 0;
        self.sync_dirty_state();
        self.status = None;
        self.rebuild_render_cache();
    }

    pub fn backspace(&mut self) {
        if self.has_selection() {
            self.push_undo_state();
            self.delete_selection_internal(false);
            return;
        }

        if self.cursor_col > 0 {
            self.push_undo_state();
            let byte_end = Self::byte_index_for_char(&self.lines[self.cursor_row], self.cursor_col);
            let byte_start =
                Self::byte_index_for_char(&self.lines[self.cursor_row], self.cursor_col - 1);
            self.lines[self.cursor_row].drain(byte_start..byte_end);
            self.cursor_col -= 1;
        } else if self.cursor_row > 0 {
            self.push_undo_state();
            let current = self.lines.remove(self.cursor_row);
            self.cursor_row -= 1;
            self.cursor_col = self.line_len(self.cursor_row);
            self.lines[self.cursor_row].push_str(&current);
        } else {
            return;
        }

        self.preferred_col = self.cursor_col;
        self.sync_dirty_state();
        self.status = None;
        self.rebuild_render_cache();
    }

    pub fn save(&mut self) -> Result<()> {
        fs::write(&self.path, self.lines.join("\n"))
            .with_context(|| format!("failed to write {}", self.path.display()))?;
        self.saved_lines = self.lines.clone();
        self.dirty = false;
        self.status = Some(format!("{} written", self.path.display()));
        Ok(())
    }

    pub fn copy_selection_or_line(&self) -> String {
        self.selection_text()
            .unwrap_or_else(|| self.lines.get(self.cursor_row).cloned().unwrap_or_default())
    }

    pub fn source_text(&self) -> String {
        self.lines.join("\n")
    }

    pub fn set_cursor(&mut self, row: usize, col: usize) {
        self.clear_selection();
        if self.mode == EditorMode::Visual {
            self.mode = EditorMode::Normal;
        }
        self.cursor_row = row.min(self.lines.len().saturating_sub(1));
        self.cursor_col = col.min(self.line_len(self.cursor_row));
        self.preferred_col = self.cursor_col;
        self.status = None;
    }

    pub fn clear_hover(&mut self) {
        self.hover = None;
        self.hover_request = None;
    }

    pub fn request_hover(&mut self, position: EditorPosition) -> bool {
        if self.hover_request == Some(position) {
            return false;
        }

        self.hover = None;
        self.hover_request = Some(position);
        true
    }

    pub fn resolve_hover(&mut self, position: EditorPosition, hover: Option<String>) -> bool {
        if self.hover_request != Some(position) {
            return false;
        }

        self.hover = hover;
        true
    }

    pub fn hover_popover(&self) -> Option<(&str, EditorPosition)> {
        self.hover.as_deref().zip(self.hover_request)
    }

    pub fn hover_request_position(&self) -> Option<EditorPosition> {
        self.hover_request
    }

    pub fn symbol_position_near(&self, row: usize, col: usize) -> Option<EditorPosition> {
        let line = self.lines.get(row)?;
        let characters = line.chars().collect::<Vec<_>>();
        if characters.is_empty() {
            return None;
        }

        if col < characters.len() && Self::is_symbol_char(characters[col]) {
            return Some(EditorPosition { row, col });
        }

        if col > 0 && col - 1 < characters.len() && Self::is_symbol_char(characters[col - 1]) {
            return Some(EditorPosition { row, col: col - 1 });
        }

        None
    }

    pub fn paste_text(&mut self, text: &str) -> bool {
        let text = WorkspaceRenderer::normalize_newlines(text);
        if text.is_empty() {
            return false;
        }

        self.push_undo_state();
        if self.mode == EditorMode::Visual {
            self.mode = EditorMode::Normal;
        }
        let _ = self.delete_selection_internal(false);
        self.clear_selection();

        let insert_at = Self::byte_index_for_char(&self.lines[self.cursor_row], self.cursor_col);
        let tail = self.lines[self.cursor_row].split_off(insert_at);
        let segments = WorkspaceRenderer::split_preserving_lines(&text);

        if segments.len() == 1 {
            self.lines[self.cursor_row].push_str(&segments[0]);
            self.lines[self.cursor_row].push_str(&tail);
            self.cursor_col += segments[0].chars().count();
        } else {
            let start_row = self.cursor_row;
            self.lines[start_row].push_str(&segments[0]);

            for (offset, segment) in segments[1..segments.len() - 1].iter().enumerate() {
                self.lines.insert(start_row + 1 + offset, segment.clone());
            }

            let last_segment = segments.last().cloned().unwrap_or_default();
            self.lines
                .insert(start_row + segments.len() - 1, last_segment.clone() + &tail);
            self.cursor_row = start_row + segments.len() - 1;
            self.cursor_col = last_segment.chars().count();
        }

        self.preferred_col = self.cursor_col;
        self.sync_dirty_state();
        self.status = None;
        self.rebuild_render_cache();
        true
    }

    pub fn undo(&mut self) -> bool {
        let Some(snapshot) = self.undo_stack.pop() else {
            self.status = Some("Nothing to undo".to_string());
            return false;
        };

        self.lines = snapshot.lines;
        if self.lines.is_empty() {
            self.lines.push(String::new());
        }
        self.cursor_row = snapshot.cursor_row.min(self.lines.len().saturating_sub(1));
        self.cursor_col = snapshot.cursor_col.min(self.line_len(self.cursor_row));
        self.vertical_scroll = snapshot.vertical_scroll;
        self.horizontal_scroll = snapshot.horizontal_scroll;
        self.preferred_col = snapshot.preferred_col.min(self.line_len(self.cursor_row));
        self.mode = EditorMode::Normal;
        self.command.clear();
        self.clear_selection();
        self.sync_dirty_state();
        self.status = Some("Undid last change".to_string());
        self.rebuild_render_cache();
        true
    }

    pub fn select_to(&mut self, row: usize, col: usize) {
        self.enter_visual_mode();
        self.cursor_row = row.min(self.lines.len().saturating_sub(1));
        self.cursor_col = col.min(self.line_len(self.cursor_row));
        self.preferred_col = self.cursor_col;
        self.status = None;
    }

    pub fn start_command(&mut self) {
        self.clear_selection();
        self.mode = EditorMode::Command;
        self.command.clear();
    }

    pub fn cancel_command(&mut self) {
        self.mode = EditorMode::Normal;
        self.command.clear();
    }

    pub fn execute_command(&mut self) -> Result<EditorCommandResult> {
        let command = self.command.trim().to_string();
        self.mode = EditorMode::Normal;
        self.command.clear();

        match command.as_str() {
            "" => Ok(EditorCommandResult {
                saved: false,
                close: false,
            }),
            "w" => {
                self.save()?;
                Ok(EditorCommandResult {
                    saved: true,
                    close: false,
                })
            }
            "q" => {
                if self.dirty {
                    self.status = Some("Unsaved changes. Use :w, :wq or :q!".to_string());
                    Ok(EditorCommandResult {
                        saved: false,
                        close: false,
                    })
                } else {
                    Ok(EditorCommandResult {
                        saved: false,
                        close: true,
                    })
                }
            }
            "q!" => Ok(EditorCommandResult {
                saved: false,
                close: true,
            }),
            "wq" | "x" => {
                self.save()?;
                Ok(EditorCommandResult {
                    saved: true,
                    close: true,
                })
            }
            other => {
                self.status = Some(format!("Unknown command: {other}"));
                Ok(EditorCommandResult {
                    saved: false,
                    close: false,
                })
            }
        }
    }

    pub fn has_selection(&self) -> bool {
        self.selection_bounds().is_some()
    }

    pub fn delete_selection(&mut self) -> bool {
        self.delete_selection_internal(true)
    }

    fn delete_selection_internal(&mut self, record_undo: bool) -> bool {
        let Some((start, end)) = self.selection_bounds() else {
            return false;
        };

        if record_undo {
            self.push_undo_state();
        }

        if start.row == end.row {
            let byte_start = Self::byte_index_for_char(&self.lines[start.row], start.col);
            let byte_end = Self::byte_index_for_char(&self.lines[end.row], end.col);
            self.lines[start.row].drain(byte_start..byte_end);
        } else {
            let start_byte = Self::byte_index_for_char(&self.lines[start.row], start.col);
            let end_byte = Self::byte_index_for_char(&self.lines[end.row], end.col);
            let prefix = self.lines[start.row][..start_byte].to_string();
            let suffix = self.lines[end.row][end_byte..].to_string();
            self.lines[start.row] = prefix + &suffix;
            self.lines.drain(start.row + 1..=end.row);
        }

        if self.lines.is_empty() {
            self.lines.push(String::new());
        }

        self.cursor_row = start.row.min(self.lines.len().saturating_sub(1));
        self.cursor_col = start.col.min(self.line_len(self.cursor_row));
        self.preferred_col = self.cursor_col;
        self.clear_selection();
        self.sync_dirty_state();
        self.status = None;
        self.rebuild_render_cache();
        true
    }

    fn line_len(&self, row: usize) -> usize {
        self.lines
            .get(row)
            .map(|line| line.chars().count())
            .unwrap_or(0)
    }

    fn cursor_position(&self) -> EditorPosition {
        EditorPosition {
            row: self.cursor_row,
            col: self.cursor_col,
        }
    }

    fn clear_selection(&mut self) {
        self.selection_anchor = None;
    }

    fn selection_bounds(&self) -> Option<(EditorPosition, EditorPosition)> {
        let anchor = self.selection_anchor?;
        let cursor = self.cursor_position();
        if anchor == cursor {
            return None;
        }

        Some(if anchor < cursor {
            (anchor, cursor)
        } else {
            (cursor, anchor)
        })
    }

    fn selection_text(&self) -> Option<String> {
        let (start, end) = self.selection_bounds()?;
        let mut text = String::new();

        for row in start.row..=end.row {
            if row > start.row {
                text.push('\n');
            }

            let line = &self.lines[row];
            let start_col = if row == start.row { start.col } else { 0 };
            let end_col = if row == end.row {
                end.col
            } else {
                self.line_len(row)
            };
            let start_byte = Self::byte_index_for_char(line, start_col);
            let end_byte = Self::byte_index_for_char(line, end_col);
            text.push_str(&line[start_byte..end_byte]);
        }

        Some(text)
    }

    fn selection_range_for_row(&self, row: usize) -> Option<(usize, usize)> {
        let (start, end) = self.selection_bounds()?;
        if row < start.row || row > end.row {
            return None;
        }

        let line_len = self.line_len(row);
        let selection_start = if row == start.row { start.col } else { 0 };
        let selection_end = if row == end.row { end.col } else { line_len };

        (selection_start < selection_end)
            .then_some((selection_start.min(line_len), selection_end.min(line_len)))
    }

    fn clamp_cursor(&mut self) {
        self.cursor_row = self.cursor_row.min(self.lines.len().saturating_sub(1));
        self.cursor_col = self.cursor_col.min(self.line_len(self.cursor_row));
        self.preferred_col = self.cursor_col;
    }

    fn push_undo_state(&mut self) {
        self.undo_stack.push(EditorUndoState {
            lines: self.lines.clone(),
            cursor_row: self.cursor_row,
            cursor_col: self.cursor_col,
            vertical_scroll: self.vertical_scroll,
            horizontal_scroll: self.horizontal_scroll,
            preferred_col: self.preferred_col,
        });

        if self.undo_stack.len() > UNDO_LIMIT {
            let overflow = self.undo_stack.len() - UNDO_LIMIT;
            self.undo_stack.drain(0..overflow);
        }
    }

    fn sync_dirty_state(&mut self) {
        self.dirty = self.lines != self.saved_lines;
    }

    fn is_symbol_char(character: char) -> bool {
        character == '_' || character.is_alphanumeric()
    }

    fn rebuild_render_cache(&mut self) {
        self.render_cache.lines =
            WorkspaceRenderer::build_editor_render_lines(&self.path, &self.lines);
    }

    fn max_vertical_scroll(&self, viewport_height: u16) -> u16 {
        self.content_height()
            .saturating_sub(usize::from(viewport_height.max(1))) as u16
    }

    fn max_horizontal_scroll(&self, viewport_width: u16) -> u16 {
        let content_width = usize::from(
            viewport_width
                .saturating_sub(self.gutter_width() as u16)
                .saturating_sub(1)
                .max(1),
        );
        self.lines
            .iter()
            .map(|line| line.chars().count())
            .max()
            .unwrap_or(0)
            .saturating_sub(content_width.saturating_sub(1)) as u16
    }

    fn byte_index_for_char(source: &str, char_index: usize) -> usize {
        source
            .char_indices()
            .nth(char_index)
            .map(|(index, _)| index)
            .unwrap_or(source.len())
    }
}
