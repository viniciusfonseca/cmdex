use super::{render::*, *};

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

        let source = normalize_newlines(&String::from_utf8_lossy(&bytes));
        let mut lines = split_preserving_lines(&source);
        if lines.is_empty() {
            lines.push(String::new());
        }

        let mut editor = Self {
            path: path.to_path_buf(),
            lines,
            cursor_row: 0,
            cursor_col: 0,
            vertical_scroll: 0,
            horizontal_scroll: 0,
            mode: EditorMode::Normal,
            command: String::new(),
            dirty: false,
            status: None,
            preferred_col: 0,
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
        if let Some(line) = self
            .cursor_row
            .checked_sub(start)
            .and_then(|visible_row| lines.get_mut(visible_row))
        {
            highlight_editor_line(line);
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

    pub fn enter_insert_mode(&mut self) {
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
        let next_row = self.cursor_row + 1;
        self.lines.insert(next_row, String::new());
        self.cursor_row = next_row;
        self.cursor_col = 0;
        self.preferred_col = 0;
        self.dirty = true;
        self.status = None;
        self.rebuild_render_cache();
        self.enter_insert_mode();
    }

    pub fn delete_char(&mut self) {
        let line_len = self.line_len(self.cursor_row);
        if self.cursor_col < line_len {
            let byte_index = byte_index_for_char(&self.lines[self.cursor_row], self.cursor_col);
            self.lines[self.cursor_row].remove(byte_index);
            self.dirty = true;
        } else if self.cursor_row + 1 < self.lines.len() {
            let next = self.lines.remove(self.cursor_row + 1);
            self.lines[self.cursor_row].push_str(&next);
            self.dirty = true;
        }
        self.clamp_cursor();
        self.status = None;
        if self.dirty {
            self.rebuild_render_cache();
        }
    }

    pub fn insert_char(&mut self, character: char) {
        let byte_index = byte_index_for_char(&self.lines[self.cursor_row], self.cursor_col);
        self.lines[self.cursor_row].insert(byte_index, character);
        self.cursor_col += 1;
        self.preferred_col = self.cursor_col;
        self.dirty = true;
        self.status = None;
        self.rebuild_render_cache();
    }

    pub fn insert_newline(&mut self) {
        let byte_index = byte_index_for_char(&self.lines[self.cursor_row], self.cursor_col);
        let tail = self.lines[self.cursor_row].split_off(byte_index);
        self.cursor_row += 1;
        self.lines.insert(self.cursor_row, tail);
        self.cursor_col = 0;
        self.preferred_col = 0;
        self.dirty = true;
        self.status = None;
        self.rebuild_render_cache();
    }

    pub fn backspace(&mut self) {
        if self.cursor_col > 0 {
            let byte_end = byte_index_for_char(&self.lines[self.cursor_row], self.cursor_col);
            let byte_start = byte_index_for_char(&self.lines[self.cursor_row], self.cursor_col - 1);
            self.lines[self.cursor_row].drain(byte_start..byte_end);
            self.cursor_col -= 1;
        } else if self.cursor_row > 0 {
            let current = self.lines.remove(self.cursor_row);
            self.cursor_row -= 1;
            self.cursor_col = self.line_len(self.cursor_row);
            self.lines[self.cursor_row].push_str(&current);
        } else {
            return;
        }

        self.preferred_col = self.cursor_col;
        self.dirty = true;
        self.status = None;
        self.rebuild_render_cache();
    }

    pub fn save(&mut self) -> Result<()> {
        fs::write(&self.path, self.lines.join("\n"))
            .with_context(|| format!("failed to write {}", self.path.display()))?;
        self.dirty = false;
        self.status = Some(format!("{} written", self.path.display()));
        Ok(())
    }

    pub fn set_cursor(&mut self, row: usize, col: usize) {
        self.cursor_row = row.min(self.lines.len().saturating_sub(1));
        self.cursor_col = col.min(self.line_len(self.cursor_row));
        self.preferred_col = self.cursor_col;
        self.status = None;
    }

    pub fn start_command(&mut self) {
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

    fn line_len(&self, row: usize) -> usize {
        self.lines
            .get(row)
            .map(|line| line.chars().count())
            .unwrap_or(0)
    }

    fn clamp_cursor(&mut self) {
        self.cursor_row = self.cursor_row.min(self.lines.len().saturating_sub(1));
        self.cursor_col = self.cursor_col.min(self.line_len(self.cursor_row));
        self.preferred_col = self.cursor_col;
    }

    fn rebuild_render_cache(&mut self) {
        self.render_cache.lines = build_editor_render_lines(&self.path, &self.lines);
    }

    fn max_vertical_scroll(&self, viewport_height: u16) -> u16 {
        self.content_height()
            .saturating_sub(usize::from(viewport_height.max(1))) as u16
    }
}

fn byte_index_for_char(source: &str, char_index: usize) -> usize {
    source
        .char_indices()
        .nth(char_index)
        .map(|(index, _)| index)
        .unwrap_or(source.len())
}
