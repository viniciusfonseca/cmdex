use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result};
use ignore::WalkBuilder;

const PREVIEW_LIMIT: usize = 200_000;

#[derive(Debug, Clone, Default)]
pub struct FileBrowserState {
    pub entries: Vec<FileEntry>,
    pub selected: usize,
    pub preview_title: String,
    pub preview: String,
    pub content_scroll: u16,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct FileEntry {
    pub label: String,
    pub path: PathBuf,
    pub is_dir: bool,
}

#[derive(Debug, Clone, Default)]
pub struct DiffBrowserState {
    pub entries: Vec<DiffEntry>,
    pub selected: usize,
    pub preview_title: String,
    pub preview: String,
    pub content_scroll: u16,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DiffEntry {
    pub label: String,
    pub path: PathBuf,
    pub status: String,
}

impl FileBrowserState {
    pub fn refresh(&mut self, root: &Path) {
        match build_file_entries(root).and_then(|entries| {
            self.entries = entries;
            self.selected = self.selected.min(self.entries.len().saturating_sub(1));
            self.content_scroll = 0;
            self.error = None;
            self.update_preview()
        }) {
            Ok(()) => {}
            Err(error) => {
                self.entries.clear();
                self.selected = 0;
                self.preview_title = "Workspace".to_string();
                self.preview = "Unable to load workspace.".to_string();
                self.error = Some(error.to_string());
            }
        }
    }

    pub fn move_up(&mut self) {
        if self.entries.is_empty() {
            return;
        }
        self.selected = self.selected.saturating_sub(1);
        self.content_scroll = 0;
        let _ = self.update_preview();
    }

    pub fn move_down(&mut self) {
        if self.entries.is_empty() {
            return;
        }
        self.selected = (self.selected + 1).min(self.entries.len().saturating_sub(1));
        self.content_scroll = 0;
        let _ = self.update_preview();
    }

    pub fn select(&mut self, index: usize) {
        if self.entries.is_empty() {
            return;
        }
        self.selected = index.min(self.entries.len().saturating_sub(1));
        self.content_scroll = 0;
        let _ = self.update_preview();
    }

    pub fn scroll_up(&mut self, lines: u16) {
        self.content_scroll = self.content_scroll.saturating_sub(lines);
    }

    pub fn scroll_down(&mut self, lines: u16) {
        self.content_scroll = self.content_scroll.saturating_add(lines);
    }

    fn update_preview(&mut self) -> Result<()> {
        if self.entries.is_empty() {
            self.preview_title = "Workspace".to_string();
            self.preview = "No files found for this workspace.".to_string();
            return Ok(());
        }

        let entry = &self.entries[self.selected];
        self.preview_title = entry.path.display().to_string();
        self.preview = if entry.is_dir {
            format!("Directory: {}", entry.path.display())
        } else {
            read_text_preview(&entry.path)?
        };
        Ok(())
    }
}

impl DiffBrowserState {
    pub fn refresh(&mut self, root: &Path) {
        match build_diff_entries(root).and_then(|entries| {
            self.entries = entries;
            self.selected = self.selected.min(self.entries.len().saturating_sub(1));
            self.content_scroll = 0;
            self.error = None;
            self.update_preview(root)
        }) {
            Ok(()) => {}
            Err(error) => {
                self.entries.clear();
                self.selected = 0;
                self.preview_title = "Git Diff".to_string();
                self.preview = "Unable to load git diff.".to_string();
                self.error = Some(error.to_string());
            }
        }
    }

    pub fn move_up(&mut self, root: &Path) {
        if self.entries.is_empty() {
            return;
        }
        self.selected = self.selected.saturating_sub(1);
        self.content_scroll = 0;
        let _ = self.update_preview(root);
    }

    pub fn move_down(&mut self, root: &Path) {
        if self.entries.is_empty() {
            return;
        }
        self.selected = (self.selected + 1).min(self.entries.len().saturating_sub(1));
        self.content_scroll = 0;
        let _ = self.update_preview(root);
    }

    pub fn select(&mut self, root: &Path, index: usize) {
        if self.entries.is_empty() {
            return;
        }
        self.selected = index.min(self.entries.len().saturating_sub(1));
        self.content_scroll = 0;
        let _ = self.update_preview(root);
    }

    pub fn scroll_up(&mut self, lines: u16) {
        self.content_scroll = self.content_scroll.saturating_sub(lines);
    }

    pub fn scroll_down(&mut self, lines: u16) {
        self.content_scroll = self.content_scroll.saturating_add(lines);
    }

    fn update_preview(&mut self, root: &Path) -> Result<()> {
        if self.entries.is_empty() {
            self.preview_title = "Git Diff".to_string();
            self.preview = "No modified files found.".to_string();
            return Ok(());
        }

        let entry = &self.entries[self.selected];
        self.preview_title = entry.path.display().to_string();
        self.preview = read_diff_preview(root, entry)?;
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
        if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name == ".git")
        {
            continue;
        }

        let relative = path
            .strip_prefix(root)
            .with_context(|| format!("failed to make {} relative", path.display()))?;
        let depth = relative.components().count().saturating_sub(1);
        let indent = "  ".repeat(depth);
        let file_name = relative
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| relative.display().to_string());
        let is_dir = entry.file_type().is_some_and(|kind| kind.is_dir());

        entries.push(FileEntry {
            label: format!(
                "{}{} {}",
                indent,
                if is_dir { "[D]" } else { "[F]" },
                file_name
            ),
            path: path.to_path_buf(),
            is_dir,
        });
    }

    entries.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(entries)
}

fn build_diff_entries(root: &Path) -> Result<Vec<DiffEntry>> {
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
    let mut entries = Vec::new();

    for line in stdout.lines() {
        if line.len() < 3 {
            continue;
        }

        let status = line[..2].trim().to_string();
        let raw_path = line[3..].trim();
        let path = raw_path
            .rsplit_once(" -> ")
            .map(|(_, new_path)| new_path)
            .unwrap_or(raw_path);
        let full_path = root.join(path);

        entries.push(DiffEntry {
            label: format!("[{}] {}", status, path),
            path: full_path,
            status,
        });
    }

    Ok(entries)
}

fn read_text_preview(path: &Path) -> Result<String> {
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    if bytes.contains(&0) {
        return Ok("Binary file preview is not available.".to_string());
    }

    let truncated = bytes.len() > PREVIEW_LIMIT;
    let preview = String::from_utf8_lossy(&bytes[..bytes.len().min(PREVIEW_LIMIT)]).into_owned();
    if truncated {
        Ok(format!("{preview}\n\n[truncated]"))
    } else {
        Ok(preview)
    }
}

fn read_diff_preview(root: &Path, entry: &DiffEntry) -> Result<String> {
    if entry.status.contains('?') {
        return Ok(format!(
            "Untracked file: {}\n\n{}",
            entry.path.display(),
            read_text_preview(&entry.path)?
        ));
    }

    let relative = entry
        .path
        .strip_prefix(root)
        .unwrap_or(entry.path.as_path())
        .to_string_lossy()
        .to_string();

    let unstaged = git_output(root, &["diff", "--no-ext-diff", "--", &relative])?;
    let staged = git_output(
        root,
        &["diff", "--no-ext-diff", "--cached", "--", &relative],
    )?;

    let mut sections = Vec::new();
    if !unstaged.trim().is_empty() {
        sections.push(unstaged);
    }
    if !staged.trim().is_empty() {
        sections.push(format!("--- staged ---\n{staged}"));
    }

    if sections.is_empty() {
        Ok("No diff available for this file.".to_string())
    } else {
        Ok(sections.join("\n\n"))
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
