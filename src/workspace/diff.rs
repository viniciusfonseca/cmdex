use super::{render::WorkspaceRenderer, *};

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
struct DiffSnapshot {
    source: String,
    default_kind: Option<DiffPreviewKind>,
    hunks: Vec<DiffHunk>,
    message: Option<String>,
}

pub(crate) struct GitDiffSupport;

impl DiffBrowserState {
    pub fn refresh(&mut self, root: &Path) {
        match GitDiffSupport::build_diff_entries(root).and_then(|sections| {
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
                self.preview = WorkspaceRenderer::plain_preview_lines("Unable to load git diff.");
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

        let output = GitDiffSupport::run_git_command(root, &["commit", "-m", message])?;
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
        let output = GitDiffSupport::run_git_command(root, &["add", "--", &relative])?;
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
        let output =
            GitDiffSupport::run_git_command(root, &["restore", "--staged", "--", &relative])?;
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
        let relative = GitDiffSupport::relative_entry_path(root, &entry.path)?;
        let output = if entry.status.contains('?') {
            GitDiffSupport::run_git_command(root, &["clean", "-f", "--", &relative])?
        } else {
            GitDiffSupport::run_git_command(root, &["restore", "--", &relative])?
        };

        self.status = Some(output);
        self.error = None;
        self.refresh(root);
        Ok(())
    }

    pub fn begin_remote_action(&mut self, action: GitRemoteAction) -> Result<()> {
        if let Some(active) = self.remote_action {
            return Err(anyhow::anyhow!(format!(
                "Wait for git {} to finish first.",
                active.label().to_lowercase()
            )));
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
        root: &Path,
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
            self.refresh(root);
        } else {
            self.error = Some(message);
        }
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
        GitDiffSupport::relative_entry_path(root, &entry.path)
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
                DiffSection::Changes => {
                    WorkspaceRenderer::plain_preview_lines("No unstaged changes found.")
                }
                DiffSection::Staged => {
                    WorkspaceRenderer::plain_preview_lines("No staged changes found.")
                }
            };
            return Ok(());
        }

        let entry = self.visible_entries()[self.selected_index()].clone();
        self.preview_title = match self.active_section {
            DiffSection::Changes => format!("Changes · {}", entry.path.display()),
            DiffSection::Staged => format!("Staged · {}", entry.path.display()),
        };
        self.preview = GitDiffSupport::read_diff_preview(root, &entry, self.active_section)?;
        Ok(())
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
    pub(crate) fn run_remote_action(root: &Path, action: GitRemoteAction) -> Result<String> {
        match action {
            GitRemoteAction::Push => Self::run_git_command(root, &["push"]),
            GitRemoteAction::Pull => Self::run_git_command(root, &["pull", "--ff-only"]),
        }
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
        Ok(Self::parse_status_sections(root, &stdout))
    }

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

    fn read_diff_preview(
        root: &Path,
        entry: &DiffEntry,
        section: DiffSection,
    ) -> Result<Vec<Line<'static>>> {
        let snapshot = Self::read_diff_snapshot(root, entry, section)?;
        Ok(Self::render_diff_snapshot_preview(
            entry.path.as_path(),
            &snapshot,
        ))
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

    fn git_output_bytes(root: &Path, args: &[&str]) -> Result<Vec<u8>> {
        let output = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .output()
            .with_context(|| format!("failed to run git {:?} in {}", args, root.display()))?;

        if !output.status.success() {
            return Ok(Vec::new());
        }

        Ok(output.stdout)
    }

    fn relative_entry_path(root: &Path, path: &Path) -> Result<String> {
        path.strip_prefix(root)
            .unwrap_or(path)
            .to_str()
            .map(|value| value.to_string())
            .ok_or_else(|| {
                anyhow::anyhow!("Path contains unsupported characters: {}", path.display())
            })
    }

    fn run_git_command(root: &Path, args: &[&str]) -> Result<String> {
        let output = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .env("GIT_TERMINAL_PROMPT", "0")
            .output()
            .with_context(|| format!("failed to run git {:?} in {}", args, root.display()))?;

        let stdout =
            WorkspaceRenderer::normalize_newlines(&String::from_utf8_lossy(&output.stdout));
        let stderr =
            WorkspaceRenderer::normalize_newlines(&String::from_utf8_lossy(&output.stderr));
        let summary = Self::summarize_git_command_output(&stdout, &stderr);

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

    fn read_diff_snapshot(
        root: &Path,
        entry: &DiffEntry,
        section: DiffSection,
    ) -> Result<DiffSnapshot> {
        if entry.status.contains('?') {
            let bytes = fs::read(&entry.path)
                .with_context(|| format!("failed to read {}", entry.path.display()))?;
            return Self::snapshot_from_bytes(bytes, Some(DiffPreviewKind::Added));
        }

        let relative = Self::relative_entry_path(root, &entry.path)?;
        let diff = match section {
            DiffSection::Changes => Self::git_output(
                root,
                &[
                    "diff",
                    "--no-ext-diff",
                    "--no-color",
                    "--unified=0",
                    "--",
                    &relative,
                ],
            )?,
            DiffSection::Staged => Self::git_output(
                root,
                &[
                    "diff",
                    "--no-ext-diff",
                    "--no-color",
                    "--cached",
                    "--unified=0",
                    "--",
                    &relative,
                ],
            )?,
        };

        let force_removed = entry.status.contains('D');
        let default_kind = if force_removed {
            Some(DiffPreviewKind::Removed)
        } else {
            None
        };

        let bytes = match section {
            DiffSection::Changes => {
                if force_removed {
                    Self::git_output_bytes(root, &["show", &format!(":{relative}")])?
                } else {
                    fs::read(&entry.path)
                        .with_context(|| format!("failed to read {}", entry.path.display()))?
                }
            }
            DiffSection::Staged => {
                if force_removed {
                    Self::git_output_bytes(root, &["show", &format!("HEAD:{relative}")])?
                } else {
                    Self::git_output_bytes(root, &["show", &format!(":{relative}")])?
                }
            }
        };

        let mut snapshot = Self::snapshot_from_bytes(bytes, default_kind)?;
        snapshot.hunks = Self::parse_unified_diff_hunks(&diff);

        if snapshot.default_kind.is_none() && snapshot.hunks.is_empty() {
            snapshot.message = Some("No diff available for this file.".to_string());
        }

        Ok(snapshot)
    }

    fn snapshot_from_bytes(
        bytes: Vec<u8>,
        default_kind: Option<DiffPreviewKind>,
    ) -> Result<DiffSnapshot> {
        if bytes.contains(&0) {
            return Ok(DiffSnapshot {
                source: String::new(),
                default_kind,
                hunks: Vec::new(),
                message: Some("Binary file preview is not available.".to_string()),
            });
        }

        let truncated = bytes.len() > PREVIEW_LIMIT;
        let source = String::from_utf8_lossy(&bytes[..bytes.len().min(PREVIEW_LIMIT)]).into_owned();
        let mut snapshot = DiffSnapshot {
            source: WorkspaceRenderer::normalize_newlines(&source),
            default_kind,
            hunks: Vec::new(),
            message: None,
        };
        if truncated {
            snapshot.message = Some("[truncated]".to_string());
        }
        Ok(snapshot)
    }

    fn render_diff_snapshot_preview(path: &Path, snapshot: &DiffSnapshot) -> Vec<Line<'static>> {
        if snapshot.source.is_empty() && snapshot.message.is_some() {
            return WorkspaceRenderer::plain_preview_lines(
                snapshot.message.as_deref().unwrap_or_default(),
            );
        }

        let mut lines = Self::render_diff_preview_rows(Self::build_diff_preview_rows(
            path,
            &snapshot.source,
            snapshot.default_kind,
            &snapshot.hunks,
        ));

        if let Some(message) = &snapshot.message {
            lines.push(Line::default());
            lines.push(Line::from(Span::styled(
                message.clone(),
                Style::default()
                    .fg(ThemeRegistry::app().muted)
                    .add_modifier(Modifier::ITALIC),
            )));
        }

        lines
    }

    #[cfg(test)]
    pub(super) fn render_modified_diff_preview(
        path: &Path,
        source: &str,
        hunks: &[DiffHunk],
    ) -> Vec<Line<'static>> {
        Self::render_diff_preview_rows(Self::build_diff_preview_rows(path, source, None, hunks))
    }

    fn build_diff_preview_rows(
        path: &Path,
        source: &str,
        default_kind: Option<DiffPreviewKind>,
        hunks: &[DiffHunk],
    ) -> Vec<DiffPreviewRow> {
        let mut highlighted = WorkspaceRenderer::highlighted_preview_lines(
            source,
            WorkspaceRenderer::syntax_for_path(path, source),
        );
        WorkspaceRenderer::maybe_trim_trailing_empty_line(&mut highlighted);

        let mut rows = highlighted
            .into_iter()
            .enumerate()
            .map(|(index, line)| DiffPreviewRow {
                line_number: Some(index + 1),
                kind: default_kind.unwrap_or_else(|| Self::diff_kind_for_line(index + 1, hunks)),
                line,
            })
            .collect::<Vec<_>>();

        if default_kind.is_none() {
            Self::insert_removed_diff_rows(&mut rows, hunks);
        }

        rows
    }

    fn diff_kind_for_line(line_number: usize, hunks: &[DiffHunk]) -> DiffPreviewKind {
        for hunk in hunks {
            if hunk.new_count == 0 {
                continue;
            }

            let start = hunk.new_start.max(1);
            let end = start + hunk.new_count.saturating_sub(1);
            if (start..=end).contains(&line_number) {
                return if hunk.old_count == 0 {
                    DiffPreviewKind::Added
                } else {
                    DiffPreviewKind::Modified
                };
            }
        }

        DiffPreviewKind::Context
    }

    fn insert_removed_diff_rows(rows: &mut Vec<DiffPreviewRow>, hunks: &[DiffHunk]) {
        let mut inserted_rows = 0usize;

        for hunk in hunks {
            if hunk.removed_lines.is_empty() {
                continue;
            }

            let insert_at = hunk
                .new_start
                .saturating_sub(1)
                .saturating_add(inserted_rows)
                .min(rows.len());

            let removed_rows = hunk
                .removed_lines
                .iter()
                .enumerate()
                .map(|(offset, line)| DiffPreviewRow {
                    line_number: Some(hunk.old_start + offset),
                    kind: DiffPreviewKind::Removed,
                    line: Line::from(line.clone()),
                })
                .collect::<Vec<_>>();

            inserted_rows += removed_rows.len();
            rows.splice(insert_at..insert_at, removed_rows);
        }
    }

    fn render_diff_preview_rows(rows: Vec<DiffPreviewRow>) -> Vec<Line<'static>> {
        let gutter_width = rows
            .iter()
            .filter_map(|row| row.line_number)
            .max()
            .unwrap_or(1)
            .to_string()
            .len();

        rows.into_iter()
            .map(|row| {
                let marker = match row.kind {
                    DiffPreviewKind::Context => ' ',
                    DiffPreviewKind::Added => '+',
                    DiffPreviewKind::Modified => '~',
                    DiffPreviewKind::Removed => '-',
                };
                let number = row
                    .line_number
                    .map(|value| format!("{value:>width$}", width = gutter_width))
                    .unwrap_or_else(|| " ".repeat(gutter_width));

                let mut line = row.line;
                let mut spans = Vec::with_capacity(line.spans.len() + 1);
                spans.push(Span::styled(
                    format!("{number} {marker} | "),
                    Style::default().fg(Self::diff_gutter_color(row.kind)),
                ));
                spans.append(&mut line.spans);
                line.spans = spans;
                Self::apply_diff_preview_kind(&mut line, row.kind);
                line
            })
            .collect()
    }

    fn diff_gutter_color(kind: DiffPreviewKind) -> Color {
        match kind {
            DiffPreviewKind::Context => ThemeRegistry::app().line_number,
            DiffPreviewKind::Added => ThemeRegistry::app().success,
            DiffPreviewKind::Modified => ThemeRegistry::app().warning,
            DiffPreviewKind::Removed => ThemeRegistry::app().error,
        }
    }

    fn apply_diff_preview_kind(line: &mut Line<'static>, kind: DiffPreviewKind) {
        let Some(background) = Self::diff_preview_background(kind) else {
            return;
        };

        line.style = line.style.bg(background);
        for span in &mut line.spans {
            span.style = span.style.bg(background);
        }
    }

    fn diff_preview_background(kind: DiffPreviewKind) -> Option<Color> {
        match kind {
            DiffPreviewKind::Context => None,
            DiffPreviewKind::Added => Some(Self::blend_tui_colors(
                ThemeRegistry::app().panel_bg,
                ThemeRegistry::app().success,
                0.22,
            )),
            DiffPreviewKind::Modified => Some(Self::blend_tui_colors(
                ThemeRegistry::app().panel_bg,
                ThemeRegistry::app().warning,
                0.18,
            )),
            DiffPreviewKind::Removed => Some(Self::blend_tui_colors(
                ThemeRegistry::app().panel_bg,
                ThemeRegistry::app().error,
                0.22,
            )),
        }
    }

    fn blend_tui_colors(base: Color, overlay: Color, alpha: f32) -> Color {
        let (Color::Rgb(base_r, base_g, base_b), Color::Rgb(overlay_r, overlay_g, overlay_b)) =
            (base, overlay)
        else {
            return overlay;
        };

        let blend = |background: u8, foreground: u8| -> u8 {
            ((background as f32 * (1.0 - alpha)) + (foreground as f32 * alpha)).round() as u8
        };

        Color::Rgb(
            blend(base_r, overlay_r),
            blend(base_g, overlay_g),
            blend(base_b, overlay_b),
        )
    }

    pub(super) fn parse_unified_diff_hunks(diff: &str) -> Vec<DiffHunk> {
        let mut hunks = Vec::new();
        let mut current: Option<DiffHunk> = None;

        for line in WorkspaceRenderer::normalize_newlines(diff).lines() {
            if line.starts_with("@@") {
                if let Some(hunk) = current.take() {
                    hunks.push(hunk);
                }
                current = Self::parse_diff_hunk_header(line).map(
                    |(old_start, old_count, new_start, new_count)| DiffHunk {
                        old_start,
                        old_count,
                        new_start,
                        new_count,
                        removed_lines: Vec::new(),
                    },
                );
                continue;
            }

            if let Some(hunk) = current.as_mut() {
                if let Some(removed) = line.strip_prefix('-') {
                    hunk.removed_lines.push(removed.to_string());
                }
            }
        }

        if let Some(hunk) = current {
            hunks.push(hunk);
        }

        hunks
    }

    fn parse_diff_hunk_header(header: &str) -> Option<(usize, usize, usize, usize)> {
        let mut parts = header.split_whitespace();
        let _ = parts.next()?;
        let old_range = parts.next()?;
        let new_range = parts.next()?;
        let (old_start, old_count) = Self::parse_diff_range(old_range, '-')?;
        let (new_start, new_count) = Self::parse_diff_range(new_range, '+')?;
        Some((old_start, old_count, new_start, new_count))
    }

    fn parse_diff_range(value: &str, prefix: char) -> Option<(usize, usize)> {
        let value = value.strip_prefix(prefix)?;
        let (start, count) = match value.split_once(',') {
            Some((start, count)) => (start, count.parse().ok()?),
            None => (value, 1),
        };
        Some((start.parse().ok()?, count))
    }
}
