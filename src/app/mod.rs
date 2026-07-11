mod actions;
mod chat;
mod components;
mod effects;
mod event_types;
mod events;
mod events_chat;
mod events_git;
mod events_lsp;
mod events_shell;
mod events_workspace;
mod input;
mod lsp;
mod lsp_actions;
mod lsp_document;
mod lsp_framing;
mod mouse_actions;
mod navigation_actions;
mod runtime;
mod session;
mod shell;
#[cfg(test)]
mod test_support;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_chat;
#[cfg(test)]
mod tests_lsp;
mod ui;
mod workspace_actions;
mod workspace_watcher;

pub(super) use session::{MessageStore, SessionLoader};

use std::ops::{Deref, DerefMut};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet, VecDeque},
    env, fs,
    path::PathBuf,
    time::{Duration, Instant},
};

use anyhow::Result;
use crossterm::{
    event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind},
    terminal::size as terminal_size,
};
use pulldown_cmark::{
    CodeBlockKind, Event as MarkdownEvent, HeadingLevel, Options as MarkdownOptions,
    Parser as MarkdownParser, Tag, TagEnd,
};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Margin, Rect},
    style::{Modifier, Style},
    symbols::border,
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Tabs, Wrap},
};
use tokio::{sync::mpsc, time::sleep};

use self::components::ChatComponent;
use self::event_types::{
    ChatEvent, ClipboardOperation, GitEvent, LspEvent, ShellEvent, UiEvent, WorkspaceEvent,
};
use self::input::AppInput;
use crate::codex::{
    CodexAppServer, HistoryEntryKind, ModelInfo, ModelReasoningEffort, ServerEvent, ThreadItem,
    WorkspaceSession,
};
use crate::config::{AgentDefinition, CmdexConfig, ConfigStore, LspServerConfig};
use crate::theme::ThemeRegistry;
use crate::workspace::{
    DiffBrowserState, DiffSection, EditorCompletionItem, EditorMode, EditorPosition,
    FileBrowserState, GitMutation, GitRemoteAction, WorkspaceEditorState, WorkspaceSearchSnapshot,
    WorkspaceSidebarTab,
};

const SIDEBAR_WIDTH: u16 = 45;
const TAB_LABELS: [(&str, AppTab); 4] = [
    ("Chat", AppTab::Chat),
    ("Workspace", AppTab::Workspace),
    ("Shell", AppTab::Shell),
    ("Git Diff", AppTab::GitDiff),
];
const SPINNER: [&str; 8] = ["⠏", "⠛", "⠹", "⢸", "⣰", "⣤", "⣆", "⡇"];
const CONTENT_SCROLL_STEP: u16 = 4;
const MOUSE_SCROLL_STEP: u16 = 4;
const MOUSE_SCROLL_DEBOUNCE: Duration = Duration::from_millis(20);
const HOVER_POPOVER_DELAY: Duration = Duration::from_millis(200);
const FAST_TICK_INTERVAL: Duration = Duration::from_millis(80);
const WORKSPACE_TICK_INTERVAL: Duration = Duration::from_millis(250);
const IDLE_TICK_INTERVAL: Duration = Duration::from_millis(1000);
const SHELL_OUTPUT_LIMIT: usize = 64_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AppTab {
    Chat,
    Workspace,
    Shell,
    GitDiff,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScrollDirection {
    Up,
    Down,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScrollAxis {
    Vertical,
    Horizontal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScrollbarDragTarget {
    Chat,
    WorkspacePreview,
    WorkspaceEditor,
    WorkspaceCompletionPopover,
    ShellOutput,
    GitDiffPreview,
}

#[derive(Debug, Clone, Copy)]
struct ScrollbarMetrics {
    track: Rect,
    content_length: usize,
    viewport_length: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct UiLayout {
    sidebar_list: Rect,
    tabs: Rect,
    body: Rect,
    footer: Option<Rect>,
    add_name: Option<Rect>,
    add_workspace: Option<Rect>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct ShellSessionKey {
    agent_index: usize,
    session_id: usize,
}

struct ShellSessionRuntime {
    command_tx: std::sync::mpsc::Sender<String>,
    pid: u32,
}

struct LspRuntime {
    command_tx: std::sync::mpsc::Sender<lsp::LspCommand>,
    server_name: String,
    starting: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct LspRuntimeKey {
    agent_index: usize,
    server_index: usize,
}

#[derive(Debug, Clone)]
struct PendingWorkspaceHover {
    agent_index: usize,
    column: u16,
    row: u16,
    path: PathBuf,
    position: EditorPosition,
    started_at: Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AddAgentField {
    Name,
    Workspace,
}

#[derive(Debug, Clone)]
struct AddAgentForm {
    name: String,
    workspace: String,
    active_field: AddAgentField,
    error: Option<String>,
}

pub struct AppShell {
    current_tab: AppTab,
    current_agent: Option<usize>,
    chat_sidebar_index: usize,
    model_picker: Option<ModelPickerState>,
    add_form: AddAgentForm,
}

impl Default for AddAgentForm {
    fn default() -> Self {
        Self {
            name: String::new(),
            workspace: std::env::current_dir()
                .map(|path| path.display().to_string())
                .unwrap_or_default(),
            active_field: AddAgentField::Name,
            error: None,
        }
    }
}

#[derive(Debug, Clone)]
struct ChatMessage {
    role: MessageRole,
    text: String,
    rendered_lines: Vec<Line<'static>>,
    item_id: Option<String>,
}

#[derive(Debug, Clone)]
struct QueuedChatMessage {
    text: String,
}

#[derive(Debug, Clone, Copy)]
enum MessageRole {
    User,
    Assistant,
    Event,
    System,
    Shell,
}

#[derive(Debug, Clone)]
struct ChatRenderCache {
    text: Text<'static>,
    content_height: Option<(u16, usize)>,
    dirty: bool,
}

impl Default for ChatRenderCache {
    fn default() -> Self {
        Self {
            text: Text::default(),
            content_height: None,
            dirty: true,
        }
    }
}

impl ChatMessage {
    fn new(role: MessageRole, text: impl Into<String>, item_id: Option<String>) -> Self {
        let text = text.into();
        Self {
            role,
            rendered_lines: chat::ChatSupport::render_message_body(&text),
            text,
            item_id,
        }
    }

    fn set_text(&mut self, text: String) {
        self.text = text;
        self.rendered_lines = chat::ChatSupport::render_message_body(&self.text);
    }

    fn append_text(&mut self, delta: &str) {
        self.text.push_str(delta);
        self.rendered_lines = self
            .text
            .split('\n')
            .map(|line| Line::from(line.to_string()))
            .collect();
        if self.rendered_lines.is_empty() {
            self.rendered_lines.push(Line::default());
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppExit {
    Quit,
    Restart,
}

#[derive(Debug, Clone)]
struct ChatState {
    agent_name: String,
    thread_id: Option<String>,
    thread_loaded: bool,
    active_turn_id: Option<String>,
    messages: Vec<ChatMessage>,
    chat_model: Option<String>,
    chat_reasoning_effort: Option<String>,
    chat_model_label: String,
    chat_settings_explicit: bool,
    chat_follow_output: bool,
    chat_scroll: u16,
    thinking: bool,
    shell_running: bool,
    streaming_item_id: Option<String>,
    queued_chat_messages: VecDeque<QueuedChatMessage>,
    queued_chat_selection: usize,
    chat_render_cache: RefCell<ChatRenderCache>,
}

#[derive(Debug, Clone)]
struct AgentState {
    definition: AgentDefinition,
    chat: ChatState,
    workspace: FileBrowserState,
    shell_tab: shell::ShellTabState,
    git_diff: DiffBrowserState,
    status: Option<String>,
}

impl ChatState {
    fn new(
        agent_name: String,
        default_chat_model: Option<String>,
        default_chat_reasoning_effort: Option<String>,
        default_chat_model_label: &str,
    ) -> Self {
        Self {
            agent_name,
            thread_id: None,
            thread_loaded: false,
            active_turn_id: None,
            messages: Vec::new(),
            chat_model: default_chat_model,
            chat_reasoning_effort: default_chat_reasoning_effort,
            chat_model_label: default_chat_model_label.to_string(),
            chat_settings_explicit: false,
            chat_follow_output: true,
            chat_scroll: 0,
            thinking: false,
            shell_running: false,
            streaming_item_id: None,
            queued_chat_messages: VecDeque::new(),
            queued_chat_selection: 0,
            chat_render_cache: RefCell::new(ChatRenderCache::default()),
        }
    }

    fn push_message(&mut self, message: ChatMessage) {
        self.messages.push(message);
        self.invalidate_chat_render_cache();
    }

    fn replace_messages(&mut self, messages: Vec<ChatMessage>) {
        self.messages = messages;
        self.invalidate_chat_render_cache();
    }

    fn invalidate_chat_render_cache(&self) {
        let mut cache = self.chat_render_cache.borrow_mut();
        cache.dirty = true;
        cache.content_height = None;
    }

    fn enqueue_chat_message(&mut self, text: String) {
        self.queued_chat_messages
            .push_back(QueuedChatMessage { text });
        self.clamp_queued_chat_selection();
    }

    fn pop_next_queued_chat_message(&mut self) -> Option<String> {
        let message = self.queued_chat_messages.pop_front()?;
        if self.queued_chat_selection > 0 {
            self.queued_chat_selection -= 1;
        }
        self.clamp_queued_chat_selection();
        Some(message.text)
    }

    fn cancel_selected_queued_chat_message(&mut self) -> Option<String> {
        let index = self.selected_queued_chat_index()?;
        let message = self.queued_chat_messages.remove(index)?;
        self.clamp_queued_chat_selection();
        Some(message.text)
    }

    fn queued_chat_count(&self) -> usize {
        self.queued_chat_messages.len()
    }

    fn has_queued_chat_messages(&self) -> bool {
        !self.queued_chat_messages.is_empty()
    }

    fn selected_queued_chat_index(&self) -> Option<usize> {
        self.has_queued_chat_messages().then_some(
            self.queued_chat_selection
                .min(self.queued_chat_messages.len() - 1),
        )
    }

    fn queued_chat_messages(&self) -> &VecDeque<QueuedChatMessage> {
        &self.queued_chat_messages
    }

    fn select_previous_queued_chat_message(&mut self) {
        let Some(selected) = self.selected_queued_chat_index() else {
            return;
        };

        self.queued_chat_selection = if selected == 0 {
            self.queued_chat_messages.len() - 1
        } else {
            selected - 1
        };
    }

    fn select_next_queued_chat_message(&mut self) {
        let Some(selected) = self.selected_queued_chat_index() else {
            return;
        };

        self.queued_chat_selection = (selected + 1) % self.queued_chat_messages.len();
    }

    fn clamp_queued_chat_selection(&mut self) {
        if self.queued_chat_messages.is_empty() {
            self.queued_chat_selection = 0;
        } else {
            self.queued_chat_selection = self
                .queued_chat_selection
                .min(self.queued_chat_messages.len() - 1);
        }
    }

    fn chat_text(&self) -> Text<'static> {
        self.ensure_chat_render_cache();
        self.chat_render_cache.borrow().text.clone()
    }

    fn chat_content_height(&self, area: Rect) -> usize {
        let width = components::UiSupport::inner_rect(area).width.max(1);
        self.ensure_chat_render_cache();

        if let Some((cached_width, content_height)) = self.chat_render_cache.borrow().content_height
            && cached_width == width
        {
            return content_height;
        }

        let content_height = {
            let cache = self.chat_render_cache.borrow();
            components::UiSupport::wrapped_text_height(&cache.text, width)
        };
        self.chat_render_cache.borrow_mut().content_height = Some((width, content_height));
        content_height
    }

    fn ensure_chat_render_cache(&self) {
        if !self.chat_render_cache.borrow().dirty {
            return;
        }

        let text = chat::ChatSupport::build_text(self);
        let mut cache = self.chat_render_cache.borrow_mut();
        cache.text = text;
        cache.content_height = None;
        cache.dirty = false;
    }
}

impl AgentState {
    fn new(
        definition: AgentDefinition,
        default_chat_model: Option<String>,
        default_chat_reasoning_effort: Option<String>,
        default_chat_model_label: &str,
    ) -> Self {
        let chat = ChatState::new(
            definition.name.clone(),
            default_chat_model,
            default_chat_reasoning_effort,
            default_chat_model_label,
        );
        Self {
            definition,
            chat,
            workspace: FileBrowserState::default(),
            shell_tab: shell::ShellTabState::default(),
            git_diff: DiffBrowserState::default(),
            status: None,
        }
    }
}

pub struct App {
    shell: AppShell,
    config_path: PathBuf,
    config: CmdexConfig,
    lsp_servers: Vec<LspServerConfig>,
    agents: Vec<AgentState>,
    default_chat_model: Option<String>,
    default_chat_reasoning_effort: Option<String>,
    chat_model_label: String,
    chat_input: String,
    spinner_index: usize,
    status_message: Option<String>,
    last_mouse_scroll: Option<(ScrollAxis, ScrollDirection, Instant)>,
    active_scrollbar_drag: Option<ScrollbarDragTarget>,
    active_workspace_selection_drag: bool,
    shell_runtimes: HashMap<ShellSessionKey, ShellSessionRuntime>,
    lsp_runtimes: HashMap<LspRuntimeKey, LspRuntime>,
    lsp_starting: HashSet<LspRuntimeKey>,
    pending_lsp_commands: HashMap<LspRuntimeKey, Vec<lsp::LspCommand>>,
    pending_workspace_hover: Option<PendingWorkspaceHover>,
    workspace_refresh_in_flight: HashSet<usize>,
    workspace_watchers: HashMap<usize, std::sync::mpsc::Sender<()>>,
    pending_effects: VecDeque<effects::AppEffect>,
}

impl Deref for App {
    type Target = AppShell;

    fn deref(&self) -> &Self::Target {
        &self.shell
    }
}

impl DerefMut for App {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.shell
    }
}

pub struct AppRuntime;

#[derive(Debug, Clone)]
struct ModelPickerState {
    agent_index: usize,
    models: Vec<ModelInfo>,
    selected: usize,
    view: ModelPickerView,
}

#[derive(Debug, Clone)]
enum ModelPickerView {
    Models,
    Efforts { model_index: usize, selected: usize },
}

impl App {
    fn new(config_path: PathBuf, config: CmdexConfig) -> Self {
        let lsp_servers = config.effective_lsp_servers();
        let default_chat_model = chat::ChatSupport::load_codex_chat_model();
        let default_chat_reasoning_effort = chat::ChatSupport::load_codex_chat_reasoning_effort();
        let default_chat_model_label = chat::ChatSupport::load_codex_chat_model_label()
            .unwrap_or_else(|| "default".to_string());
        let agents = config
            .agents
            .iter()
            .cloned()
            .map(|definition| {
                AgentState::new(
                    definition,
                    default_chat_model.clone(),
                    default_chat_reasoning_effort.clone(),
                    &default_chat_model_label,
                )
            })
            .collect::<Vec<_>>();
        let current_agent = if agents.is_empty() { None } else { Some(0) };
        let chat_sidebar_index = if agents.is_empty() {
            0
        } else {
            current_agent.map(|index| index + 1).unwrap_or(0)
        };

        let app = Self {
            shell: AppShell {
                current_tab: AppTab::Chat,
                current_agent,
                chat_sidebar_index,
                model_picker: None,
                add_form: AddAgentForm::default(),
            },
            config_path,
            config,
            lsp_servers,
            agents,
            default_chat_model,
            default_chat_reasoning_effort,
            chat_model_label: default_chat_model_label,
            chat_input: String::new(),
            spinner_index: 0,
            status_message: None,
            last_mouse_scroll: None,
            active_scrollbar_drag: None,
            active_workspace_selection_drag: false,
            shell_runtimes: HashMap::new(),
            lsp_runtimes: HashMap::new(),
            lsp_starting: HashSet::new(),
            pending_lsp_commands: HashMap::new(),
            pending_workspace_hover: None,
            workspace_refresh_in_flight: HashSet::new(),
            workspace_watchers: HashMap::new(),
            pending_effects: VecDeque::new(),
        };

        #[cfg(test)]
        let app = {
            let mut app = app;
            app.hydrate_workspace_snapshots_for_tests();
            app
        };

        app
    }

    #[cfg(test)]
    fn hydrate_workspace_snapshots_for_tests(&mut self) {
        for agent in &mut self.agents {
            let root = agent.definition.workspace.clone();
            match FileBrowserState::scan_entries(&root).and_then(|entries| {
                agent
                    .workspace
                    .apply_scanned_entries_without_io(entries)
                    .map(|_| ())
            }) {
                Ok(()) => {}
                Err(error) => agent.workspace.error = Some(error.to_string()),
            }
        }
    }

    fn active_agent(&self) -> Option<&AgentState> {
        self.current_agent.and_then(|index| self.agents.get(index))
    }

    fn active_chat_model_label(&self) -> &str {
        self.active_agent()
            .map(|agent| agent.chat.chat_model_label.as_str())
            .unwrap_or(self.chat_model_label.as_str())
    }

    fn active_agent_mut(&mut self) -> Option<&mut AgentState> {
        self.current_agent
            .and_then(move |index| self.agents.get_mut(index))
    }

    fn enqueue_effect(&mut self, effect: effects::AppEffect) {
        self.pending_effects.push_back(effect);
    }

    fn take_effects(&mut self) -> Vec<effects::AppEffect> {
        self.pending_effects.drain(..).collect()
    }

    fn add_agent_selected(&self) -> bool {
        self.current_tab == AppTab::Chat && self.chat_sidebar_index == 0
    }

    fn lsp_server_for_path(&self, path: &std::path::Path) -> Option<(usize, &LspServerConfig)> {
        self.lsp_servers
            .iter()
            .enumerate()
            .find(|(_, server)| server.matches_path(path))
    }

    fn has_active_workspace_lsp_startup(&self) -> bool {
        self.active_workspace_lsp_runtime()
            .is_some_and(|runtime| runtime.starting)
    }

    fn active_workspace_lsp_loading_label(&self) -> Option<String> {
        let runtime = self.active_workspace_lsp_runtime()?;
        runtime
            .starting
            .then(|| format!("{} {}", SPINNER[self.spinner_index], runtime.server_name))
    }

    fn active_workspace_lsp_runtime(&self) -> Option<&LspRuntime> {
        if self.current_tab != AppTab::Workspace {
            return None;
        }

        let agent_index = self.current_agent?;
        let editor = self.agents.get(agent_index)?.workspace.editor.as_ref()?;
        let server_index = self
            .lsp_server_for_path(&editor.path)
            .map(|(index, _)| index)?;

        self.lsp_runtimes.get(&LspRuntimeKey {
            agent_index,
            server_index,
        })
    }
}
