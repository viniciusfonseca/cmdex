use super::*;

#[derive(Debug, Clone, Default)]
pub(crate) struct WorkspaceIndex {
    entries: Vec<FileEntry>,
}

impl WorkspaceIndex {
    pub(crate) fn scan(root: &Path) -> Result<Vec<FileEntry>> {
        WorkspaceBrowserSupport::build_file_entries(root)
    }

    pub(crate) fn entries(&self) -> &[FileEntry] {
        &self.entries
    }

    pub(crate) fn replace(&mut self, entries: Vec<FileEntry>) {
        if self.entries != entries {
            self.entries = entries;
        }
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

    pub(in crate::workspace) fn file_index(&self) -> Option<usize> {
        match self.kind {
            FileTreeRowKind::Directory { .. } => None,
            FileTreeRowKind::File { file_index } => Some(file_index),
        }
    }

    pub(in crate::workspace) fn directory_state(&self) -> Option<(&PathBuf, bool)> {
        match &self.kind {
            FileTreeRowKind::Directory {
                relative_path,
                expanded,
            } => Some((relative_path, *expanded)),
            FileTreeRowKind::File { .. } => None,
        }
    }

    pub(in crate::workspace) fn directory_path(&self) -> Option<&PathBuf> {
        match &self.kind {
            FileTreeRowKind::Directory { relative_path, .. } => Some(relative_path),
            FileTreeRowKind::File { .. } => None,
        }
    }
}

impl WorkspaceSearchRow {
    pub(in crate::workspace) fn label(&self) -> &str {
        match self {
            Self::FileHeader { label } | Self::Match { label, .. } => label,
        }
    }

    pub(in crate::workspace) fn is_match(&self) -> bool {
        matches!(self, Self::Match { .. })
    }

    pub(in crate::workspace) fn target(&self) -> Option<(usize, usize)> {
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
    pub(in crate::workspace) fn build_search_snapshot(
        entries: &[FileEntry],
        query: &str,
    ) -> Result<WorkspaceSearchSnapshot> {
        let query = query.trim();
        if query.is_empty() {
            return Ok(WorkspaceSearchSnapshot::default());
        }

        let mut rows = Vec::new();
        let mut match_count = 0;
        for (file_index, entry) in entries.iter().enumerate() {
            let Ok(bytes) = fs::read(&entry.path) else {
                continue;
            };
            if bytes.contains(&0) {
                continue;
            }

            let source = WorkspaceRenderer::normalize_newlines(&String::from_utf8_lossy(&bytes));
            let matches = source
                .lines()
                .enumerate()
                .filter(|(_, line)| line.contains(query))
                .map(|(line_index, line)| WorkspaceSearchRow::Match {
                    label: format!(
                        "  {}: {}",
                        line_index + 1,
                        Self::search_result_excerpt(line)
                    ),
                    file_index,
                    line_number: line_index + 1,
                })
                .collect::<Vec<_>>();

            if !matches.is_empty() {
                match_count += matches.len();
                rows.push(WorkspaceSearchRow::FileHeader {
                    label: entry.relative_path.display().to_string(),
                });
                rows.extend(matches);
            }
        }

        Ok(WorkspaceSearchSnapshot { rows, match_count })
    }

    pub(crate) fn build_file_entries(root: &Path) -> Result<Vec<FileEntry>> {
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

    pub(in crate::workspace) fn collect_directory_paths(
        entries: &[FileEntry],
    ) -> BTreeSet<PathBuf> {
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

    pub(in crate::workspace) fn build_file_tree_rows(
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

    pub(in crate::workspace) fn contains_git_component(path: &Path) -> bool {
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
