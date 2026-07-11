mod browser;
mod diff;
mod editor;
mod editor_commands;
pub(crate) mod editor_keymap;
mod editor_overlays;
mod git_repository;
mod render;
#[cfg(test)]
mod tests;

pub(crate) use diff::GitDiffLoadResult;
pub(crate) use git_repository::GitRepository;

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use ignore::WalkBuilder;
use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};

use crate::theme::ThemeRegistry;

const PREVIEW_LIMIT: usize = 200_000;
pub(crate) const COMPLETION_POPOVER_MAX_ITEMS: usize = 8;
const SEARCH_DEBOUNCE: Duration = Duration::from_millis(80);

#[derive(Debug, Clone, Default)]
pub struct FileBrowserState {
    pub entries: Vec<FileEntry>,
    pub tree_rows: Vec<FileTreeRow>,
    pub selected: usize,
    pub tree_cursor: usize,
    pub focus: WorkspaceFocus,
    pub sidebar_tab: WorkspaceSidebarTab,
    pub search_query: String,
    pub preview_title: String,
    pub preview: Vec<Line<'static>>,
    pub content_scroll: u16,
    pub error: Option<String>,
    pub editor: Option<WorkspaceEditorState>,
    collapsed_dirs: BTreeSet<PathBuf>,
    known_dirs: BTreeSet<PathBuf>,
    search_rows: Vec<WorkspaceSearchRow>,
    search_selected_row: usize,
    search_match_count: usize,
    search_generation: u64,
    search_requested_at: Option<Instant>,
    search_in_flight: bool,
    editor_load_in_flight: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    pub path: PathBuf,
    pub relative_path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct FileTreeRow {
    pub label: String,
    branch_prefix_len: usize,
    kind: FileTreeRowKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WorkspaceSidebarTab {
    #[default]
    Files,
    Search,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WorkspaceFocus {
    #[default]
    Sidebar,
    Editor,
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

#[derive(Debug, Clone)]
enum WorkspaceSearchRow {
    FileHeader {
        label: String,
    },
    Match {
        label: String,
        file_index: usize,
        line_number: usize,
    },
}

#[derive(Debug, Clone, Default)]
pub(crate) struct WorkspaceSearchSnapshot {
    rows: Vec<WorkspaceSearchRow>,
    match_count: usize,
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
    pub remote_action: Option<GitRemoteAction>,
    pub mutation_running: bool,
    pub refresh_generation: u64,
    pub refresh_in_flight: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DiffSection {
    #[default]
    Changes,
    Staged,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitRemoteAction {
    Push,
    Pull,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitMutation {
    Commit(String),
    Stage(String),
    Unstage(String),
    Discard { path: String, untracked: bool },
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
    Visual,
    Insert,
    Command,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorDirection {
    Left,
    Right,
    Up,
    Down,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorCommand {
    Move {
        direction: EditorDirection,
        extend: bool,
    },
    MoveLineStart {
        extend: bool,
    },
    MoveLineEnd {
        extend: bool,
    },
    MovePage {
        lines: usize,
        extend: bool,
        up: bool,
    },
    EnterInsert,
    EnterVisual,
    ExitVisual,
    StartCommand,
    DeleteChar,
    Backspace,
    InsertNewline,
    InsertTab,
    OpenBelow,
    Undo,
    EnterInsertAfter,
    ExitInsert,
    DeleteSelection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditorCommandResult {
    pub saved: bool,
    pub close: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct EditorPosition {
    pub row: usize,
    pub col: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorCompletionItem {
    pub label: String,
    pub detail: Option<String>,
    pub insert_text: String,
    pub replace_start: EditorPosition,
    pub replace_end: EditorPosition,
    pub preselected: bool,
}

#[derive(Debug, Clone)]
enum EditorOverlay {
    None,
    ShortcutsHelp,
    Completion(EditorCompletionState),
}

#[derive(Debug, Clone)]
struct EditorCompletionState {
    items: Vec<EditorCompletionItem>,
    request_position: EditorPosition,
    selected: usize,
    scroll: usize,
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
    pub hover: Option<String>,
    saved_lines: Vec<String>,
    undo_stack: Vec<EditorUndoState>,
    preferred_col: usize,
    selection_anchor: Option<EditorPosition>,
    hover_request: Option<EditorPosition>,
    overlay: EditorOverlay,
    render_cache: EditorRenderCache,
}

#[derive(Debug, Clone)]
struct EditorUndoState {
    lines: Vec<String>,
    cursor_row: usize,
    cursor_col: usize,
    vertical_scroll: u16,
    horizontal_scroll: u16,
    preferred_col: usize,
}

#[derive(Debug, Clone, Default)]
struct EditorRenderCache {
    lines: Vec<Line<'static>>,
}
