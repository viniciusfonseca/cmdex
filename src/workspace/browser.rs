use super::{render::*, *};

#[path = "browser_loading.rs"]
mod browser_loading;
#[path = "browser_support.rs"]
mod browser_support;

pub(crate) use browser_support::WorkspaceIndex;
pub(super) struct WorkspaceBrowserSupport;

impl FileBrowserState {
    pub(crate) fn entries(&self) -> &[FileEntry] {
        self.index.entries()
    }

    #[cfg(test)]
    pub(crate) fn with_entries(entries: Vec<FileEntry>) -> Self {
        let mut browser = Self::default();
        browser.index.replace(entries);
        browser.rebuild_tree_rows();
        browser
    }

    pub(crate) fn scan_entries(root: &Path) -> Result<Vec<FileEntry>> {
        browser_support::WorkspaceIndex::scan(root)
    }

    pub(crate) fn apply_scanned_entries_without_io(
        &mut self,
        entries: Vec<FileEntry>,
    ) -> Result<bool> {
        if entries == self.entries() {
            return Ok(false);
        }

        let keep_dirty_editor = self.reconcile_entries(entries);
        if !keep_dirty_editor
            && self.editor.as_ref().is_some_and(|editor| {
                self.entries()
                    .get(self.selected)
                    .is_none_or(|entry| editor.path != entry.path)
            })
        {
            self.editor = None;
            self.focus_sidebar();
        }
        self.preview_title = self
            .entries()
            .get(self.selected)
            .map(|entry| entry.path.display().to_string())
            .unwrap_or_else(|| "Workspace".to_string());
        self.preview = WorkspaceRenderer::plain_preview_lines("Loading preview...");
        Ok(true)
    }

    #[allow(dead_code)]
    pub fn move_up(&mut self) {
        if self.tree_rows.is_empty() {
            return;
        }
        let _ = self.select_tree_row(self.tree_cursor.saturating_sub(1), true);
    }

    pub(crate) fn move_up_without_io(&mut self) -> bool {
        if self.tree_rows.is_empty() {
            return false;
        }
        self.select_tree_row_without_io(self.tree_cursor.saturating_sub(1))
    }

    #[allow(dead_code)]
    pub fn move_down(&mut self) {
        if self.tree_rows.is_empty() {
            return;
        }
        let _ = self.select_tree_row(
            (self.tree_cursor + 1).min(self.tree_rows.len().saturating_sub(1)),
            true,
        );
    }

    pub(crate) fn move_down_without_io(&mut self) -> bool {
        if self.tree_rows.is_empty() {
            return false;
        }
        self.select_tree_row_without_io(
            (self.tree_cursor + 1).min(self.tree_rows.len().saturating_sub(1)),
        )
    }

    pub(crate) fn select_without_io(&mut self, index: usize) -> bool {
        if self.entries().is_empty() {
            return false;
        }
        self.select_index_without_io(index.min(self.entries().len().saturating_sub(1)))
    }

    #[allow(dead_code)]
    pub fn select(&mut self, index: usize) {
        if self.entries().is_empty() {
            return;
        }
        let _ = self.select_index(index.min(self.entries().len().saturating_sub(1)), true);
    }

    pub fn scroll_up(&mut self, lines: u16) {
        self.content_scroll = self.content_scroll.saturating_sub(lines);
    }

    pub fn scroll_down(&mut self, lines: u16) {
        self.content_scroll = self.content_scroll.saturating_add(lines);
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn open_editor(&mut self) -> Result<()> {
        if self.entries().is_empty() {
            return Ok(());
        }

        self.editor = Some(WorkspaceEditorState::open(
            &self.entries()[self.selected].path,
        )?);
        self.focus_editor();
        self.error = None;
        Ok(())
    }

    pub fn sidebar_items(&self) -> Vec<Line<'static>> {
        self.tree_rows
            .iter()
            .map(FileTreeRow::styled_label)
            .collect::<Vec<_>>()
    }

    pub fn sidebar_len(&self) -> usize {
        self.tree_rows.len()
    }

    pub fn set_sidebar_tab(&mut self, tab: WorkspaceSidebarTab) {
        self.sidebar_tab = tab;
    }

    pub fn focus_sidebar(&mut self) {
        self.focus = WorkspaceFocus::Sidebar;
    }

    pub fn focus_editor(&mut self) {
        if self.editor.is_some() {
            self.focus = WorkspaceFocus::Editor;
        }
    }

    pub fn toggle_focus(&mut self) {
        if self.editor.is_none() {
            self.focus = WorkspaceFocus::Sidebar;
            return;
        }

        self.focus = match self.focus {
            WorkspaceFocus::Sidebar => WorkspaceFocus::Editor,
            WorkspaceFocus::Editor => WorkspaceFocus::Sidebar,
        };
    }

    pub fn sidebar_focused(&self) -> bool {
        self.editor.is_none() || self.focus == WorkspaceFocus::Sidebar
    }

    pub fn editor_focused(&self) -> bool {
        self.editor.is_some() && self.focus == WorkspaceFocus::Editor
    }

    pub fn search_rows_labels(&self) -> Vec<String> {
        self.search_rows
            .iter()
            .map(|row| row.label().to_string())
            .collect::<Vec<_>>()
    }

    pub fn search_total_rows(&self) -> usize {
        self.search_rows.len()
    }

    pub fn search_selected_row(&self) -> usize {
        self.search_selected_row
            .min(self.search_rows.len().saturating_sub(1))
    }

    pub fn search_match_count(&self) -> usize {
        self.search_match_count
    }

    pub fn push_search_char(&mut self, character: char) {
        self.search_query.push(character);
        self.request_search();
    }

    pub fn pop_search_char(&mut self) {
        self.search_query.pop();
        self.request_search();
    }

    pub(crate) fn take_search_request(&mut self) -> Option<(String, u64)> {
        if self.search_query.trim().is_empty() {
            self.search_requested_at = None;
            self.search_in_flight = false;
            return None;
        }
        if self.search_in_flight
            || self
                .search_requested_at
                .is_none_or(|requested| requested.elapsed() < SEARCH_DEBOUNCE)
        {
            return None;
        }

        self.search_requested_at = None;
        self.search_in_flight = true;
        Some((self.search_query.clone(), self.search_generation))
    }

    #[cfg(test)]
    pub(crate) fn search_generation(&self) -> u64 {
        self.search_generation
    }

    pub(crate) fn search_entries(
        entries: &[FileEntry],
        query: &str,
    ) -> Result<WorkspaceSearchSnapshot> {
        WorkspaceBrowserSupport::build_search_snapshot(entries, query)
    }

    pub(crate) fn apply_search_snapshot(
        &mut self,
        generation: u64,
        query: &str,
        snapshot: WorkspaceSearchSnapshot,
    ) -> bool {
        if generation != self.search_generation || query != self.search_query {
            self.search_in_flight = false;
            self.search_requested_at = Some(Instant::now());
            return false;
        }

        let previous_target = self.selected_search_target();
        self.search_rows = snapshot.rows;
        self.search_match_count = snapshot.match_count;
        self.search_in_flight = false;
        self.search_requested_at = None;
        self.search_selected_row = previous_target
            .and_then(|(file_index, line_number)| {
                self.find_search_match_row(file_index, line_number)
            })
            .or_else(|| self.first_search_match_row())
            .unwrap_or(0);
        true
    }

    pub fn search_move_up(&mut self) {
        if let Some(row) = self.previous_search_match_row(self.search_selected_row) {
            self.search_selected_row = row;
        }
    }

    pub fn search_move_down(&mut self) {
        let start = self.search_selected_row.saturating_add(1);
        if let Some(row) = self.next_search_match_row(start) {
            self.search_selected_row = row;
        }
    }

    pub fn select_search_row(&mut self, row: usize) {
        if self
            .search_rows
            .get(row)
            .is_some_and(WorkspaceSearchRow::is_match)
        {
            self.search_selected_row = row;
        }
    }

    #[allow(dead_code)]
    pub fn open_selected_search_result(&mut self) -> Result<bool> {
        let Some((file_index, line_number)) = self.selected_search_target() else {
            return Ok(false);
        };

        self.select_index(file_index, true)?;
        if self.selected != file_index {
            return Ok(false);
        }

        if let Some(editor) = self.editor.as_mut() {
            let row = line_number.saturating_sub(1);
            editor.set_cursor(row, 0);
            editor.vertical_scroll = row as u16;
            editor.horizontal_scroll = 0;
        }
        self.focus_editor();

        Ok(true)
    }

    pub(crate) fn open_selected_search_result_without_io(&mut self) -> Option<EditorPosition> {
        let (file_index, line_number) = self.selected_search_target()?;
        if !self.select_index_without_io(file_index) {
            return None;
        }
        Some(EditorPosition {
            row: line_number.saturating_sub(1),
            col: 0,
        })
    }

    #[allow(dead_code)]
    pub fn open_path_at_position(&mut self, path: &Path, row: usize, col: usize) -> Result<bool> {
        let Some(file_index) = self.entries().iter().position(|entry| entry.path == path) else {
            return Ok(false);
        };

        self.select_index(file_index, true)?;
        if self.selected != file_index {
            return Ok(false);
        }

        if let Some(editor) = self.editor.as_mut() {
            editor.set_cursor(row, col);
            editor.vertical_scroll = row.saturating_sub(3) as u16;
            editor.horizontal_scroll = col.saturating_sub(8) as u16;
        }
        self.focus_editor();

        Ok(true)
    }

    pub(crate) fn open_path_at_position_without_io(&mut self, path: &Path) -> Option<PathBuf> {
        let file_index = self.entries().iter().position(|entry| entry.path == path)?;
        self.select_index_without_io(file_index)
            .then(|| path.to_path_buf())
    }

    pub fn sidebar_selected_row(&self) -> usize {
        self.tree_cursor.min(self.tree_rows.len().saturating_sub(1))
    }

    #[allow(dead_code)]
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

    pub(crate) fn select_sidebar_row_without_io(&mut self, row: usize) -> bool {
        let Some(row_kind) = self.tree_rows.get(row).map(|entry| entry.kind.clone()) else {
            return false;
        };

        match row_kind {
            FileTreeRowKind::Directory { .. } => {
                self.tree_cursor = row;
                self.toggle_directory_at_row(row);
                false
            }
            FileTreeRowKind::File { file_index } => self.select_without_io(file_index),
        }
    }

    pub fn toggle_current_directory(&mut self) -> bool {
        if self.tree_rows.is_empty() {
            return false;
        }

        self.toggle_directory_at_row(self.tree_cursor)
    }

    #[allow(dead_code)]
    fn select_index(&mut self, index: usize, auto_open: bool) -> Result<()> {
        if !self.select_index_without_io(index) {
            return Ok(());
        }

        self.update_preview()?;
        self.sync_editor_to_selection(auto_open)
    }

    fn select_index_without_io(&mut self, index: usize) -> bool {
        let index = index.min(self.entries().len().saturating_sub(1));
        if index != self.selected && self.editor.as_ref().is_some_and(|editor| editor.dirty) {
            if let Some(editor) = self.editor.as_mut() {
                editor.enter_normal_mode();
                editor.status =
                    Some("Unsaved changes. Use :w, :q or :q! before switching files.".to_string());
            }
            return false;
        }

        self.selected = index;
        self.content_scroll = 0;
        self.error = None;
        if let Some(row) = self.row_for_file(index) {
            self.tree_cursor = row;
        }
        let selected_path = self.entries().get(self.selected).map(|entry| &entry.path);
        if self
            .editor
            .as_ref()
            .is_some_and(|editor| Some(&editor.path) != selected_path && !editor.dirty)
        {
            self.editor = None;
            self.focus_sidebar();
        }
        true
    }

    fn reconcile_entries(&mut self, entries: Vec<FileEntry>) -> bool {
        let previous_selected = self.selected;
        let previous_selected_path = self
            .entries()
            .get(self.selected)
            .map(|entry| entry.path.clone());
        let previous_editor_path = self.editor.as_ref().map(|editor| editor.path.clone());
        let previous_directory_cursor = self
            .tree_rows
            .get(self.tree_cursor)
            .and_then(FileTreeRow::directory_path)
            .cloned();
        let previous_content_scroll = self.content_scroll;
        let keep_dirty_editor = self.editor.as_ref().is_some_and(|editor| {
            editor.dirty && !entries.iter().any(|entry| entry.path == editor.path)
        });

        self.index.replace(entries);
        self.selected = self.resolve_selected_index(
            previous_editor_path
                .as_ref()
                .or(previous_selected_path.as_ref()),
            previous_selected,
        );
        self.sync_collapsed_dirs();
        self.rebuild_tree_rows();
        if let Some(row) = previous_directory_cursor
            .as_ref()
            .and_then(|relative_path| self.row_for_directory(relative_path))
        {
            self.tree_cursor = row;
        }
        self.request_search();
        self.content_scroll = if previous_selected_path.as_ref().is_some_and(|path| {
            self.entries()
                .get(self.selected)
                .is_some_and(|entry| &entry.path == path)
        }) {
            previous_content_scroll
        } else {
            0
        };
        self.error = None;
        keep_dirty_editor
    }

    fn resolve_selected_index(
        &self,
        selected_path: Option<&PathBuf>,
        fallback_index: usize,
    ) -> usize {
        if self.entries().is_empty() {
            return 0;
        }

        selected_path
            .and_then(|path| self.entries().iter().position(|entry| entry.path == *path))
            .unwrap_or_else(|| fallback_index.min(self.entries().len().saturating_sub(1)))
    }

    #[allow(dead_code)]
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

    fn select_tree_row_without_io(&mut self, row: usize) -> bool {
        if self.tree_rows.is_empty() {
            return false;
        }

        let target_row = row.min(self.tree_rows.len().saturating_sub(1));
        let previous_row = self.tree_cursor;
        self.tree_cursor = target_row;

        if let Some(file_index) = self
            .tree_rows
            .get(target_row)
            .and_then(FileTreeRow::file_index)
        {
            if !self.select_index_without_io(file_index) {
                self.tree_cursor = previous_row;
                return false;
            }
            return true;
        }
        false
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
        let (tree_rows, tree_file_rows) =
            WorkspaceBrowserSupport::build_file_tree_rows(self.entries(), &self.collapsed_dirs);
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

    fn sync_collapsed_dirs(&mut self) {
        let directory_paths = WorkspaceBrowserSupport::collect_directory_paths(self.entries());

        if self.known_dirs.is_empty() {
            self.collapsed_dirs = directory_paths.clone();
        } else {
            self.collapsed_dirs
                .retain(|path| directory_paths.contains(path));
            for path in directory_paths.difference(&self.known_dirs) {
                self.collapsed_dirs.insert(path.clone());
            }
        }

        self.known_dirs = directory_paths;
    }

    fn request_search(&mut self) {
        self.search_generation = self.search_generation.wrapping_add(1);
        self.search_requested_at = Some(Instant::now());
        self.search_in_flight = false;
        self.search_rows.clear();
        self.search_selected_row = 0;
        self.search_match_count = 0;
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
        let relative = self.entries().get(file_index)?.relative_path.as_path();
        let mut ancestor = relative.parent();

        while let Some(path) = ancestor {
            if self.collapsed_dirs.contains(path) {
                return self.row_for_directory(path);
            }
            ancestor = path.parent();
        }

        None
    }

    fn selected_search_target(&self) -> Option<(usize, usize)> {
        self.search_rows
            .get(self.search_selected_row)
            .and_then(WorkspaceSearchRow::target)
    }

    fn first_search_match_row(&self) -> Option<usize> {
        self.next_search_match_row(0)
    }

    fn next_search_match_row(&self, start: usize) -> Option<usize> {
        self.search_rows
            .iter()
            .enumerate()
            .skip(start)
            .find_map(|(index, row)| row.is_match().then_some(index))
    }

    fn previous_search_match_row(&self, start: usize) -> Option<usize> {
        self.search_rows
            .iter()
            .enumerate()
            .take(start)
            .rev()
            .find_map(|(index, row)| row.is_match().then_some(index))
            .or_else(|| {
                self.search_rows
                    .get(start)
                    .is_some_and(WorkspaceSearchRow::is_match)
                    .then_some(start)
            })
    }

    fn find_search_match_row(&self, file_index: usize, line_number: usize) -> Option<usize> {
        self.search_rows
            .iter()
            .enumerate()
            .find_map(|(index, row)| {
                row.target()
                    .is_some_and(|(candidate_file, candidate_line)| {
                        candidate_file == file_index && candidate_line == line_number
                    })
                    .then_some(index)
            })
    }

    fn sync_editor_to_selection(&mut self, auto_open: bool) -> Result<()> {
        if self.entries().is_empty() {
            self.editor = None;
            self.focus_sidebar();
            return Ok(());
        }

        let entry = &self.entries()[self.selected];
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
        if self.entries().is_empty() {
            self.preview_title = "Workspace".to_string();
            self.preview =
                WorkspaceRenderer::plain_preview_lines("No files found for this workspace.");
            return Ok(());
        }

        let path = self.entries()[self.selected].path.clone();
        self.preview_title = path.display().to_string();
        self.preview = WorkspaceRenderer::read_text_preview(&path)?;
        Ok(())
    }
}
