use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;
use crossterm::{
    event::{
        Event, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent,
        MouseEventKind,
    },
    terminal::size as terminal_size,
};
use futures_util::StreamExt;
use ratatui::{
    DefaultTerminal, Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Tabs, Wrap},
};
use tokio::{
    sync::mpsc,
    time::{MissedTickBehavior, interval},
};

use crate::codex::{CodexAppServer, ServerEvent, ThreadInfo, ThreadItem};
use crate::config::{
    AgentDefinition, CmdexConfig, compact_home, default_config_path, load_config, save_config,
    validate_agent_input,
};
use crate::workspace::{DiffBrowserState, FileBrowserState};

const LOGO: [&str; 7] = [
    " ██████╗███╗   ███╗██████╗ ███████╗██╗  ██╗",
    "██╔════╝████╗ ████║██╔══██╗██╔════╝╚██╗██╔╝",
    "██║     ██╔████╔██║██║  ██║█████╗   ╚███╔╝ ",
    "██║     ██║╚██╔╝██║██║  ██║██╔══╝   ██╔██╗ ",
    "╚██████╗██║ ╚═╝ ██║██████╔╝███████╗██╔╝ ██╗",
    " ╚═════╝╚═╝     ╚═╝╚═════╝ ╚══════╝╚═╝  ╚═╝",
    "                                           ",
];

const LOGO_WIDTH: u16 = 43;
const SIDEBAR_WIDTH: u16 = LOGO_WIDTH + 2;
const TAB_LABELS: [(&str, AppTab); 3] = [
    ("Chat", AppTab::Chat),
    ("Workspace", AppTab::Workspace),
    ("Git Diff", AppTab::GitDiff),
];
const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const CONTENT_SCROLL_STEP: u16 = 12;

#[derive(Debug, Clone)]
enum UiEvent {
    ThreadReady {
        agent_index: usize,
        thread: ThreadInfo,
    },
    SubmissionFailed {
        agent_index: usize,
        message: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AppTab {
    Chat,
    Workspace,
    GitDiff,
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
    item_id: Option<String>,
}

#[derive(Debug, Clone, Copy)]
enum MessageRole {
    User,
    Assistant,
    System,
}

#[derive(Debug, Clone)]
struct AgentState {
    definition: AgentDefinition,
    thread_id: Option<String>,
    messages: Vec<ChatMessage>,
    thinking: bool,
    streaming_item_id: Option<String>,
    workspace: FileBrowserState,
    git_diff: DiffBrowserState,
    status: Option<String>,
}

#[derive(Debug, Clone, Copy)]
struct UiLayout {
    sidebar_list: Rect,
    tabs: Rect,
    body: Rect,
    footer: Option<Rect>,
    add_name: Option<Rect>,
    add_workspace: Option<Rect>,
}

impl AgentState {
    fn new(definition: AgentDefinition) -> Self {
        Self {
            definition,
            thread_id: None,
            messages: Vec::new(),
            thinking: false,
            streaming_item_id: None,
            workspace: FileBrowserState::default(),
            git_diff: DiffBrowserState::default(),
            status: None,
        }
    }
}

pub struct App {
    config_path: PathBuf,
    config: CmdexConfig,
    agents: Vec<AgentState>,
    current_tab: AppTab,
    current_agent: Option<usize>,
    chat_sidebar_index: usize,
    chat_input: String,
    add_form: AddAgentForm,
    spinner_index: usize,
    status_message: Option<String>,
    should_quit: bool,
}

impl App {
    fn new(config_path: PathBuf, config: CmdexConfig) -> Self {
        let agents = config
            .agents
            .iter()
            .cloned()
            .map(AgentState::new)
            .collect::<Vec<_>>();

        let current_agent = if agents.is_empty() { None } else { Some(0) };
        let chat_sidebar_index = if agents.is_empty() {
            0
        } else {
            current_agent.unwrap_or(0)
        };

        Self {
            config_path,
            config,
            agents,
            current_tab: AppTab::Chat,
            current_agent,
            chat_sidebar_index,
            chat_input: String::new(),
            add_form: AddAgentForm::default(),
            spinner_index: 0,
            status_message: None,
            should_quit: false,
        }
    }

    fn active_agent(&self) -> Option<&AgentState> {
        self.current_agent.and_then(|index| self.agents.get(index))
    }

    fn active_agent_mut(&mut self) -> Option<&mut AgentState> {
        self.current_agent
            .and_then(move |index| self.agents.get_mut(index))
    }

    fn add_agent_selected(&self) -> bool {
        self.current_tab == AppTab::Chat && self.chat_sidebar_index >= self.agents.len()
    }

    fn sidebar_labels(&self) -> Vec<String> {
        match self.current_tab {
            AppTab::Chat => {
                let mut items = self
                    .agents
                    .iter()
                    .map(|agent| {
                        format!(
                            "{}  {}",
                            agent.definition.name,
                            compact_home(&agent.definition.workspace)
                        )
                    })
                    .collect::<Vec<_>>();
                items.push("+ Add agent".to_string());
                items
            }
            AppTab::Workspace => self
                .active_agent()
                .map(|agent| {
                    agent
                        .workspace
                        .entries
                        .iter()
                        .map(|entry| entry.label.clone())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default(),
            AppTab::GitDiff => self
                .active_agent()
                .map(|agent| {
                    agent
                        .git_diff
                        .entries
                        .iter()
                        .map(|entry| entry.label.clone())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default(),
        }
    }

    fn on_tick(&mut self) {
        self.spinner_index = (self.spinner_index + 1) % SPINNER.len();
    }

    fn previous_tab(&mut self) {
        self.current_tab = match self.current_tab {
            AppTab::Chat => AppTab::GitDiff,
            AppTab::Workspace => AppTab::Chat,
            AppTab::GitDiff => AppTab::Workspace,
        };
        self.refresh_current_tab();
    }

    fn next_tab(&mut self) {
        self.current_tab = match self.current_tab {
            AppTab::Chat => AppTab::Workspace,
            AppTab::Workspace => AppTab::GitDiff,
            AppTab::GitDiff => AppTab::Chat,
        };
        self.refresh_current_tab();
    }

    fn set_tab(&mut self, tab: AppTab) {
        if self.current_tab != tab {
            self.current_tab = tab;
            self.refresh_current_tab();
        }
    }

    fn refresh_current_tab(&mut self) {
        match self.current_tab {
            AppTab::Chat => {}
            AppTab::Workspace => {
                if let Some(agent) = self.active_agent_mut() {
                    agent.workspace.refresh(&agent.definition.workspace);
                }
            }
            AppTab::GitDiff => {
                if let Some(agent) = self.active_agent_mut() {
                    agent.git_diff.refresh(&agent.definition.workspace);
                }
            }
        }
    }

    fn move_selection_up(&mut self) {
        match self.current_tab {
            AppTab::Chat => {
                self.select_chat_sidebar_index(self.chat_sidebar_index.saturating_sub(1));
            }
            AppTab::Workspace => {
                if let Some(agent) = self.active_agent_mut() {
                    agent.workspace.move_up();
                }
            }
            AppTab::GitDiff => {
                if let Some(agent) = self.active_agent_mut() {
                    let root = agent.definition.workspace.clone();
                    agent.git_diff.move_up(&root);
                }
            }
        }
    }

    fn move_selection_down(&mut self) {
        match self.current_tab {
            AppTab::Chat => {
                let max_index = self.agents.len();
                self.select_chat_sidebar_index((self.chat_sidebar_index + 1).min(max_index));
            }
            AppTab::Workspace => {
                if let Some(agent) = self.active_agent_mut() {
                    agent.workspace.move_down();
                }
            }
            AppTab::GitDiff => {
                if let Some(agent) = self.active_agent_mut() {
                    let root = agent.definition.workspace.clone();
                    agent.git_diff.move_down(&root);
                }
            }
        }
    }

    fn scroll_content_up(&mut self) {
        match self.current_tab {
            AppTab::Workspace => {
                if let Some(agent) = self.active_agent_mut() {
                    agent.workspace.scroll_up(CONTENT_SCROLL_STEP);
                }
            }
            AppTab::GitDiff => {
                if let Some(agent) = self.active_agent_mut() {
                    agent.git_diff.scroll_up(CONTENT_SCROLL_STEP);
                }
            }
            AppTab::Chat => {}
        }
    }

    fn handle_mouse(&mut self, mouse: MouseEvent, area: Rect) {
        let layout = self.compute_layout(area);

        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) | MouseEventKind::Up(MouseButton::Left) => {
                self.handle_left_click(mouse.column, mouse.row, layout);
            }
            MouseEventKind::ScrollUp => self.handle_scroll(mouse.column, mouse.row, layout, true),
            MouseEventKind::ScrollDown => {
                self.handle_scroll(mouse.column, mouse.row, layout, false)
            }
            _ => {}
        }
    }

    fn scroll_content_down(&mut self) {
        match self.current_tab {
            AppTab::Workspace => {
                if let Some(agent) = self.active_agent_mut() {
                    agent.workspace.scroll_down(CONTENT_SCROLL_STEP);
                }
            }
            AppTab::GitDiff => {
                if let Some(agent) = self.active_agent_mut() {
                    agent.git_diff.scroll_down(CONTENT_SCROLL_STEP);
                }
            }
            AppTab::Chat => {}
        }
    }

    fn handle_key(
        &mut self,
        key: KeyEvent,
        codex: &CodexAppServer,
        ui_tx: &mpsc::UnboundedSender<UiEvent>,
    ) {
        match key.code {
            KeyCode::Char('q') if key.modifiers.is_empty() => self.should_quit = true,
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.should_quit = true;
            }
            KeyCode::Left => self.previous_tab(),
            KeyCode::Right => self.next_tab(),
            KeyCode::Up => self.move_selection_up(),
            KeyCode::Down => self.move_selection_down(),
            KeyCode::PageUp => self.scroll_content_up(),
            KeyCode::PageDown => self.scroll_content_down(),
            KeyCode::F(5) => self.refresh_current_tab(),
            KeyCode::Esc if self.add_agent_selected() => {
                self.add_form = AddAgentForm::default();
                if !self.agents.is_empty() {
                    self.chat_sidebar_index = self.current_agent.unwrap_or(0);
                }
            }
            KeyCode::Tab if self.add_agent_selected() => {
                self.add_form.active_field = match self.add_form.active_field {
                    AddAgentField::Name => AddAgentField::Workspace,
                    AddAgentField::Workspace => AddAgentField::Name,
                };
            }
            KeyCode::Enter if self.add_agent_selected() => self.submit_new_agent(),
            KeyCode::Enter if self.current_tab == AppTab::Chat => {
                self.submit_message(codex.clone(), ui_tx.clone());
            }
            KeyCode::Backspace => self.handle_backspace(),
            KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.handle_text_input(character);
            }
            _ => {}
        }
    }

    fn handle_text_input(&mut self, character: char) {
        if self.add_agent_selected() {
            self.add_form.error = None;
            match self.add_form.active_field {
                AddAgentField::Name => self.add_form.name.push(character),
                AddAgentField::Workspace => self.add_form.workspace.push(character),
            }
        } else if self.current_tab == AppTab::Chat {
            self.chat_input.push(character);
        }
    }

    fn handle_backspace(&mut self) {
        if self.add_agent_selected() {
            match self.add_form.active_field {
                AddAgentField::Name => {
                    self.add_form.name.pop();
                }
                AddAgentField::Workspace => {
                    self.add_form.workspace.pop();
                }
            }
        } else if self.current_tab == AppTab::Chat {
            self.chat_input.pop();
        }
    }

    fn select_chat_sidebar_index(&mut self, index: usize) {
        self.chat_sidebar_index = index.min(self.agents.len());
        if self.chat_sidebar_index < self.agents.len() {
            self.current_agent = Some(self.chat_sidebar_index);
            self.add_form.error = None;
        }
    }

    fn submit_new_agent(&mut self) {
        match validate_agent_input(&self.add_form.name, &self.add_form.workspace) {
            Ok(agent) => {
                self.config.agents.push(agent.clone());
                if let Err(error) = save_config(&self.config_path, &self.config) {
                    self.add_form.error = Some(error.to_string());
                    return;
                }

                self.agents.push(AgentState::new(agent));
                let new_index = self.agents.len().saturating_sub(1);
                self.current_agent = Some(new_index);
                self.chat_sidebar_index = new_index;
                self.add_form = AddAgentForm::default();
                self.status_message = Some("Agent saved to ~/.cmdex.yml".to_string());
            }
            Err(error) => {
                self.add_form.error = Some(error.to_string());
            }
        }
    }

    fn submit_message(&mut self, codex: CodexAppServer, ui_tx: mpsc::UnboundedSender<UiEvent>) {
        let text = self.chat_input.trim().to_string();
        if text.is_empty() {
            return;
        }

        let Some(agent_index) = self.current_agent else {
            self.status_message = Some("Add an agent before sending messages.".to_string());
            return;
        };

        let agent = &mut self.agents[agent_index];
        if agent.thinking {
            self.status_message = Some("Wait for the current response to finish.".to_string());
            return;
        }

        agent.messages.push(ChatMessage {
            role: MessageRole::User,
            text: text.clone(),
            item_id: None,
        });
        agent.thinking = true;
        agent.status = None;
        let existing_thread = agent.thread_id.clone();
        let workspace = agent.definition.workspace.clone();
        self.chat_input.clear();

        tokio::spawn(async move {
            let thread_id = match existing_thread {
                Some(thread_id) => thread_id,
                None => match codex.start_thread(&workspace).await {
                    Ok(thread) => {
                        let id = thread.id.clone();
                        let _ = ui_tx.send(UiEvent::ThreadReady {
                            agent_index,
                            thread,
                        });
                        id
                    }
                    Err(error) => {
                        let _ = ui_tx.send(UiEvent::SubmissionFailed {
                            agent_index,
                            message: error.to_string(),
                        });
                        return;
                    }
                },
            };

            if let Err(error) = codex.start_turn(&thread_id, &text).await {
                let _ = ui_tx.send(UiEvent::SubmissionFailed {
                    agent_index,
                    message: error.to_string(),
                });
            }
        });
    }

    fn handle_server_event(&mut self, event: ServerEvent) {
        match event {
            ServerEvent::ThreadStatusChanged { thread_id, active } => {
                if let Some(agent) = self.find_agent_by_thread_mut(&thread_id) {
                    agent.thinking = active;
                }
            }
            ServerEvent::ItemStarted { thread_id, item } => {
                if let Some(agent) = self.find_agent_by_thread_mut(&thread_id) {
                    if let ThreadItem::AgentMessage { id, text } = item {
                        agent.streaming_item_id = Some(id.clone());
                        agent.messages.push(ChatMessage {
                            role: MessageRole::Assistant,
                            text,
                            item_id: Some(id),
                        });
                    }
                }
            }
            ServerEvent::ItemCompleted { thread_id, item } => {
                if let Some(agent) = self.find_agent_by_thread_mut(&thread_id) {
                    match item {
                        ThreadItem::AgentMessage { id, text } => {
                            upsert_message(&mut agent.messages, MessageRole::Assistant, &id, text);
                            agent.streaming_item_id = None;
                        }
                        ThreadItem::UserMessage => {}
                        ThreadItem::Other => {}
                    }
                }
            }
            ServerEvent::AgentMessageDelta {
                thread_id,
                item_id,
                delta,
            } => {
                if let Some(agent) = self.find_agent_by_thread_mut(&thread_id) {
                    if let Some(message) = agent
                        .messages
                        .iter_mut()
                        .find(|message| message.item_id.as_deref() == Some(item_id.as_str()))
                    {
                        message.text.push_str(&delta);
                    } else {
                        agent.messages.push(ChatMessage {
                            role: MessageRole::Assistant,
                            text: delta,
                            item_id: Some(item_id),
                        });
                    }
                }
            }
            ServerEvent::TurnCompleted { thread_id } => {
                if let Some(agent) = self.find_agent_by_thread_mut(&thread_id) {
                    agent.thinking = false;
                    agent.streaming_item_id = None;
                }
            }
            ServerEvent::Warning(message)
            | ServerEvent::Error(message)
            | ServerEvent::TransportError(message) => {
                self.status_message = Some(message);
            }
        }
    }

    fn handle_ui_event(&mut self, event: UiEvent) {
        match event {
            UiEvent::ThreadReady {
                agent_index,
                thread,
            } => {
                if let Some(agent) = self.agents.get_mut(agent_index) {
                    agent.thread_id = Some(thread.id);
                }
            }
            UiEvent::SubmissionFailed {
                agent_index,
                message,
            } => {
                if let Some(agent) = self.agents.get_mut(agent_index) {
                    agent.thinking = false;
                    agent.streaming_item_id = None;
                    agent.status = Some(message.clone());
                    agent.messages.push(ChatMessage {
                        role: MessageRole::System,
                        text: message.clone(),
                        item_id: None,
                    });
                }
                self.status_message = Some(message);
            }
        }
    }

    fn find_agent_by_thread_mut(&mut self, thread_id: &str) -> Option<&mut AgentState> {
        self.agents
            .iter_mut()
            .find(|agent| agent.thread_id.as_deref() == Some(thread_id))
    }

    fn selected_tab_index(&self) -> usize {
        match self.current_tab {
            AppTab::Chat => 0,
            AppTab::Workspace => 1,
            AppTab::GitDiff => 2,
        }
    }

    fn compute_layout(&self, area: Rect) -> UiLayout {
        let root = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(SIDEBAR_WIDTH), Constraint::Min(40)])
            .split(area);

        let sidebar = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(LOGO.len() as u16 + 2),
                Constraint::Min(10),
            ])
            .split(root[0]);

        if self.current_tab == AppTab::Chat && !self.add_agent_selected() {
            let main = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3),
                    Constraint::Min(10),
                    Constraint::Length(3),
                ])
                .split(root[1]);

            UiLayout {
                sidebar_list: sidebar[1],
                tabs: main[0],
                body: main[1],
                footer: Some(main[2]),
                add_name: None,
                add_workspace: None,
            }
        } else {
            let main = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(3), Constraint::Min(10)])
                .split(root[1]);

            if self.current_tab == AppTab::Chat {
                let outer = Block::default().borders(Borders::ALL).title("New Agent");
                let inner = outer.inner(main[1]);
                let fields = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Length(3),
                        Constraint::Length(3),
                        Constraint::Length(3),
                        Constraint::Min(3),
                    ])
                    .margin(1)
                    .split(inner);

                UiLayout {
                    sidebar_list: sidebar[1],
                    tabs: main[0],
                    body: main[1],
                    footer: None,
                    add_name: Some(fields[1]),
                    add_workspace: Some(fields[2]),
                }
            } else {
                UiLayout {
                    sidebar_list: sidebar[1],
                    tabs: main[0],
                    body: main[1],
                    footer: None,
                    add_name: None,
                    add_workspace: None,
                }
            }
        }
    }

    fn handle_left_click(&mut self, column: u16, row: u16, layout: UiLayout) {
        if rect_contains(layout.tabs, column, row) {
            if let Some(tab) = tab_from_click(layout.tabs, column, row) {
                self.set_tab(tab);
            }
            return;
        }

        if rect_contains(layout.sidebar_list, column, row) {
            self.handle_sidebar_click(column, row, layout.sidebar_list);
            return;
        }

        if self.add_agent_selected() {
            if layout
                .add_name
                .is_some_and(|rect| rect_contains(rect, column, row))
            {
                self.add_form.active_field = AddAgentField::Name;
                return;
            }
            if layout
                .add_workspace
                .is_some_and(|rect| rect_contains(rect, column, row))
            {
                self.add_form.active_field = AddAgentField::Workspace;
            }
        }
    }

    fn handle_scroll(&mut self, column: u16, row: u16, layout: UiLayout, up: bool) {
        if rect_contains(layout.sidebar_list, column, row) {
            if up {
                self.move_selection_up();
            } else {
                self.move_selection_down();
            }
            return;
        }

        if rect_contains(layout.body, column, row)
            || layout
                .footer
                .is_some_and(|rect| rect_contains(rect, column, row))
        {
            if up {
                self.scroll_content_up();
            } else {
                self.scroll_content_down();
            }
        }
    }

    fn handle_sidebar_click(&mut self, column: u16, row: u16, sidebar_list: Rect) {
        let inner = inner_rect(sidebar_list);
        if inner.height == 0 || !rect_contains(inner, column, row) {
            return;
        }

        let visible_row = row.saturating_sub(inner.y) as usize;
        match self.current_tab {
            AppTab::Chat => {
                let total = self.agents.len() + 1;
                let offset = list_offset(self.chat_sidebar_index, total, inner.height as usize);
                let index = (offset + visible_row).min(total.saturating_sub(1));
                self.select_chat_sidebar_index(index);
            }
            AppTab::Workspace => {
                if let Some(agent) = self.active_agent_mut() {
                    let total = agent.workspace.entries.len();
                    if total == 0 {
                        return;
                    }
                    let offset =
                        list_offset(agent.workspace.selected, total, inner.height as usize);
                    let index = (offset + visible_row).min(total.saturating_sub(1));
                    agent.workspace.select(index);
                }
            }
            AppTab::GitDiff => {
                if let Some(agent) = self.active_agent_mut() {
                    let total = agent.git_diff.entries.len();
                    if total == 0 {
                        return;
                    }
                    let offset = list_offset(agent.git_diff.selected, total, inner.height as usize);
                    let index = (offset + visible_row).min(total.saturating_sub(1));
                    let root = agent.definition.workspace.clone();
                    agent.git_diff.select(&root, index);
                }
            }
        }
    }
}

fn upsert_message(messages: &mut Vec<ChatMessage>, role: MessageRole, item_id: &str, text: String) {
    if let Some(message) = messages
        .iter_mut()
        .find(|message| message.item_id.as_deref() == Some(item_id))
    {
        message.text = text;
    } else {
        messages.push(ChatMessage {
            role,
            text,
            item_id: Some(item_id.to_string()),
        });
    }
}

pub async fn run(terminal: &mut DefaultTerminal) -> Result<()> {
    let config_path = default_config_path()?;
    let config = load_config(&config_path)?;

    let (server_tx, mut server_rx) = mpsc::unbounded_channel();
    let (ui_tx, mut ui_rx) = mpsc::unbounded_channel();
    let codex = CodexAppServer::spawn(server_tx).await?;
    let mut app = App::new(config_path, config);

    let mut events = EventStream::new();
    let mut ticker = interval(Duration::from_millis(120));
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);

    app.refresh_current_tab();

    loop {
        terminal.draw(|frame| draw(frame, &app))?;

        if app.should_quit {
            break;
        }

        tokio::select! {
            maybe_event = events.next() => {
                match maybe_event {
                    Some(Ok(Event::Key(key))) if key.kind == KeyEventKind::Press => {
                        app.handle_key(key, &codex, &ui_tx);
                    }
                    Some(Ok(Event::Mouse(mouse))) => {
                        let (width, height) = terminal_size()?;
                        app.handle_mouse(mouse, Rect::new(0, 0, width, height));
                    }
                    Some(Ok(Event::Paste(text))) => {
                        for character in text.chars() {
                            app.handle_text_input(character);
                        }
                    }
                    Some(Ok(_)) => {}
                    Some(Err(error)) => {
                        app.status_message = Some(error.to_string());
                    }
                    None => break,
                }
            }
            Some(server_event) = server_rx.recv() => {
                app.handle_server_event(server_event);
            }
            Some(ui_event) = ui_rx.recv() => {
                app.handle_ui_event(ui_event);
            }
            _ = ticker.tick() => {
                app.on_tick();
            }
        }
    }

    Ok(())
}

fn draw(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let root = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(SIDEBAR_WIDTH), Constraint::Min(40)])
        .split(area);

    draw_sidebar(frame, app, root[0]);
    draw_main(frame, app, root[1]);
}

fn draw_sidebar(frame: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(LOGO.len() as u16 + 2),
            Constraint::Min(10),
        ])
        .split(area);

    let logo = Paragraph::new(LOGO.join("\n"))
        .block(Block::default().borders(Borders::ALL).title("Cmdex"))
        .style(Style::default().fg(Color::Cyan));
    frame.render_widget(logo, chunks[0]);

    let title = match app.current_tab {
        AppTab::Chat => "Agents",
        AppTab::Workspace => "Workspace",
        AppTab::GitDiff => "Git Diff",
    };

    let items = app
        .sidebar_labels()
        .into_iter()
        .map(ListItem::new)
        .collect::<Vec<_>>();

    let mut state = ListState::default();
    match app.current_tab {
        AppTab::Chat => state.select(Some(
            app.chat_sidebar_index.min(items.len().saturating_sub(1)),
        )),
        AppTab::Workspace => state.select(
            app.active_agent()
                .map(|agent| agent.workspace.selected.min(items.len().saturating_sub(1))),
        ),
        AppTab::GitDiff => state.select(
            app.active_agent()
                .map(|agent| agent.git_diff.selected.min(items.len().saturating_sub(1))),
        ),
    }

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(title))
        .highlight_style(
            Style::default()
                .fg(Color::Black)
                .bg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("› ");
    frame.render_stateful_widget(list, chunks[1], &mut state);
}

fn rect_contains(rect: Rect, column: u16, row: u16) -> bool {
    column >= rect.x
        && column < rect.x.saturating_add(rect.width)
        && row >= rect.y
        && row < rect.y.saturating_add(rect.height)
}

fn inner_rect(rect: Rect) -> Rect {
    let width = rect.width.saturating_sub(2);
    let height = rect.height.saturating_sub(2);
    Rect::new(
        rect.x.saturating_add(1),
        rect.y.saturating_add(1),
        width,
        height,
    )
}

fn list_offset(selected: usize, len: usize, visible_rows: usize) -> usize {
    if len <= visible_rows || visible_rows == 0 {
        0
    } else {
        selected.saturating_add(1).saturating_sub(visible_rows)
    }
}

fn tab_from_click(tabs: Rect, column: u16, row: u16) -> Option<AppTab> {
    let inner = inner_rect(tabs);
    if inner.width == 0 || inner.height == 0 {
        return None;
    }

    let mut x = inner.x;
    for (label, tab) in TAB_LABELS {
        let label_width = label.chars().count() as u16;
        let clickable = Rect::new(
            x,
            inner.y,
            label_width
                .saturating_add(1)
                .min(inner.x.saturating_add(inner.width).saturating_sub(x)),
            1,
        );
        if rect_contains(clickable, column, row) {
            return Some(tab);
        }

        x = x.saturating_add(label_width).saturating_add(3);
        if x >= inner.x.saturating_add(inner.width) {
            break;
        }
    }

    None
}

fn draw_main(frame: &mut Frame, app: &App, area: Rect) {
    let tabs = Tabs::new(TAB_LABELS.map(|(label, _)| label))
        .select(app.selected_tab_index())
        .block(Block::default().borders(Borders::ALL))
        .highlight_style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        );

    if app.current_tab == AppTab::Chat && !app.add_agent_selected() {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(10),
                Constraint::Length(3),
            ])
            .split(area);
        frame.render_widget(tabs, chunks[0]);
        draw_chat(frame, app, chunks[1]);
        draw_chat_input(frame, app, chunks[2]);
    } else {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(10)])
            .split(area);
        frame.render_widget(tabs, chunks[0]);

        match app.current_tab {
            AppTab::Chat => draw_add_agent_form(frame, app, chunks[1]),
            AppTab::Workspace => draw_workspace(frame, app, chunks[1]),
            AppTab::GitDiff => draw_git_diff(frame, app, chunks[1]),
        }
    }
}

fn draw_chat(frame: &mut Frame, app: &App, area: Rect) {
    let Some(agent) = app.active_agent() else {
        let empty = Paragraph::new("Add an agent from the sidebar to start chatting.")
            .block(Block::default().borders(Borders::ALL).title("Chat"));
        frame.render_widget(empty, area);
        return;
    };

    let items = if agent.messages.is_empty() {
        vec![ListItem::new(Line::from("No messages yet."))]
    } else {
        agent
            .messages
            .iter()
            .map(|message| {
                let role = match message.role {
                    MessageRole::User => ("You", Color::Yellow),
                    MessageRole::Assistant => (agent.definition.name.as_str(), Color::Green),
                    MessageRole::System => ("System", Color::Red),
                };
                let lines = message
                    .text
                    .lines()
                    .map(|line| Line::from(line.to_string()))
                    .collect::<Vec<_>>();
                let mut block_lines = vec![Line::from(vec![Span::styled(
                    format!("{}:", role.0),
                    Style::default().fg(role.1).add_modifier(Modifier::BOLD),
                )])];
                if lines.is_empty() {
                    block_lines.push(Line::from(String::new()));
                } else {
                    block_lines.extend(lines);
                }
                ListItem::new(block_lines)
            })
            .collect::<Vec<_>>()
    };

    let mut state = ListState::default();
    if !items.is_empty() {
        state.select(Some(items.len().saturating_sub(1)));
    }

    let title = if let Some(status) = &agent.status {
        format!("Chat - {} ({status})", agent.definition.name)
    } else {
        format!("Chat - {}", agent.definition.name)
    };

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(title))
        .highlight_style(Style::default());
    frame.render_stateful_widget(list, area, &mut state);
}

fn draw_chat_input(frame: &mut Frame, app: &App, area: Rect) {
    let thinking = app.active_agent().is_some_and(|agent| agent.thinking);
    let title = if thinking {
        format!("Message  {} Thinking...", SPINNER[app.spinner_index])
    } else {
        "Message".to_string()
    };

    let input = Paragraph::new(app.chat_input.as_str())
        .block(Block::default().borders(Borders::ALL).title(title))
        .wrap(Wrap { trim: false });
    frame.render_widget(input, area);

    let x = area
        .x
        .saturating_add(1 + app.chat_input.chars().count() as u16)
        .min(area.x + area.width.saturating_sub(2));
    let y = area.y + 1;
    frame.set_cursor_position((x, y));
}

fn draw_add_agent_form(frame: &mut Frame, app: &App, area: Rect) {
    frame.render_widget(Clear, area);
    let outer = Block::default().borders(Borders::ALL).title("New Agent");
    let inner = outer.inner(area);
    frame.render_widget(outer, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(3),
        ])
        .margin(1)
        .split(inner);

    let help = Paragraph::new("Use Tab to switch fields and Enter to save the agent.")
        .style(Style::default().fg(Color::DarkGray));
    frame.render_widget(help, chunks[0]);

    let name_title = if app.add_form.active_field == AddAgentField::Name {
        "Name *"
    } else {
        "Name"
    };
    let workspace_title = if app.add_form.active_field == AddAgentField::Workspace {
        "Workspace *"
    } else {
        "Workspace"
    };

    let name = Paragraph::new(app.add_form.name.as_str())
        .block(Block::default().borders(Borders::ALL).title(name_title));
    let workspace = Paragraph::new(app.add_form.workspace.as_str()).block(
        Block::default()
            .borders(Borders::ALL)
            .title(workspace_title),
    );
    frame.render_widget(name, chunks[1]);
    frame.render_widget(workspace, chunks[2]);

    let status = app
        .add_form
        .error
        .clone()
        .unwrap_or_else(|| "Saved agents live in ~/.cmdex.yml".to_string());
    let status =
        Paragraph::new(status).style(Style::default().fg(if app.add_form.error.is_some() {
            Color::Red
        } else {
            Color::DarkGray
        }));
    frame.render_widget(status, chunks[3]);

    let target = match app.add_form.active_field {
        AddAgentField::Name => chunks[1],
        AddAgentField::Workspace => chunks[2],
    };
    let content = match app.add_form.active_field {
        AddAgentField::Name => &app.add_form.name,
        AddAgentField::Workspace => &app.add_form.workspace,
    };
    let cursor_x = target
        .x
        .saturating_add(1 + content.chars().count() as u16)
        .min(target.x + target.width.saturating_sub(2));
    frame.set_cursor_position((cursor_x, target.y + 1));
}

fn draw_workspace(frame: &mut Frame, app: &App, area: Rect) {
    let Some(agent) = app.active_agent() else {
        let empty = Paragraph::new("Select or create an agent in the Chat tab.")
            .block(Block::default().borders(Borders::ALL).title("Workspace"));
        frame.render_widget(empty, area);
        return;
    };

    let mut lines = vec![Line::from(format!(
        "Workspace: {}",
        compact_home(&agent.definition.workspace)
    ))];
    lines.push(Line::from(String::new()));
    lines.push(Line::from(agent.workspace.preview.clone()));
    if let Some(error) = &agent.workspace.error {
        lines.push(Line::from(String::new()));
        lines.push(Line::from(Span::styled(
            error.clone(),
            Style::default().fg(Color::Red),
        )));
    }

    let widget = Paragraph::new(Text::from(lines))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(agent.workspace.preview_title.clone()),
        )
        .scroll((agent.workspace.content_scroll, 0))
        .wrap(Wrap { trim: false });
    frame.render_widget(widget, area);
}

fn draw_git_diff(frame: &mut Frame, app: &App, area: Rect) {
    let Some(agent) = app.active_agent() else {
        let empty = Paragraph::new("Select or create an agent in the Chat tab.")
            .block(Block::default().borders(Borders::ALL).title("Git Diff"));
        frame.render_widget(empty, area);
        return;
    };

    let mut lines = vec![Line::from(format!(
        "Workspace: {}",
        compact_home(&agent.definition.workspace)
    ))];
    lines.push(Line::from(String::new()));
    lines.push(Line::from(agent.git_diff.preview.clone()));
    if let Some(error) = &agent.git_diff.error {
        lines.push(Line::from(String::new()));
        lines.push(Line::from(Span::styled(
            error.clone(),
            Style::default().fg(Color::Red),
        )));
    }

    let widget = Paragraph::new(Text::from(lines))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(agent.git_diff.preview_title.clone()),
        )
        .scroll((agent.git_diff.content_scroll, 0))
        .wrap(Wrap { trim: false });
    frame.render_widget(widget, area);
}
