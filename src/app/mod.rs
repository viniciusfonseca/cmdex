mod actions;
mod chat;
mod components;
mod shell;
#[cfg(test)]
mod tests;
mod ui;

use std::{
    cell::RefCell,
    collections::HashMap,
    env, fs,
    path::PathBuf,
    time::{Duration, Instant},
};

use anyhow::Result;
use crossterm::{
    event::{
        Event, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent,
        MouseEventKind,
    },
    terminal::size as terminal_size,
};
use futures_util::StreamExt;
use pulldown_cmark::{
    CodeBlockKind, Event as MarkdownEvent, HeadingLevel, Options as MarkdownOptions,
    Parser as MarkdownParser, Tag, TagEnd,
};
use ratatui::{
    DefaultTerminal, Frame,
    layout::{Alignment, Constraint, Direction, Layout, Margin, Rect},
    style::{Modifier, Style},
    symbols::border,
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Tabs, Wrap},
};
use tokio::{sync::mpsc, time::sleep};

use crate::codex::{
    CodexAppServer, HistoryEntryKind, ModelInfo, ServerEvent, ThreadInfo, ThreadItem,
    WorkspaceSession,
};
use crate::config::{AgentDefinition, CmdexConfig, ConfigStore};
use crate::theme::ThemeRegistry;
use crate::workspace::{
    DiffBrowserState, DiffSection, EditorMode, FileBrowserState, GitRemoteAction,
    WorkspaceEditorState, WorkspaceSidebarTab,
};
use tokio::process::Command;

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
const WORKSPACE_AUTO_REFRESH_INTERVAL: Duration = Duration::from_millis(750);
const FAST_TICK_INTERVAL: Duration = Duration::from_millis(80);
const WORKSPACE_TICK_INTERVAL: Duration = Duration::from_millis(250);
const IDLE_TICK_INTERVAL: Duration = Duration::from_millis(1000);
const SHELL_OUTPUT_LIMIT: usize = 64_000;

#[derive(Debug, Clone)]
enum UiEvent {
    ThreadReady {
        agent_index: usize,
        thread: ThreadInfo,
    },
    ModelCommandResult {
        agent_index: usize,
        message: String,
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
    ShellCompleted {
        agent_index: usize,
        output: String,
        success: bool,
    },
    TurnInterruptFailed {
        agent_index: usize,
        message: String,
    },
    GitDiffRemoteCompleted {
        agent_index: usize,
        action: GitRemoteAction,
        success: bool,
        message: String,
    },
    ShellSessionOutput {
        agent_index: usize,
        session_id: usize,
        line: String,
        stderr: bool,
    },
    ShellSessionCommandFinished {
        agent_index: usize,
        session_id: usize,
        exit_code: i32,
    },
    ShellSessionExited {
        agent_index: usize,
        session_id: usize,
        message: String,
    },
}

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
enum ScrollbarDragTarget {
    Chat,
    WorkspacePreview,
    WorkspaceEditor,
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
struct AgentState {
    definition: AgentDefinition,
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
    chat_render_cache: RefCell<ChatRenderCache>,
    workspace: FileBrowserState,
    shell_tab: shell::ShellTabState,
    git_diff: DiffBrowserState,
    status: Option<String>,
}

impl AgentState {
    fn new(
        definition: AgentDefinition,
        default_chat_model: Option<String>,
        default_chat_reasoning_effort: Option<String>,
        default_chat_model_label: &str,
    ) -> Self {
        Self {
            definition,
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
            chat_render_cache: RefCell::new(ChatRenderCache::default()),
            workspace: FileBrowserState::default(),
            shell_tab: shell::ShellTabState::default(),
            git_diff: DiffBrowserState::default(),
            status: None,
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

    fn chat_text(&self) -> Text<'static> {
        self.ensure_chat_render_cache();
        self.chat_render_cache.borrow().text.clone()
    }

    fn chat_content_height(&self, area: Rect) -> usize {
        let width = components::UiSupport::inner_rect(area).width.max(1);
        self.ensure_chat_render_cache();

        if let Some((cached_width, content_height)) = self.chat_render_cache.borrow().content_height
        {
            if cached_width == width {
                return content_height;
            }
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

pub struct App {
    config_path: PathBuf,
    config: CmdexConfig,
    agents: Vec<AgentState>,
    default_chat_model: Option<String>,
    default_chat_reasoning_effort: Option<String>,
    chat_model_label: String,
    current_tab: AppTab,
    current_agent: Option<usize>,
    chat_sidebar_index: usize,
    chat_input: String,
    add_form: AddAgentForm,
    spinner_index: usize,
    status_message: Option<String>,
    should_quit: bool,
    should_restart: bool,
    last_mouse_scroll: Option<(ScrollDirection, Instant)>,
    active_scrollbar_drag: Option<ScrollbarDragTarget>,
    last_workspace_refresh_at: Option<Instant>,
    shell_runtimes: HashMap<ShellSessionKey, ShellSessionRuntime>,
}

impl App {
    fn new(config_path: PathBuf, config: CmdexConfig) -> Self {
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

        Self {
            config_path,
            config,
            agents,
            default_chat_model,
            default_chat_reasoning_effort,
            chat_model_label: default_chat_model_label,
            current_tab: AppTab::Chat,
            current_agent,
            chat_sidebar_index,
            chat_input: String::new(),
            add_form: AddAgentForm::default(),
            spinner_index: 0,
            status_message: None,
            should_quit: false,
            should_restart: false,
            last_mouse_scroll: None,
            active_scrollbar_drag: None,
            last_workspace_refresh_at: None,
            shell_runtimes: HashMap::new(),
        }
    }

    fn active_agent(&self) -> Option<&AgentState> {
        self.current_agent.and_then(|index| self.agents.get(index))
    }

    fn active_chat_model_label(&self) -> &str {
        self.active_agent()
            .map(|agent| agent.chat_model_label.as_str())
            .unwrap_or(self.chat_model_label.as_str())
    }

    fn active_agent_mut(&mut self) -> Option<&mut AgentState> {
        self.current_agent
            .and_then(move |index| self.agents.get_mut(index))
    }

    fn add_agent_selected(&self) -> bool {
        self.current_tab == AppTab::Chat && self.chat_sidebar_index == 0
    }
}

struct MessageStore;

impl MessageStore {
    fn upsert(agent: &mut AgentState, role: MessageRole, item_id: &str, text: String) {
        if let Some(message) = agent
            .messages
            .iter_mut()
            .find(|message| message.item_id.as_deref() == Some(item_id))
        {
            message.set_text(text);
        } else {
            agent
                .messages
                .push(ChatMessage::new(role, text, Some(item_id.to_string())));
        }
        agent.invalidate_chat_render_cache();
    }
}

struct SessionLoader;

impl SessionLoader {
    fn session_messages(session: WorkspaceSession) -> (String, Vec<ChatMessage>) {
        let messages = session
            .entries
            .into_iter()
            .map(|entry| {
                ChatMessage::new(
                    match entry.kind {
                        HistoryEntryKind::User => MessageRole::User,
                        HistoryEntryKind::Assistant => MessageRole::Assistant,
                        HistoryEntryKind::Event => MessageRole::Event,
                    },
                    entry.text,
                    None,
                )
            })
            .collect::<Vec<_>>();

        (session.thread.id, messages)
    }

    fn spawn(
        codex: CodexAppServer,
        ui_tx: mpsc::UnboundedSender<UiEvent>,
        agent_index: usize,
        workspace: PathBuf,
    ) {
        tokio::spawn(async move {
            match codex.load_latest_workspace_session(&workspace).await {
                Ok(session) => {
                    let _ = ui_tx.send(UiEvent::SessionLoaded {
                        agent_index,
                        session,
                    });
                }
                Err(error) => {
                    let _ = ui_tx.send(UiEvent::SubmissionFailed {
                        agent_index,
                        message: format!("Failed to load the latest workspace session: {error}"),
                    });
                }
            }
        });
    }

    async fn hydrate_latest_sessions(app: &mut App, codex: &CodexAppServer) -> Result<()> {
        for agent in &mut app.agents {
            if let Some(session) = codex
                .load_latest_workspace_session(&agent.definition.workspace)
                .await?
            {
                let (thread_id, messages) = Self::session_messages(session);
                agent.thread_id = Some(thread_id);
                agent.replace_messages(messages);
            }
        }

        Ok(())
    }
}

pub struct AppRuntime;

impl AppRuntime {
    pub async fn run(terminal: &mut DefaultTerminal) -> Result<AppExit> {
        let config_path = ConfigStore::default_path()?;
        let config = ConfigStore::load(&config_path)?;

        let (server_tx, mut server_rx) = mpsc::unbounded_channel();
        let (ui_tx, mut ui_rx) = mpsc::unbounded_channel();
        let codex = CodexAppServer::spawn(server_tx).await?;
        let mut app = App::new(config_path, config);
        SessionLoader::hydrate_latest_sessions(&mut app, &codex).await?;

        let mut events = EventStream::new();
        components::TopNavigationComponent::refresh_current_tab(&mut app);
        let mut needs_redraw = true;

        let exit = loop {
            if app.should_quit {
                break if app.should_restart {
                    AppExit::Restart
                } else {
                    AppExit::Quit
                };
            }

            if needs_redraw {
                terminal.draw(|frame| ui::AppUi::draw(frame, &app))?;
            }

            let tick = sleep(app.tick_interval());
            tokio::pin!(tick);

            tokio::select! {
                maybe_event = events.next() => {
                    match maybe_event {
                        Some(Ok(Event::Key(key))) if key.kind == KeyEventKind::Press => {
                            let (width, height) = terminal_size()?;
                            app.handle_key(key, &codex, &ui_tx, Rect::new(0, 0, width, height));
                            needs_redraw = true;
                        }
                        Some(Ok(Event::Mouse(mouse))) => {
                            let (width, height) = terminal_size()?;
                            app.handle_mouse(mouse, Rect::new(0, 0, width, height), &ui_tx);
                            needs_redraw = true;
                        }
                        Some(Ok(Event::Paste(text))) => {
                            for character in text.chars() {
                                app.handle_text_input(character);
                            }
                            needs_redraw = true;
                        }
                        Some(Ok(_)) => {
                            needs_redraw = true;
                        }
                        Some(Err(error)) => {
                            app.status_message = Some(error.to_string());
                            needs_redraw = true;
                        }
                        None => break AppExit::Quit,
                    }
                }
                Some(server_event) = server_rx.recv() => {
                    app.handle_server_event(server_event);
                    needs_redraw = true;
                }
                Some(ui_event) = ui_rx.recv() => {
                    app.handle_ui_event(ui_event);
                    needs_redraw = true;
                }
                _ = &mut tick => {
                    needs_redraw = app.on_tick();
                }
            }
        };

        app.shutdown_shell_sessions();

        Ok(exit)
    }
}
