use super::{render::*, *};

pub(super) struct WorkspaceBrowserSupport;

impl FileBrowserState {
    pub fn refresh(&mut self, root: &Path) {
        match WorkspaceBrowserSupport::build_file_entries(root)
            .and_then(|entries| self.apply_entries(entries))
        {
            Ok(()) => {}
            Err(error) => self.reset_with_error(error),
        }
    }

    pub fn refresh_if_changed(&mut self, root: &Path) -> bool {
        match WorkspaceBrowserSupport::build_file_entries(root).and_then(|entries| {
            if entries == self.entries {
                Ok(false)
            } else {
                self.apply_entries(entries)?;
                Ok(true)
            }
        }) {
            Ok(changed) => changed,
            Err(error) => {
                self.reset_with_error(error);
                true
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
        self.focus_editor();
        self.error = None;
        Ok(())
    }

    pub fn close_editor(&mut self) -> Result<()> {
        self.editor = None;
        self.focus_sidebar();
        self.content_scroll = 0;
        self.update_preview()
    }

    pub fn sidebar_labels(&self) -> Vec<String> {
        self.tree_rows
            .iter()
            .map(|row| row.label.clone())
            .collect::<Vec<_>>()
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
        self.refresh_search_results();
    }

    pub fn pop_search_char(&mut self) {
        self.search_query.pop();
        self.refresh_search_results();
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

    fn apply_entries(&mut self, entries: Vec<FileEntry>) -> Result<()> {
        let previous_selected = self.selected;
        let previous_selected_path = self
            .entries
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

        self.entries = entries;
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
        self.refresh_search_results();
        self.content_scroll = if previous_selected_path.as_ref().is_some_and(|path| {
            self.entries
                .get(self.selected)
                .is_some_and(|entry| &entry.path == path)
        }) {
            previous_content_scroll
        } else {
            0
        };
        self.error = None;
        self.update_preview()?;
        if keep_dirty_editor {
            Ok(())
        } else {
            self.sync_editor_to_selection(true)
        }
    }

    fn resolve_selected_index(
        &self,
        selected_path: Option<&PathBuf>,
        fallback_index: usize,
    ) -> usize {
        if self.entries.is_empty() {
            return 0;
        }

        selected_path
            .and_then(|path| self.entries.iter().position(|entry| entry.path == *path))
            .unwrap_or_else(|| fallback_index.min(self.entries.len().saturating_sub(1)))
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
        let (tree_rows, tree_file_rows) =
            WorkspaceBrowserSupport::build_file_tree_rows(&self.entries, &self.collapsed_dirs);
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
        let directory_paths = WorkspaceBrowserSupport::collect_directory_paths(&self.entries);

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

    fn refresh_search_results(&mut self) {
        let previous_target = self.selected_search_target();
        self.search_rows.clear();
        self.search_selected_row = 0;
        self.search_match_count = 0;

        let query = self.search_query.trim();
        if query.is_empty() {
            return;
        }

        for (file_index, entry) in self.entries.iter().enumerate() {
            let Ok(bytes) = fs::read(&entry.path) else {
                continue;
            };
            if bytes.contains(&0) {
                continue;
            }

            let source = WorkspaceRenderer::normalize_newlines(&String::from_utf8_lossy(&bytes));
            let mut matches = Vec::new();
            for (line_index, line) in source.lines().enumerate() {
                if line.contains(query) {
                    matches.push(WorkspaceSearchRow::Match {
                        label: format!(
                            "  {}: {}",
                            line_index + 1,
                            WorkspaceBrowserSupport::search_result_excerpt(line)
                        ),
                        file_index,
                        line_number: line_index + 1,
                    });
                }
            }

            if !matches.is_empty() {
                self.search_match_count += matches.len();
                self.search_rows.push(WorkspaceSearchRow::FileHeader {
                    label: entry.relative_path.display().to_string(),
                });
                self.search_rows.extend(matches);
            }
        }

        if let Some((file_index, line_number)) = previous_target {
            if let Some(row) = self.find_search_match_row(file_index, line_number) {
                self.search_selected_row = row;
                return;
            }
        }

        if let Some(row) = self.first_search_match_row() {
            self.search_selected_row = row;
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
        if self.entries.is_empty() {
            self.editor = None;
            self.focus_sidebar();
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
            self.preview =
                WorkspaceRenderer::plain_preview_lines("No files found for this workspace.");
            return Ok(());
        }

        let entry = &self.entries[self.selected];
        self.preview_title = entry.path.display().to_string();
        self.preview = WorkspaceRenderer::read_text_preview(&entry.path)?;
        Ok(())
    }

    fn reset_with_error(&mut self, error: anyhow::Error) {
        self.entries.clear();
        self.tree_rows.clear();
        self.selected = 0;
        self.tree_cursor = 0;
        self.focus = WorkspaceFocus::Sidebar;
        self.sidebar_tab = WorkspaceSidebarTab::Files;
        self.search_query.clear();
        self.collapsed_dirs.clear();
        self.known_dirs.clear();
        self.search_rows.clear();
        self.search_selected_row = 0;
        self.search_match_count = 0;
        self.preview_title = "Workspace".to_string();
        self.preview = WorkspaceRenderer::plain_preview_lines("Unable to load workspace.");
        self.error = Some(error.to_string());
    }
}

impl FileTreeRow {
    pub(super) fn styled_label(&self) -> Line<'static> {
        if self.branch_prefix_len == 0 || self.branch_prefix_len >= self.label.len() {
            return Line::from(self.label.clone());
        }

        let (branch, label) = self.label.split_at(self.branch_prefix_len);
        Line::from(vec![
            Span::styled(
                branch.to_string(),
                Style::default().fg(ThemeRegistry::app().line_number),
            ),
            Span::raw(label.to_string()),
        ])
    }

    pub(super) fn file_index(&self) -> Option<usize> {
        match self.kind {
            FileTreeRowKind::Directory { .. } => None,
            FileTreeRowKind::File { file_index } => Some(file_index),
        }
    }

    pub(super) fn directory_state(&self) -> Option<(&PathBuf, bool)> {
        match &self.kind {
            FileTreeRowKind::Directory {
                relative_path,
                expanded,
            } => Some((relative_path, *expanded)),
            FileTreeRowKind::File { .. } => None,
        }
    }

    pub(super) fn directory_path(&self) -> Option<&PathBuf> {
        match &self.kind {
            FileTreeRowKind::Directory { relative_path, .. } => Some(relative_path),
            FileTreeRowKind::File { .. } => None,
        }
    }
}

impl WorkspaceSearchRow {
    fn label(&self) -> &str {
        match self {
            Self::FileHeader { label } | Self::Match { label, .. } => label,
        }
    }

    fn is_match(&self) -> bool {
        matches!(self, Self::Match { .. })
    }

    fn target(&self) -> Option<(usize, usize)> {
        match self {
            Self::FileHeader { .. } => None,
            Self::Match {
                file_index,
                line_number,
                ..
            } => Some((*file_index, *line_number)),
        }
    }
}

#[derive(Debug, Default)]
struct FileTreeNode {
    directories: BTreeMap<String, FileTreeNode>,
    files: Vec<(String, usize)>,
}

impl WorkspaceBrowserSupport {
    pub(super) fn build_file_entries(root: &Path) -> Result<Vec<FileEntry>> {
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
            if Self::contains_git_component(path) {
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

    fn collect_directory_paths(entries: &[FileEntry]) -> BTreeSet<PathBuf> {
        let mut directories = BTreeSet::new();

        for entry in entries {
            let mut ancestor = entry.relative_path.parent();
            while let Some(path) = ancestor {
                if path.as_os_str().is_empty() {
                    break;
                }
                directories.insert(path.to_path_buf());
                ancestor = path.parent();
            }
        }

        directories
    }

    pub(super) fn build_file_tree_rows(
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
        Self::append_tree_rows(
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
            let mut label = Self::tree_row_prefix(ancestor_has_more);
            label.push_str(if is_last { " └─ " } else { " ├─ " });
            let mut branch_prefix_len = label.len();

            match child {
                TreeChild::Directory(name) => {
                    let relative_path = if base_path.as_os_str().is_empty() {
                        PathBuf::from(name)
                    } else {
                        base_path.join(name)
                    };
                    let expanded = !collapsed_dirs.contains(&relative_path);
                    label.push_str(if expanded { "▾ " } else { "▸ " });
                    branch_prefix_len = label.len();
                    label.push_str(name);
                    rows.push(FileTreeRow {
                        label,
                        branch_prefix_len,
                        kind: FileTreeRowKind::Directory {
                            relative_path: relative_path.clone(),
                            expanded,
                        },
                    });

                    if expanded {
                        let mut next_prefix = ancestor_has_more.to_vec();
                        next_prefix.push(!is_last);
                        Self::append_tree_rows(
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
                        branch_prefix_len,
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
            .map(|has_more| if *has_more { " │ " } else { "   " })
            .collect::<Vec<_>>()
            .join("")
    }

    pub(super) fn contains_git_component(path: &Path) -> bool {
        path.components()
            .any(|component| component.as_os_str() == ".git")
    }

    fn search_result_excerpt(line: &str) -> String {
        let collapsed = line.replace('\t', " ").trim().to_string();
        let chars = collapsed.chars().collect::<Vec<_>>();
        if chars.len() <= 120 {
            collapsed
        } else {
            chars[..117].iter().collect::<String>() + "..."
        }
    }
}

enum TreeChild<'a> {
    Directory(&'a str),
    File(&'a str, usize),
}
