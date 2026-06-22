use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::LazyLock,
};

use anyhow::{Context, Result};
use ignore::WalkBuilder;
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use syntect::{
    easy::HighlightLines,
    highlighting::{FontStyle, Theme, ThemeSet},
    parsing::{SyntaxReference, SyntaxSet},
};

const PREVIEW_LIMIT: usize = 200_000;
static SYNTAX_SET: LazyLock<SyntaxSet> = LazyLock::new(SyntaxSet::load_defaults_newlines);
static THEME: LazyLock<Theme> = LazyLock::new(|| {
    let themes = ThemeSet::load_defaults();
    themes
        .themes
        .get("base16-ocean.dark")
        .cloned()
        .unwrap_or_default()
});

#[derive(Debug, Clone, Default)]
pub struct FileBrowserState {
    pub entries: Vec<FileEntry>,
    pub tree_rows: Vec<FileTreeRow>,
    pub selected: usize,
    pub tree_cursor: usize,
    pub preview_title: String,
    pub preview: Vec<Line<'static>>,
    pub content_scroll: u16,
    pub error: Option<String>,
    pub editor: Option<WorkspaceEditorState>,
    collapsed_dirs: BTreeSet<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct FileEntry {
    pub path: PathBuf,
    pub relative_path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct FileTreeRow {
    pub label: String,
    kind: FileTreeRowKind,
}

#[derive(Debug, Clone)]
enum FileTreeRowKind {
    Directory {
        relative_path: PathBuf,
        expanded: bool,
    },
    File {
        file_index: usize,
    },
}

#[derive(Debug, Clone, Default)]
pub struct DiffBrowserState {
    pub changes: Vec<DiffEntry>,
    pub staged: Vec<DiffEntry>,
    pub active_section: DiffSection,
    pub selected_changes: usize,
    pub selected_staged: usize,
    pub preview_title: String,
    pub preview: Vec<Line<'static>>,
    pub content_scroll: u16,
    pub commit_message: String,
    pub status: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DiffSection {
    #[default]
    Changes,
    Staged,
}

#[derive(Debug, Clone)]
pub struct DiffEntry {
    pub label: String,
    pub path: PathBuf,
    pub status: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorMode {
    Normal,
    Insert,
    Command,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditorCommandResult {
    pub saved: bool,
    pub close: bool,
}

#[derive(Debug, Clone)]
pub struct WorkspaceEditorState {
    pub path: PathBuf,
    pub lines: Vec<String>,
    pub cursor_row: usize,
    pub cursor_col: usize,
    pub vertical_scroll: u16,
    pub horizontal_scroll: u16,
    pub mode: EditorMode,
    pub command: String,
    pub dirty: bool,
    pub status: Option<String>,
    preferred_col: usize,
}

impl FileBrowserState {
    pub fn refresh(&mut self, root: &Path) {
        match build_file_entries(root).and_then(|entries| {
            self.entries = entries;
            self.selected = self.selected.min(self.entries.len().saturating_sub(1));
            self.rebuild_tree_rows();
            self.content_scroll = 0;
            self.error = None;
            self.update_preview()?;
            self.sync_editor_to_selection(true)
        }) {
            Ok(()) => {}
            Err(error) => {
                self.entries.clear();
                self.tree_rows.clear();
                self.selected = 0;
                self.tree_cursor = 0;
                self.preview_title = "Workspace".to_string();
                self.preview = plain_preview_lines("Unable to load workspace.");
                self.error = Some(error.to_string());
            }
        }
    }

    pub fn move_up(&mut self) {
        if self.tree_rows.is_empty() {
            return;
        }
        let _ = self.select_tree_row(self.tree_cursor.saturating_sub(1), true);
    }

    pub fn move_down(&mut self) {
        if self.tree_rows.is_empty() {
            return;
        }
        let _ = self.select_tree_row(
            (self.tree_cursor + 1).min(self.tree_rows.len().saturating_sub(1)),
            true,
        );
    }

    pub fn select(&mut self, index: usize) {
        if self.entries.is_empty() {
            return;
        }
        let _ = self.select_index(index.min(self.entries.len().saturating_sub(1)), true);
    }

    pub fn scroll_up(&mut self, lines: u16) {
        self.content_scroll = self.content_scroll.saturating_sub(lines);
    }

    pub fn scroll_down(&mut self, lines: u16) {
        self.content_scroll = self.content_scroll.saturating_add(lines);
    }

    pub fn open_editor(&mut self) -> Result<()> {
        if self.entries.is_empty() {
            return Ok(());
        }

        self.editor = Some(WorkspaceEditorState::open(
            &self.entries[self.selected].path,
        )?);
        self.error = None;
        Ok(())
    }

    pub fn close_editor(&mut self) -> Result<()> {
        self.editor = None;
        self.content_scroll = 0;
        self.update_preview()
    }

    pub fn sidebar_labels(&self) -> Vec<String> {
        self.tree_rows
            .iter()
            .map(|row| row.label.clone())
            .collect::<Vec<_>>()
    }

    pub fn sidebar_len(&self) -> usize {
        self.tree_rows.len()
    }

    pub fn sidebar_selected_row(&self) -> usize {
        self.tree_cursor.min(self.tree_rows.len().saturating_sub(1))
    }

    pub fn select_sidebar_row(&mut self, row: usize) {
        let Some(row_kind) = self.tree_rows.get(row).map(|entry| entry.kind.clone()) else {
            return;
        };

        match row_kind {
            FileTreeRowKind::Directory { .. } => {
                self.tree_cursor = row;
                self.toggle_directory_at_row(row);
            }
            FileTreeRowKind::File { .. } => {
                if let Some(file_index) = self.tree_rows.get(row).and_then(FileTreeRow::file_index)
                {
                    self.select(file_index);
                }
            }
        }
    }

    pub fn toggle_current_directory(&mut self) -> bool {
        if self.tree_rows.is_empty() {
            return false;
        }

        self.toggle_directory_at_row(self.tree_cursor)
    }

    fn select_index(&mut self, index: usize, auto_open: bool) -> Result<()> {
        let index = index.min(self.entries.len().saturating_sub(1));
        if index != self.selected && self.editor.as_ref().is_some_and(|editor| editor.dirty) {
            if let Some(editor) = self.editor.as_mut() {
                editor.mode = EditorMode::Normal;
                editor.status =
                    Some("Unsaved changes. Use :w, :q or :q! before switching files.".to_string());
            }
            return Ok(());
        }

        self.selected = index;
        self.content_scroll = 0;
        self.error = None;
        if let Some(row) = self.row_for_file(index) {
            self.tree_cursor = row;
        }
        self.update_preview()?;
        self.sync_editor_to_selection(auto_open)
    }

    fn select_tree_row(&mut self, row: usize, auto_open: bool) -> Result<()> {
        if self.tree_rows.is_empty() {
            return Ok(());
        }

        let target_row = row.min(self.tree_rows.len().saturating_sub(1));
        let previous_row = self.tree_cursor;
        let previous_selected = self.selected;
        self.tree_cursor = target_row;

        if let Some(file_index) = self
            .tree_rows
            .get(target_row)
            .and_then(FileTreeRow::file_index)
        {
            self.select_index(file_index, auto_open)?;
            if file_index != previous_selected && self.selected != file_index {
                self.tree_cursor = previous_row;
            }
        }

        Ok(())
    }

    fn toggle_directory_at_row(&mut self, row: usize) -> bool {
        let Some((relative_path, expanded)) = self
            .tree_rows
            .get(row)
            .and_then(FileTreeRow::directory_state)
        else {
            return false;
        };
        let relative_path = relative_path.clone();

        if expanded {
            self.collapsed_dirs.insert(relative_path.clone());
        } else {
            self.collapsed_dirs.remove(relative_path.as_path());
        }

        self.rebuild_tree_rows();
        if let Some(index) = self.row_for_directory(&relative_path) {
            self.tree_cursor = index;
        }
        true
    }

    fn rebuild_tree_rows(&mut self) {
        let (tree_rows, tree_file_rows) = build_file_tree_rows(&self.entries, &self.collapsed_dirs);
        self.tree_rows = tree_rows;

        if let Some(row) = tree_file_rows
            .get(self.selected)
            .and_then(|row| row.or_else(|| self.visible_ancestor_row(self.selected)))
        {
            self.tree_cursor = row;
        } else {
            self.tree_cursor = self.tree_cursor.min(self.tree_rows.len().saturating_sub(1));
        }
    }

    fn row_for_file(&self, file_index: usize) -> Option<usize> {
        self.tree_rows
            .iter()
            .position(|row| row.file_index() == Some(file_index))
    }

    fn row_for_directory(&self, relative_path: &Path) -> Option<usize> {
        self.tree_rows.iter().position(|row| {
            row.directory_path()
                .is_some_and(|path| path == relative_path)
        })
    }

    fn visible_ancestor_row(&self, file_index: usize) -> Option<usize> {
        let relative = self.entries.get(file_index)?.relative_path.as_path();
        let mut ancestor = relative.parent();

        while let Some(path) = ancestor {
            if self.collapsed_dirs.contains(path) {
                return self.row_for_directory(path);
            }
            ancestor = path.parent();
        }

        None
    }

    fn sync_editor_to_selection(&mut self, auto_open: bool) -> Result<()> {
        if self.entries.is_empty() {
            self.editor = None;
            return Ok(());
        }

        let entry = &self.entries[self.selected];
        if self
            .editor
            .as_ref()
            .is_some_and(|editor| editor.path == entry.path)
        {
            return Ok(());
        }

        if auto_open {
            match WorkspaceEditorState::open(&entry.path) {
                Ok(editor) => {
                    self.editor = Some(editor);
                    self.error = None;
                }
                Err(error) => {
                    self.editor = None;
                    self.error = Some(error.to_string());
                }
            }
        }

        Ok(())
    }

    fn update_preview(&mut self) -> Result<()> {
        if self.entries.is_empty() {
            self.preview_title = "Workspace".to_string();
            self.preview = plain_preview_lines("No files found for this workspace.");
            return Ok(());
        }

        let entry = &self.entries[self.selected];
        self.preview_title = entry.path.display().to_string();
        self.preview = read_text_preview(&entry.path)?;
        Ok(())
    }
}

impl FileTreeRow {
    fn file_index(&self) -> Option<usize> {
        match self.kind {
            FileTreeRowKind::Directory { .. } => None,
            FileTreeRowKind::File { file_index } => Some(file_index),
        }
    }

    fn directory_state(&self) -> Option<(&PathBuf, bool)> {
        match &self.kind {
            FileTreeRowKind::Directory {
                relative_path,
                expanded,
            } => Some((relative_path, *expanded)),
            FileTreeRowKind::File { .. } => None,
        }
    }

    fn directory_path(&self) -> Option<&PathBuf> {
        match &self.kind {
            FileTreeRowKind::Directory { relative_path, .. } => Some(relative_path),
            FileTreeRowKind::File { .. } => None,
        }
    }
}

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

        Ok(Self {
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
        })
    }

    pub fn rendered_lines(&self) -> Vec<Line<'static>> {
        let source = self.lines.join("\n");
        let syntax = syntax_for_path(&self.path, &source);
        let mut lines = add_line_numbers(highlighted_preview_lines(&source, syntax));
        if let Some(line) = lines.get_mut(self.cursor_row) {
            highlight_editor_line(line);
        }
        lines
    }

    pub fn content_height(&self) -> usize {
        self.lines.len().max(1)
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
    }

    pub fn insert_char(&mut self, character: char) {
        let byte_index = byte_index_for_char(&self.lines[self.cursor_row], self.cursor_col);
        self.lines[self.cursor_row].insert(byte_index, character);
        self.cursor_col += 1;
        self.preferred_col = self.cursor_col;
        self.dirty = true;
        self.status = None;
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
}

impl DiffBrowserState {
    pub fn refresh(&mut self, root: &Path) {
        match build_diff_entries(root).and_then(|sections| {
            self.changes = sections.changes;
            self.staged = sections.staged;
            self.selected_changes = self
                .selected_changes
                .min(self.changes.len().saturating_sub(1));
            self.selected_staged = self
                .selected_staged
                .min(self.staged.len().saturating_sub(1));
            self.adjust_active_section();
            self.content_scroll = 0;
            self.error = None;
            self.update_preview(root)
        }) {
            Ok(()) => {}
            Err(error) => {
                self.changes.clear();
                self.staged.clear();
                self.selected_changes = 0;
                self.selected_staged = 0;
                self.preview_title = "Git Diff".to_string();
                self.preview = plain_preview_lines("Unable to load git diff.");
                self.error = Some(error.to_string());
            }
        }
    }

    pub fn move_up(&mut self, root: &Path) {
        if self.visible_entries().is_empty() {
            return;
        }
        *self.selected_index_mut() = self.selected_index().saturating_sub(1);
        self.content_scroll = 0;
        let _ = self.update_preview(root);
    }

    pub fn move_down(&mut self, root: &Path) {
        if self.visible_entries().is_empty() {
            return;
        }
        let max_index = self.visible_entries().len().saturating_sub(1);
        *self.selected_index_mut() = (self.selected_index() + 1).min(max_index);
        self.content_scroll = 0;
        let _ = self.update_preview(root);
    }

    pub fn select(&mut self, root: &Path, index: usize) {
        if self.visible_entries().is_empty() {
            return;
        }
        *self.selected_index_mut() = index.min(self.visible_entries().len().saturating_sub(1));
        self.content_scroll = 0;
        let _ = self.update_preview(root);
    }

    pub fn set_active_section(&mut self, root: &Path, section: DiffSection) {
        if self.active_section == section {
            return;
        }

        self.active_section = section;
        self.content_scroll = 0;
        self.error = None;
        let _ = self.update_preview(root);
    }

    pub fn scroll_up(&mut self, lines: u16) {
        self.content_scroll = self.content_scroll.saturating_sub(lines);
    }

    pub fn scroll_down(&mut self, lines: u16) {
        self.content_scroll = self.content_scroll.saturating_add(lines);
    }

    pub fn commit(&mut self, root: &Path) -> Result<()> {
        let message = self.commit_message.trim();
        if message.is_empty() {
            return Err(anyhow::anyhow!("Commit message cannot be empty."));
        }

        let output = run_git_command(root, &["commit", "-m", message])?;
        self.commit_message.clear();
        self.status = Some(output);
        self.error = None;
        self.refresh(root);
        Ok(())
    }

    pub fn stage_selected(&mut self, root: &Path) -> Result<()> {
        if self.active_section != DiffSection::Changes {
            return Err(anyhow::anyhow!("Switch to Changes before staging a file."));
        }

        let relative = self.selected_relative_path(root)?;
        let output = run_git_command(root, &["add", "--", &relative])?;
        self.status = Some(output);
        self.error = None;
        self.refresh(root);
        Ok(())
    }

    pub fn unstage_selected(&mut self, root: &Path) -> Result<()> {
        if self.active_section != DiffSection::Staged {
            return Err(anyhow::anyhow!("Switch to Staged before unstaging a file."));
        }

        let relative = self.selected_relative_path(root)?;
        let output = run_git_command(root, &["restore", "--staged", "--", &relative])?;
        self.status = Some(output);
        self.error = None;
        self.refresh(root);
        Ok(())
    }

    pub fn discard_selected(&mut self, root: &Path) -> Result<()> {
        if self.active_section != DiffSection::Changes {
            return Err(anyhow::anyhow!("Discard is only available in Changes."));
        }

        let entry = self.selected_entry()?.clone();
        let relative = relative_entry_path(root, &entry.path)?;
        let output = if entry.status.contains('?') {
            run_git_command(root, &["clean", "-f", "--", &relative])?
        } else {
            run_git_command(root, &["restore", "--", &relative])?
        };

        self.status = Some(output);
        self.error = None;
        self.refresh(root);
        Ok(())
    }

    pub fn push(&mut self, root: &Path) -> Result<()> {
        let output = run_git_command(root, &["push"])?;
        self.status = Some(output);
        self.error = None;
        self.refresh(root);
        Ok(())
    }

    pub fn pull(&mut self, root: &Path) -> Result<()> {
        let output = run_git_command(root, &["pull", "--ff-only"])?;
        self.status = Some(output);
        self.error = None;
        self.refresh(root);
        Ok(())
    }

    pub fn visible_entries(&self) -> &[DiffEntry] {
        match self.active_section {
            DiffSection::Changes => &self.changes,
            DiffSection::Staged => &self.staged,
        }
    }

    pub fn selected_index(&self) -> usize {
        match self.active_section {
            DiffSection::Changes => self.selected_changes,
            DiffSection::Staged => self.selected_staged,
        }
    }

    pub fn count(&self, section: DiffSection) -> usize {
        match section {
            DiffSection::Changes => self.changes.len(),
            DiffSection::Staged => self.staged.len(),
        }
    }

    fn selected_index_mut(&mut self) -> &mut usize {
        match self.active_section {
            DiffSection::Changes => &mut self.selected_changes,
            DiffSection::Staged => &mut self.selected_staged,
        }
    }

    fn selected_entry(&self) -> Result<&DiffEntry> {
        self.visible_entries()
            .get(self.selected_index())
            .ok_or_else(|| anyhow::anyhow!("No file is selected."))
    }

    fn selected_relative_path(&self, root: &Path) -> Result<String> {
        let entry = self.selected_entry()?;
        relative_entry_path(root, &entry.path)
    }

    fn adjust_active_section(&mut self) {
        if self.visible_entries().is_empty() {
            if !self.changes.is_empty() {
                self.active_section = DiffSection::Changes;
            } else if !self.staged.is_empty() {
                self.active_section = DiffSection::Staged;
            }
        }
    }

    fn update_preview(&mut self, root: &Path) -> Result<()> {
        if self.visible_entries().is_empty() {
            self.preview_title = match self.active_section {
                DiffSection::Changes => "Changes".to_string(),
                DiffSection::Staged => "Staged".to_string(),
            };
            self.preview = match self.active_section {
                DiffSection::Changes => plain_preview_lines("No unstaged changes found."),
                DiffSection::Staged => plain_preview_lines("No staged changes found."),
            };
            return Ok(());
        }

        let entry = self.visible_entries()[self.selected_index()].clone();
        self.preview_title = match self.active_section {
            DiffSection::Changes => format!("Changes · {}", entry.path.display()),
            DiffSection::Staged => format!("Staged · {}", entry.path.display()),
        };
        self.preview = read_diff_preview(root, &entry, self.active_section)?;
        Ok(())
    }
}

fn build_file_entries(root: &Path) -> Result<Vec<FileEntry>> {
    let mut entries = Vec::new();
    let walker = WalkBuilder::new(root)
        .hidden(false)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .build();

    for result in walker {
        let entry = result.with_context(|| format!("failed to walk {}", root.display()))?;
        let path = entry.path();
        if path == root {
            continue;
        }
        if contains_git_component(path) {
            continue;
        }

        let relative = path
            .strip_prefix(root)
            .with_context(|| format!("failed to make {} relative", path.display()))?;
        if entry.file_type().is_some_and(|kind| kind.is_dir()) {
            continue;
        }

        entries.push(FileEntry {
            path: path.to_path_buf(),
            relative_path: relative.to_path_buf(),
        });
    }

    entries.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    Ok(entries)
}

#[derive(Debug, Default)]
struct FileTreeNode {
    directories: BTreeMap<String, FileTreeNode>,
    files: Vec<(String, usize)>,
}

fn build_file_tree_rows(
    entries: &[FileEntry],
    collapsed_dirs: &BTreeSet<PathBuf>,
) -> (Vec<FileTreeRow>, Vec<Option<usize>>) {
    let mut root = FileTreeNode::default();

    for (index, entry) in entries.iter().enumerate() {
        let components = entry
            .relative_path
            .components()
            .map(|component| component.as_os_str().to_string_lossy().to_string())
            .collect::<Vec<_>>();
        if components.is_empty() {
            continue;
        }

        let mut node = &mut root;
        for directory in &components[..components.len().saturating_sub(1)] {
            node = node.directories.entry(directory.clone()).or_default();
        }

        if let Some(file_name) = components.last() {
            node.files.push((file_name.clone(), index));
        }
    }

    let mut rows = Vec::new();
    let mut file_rows = vec![None; entries.len()];
    append_tree_rows(
        &root,
        Path::new(""),
        &[],
        collapsed_dirs,
        &mut rows,
        &mut file_rows,
    );
    (rows, file_rows)
}

fn append_tree_rows(
    node: &FileTreeNode,
    base_path: &Path,
    ancestor_has_more: &[bool],
    collapsed_dirs: &BTreeSet<PathBuf>,
    rows: &mut Vec<FileTreeRow>,
    file_rows: &mut [Option<usize>],
) {
    let directories = node
        .directories
        .iter()
        .map(|(name, child)| (TreeChild::Directory(name), child))
        .collect::<Vec<_>>();
    let files = node
        .files
        .iter()
        .map(|(name, index)| (TreeChild::File(name, *index), node))
        .collect::<Vec<_>>();

    let children = directories.into_iter().chain(files).collect::<Vec<_>>();

    for (position, (child, child_node)) in children.iter().enumerate() {
        let is_last = position + 1 == children.len();
        let mut label = tree_row_prefix(ancestor_has_more);
        label.push_str(if is_last { "└── " } else { "├── " });

        match child {
            TreeChild::Directory(name) => {
                let relative_path = if base_path.as_os_str().is_empty() {
                    PathBuf::from(name)
                } else {
                    base_path.join(name)
                };
                let expanded = !collapsed_dirs.contains(&relative_path);
                label.push_str(if expanded { "▾ " } else { "▸ " });
                label.push_str(name);
                rows.push(FileTreeRow {
                    label,
                    kind: FileTreeRowKind::Directory {
                        relative_path: relative_path.clone(),
                        expanded,
                    },
                });

                if expanded {
                    let mut next_prefix = ancestor_has_more.to_vec();
                    next_prefix.push(!is_last);
                    append_tree_rows(
                        child_node,
                        &relative_path,
                        &next_prefix,
                        collapsed_dirs,
                        rows,
                        file_rows,
                    );
                }
            }
            TreeChild::File(name, file_index) => {
                label.push_str(name);
                file_rows[*file_index] = Some(rows.len());
                rows.push(FileTreeRow {
                    label,
                    kind: FileTreeRowKind::File {
                        file_index: *file_index,
                    },
                });
            }
        }
    }
}

fn tree_row_prefix(ancestor_has_more: &[bool]) -> String {
    ancestor_has_more
        .iter()
        .map(|has_more| if *has_more { "│   " } else { "    " })
        .collect::<Vec<_>>()
        .join("")
}

enum TreeChild<'a> {
    Directory(&'a str),
    File(&'a str, usize),
}

#[derive(Debug, Clone, Default)]
struct DiffSections {
    changes: Vec<DiffEntry>,
    staged: Vec<DiffEntry>,
}

fn build_diff_entries(root: &Path) -> Result<DiffSections> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["status", "--short", "--untracked-files=all"])
        .output()
        .with_context(|| format!("failed to run git status in {}", root.display()))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow::anyhow!(stderr.trim().to_string()));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut sections = DiffSections::default();

    for line in stdout.lines() {
        if line.len() < 3 || !line.is_char_boundary(2) || !line.is_char_boundary(3) {
            continue;
        }

        let index_status = line.as_bytes()[0] as char;
        let worktree_status = line.as_bytes()[1] as char;
        let raw_path = line[3..].trim();
        let path = raw_path
            .rsplit_once(" -> ")
            .map(|(_, new_path)| new_path)
            .unwrap_or(raw_path);
        let full_path = root.join(path);

        if index_status != ' ' && index_status != '?' {
            sections.staged.push(DiffEntry {
                label: format!("[{}] {}", index_status, path),
                path: full_path.clone(),
                status: index_status.to_string(),
            });
        }

        if worktree_status != ' ' || (index_status == '?' && worktree_status == '?') {
            let status = if index_status == '?' && worktree_status == '?' {
                "??".to_string()
            } else {
                worktree_status.to_string()
            };
            sections.changes.push(DiffEntry {
                label: format!("[{}] {}", status, path),
                path: full_path,
                status,
            });
        }
    }

    sections
        .changes
        .sort_by(|left, right| left.path.cmp(&right.path));
    sections
        .staged
        .sort_by(|left, right| left.path.cmp(&right.path));

    Ok(sections)
}

fn read_text_preview(path: &Path) -> Result<Vec<Line<'static>>> {
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    if bytes.contains(&0) {
        return Ok(plain_preview_lines("Binary file preview is not available."));
    }

    let truncated = bytes.len() > PREVIEW_LIMIT;
    let preview = String::from_utf8_lossy(&bytes[..bytes.len().min(PREVIEW_LIMIT)]).into_owned();
    let preview = normalize_newlines(&preview);
    let mut lines = highlighted_preview_lines(&preview, syntax_for_path(path, &preview));
    if !truncated {
        maybe_trim_trailing_empty_line(&mut lines);
    }
    let mut lines = add_line_numbers(lines);
    if truncated {
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(
            "[truncated]".to_string(),
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::ITALIC),
        )));
    }
    Ok(lines)
}

fn read_diff_preview(
    root: &Path,
    entry: &DiffEntry,
    section: DiffSection,
) -> Result<Vec<Line<'static>>> {
    if entry.status.contains('?') {
        let mut lines = plain_preview_lines(&format!("Untracked file: {}", entry.path.display()));
        lines.push(Line::default());
        lines.extend(read_text_preview(&entry.path)?);
        return Ok(lines);
    }

    let relative = relative_entry_path(root, &entry.path)?;

    let diff = match section {
        DiffSection::Changes => git_output(root, &["diff", "--no-ext-diff", "--", &relative])?,
        DiffSection::Staged => git_output(
            root,
            &["diff", "--no-ext-diff", "--cached", "--", &relative],
        )?,
    };

    if diff.trim().is_empty() {
        Ok(plain_preview_lines("No diff available for this file."))
    } else {
        Ok(add_line_numbers(highlighted_preview_lines(
            &normalize_newlines(&diff),
            SYNTAX_SET.find_syntax_by_extension("diff"),
        )))
    }
}

fn git_output(root: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .with_context(|| format!("failed to run git {:?} in {}", args, root.display()))?;

    if !output.status.success() {
        return Ok(String::new());
    }

    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    if stdout.len() > PREVIEW_LIMIT {
        Ok(format!("{}\n\n[truncated]", &stdout[..PREVIEW_LIMIT]))
    } else {
        Ok(stdout)
    }
}

fn relative_entry_path(root: &Path, path: &Path) -> Result<String> {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_str()
        .map(|value| value.to_string())
        .ok_or_else(|| anyhow::anyhow!("Path contains unsupported characters: {}", path.display()))
}

fn run_git_command(root: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .with_context(|| format!("failed to run git {:?} in {}", args, root.display()))?;

    let stdout = normalize_newlines(&String::from_utf8_lossy(&output.stdout));
    let stderr = normalize_newlines(&String::from_utf8_lossy(&output.stderr));
    let summary = summarize_git_command_output(&stdout, &stderr);

    if !output.status.success() {
        return Err(anyhow::anyhow!(if summary.is_empty() {
            format!("git {:?} failed", args)
        } else {
            summary
        }));
    }

    Ok(if summary.is_empty() {
        format!("git {} finished successfully", args.join(" "))
    } else {
        summary
    })
}

fn summarize_git_command_output(stdout: &str, stderr: &str) -> String {
    let combined = [stdout.trim(), stderr.trim()]
        .into_iter()
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>()
        .join("\n");

    if combined.is_empty() {
        return String::new();
    }

    let mut summary = combined.lines().take(3).collect::<Vec<_>>().join(" ");
    if summary.chars().count() > 240 {
        summary = summary.chars().take(237).collect::<String>();
        summary.push_str("...");
    }
    summary
}

fn contains_git_component(path: &Path) -> bool {
    path.components()
        .any(|component| component.as_os_str() == ".git")
}

fn syntax_for_path<'a>(path: &Path, source: &'a str) -> Option<&'static SyntaxReference> {
    path.extension()
        .and_then(|extension| extension.to_str())
        .and_then(|extension| SYNTAX_SET.find_syntax_by_extension(extension))
        .or_else(|| {
            source
                .lines()
                .next()
                .and_then(|line| SYNTAX_SET.find_syntax_by_first_line(line))
        })
}

fn highlighted_preview_lines(
    source: &str,
    syntax: Option<&'static SyntaxReference>,
) -> Vec<Line<'static>> {
    match syntax {
        Some(syntax) => {
            let mut highlighter = HighlightLines::new(syntax, &THEME);
            split_preserving_lines(source)
                .into_iter()
                .map(|line| {
                    if line.is_empty() {
                        return Line::default();
                    }

                    match highlighter.highlight_line(&line, &SYNTAX_SET) {
                        Ok(ranges) => Line::from(
                            ranges
                                .into_iter()
                                .map(|(style, text)| {
                                    Span::styled(text.to_string(), to_ratatui_style(style))
                                })
                                .collect::<Vec<_>>(),
                        ),
                        Err(_) => Line::from(line),
                    }
                })
                .collect()
        }
        None => plain_preview_lines(source),
    }
}

fn plain_preview_lines(source: &str) -> Vec<Line<'static>> {
    split_preserving_lines(source)
        .into_iter()
        .map(Line::from)
        .collect()
}

fn add_line_numbers(lines: Vec<Line<'static>>) -> Vec<Line<'static>> {
    let gutter_width = lines.len().max(1).to_string().len();

    lines
        .into_iter()
        .enumerate()
        .map(|(index, mut line)| {
            let mut spans = Vec::with_capacity(line.spans.len() + 1);
            spans.push(Span::styled(
                format!("{:>width$} | ", index + 1, width = gutter_width),
                Style::default().fg(Color::DarkGray),
            ));
            spans.append(&mut line.spans);
            line.spans = spans;
            line
        })
        .collect()
}

fn highlight_editor_line(line: &mut Line<'static>) {
    let background = Color::Rgb(45, 50, 60);
    for span in &mut line.spans {
        span.style = span.style.bg(background);
    }
}

fn byte_index_for_char(source: &str, char_index: usize) -> usize {
    source
        .char_indices()
        .nth(char_index)
        .map(|(index, _)| index)
        .unwrap_or(source.len())
}

fn split_preserving_lines(source: &str) -> Vec<String> {
    if source.is_empty() {
        return vec![String::new()];
    }

    source.split('\n').map(ToString::to_string).collect()
}

fn normalize_newlines(source: &str) -> String {
    source.replace("\r\n", "\n").replace('\r', "\n")
}

fn maybe_trim_trailing_empty_line(lines: &mut Vec<Line<'static>>) {
    if lines.len() > 1
        && lines
            .last()
            .is_some_and(|line| line.spans.iter().all(|span| span.content.is_empty()))
    {
        lines.pop();
    }
}

fn to_ratatui_style(style: syntect::highlighting::Style) -> Style {
    let mut modifiers = Modifier::empty();
    if style.font_style.contains(FontStyle::BOLD) {
        modifiers |= Modifier::BOLD;
    }
    if style.font_style.contains(FontStyle::ITALIC) {
        modifiers |= Modifier::ITALIC;
    }
    if style.font_style.contains(FontStyle::UNDERLINE) {
        modifiers |= Modifier::UNDERLINED;
    }

    Style::default()
        .fg(Color::Rgb(
            style.foreground.r,
            style.foreground.g,
            style.foreground.b,
        ))
        .add_modifier(modifiers)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skips_git_subtree_entries() {
        assert!(contains_git_component(Path::new("/tmp/repo/.git/config")));
        assert!(contains_git_component(Path::new(
            "/tmp/repo/.git/objects/aa"
        )));
        assert!(!contains_git_component(Path::new("/tmp/repo/src/main.rs")));
    }

    #[test]
    fn preserves_blank_lines_in_plain_preview() {
        let lines = plain_preview_lines("first\n\nthird");
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0].spans[0].content, "first");
        assert!(lines[1].spans.is_empty() || lines[1].spans[0].content.is_empty());
        assert_eq!(lines[2].spans[0].content, "third");
    }

    #[test]
    fn adds_line_numbers_to_blank_and_non_blank_lines() {
        let lines = add_line_numbers(plain_preview_lines("first\n\nthird"));

        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0].spans[0].content, "1 | ");
        assert_eq!(lines[0].spans[1].content, "first");
        assert_eq!(lines[1].spans[0].content, "2 | ");
        assert_eq!(lines[2].spans[0].content, "3 | ");
        assert_eq!(lines[2].spans[1].content, "third");
    }

    #[test]
    fn editor_inserts_text_and_saves() {
        let path =
            std::env::temp_dir().join(format!("cmdex-editor-save-{}.txt", std::process::id()));
        fs::write(&path, "hello\n").unwrap();

        let mut editor = WorkspaceEditorState::open(&path).unwrap();
        editor.move_line_end();
        editor.enter_insert_mode();
        editor.insert_char('!');
        editor.save().unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), "hello!\n");

        let _ = fs::remove_file(path);
    }

    #[test]
    fn editor_backspace_merges_lines() {
        let path =
            std::env::temp_dir().join(format!("cmdex-editor-merge-{}.txt", std::process::id()));
        fs::write(&path, "hello\nworld").unwrap();

        let mut editor = WorkspaceEditorState::open(&path).unwrap();
        editor.cursor_row = 1;
        editor.cursor_col = 0;
        editor.backspace();

        assert_eq!(editor.lines, vec!["helloworld".to_string()]);

        let _ = fs::remove_file(path);
    }

    #[test]
    fn editor_refuses_quit_when_dirty_without_bang() {
        let path = std::env::temp_dir().join(format!("cmdex-editor-q-{}.txt", std::process::id()));
        fs::write(&path, "hello").unwrap();

        let mut editor = WorkspaceEditorState::open(&path).unwrap();
        editor.enter_insert_mode();
        editor.insert_char('!');
        editor.mode = EditorMode::Command;
        editor.command = "q".to_string();

        let result = editor.execute_command().unwrap();

        assert!(!result.close);
        assert!(
            editor
                .status
                .as_deref()
                .is_some_and(|status| status.contains("Unsaved"))
        );

        let _ = fs::remove_file(path);
    }

    #[test]
    fn selecting_a_file_auto_opens_editor_in_normal_mode() {
        let path =
            std::env::temp_dir().join(format!("cmdex-editor-auto-open-{}.txt", std::process::id()));
        fs::write(&path, "hello").unwrap();

        let mut browser = FileBrowserState {
            entries: vec![FileEntry {
                path: path.clone(),
                relative_path: PathBuf::from("hello.txt"),
            }],
            ..Default::default()
        };

        browser.select(0);

        let editor = browser.editor.as_ref().expect("editor");
        assert_eq!(editor.path, path);
        assert_eq!(editor.mode, EditorMode::Normal);

        let _ = fs::remove_file(path);
    }

    #[test]
    fn dirty_editor_blocks_switching_selected_file() {
        let first =
            std::env::temp_dir().join(format!("cmdex-editor-first-{}.txt", std::process::id()));
        let second =
            std::env::temp_dir().join(format!("cmdex-editor-second-{}.txt", std::process::id()));
        fs::write(&first, "first").unwrap();
        fs::write(&second, "second").unwrap();

        let mut browser = FileBrowserState {
            entries: vec![
                FileEntry {
                    path: first.clone(),
                    relative_path: PathBuf::from("first.txt"),
                },
                FileEntry {
                    path: second.clone(),
                    relative_path: PathBuf::from("second.txt"),
                },
            ],
            ..Default::default()
        };

        browser.select(0);
        browser.editor.as_mut().unwrap().insert_char('!');
        browser.select(1);

        assert_eq!(browser.selected, 0);
        assert_eq!(browser.editor.as_ref().unwrap().path, first);
        assert!(
            browser
                .editor
                .as_ref()
                .and_then(|editor| editor.status.as_deref())
                .is_some_and(|status| status.contains("Unsaved changes"))
        );

        let _ = fs::remove_file(first);
        let _ = fs::remove_file(second);
    }

    #[test]
    fn builds_workspace_sidebar_tree_rows_from_files() {
        let entries = vec![
            FileEntry {
                path: PathBuf::from("/tmp/repo/README.md"),
                relative_path: PathBuf::from("README.md"),
            },
            FileEntry {
                path: PathBuf::from("/tmp/repo/src/app.rs"),
                relative_path: PathBuf::from("src/app.rs"),
            },
            FileEntry {
                path: PathBuf::from("/tmp/repo/src/ui/mod.rs"),
                relative_path: PathBuf::from("src/ui/mod.rs"),
            },
        ];

        let (rows, file_rows) = build_file_tree_rows(&entries, &BTreeSet::new());

        assert_eq!(
            rows.iter()
                .map(|row| row.label.as_str())
                .collect::<Vec<_>>(),
            vec![
                "├── ▾ src",
                "│   ├── ▾ ui",
                "│   │   └── mod.rs",
                "│   └── app.rs",
                "└── README.md",
            ]
        );
        assert_eq!(file_rows, vec![Some(4), Some(3), Some(2)]);
        assert!(rows[1].directory_path().is_some());
        assert_eq!(rows[3].file_index(), Some(1));
    }

    #[test]
    fn hides_descendants_for_collapsed_directory_rows() {
        let entries = vec![
            FileEntry {
                path: PathBuf::from("/tmp/repo/src/app.rs"),
                relative_path: PathBuf::from("src/app.rs"),
            },
            FileEntry {
                path: PathBuf::from("/tmp/repo/src/ui/mod.rs"),
                relative_path: PathBuf::from("src/ui/mod.rs"),
            },
        ];
        let collapsed = BTreeSet::from([PathBuf::from("src")]);

        let (rows, file_rows) = build_file_tree_rows(&entries, &collapsed);

        assert_eq!(
            rows.iter()
                .map(|row| row.label.as_str())
                .collect::<Vec<_>>(),
            vec!["└── ▸ src"]
        );
        assert_eq!(file_rows, vec![None, None]);
    }

    #[test]
    fn splits_git_status_into_changes_and_staged_sections() {
        let root = Path::new("/tmp/repo");
        let output = "\
MM src/app.rs
A  src/new.rs
 M src/dirty.rs
?? README.md
R  src/old.rs -> src/new_name.rs
";

        let mut sections = DiffSections::default();

        for line in output.lines() {
            if line.len() < 3 || !line.is_char_boundary(2) || !line.is_char_boundary(3) {
                continue;
            }

            let index_status = line.as_bytes()[0] as char;
            let worktree_status = line.as_bytes()[1] as char;
            let raw_path = line[3..].trim();
            let path = raw_path
                .rsplit_once(" -> ")
                .map(|(_, new_path)| new_path)
                .unwrap_or(raw_path);
            let full_path = root.join(path);

            if index_status != ' ' && index_status != '?' {
                sections.staged.push(DiffEntry {
                    label: format!("[{}] {}", index_status, path),
                    path: full_path.clone(),
                    status: index_status.to_string(),
                });
            }

            if worktree_status != ' ' || (index_status == '?' && worktree_status == '?') {
                let status = if index_status == '?' && worktree_status == '?' {
                    "??".to_string()
                } else {
                    worktree_status.to_string()
                };
                sections.changes.push(DiffEntry {
                    label: format!("[{}] {}", status, path),
                    path: full_path,
                    status,
                });
            }
        }

        assert_eq!(
            sections
                .changes
                .iter()
                .map(|entry| entry.label.as_str())
                .collect::<Vec<_>>(),
            vec!["[M] src/app.rs", "[M] src/dirty.rs", "[??] README.md"]
        );
        assert_eq!(
            sections
                .staged
                .iter()
                .map(|entry| entry.label.as_str())
                .collect::<Vec<_>>(),
            vec!["[M] src/app.rs", "[A] src/new.rs", "[R] src/new_name.rs"]
        );
    }
}
