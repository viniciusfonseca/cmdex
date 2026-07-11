use std::path::PathBuf;

use crate::codex::{ModelInfo, ThreadInfo, WorkspaceSession};
use crate::workspace::{
    EditorCompletionItem, EditorPosition, FileEntry, GitDiffLoadResult, GitMutation,
    GitRemoteAction,
};

use super::{WorkspaceSearchSnapshot, lsp};
use tokio::sync::mpsc;

#[derive(Debug, Clone)]
pub(super) enum UiEvent {
    Chat(ChatEvent),
    Shell(ShellEvent),
    Git(GitEvent),
    Workspace(WorkspaceEvent),
    Lsp(LspEvent),
}

#[derive(Debug, Clone)]
pub(super) enum ChatEvent {
    ThreadReady {
        agent_index: usize,
        thread: ThreadInfo,
    },
    ModelCommandResult {
        agent_index: usize,
        message: String,
    },
    ModelListLoaded {
        agent_index: usize,
        models: Vec<ModelInfo>,
    },
    SessionLoaded {
        agent_index: usize,
        session: Option<WorkspaceSession>,
    },
    SubmissionFailed {
        agent_index: usize,
        message: String,
    },
    TurnStartedLocal {
        agent_index: usize,
        turn_id: String,
    },
    TurnInterruptFailed {
        agent_index: usize,
        message: String,
    },
}

#[derive(Debug, Clone)]
pub(super) enum ShellEvent {
    RuntimeReady {
        agent_index: usize,
        session_id: usize,
        command_tx: std::sync::mpsc::Sender<String>,
        pid: u32,
    },
    CommandCompleted {
        agent_index: usize,
        output: String,
        success: bool,
    },
    SessionReady {
        agent_index: usize,
        session_id: usize,
    },
    SessionOutput {
        agent_index: usize,
        session_id: usize,
        line: String,
        stderr: bool,
    },
    SessionCommandFinished {
        agent_index: usize,
        session_id: usize,
        exit_code: i32,
    },
    SessionExited {
        agent_index: usize,
        session_id: usize,
        message: String,
    },
}

#[derive(Debug, Clone)]
pub(super) enum GitEvent {
    RemoteCompleted {
        agent_index: usize,
        action: GitRemoteAction,
        success: bool,
        message: String,
    },
    MutationCompleted {
        agent_index: usize,
        mutation: GitMutation,
        success: bool,
        message: String,
    },
    Loaded {
        agent_index: usize,
        generation: u64,
        result: Option<GitDiffLoadResult>,
        error: Option<String>,
    },
}

#[derive(Debug, Clone, Copy)]
pub(super) enum ClipboardOperation {
    Copy,
    Paste,
}

#[derive(Debug, Clone)]
pub(super) enum WorkspaceEvent {
    WatcherReady {
        agent_index: usize,
        stop_tx: std::sync::mpsc::Sender<()>,
    },
    WatcherFailed {
        agent_index: usize,
        message: String,
    },
    WatcherError {
        agent_index: usize,
        message: String,
    },
    FilesystemChanged {
        agent_index: usize,
    },
    EntriesLoaded {
        agent_index: usize,
        entries: Vec<FileEntry>,
        error: Option<String>,
    },
    SearchCompleted {
        agent_index: usize,
        generation: u64,
        query: String,
        snapshot: WorkspaceSearchSnapshot,
        error: Option<String>,
    },
    EditorLoaded {
        agent_index: usize,
        path: PathBuf,
        position: Option<EditorPosition>,
        source: Option<String>,
        error: Option<String>,
    },
    PreviewLoaded {
        agent_index: usize,
        path: PathBuf,
        preview: Vec<ratatui::text::Line<'static>>,
        error: Option<String>,
    },
    ClipboardCompleted {
        agent_index: usize,
        path: PathBuf,
        operation: ClipboardOperation,
        text: Option<String>,
        error: Option<String>,
    },
}

#[derive(Debug, Clone)]
pub(super) enum LspEvent {
    RuntimeReady {
        agent_index: usize,
        server_index: usize,
        server_name: String,
        command_tx: std::sync::mpsc::Sender<lsp::LspCommand>,
    },
    RuntimeFailed {
        agent_index: usize,
        server_index: usize,
        message: String,
    },
    HoverResult {
        agent_index: usize,
        path: PathBuf,
        position: EditorPosition,
        contents: Option<String>,
        error: Option<String>,
    },
    DefinitionResult {
        agent_index: usize,
        source_path: PathBuf,
        _source_position: EditorPosition,
        target: Option<lsp::DefinitionTarget>,
        error: Option<String>,
    },
    CompletionResult {
        agent_index: usize,
        path: PathBuf,
        position: EditorPosition,
        items: Vec<EditorCompletionItem>,
        error: Option<String>,
    },
    Notification {
        agent_index: usize,
        server_name: String,
        method: String,
        params: serde_json::Value,
    },
}

impl From<ChatEvent> for UiEvent {
    fn from(event: ChatEvent) -> Self {
        Self::Chat(event)
    }
}

impl From<ShellEvent> for UiEvent {
    fn from(event: ShellEvent) -> Self {
        Self::Shell(event)
    }
}

impl From<GitEvent> for UiEvent {
    fn from(event: GitEvent) -> Self {
        Self::Git(event)
    }
}

impl From<WorkspaceEvent> for UiEvent {
    fn from(event: WorkspaceEvent) -> Self {
        Self::Workspace(event)
    }
}

impl From<LspEvent> for UiEvent {
    fn from(event: LspEvent) -> Self {
        Self::Lsp(event)
    }
}

pub(super) fn send<T>(tx: &mpsc::UnboundedSender<UiEvent>, event: T)
where
    UiEvent: From<T>,
{
    let _ = tx.send(event.into());
}
