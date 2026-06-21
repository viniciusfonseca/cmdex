use std::{
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
    pub selected: usize,
    pub preview_title: String,
    pub preview: Vec<Line<'static>>,
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
    pub preview: Vec<Line<'static>>,
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
                self.preview = plain_preview_lines("Unable to load workspace.");
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
            self.preview = plain_preview_lines("No files found for this workspace.");
            return Ok(());
        }

        let entry = &self.entries[self.selected];
        self.preview_title = entry.path.display().to_string();
        self.preview = if entry.is_dir {
            plain_preview_lines(&format!("Directory: {}", entry.path.display()))
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
                self.preview = plain_preview_lines("Unable to load git diff.");
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
            self.preview = plain_preview_lines("No modified files found.");
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
        if contains_git_component(path) {
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

fn read_diff_preview(root: &Path, entry: &DiffEntry) -> Result<Vec<Line<'static>>> {
    if entry.status.contains('?') {
        let mut lines = plain_preview_lines(&format!("Untracked file: {}", entry.path.display()));
        lines.push(Line::default());
        lines.extend(read_text_preview(&entry.path)?);
        return Ok(lines);
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
        Ok(plain_preview_lines("No diff available for this file."))
    } else {
        let combined = normalize_newlines(&sections.join("\n\n"));
        Ok(add_line_numbers(highlighted_preview_lines(
            &combined,
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
}
