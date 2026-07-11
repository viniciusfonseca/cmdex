use super::{git_repository::GitRepository, render::WorkspaceRenderer, *};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DiffPreviewKind {
    Context,
    Added,
    Modified,
    Removed,
}

#[derive(Debug, Clone)]
struct DiffPreviewRow {
    line_number: Option<usize>,
    kind: DiffPreviewKind,
    line: Line<'static>,
}

#[derive(Debug, Clone)]
pub(super) struct DiffHunk {
    pub(super) old_start: usize,
    pub(super) old_count: usize,
    pub(super) new_start: usize,
    pub(super) new_count: usize,
    pub(super) removed_lines: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub(super) struct DiffSections {
    pub(super) changes: Vec<DiffEntry>,
    pub(super) staged: Vec<DiffEntry>,
}

#[derive(Debug, Clone)]
pub(crate) struct GitDiffLoadResult {
    pub(crate) changes: Vec<DiffEntry>,
    pub(crate) staged: Vec<DiffEntry>,
    pub(crate) active_section: DiffSection,
    pub(crate) selected_path: Option<PathBuf>,
    pub(crate) preview_title: String,
    pub(crate) preview: Vec<Line<'static>>,
}

#[derive(Debug, Clone)]
struct DiffSnapshot {
    source: String,
    default_kind: Option<DiffPreviewKind>,
    hunks: Vec<DiffHunk>,
    message: Option<String>,
}

pub(crate) struct GitDiffSupport;

impl DiffBrowserState {
    pub fn move_up(&mut self, _root: &Path) {
        if self.visible_entries().is_empty() {
            return;
        }
        *self.selected_index_mut() = self.selected_index().saturating_sub(1);
        self.content_scroll = 0;
    }

    pub fn move_down(&mut self, _root: &Path) {
        if self.visible_entries().is_empty() {
            return;
        }
        let max_index = self.visible_entries().len().saturating_sub(1);
        *self.selected_index_mut() = (self.selected_index() + 1).min(max_index);
        self.content_scroll = 0;
    }

    pub fn select(&mut self, _root: &Path, index: usize) {
        if self.visible_entries().is_empty() {
            return;
        }
        *self.selected_index_mut() = index.min(self.visible_entries().len().saturating_sub(1));
        self.content_scroll = 0;
    }

    pub fn set_active_section(&mut self, _root: &Path, section: DiffSection) {
        if self.active_section == section {
            return;
        }

        self.active_section = section;
        self.content_scroll = 0;
        self.error = None;
    }

    pub fn scroll_up(&mut self, lines: u16) {
        self.content_scroll = self.content_scroll.saturating_sub(lines);
    }

    pub fn scroll_down(&mut self, lines: u16) {
        self.content_scroll = self.content_scroll.saturating_add(lines);
    }

    pub fn prepare_commit(&self) -> Result<GitMutation> {
        let message = self.commit_message.trim();
        if message.is_empty() {
            return Err(anyhow::anyhow!("Commit message cannot be empty."));
        }
        Ok(GitMutation::Commit(message.to_string()))
    }

    pub fn prepare_stage(&self, root: &Path) -> Result<GitMutation> {
        if self.active_section != DiffSection::Changes {
            return Err(anyhow::anyhow!("Switch to Changes before staging a file."));
        }
        Ok(GitMutation::Stage(self.selected_relative_path(root)?))
    }

    pub fn prepare_unstage(&self, root: &Path) -> Result<GitMutation> {
        if self.active_section != DiffSection::Staged {
            return Err(anyhow::anyhow!("Switch to Staged before unstaging a file."));
        }
        Ok(GitMutation::Unstage(self.selected_relative_path(root)?))
    }

    pub fn prepare_discard(&self, root: &Path) -> Result<GitMutation> {
        if self.active_section != DiffSection::Changes {
            return Err(anyhow::anyhow!("Discard is only available in Changes."));
        }
        let entry = self.selected_entry()?;
        Ok(GitMutation::Discard {
            path: GitRepository::new(root).relative_path(&entry.path)?,
            untracked: entry.status.contains('?'),
        })
    }

    pub fn begin_mutation(&mut self) -> Result<()> {
        if self.remote_action.is_some() || self.mutation_running {
            return Err(anyhow::anyhow!(
                "Wait for the current git operation to finish."
            ));
        }
        self.mutation_running = true;
        self.error = None;
        self.status = Some("Git operation in progress...".to_string());
        Ok(())
    }

    pub fn complete_mutation(
        &mut self,
        _root: &Path,
        mutation: &GitMutation,
        success: bool,
        message: String,
    ) {
        self.mutation_running = false;
        if success {
            if matches!(mutation, GitMutation::Commit(_)) {
                self.commit_message.clear();
            }
            self.status = Some(message);
            self.error = None;
        } else {
            self.error = Some(message);
        }
    }

    pub fn begin_remote_action(&mut self, action: GitRemoteAction) -> Result<()> {
        if let Some(active) = self.remote_action {
            return Err(anyhow::anyhow!(format!(
                "Wait for git {} to finish first.",
                active.label().to_lowercase()
            )));
        }
        if self.mutation_running {
            return Err(anyhow::anyhow!(
                "Wait for the current git operation to finish."
            ));
        }

        self.remote_action = Some(action);
        self.error = None;
        self.status = Some(format!(
            "Git {} in progress...",
            action.label().to_lowercase()
        ));
        Ok(())
    }

    pub fn complete_remote_action(
        &mut self,
        _root: &Path,
        action: GitRemoteAction,
        success: bool,
        message: String,
    ) {
        if self.remote_action == Some(action) {
            self.remote_action = None;
        }

        if success {
            self.status = Some(message);
            self.error = None;
        } else {
            self.error = Some(message);
        }
    }

    pub(crate) fn begin_refresh(&mut self) -> u64 {
        self.refresh_generation = self.refresh_generation.wrapping_add(1);
        self.refresh_in_flight = true;
        self.refresh_generation
    }

    pub(crate) fn apply_load_result(
        &mut self,
        generation: u64,
        result: Option<GitDiffLoadResult>,
        error: Option<String>,
    ) -> bool {
        if generation != self.refresh_generation {
            return false;
        }
        self.refresh_in_flight = false;
        match result {
            Some(result) => {
                self.changes = result.changes;
                self.staged = result.staged;
                self.active_section = result.active_section;
                let selected_index = match self.active_section {
                    DiffSection::Changes => self
                        .changes
                        .iter()
                        .position(|entry| Some(&entry.path) == result.selected_path.as_ref()),
                    DiffSection::Staged => self
                        .staged
                        .iter()
                        .position(|entry| Some(&entry.path) == result.selected_path.as_ref()),
                };
                *self.selected_index_mut() = selected_index.unwrap_or(0);
                self.preview_title = result.preview_title;
                self.preview = result.preview;
                self.content_scroll = 0;
                self.error = None;
            }
            None => {
                self.preview_title = "Git Diff".to_string();
                self.preview = WorkspaceRenderer::plain_preview_lines("Unable to load git diff.");
                self.error = error;
            }
        }
        true
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
        GitRepository::new(root).relative_path(&entry.path)
    }
}

impl GitRemoteAction {
    pub fn label(self) -> &'static str {
        match self {
            GitRemoteAction::Push => "Push",
            GitRemoteAction::Pull => "Pull",
        }
    }
}

impl GitDiffSupport {
    pub(super) fn parse_status_sections(root: &Path, output: &str) -> DiffSections {
        let mut sections = DiffSections::default();

        for line in output.lines() {
            let Some((index_status, worktree_status, path)) = Self::parse_status_line(line) else {
                continue;
            };
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

        sections
    }

    fn parse_status_line(line: &str) -> Option<(char, char, &str)> {
        if line.len() < 3 || !line.is_char_boundary(2) || !line.is_char_boundary(3) {
            return None;
        }

        let index_status = line.as_bytes()[0] as char;
        let worktree_status = line.as_bytes()[1] as char;
        let raw_path = line[3..].trim();
        let path = raw_path
            .rsplit_once(" -> ")
            .map(|(_, new_path)| new_path)
            .unwrap_or(raw_path);

        Some((index_status, worktree_status, path))
    }

    #[cfg(test)]
    pub(super) fn parse_unified_diff_hunks(diff: &str) -> Vec<DiffHunk> {
        Self::parse_unified_diff_hunks_impl(diff)
    }
}

#[path = "diff_preview.rs"]
mod diff_preview;
