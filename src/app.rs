use std::{env, fs, path::PathBuf, time::Duration};

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
    widgets::{
        Block, Borders, Clear, List, ListItem, ListState, Paragraph, Scrollbar,
        ScrollbarOrientation, ScrollbarState, Tabs, Wrap,
    },
};
use tokio::{
    sync::mpsc,
    time::{MissedTickBehavior, interval},
};

use crate::codex::{
    CodexAppServer, HistoryEntryKind, ServerEvent, ThreadInfo, ThreadItem, WorkspaceSession,
};
use crate::config::{
    AgentDefinition, CmdexConfig, compact_home, default_config_path, load_config, save_config,
    validate_agent_input,
};
use crate::theme::app_theme;
use crate::workspace::{
    DiffBrowserState, DiffSection, EditorMode, FileBrowserState, WorkspaceEditorState,
    WorkspaceSidebarTab,
};
use tokio::process::Command;

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
const SPINNER: [&str; 8] = ["⠏", "⠛", "⠹", "⢸", "⣰", "⣤", "⣆", "⡇"];
const CONTENT_SCROLL_STEP: u16 = 4;
const MOUSE_SCROLL_STEP: u16 = 4;
const SHELL_OUTPUT_LIMIT: usize = 64_000;

#[derive(Debug, Clone)]
enum UiEvent {
    ThreadReady {
        agent_index: usize,
        thread: ThreadInfo,
    },
    SessionLoaded {
        agent_index: usize,
        session: Option<WorkspaceSession>,
    },
    SubmissionFailed {
        agent_index: usize,
        message: String,
    },
    ShellCompleted {
        agent_index: usize,
        output: String,
        success: bool,
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
    Event,
    System,
    Shell,
}

#[derive(Debug, Clone)]
struct AgentState {
    definition: AgentDefinition,
    thread_id: Option<String>,
    thread_loaded: bool,
    messages: Vec<ChatMessage>,
    chat_follow_output: bool,
    chat_scroll: u16,
    thinking: bool,
    shell_running: bool,
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

#[derive(Debug, Clone, Copy)]
struct GitDiffLayout {
    sections: Rect,
    preview: Rect,
    commit_input: Rect,
    stage_button: Rect,
    discard_button: Rect,
    push_button: Rect,
    pull_button: Rect,
    status: Rect,
}

#[derive(Debug, Clone, Copy)]
struct WorkspaceSidebarLayout {
    tabs: Rect,
    input: Option<Rect>,
    content: Rect,
}

impl AgentState {
    fn new(definition: AgentDefinition) -> Self {
        Self {
            definition,
            thread_id: None,
            thread_loaded: false,
            messages: Vec::new(),
            chat_follow_output: true,
            chat_scroll: 0,
            thinking: false,
            shell_running: false,
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
    chat_model_label: String,
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
            current_agent.map(|index| index + 1).unwrap_or(0)
        };

        Self {
            config_path,
            config,
            agents,
            chat_model_label: load_codex_chat_model_label()
                .unwrap_or_else(|| "default".to_string()),
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
        self.current_tab == AppTab::Chat && self.chat_sidebar_index == 0
    }

    fn sidebar_labels(&self) -> Vec<String> {
        match self.current_tab {
            AppTab::Chat => {
                let mut items = vec!["+ Add agent".to_string()];
                items.extend(self.agents.iter().map(|agent| {
                    format!(
                        "{}  {}",
                        agent.definition.name,
                        compact_home(&agent.definition.workspace)
                    )
                }));
                items
            }
            AppTab::Workspace => self
                .active_agent()
                .map(|agent| agent.workspace.sidebar_labels())
                .unwrap_or_default(),
            AppTab::GitDiff => self
                .active_agent()
                .map(|agent| {
                    agent
                        .git_diff
                        .visible_entries()
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
                    if agent.workspace.sidebar_tab == WorkspaceSidebarTab::Search {
                        agent.workspace.search_move_up();
                    } else {
                        agent.workspace.move_up();
                    }
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
                    if agent.workspace.sidebar_tab == WorkspaceSidebarTab::Search {
                        agent.workspace.search_move_down();
                    } else {
                        agent.workspace.move_down();
                    }
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

    fn scroll_content_up(&mut self, area: Rect) {
        self.scroll_content_up_by(area, CONTENT_SCROLL_STEP);
    }

    fn scroll_content_up_by(&mut self, _area: Rect, lines: u16) {
        match self.current_tab {
            AppTab::Workspace => {
                if let Some(agent) = self.active_agent_mut() {
                    if let Some(editor) = agent.workspace.editor.as_mut() {
                        editor.scroll_up(lines);
                        return;
                    }
                }
                if let Some(agent) = self.active_agent_mut() {
                    agent.workspace.scroll_up(lines);
                }
            }
            AppTab::GitDiff => {
                if let Some(agent) = self.active_agent_mut() {
                    agent.git_diff.scroll_up(lines);
                }
            }
            AppTab::Chat => {}
        }
    }

    fn handle_mouse(&mut self, mouse: MouseEvent, area: Rect) {
        let layout = self.compute_layout(area);

        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                self.handle_left_click(mouse.column, mouse.row, layout);
            }
            MouseEventKind::ScrollUp => self.handle_scroll(mouse.column, mouse.row, layout, true),
            MouseEventKind::ScrollDown => {
                self.handle_scroll(mouse.column, mouse.row, layout, false)
            }
            _ => {}
        }
    }

    fn scroll_content_down(&mut self, area: Rect) {
        self.scroll_content_down_by(area, CONTENT_SCROLL_STEP);
    }

    fn scroll_content_down_by(&mut self, area: Rect, lines: u16) {
        match self.current_tab {
            AppTab::Workspace => {
                if let Some(agent) = self.active_agent_mut() {
                    if let Some(editor) = agent.workspace.editor.as_mut() {
                        let viewport = workspace_editor_viewport(area);
                        editor.scroll_down(lines, viewport.height);
                        return;
                    }
                }
                if let Some(agent) = self.active_agent_mut() {
                    agent.workspace.scroll_down(lines);
                }
            }
            AppTab::GitDiff => {
                if let Some(agent) = self.active_agent_mut() {
                    agent.git_diff.scroll_down(lines);
                }
            }
            AppTab::Chat => {}
        }
    }

    fn scroll_chat_up(&mut self, area: Rect, lines: u16) {
        let Some(agent) = self.active_agent_mut() else {
            return;
        };

        let max_scroll = chat_max_scroll(agent, area);
        let current = if agent.chat_follow_output {
            max_scroll
        } else {
            agent.chat_scroll.min(max_scroll)
        };
        let next = current.saturating_sub(lines);

        agent.chat_scroll = next;
        agent.chat_follow_output = next >= max_scroll;
    }

    fn scroll_chat_down(&mut self, area: Rect, lines: u16) {
        let Some(agent) = self.active_agent_mut() else {
            return;
        };

        let max_scroll = chat_max_scroll(agent, area);
        let current = if agent.chat_follow_output {
            max_scroll
        } else {
            agent.chat_scroll.min(max_scroll)
        };
        let next = current.saturating_add(lines).min(max_scroll);

        agent.chat_scroll = next;
        agent.chat_follow_output = next >= max_scroll;
    }

    fn handle_key(
        &mut self,
        key: KeyEvent,
        codex: &CodexAppServer,
        ui_tx: &mpsc::UnboundedSender<UiEvent>,
        area: Rect,
    ) {
        if self.handle_workspace_key(key, area) {
            return;
        }
        if self.handle_git_diff_key(key) {
            return;
        }

        match key.code {
            KeyCode::Char('q') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.should_quit = true;
            }
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.should_quit = true;
            }
            KeyCode::Left if key.modifiers.contains(KeyModifiers::CONTROL) => self.previous_tab(),
            KeyCode::Right if key.modifiers.contains(KeyModifiers::CONTROL) => self.next_tab(),
            KeyCode::Enter
                if self.current_tab == AppTab::Chat
                    && !self.add_agent_selected()
                    && key.modifiers.contains(KeyModifiers::SHIFT) =>
            {
                self.chat_input.push('\n');
            }
            KeyCode::Up => self.move_selection_up(),
            KeyCode::Down => self.move_selection_down(),
            KeyCode::PageUp => {
                if self.current_tab == AppTab::Chat && !self.add_agent_selected() {
                    self.scroll_chat_up(self.compute_layout(area).body, CONTENT_SCROLL_STEP);
                } else {
                    self.scroll_content_up(self.compute_layout(area).body);
                }
            }
            KeyCode::PageDown => {
                if self.current_tab == AppTab::Chat && !self.add_agent_selected() {
                    self.scroll_chat_down(self.compute_layout(area).body, CONTENT_SCROLL_STEP);
                } else {
                    self.scroll_content_down(self.compute_layout(area).body);
                }
            }
            KeyCode::F(5) => self.refresh_current_tab(),
            KeyCode::Esc if self.add_agent_selected() => {
                self.add_form = AddAgentForm::default();
                if !self.agents.is_empty() {
                    self.chat_sidebar_index =
                        self.current_agent.map(|index| index + 1).unwrap_or(0);
                }
            }
            KeyCode::Tab if self.add_agent_selected() => {
                self.add_form.active_field = match self.add_form.active_field {
                    AddAgentField::Name => AddAgentField::Workspace,
                    AddAgentField::Workspace => AddAgentField::Name,
                };
            }
            KeyCode::Enter if self.add_agent_selected() => {
                self.submit_new_agent(codex.clone(), ui_tx.clone())
            }
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
        } else if self.current_tab == AppTab::Workspace {
            if let Some(agent) = self.active_agent_mut() {
                if agent.workspace.sidebar_tab == WorkspaceSidebarTab::Search {
                    agent.workspace.push_search_char(character);
                    return;
                }
                if let Some(editor) = agent.workspace.editor.as_mut() {
                    match editor.mode {
                        EditorMode::Insert => editor.insert_char(character),
                        EditorMode::Command => editor.command.push(character),
                        EditorMode::Normal => {}
                    }
                }
            }
        } else if self.current_tab == AppTab::GitDiff {
            if let Some(agent) = self.active_agent_mut() {
                agent.git_diff.commit_message.push(character);
                agent.git_diff.error = None;
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
        } else if self.current_tab == AppTab::Workspace {
            if let Some(agent) = self.active_agent_mut() {
                if agent.workspace.sidebar_tab == WorkspaceSidebarTab::Search {
                    agent.workspace.pop_search_char();
                    return;
                }
                if let Some(editor) = agent.workspace.editor.as_mut() {
                    match editor.mode {
                        EditorMode::Insert => editor.backspace(),
                        EditorMode::Command => {
                            editor.command.pop();
                        }
                        EditorMode::Normal => {}
                    }
                }
            }
        } else if self.current_tab == AppTab::GitDiff {
            if let Some(agent) = self.active_agent_mut() {
                agent.git_diff.commit_message.pop();
                agent.git_diff.error = None;
            }
        } else if self.current_tab == AppTab::Chat {
            self.chat_input.pop();
        }
    }

    fn handle_workspace_key(&mut self, key: KeyEvent, area: Rect) -> bool {
        if self.current_tab != AppTab::Workspace {
            return false;
        }

        let Some(agent_index) = self.current_agent else {
            return false;
        };

        let viewport = workspace_editor_viewport(self.compute_layout(area).body);
        let page_step = usize::from(CONTENT_SCROLL_STEP);
        let mut saved = false;
        let mut close = false;
        let mut handled = false;
        let mut selection_delta = 0i8;

        {
            let agent = &mut self.agents[agent_index];
            let workspace = &mut agent.workspace;

            if workspace.sidebar_tab == WorkspaceSidebarTab::Search {
                match key.code {
                    KeyCode::Esc => {
                        workspace.set_sidebar_tab(WorkspaceSidebarTab::Files);
                        return true;
                    }
                    KeyCode::Up => {
                        workspace.search_move_up();
                        return true;
                    }
                    KeyCode::Down => {
                        workspace.search_move_down();
                        return true;
                    }
                    KeyCode::Enter => {
                        match workspace.open_selected_search_result() {
                            Ok(true) => {
                                if let Some(editor) = workspace.editor.as_mut() {
                                    editor.ensure_visible(viewport.width, viewport.height);
                                }
                            }
                            Ok(false) => {}
                            Err(error) => workspace.error = Some(error.to_string()),
                        }
                        return true;
                    }
                    KeyCode::Backspace => {
                        workspace.pop_search_char();
                        return true;
                    }
                    KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                        workspace.push_search_char(character);
                        return true;
                    }
                    _ => {}
                }
            }

            if workspace.editor.is_none() {
                if key.code == KeyCode::Enter {
                    if workspace.toggle_current_directory() {
                        return true;
                    }
                    match workspace.open_editor() {
                        Ok(()) => {}
                        Err(error) => workspace.error = Some(error.to_string()),
                    }
                    return true;
                }
                return false;
            }

            {
                let editor = workspace.editor.as_mut().expect("editor checked above");
                match editor.mode {
                    EditorMode::Command => match key.code {
                        KeyCode::Esc => {
                            editor.cancel_command();
                            handled = true;
                        }
                        KeyCode::Backspace => {
                            editor.command.pop();
                            handled = true;
                        }
                        KeyCode::Enter => {
                            match editor.execute_command() {
                                Ok(result) => {
                                    saved = result.saved;
                                    close = result.close;
                                }
                                Err(error) => editor.status = Some(error.to_string()),
                            }
                            handled = true;
                        }
                        KeyCode::Char(character)
                            if !key.modifiers.contains(KeyModifiers::CONTROL) =>
                        {
                            editor.command.push(character);
                            handled = true;
                        }
                        _ => handled = true,
                    },
                    EditorMode::Insert => match key.code {
                        KeyCode::Esc => {
                            editor.mode = EditorMode::Normal;
                            handled = true;
                        }
                        KeyCode::Enter => {
                            editor.insert_newline();
                            handled = true;
                        }
                        KeyCode::Backspace => {
                            editor.backspace();
                            handled = true;
                        }
                        KeyCode::Delete => {
                            editor.delete_char();
                            handled = true;
                        }
                        KeyCode::Left => {
                            editor.move_left();
                            handled = true;
                        }
                        KeyCode::Right => {
                            editor.move_right();
                            handled = true;
                        }
                        KeyCode::Up => {
                            editor.move_up();
                            handled = true;
                        }
                        KeyCode::Down => {
                            editor.move_down();
                            handled = true;
                        }
                        KeyCode::Home => {
                            editor.move_line_start();
                            handled = true;
                        }
                        KeyCode::End => {
                            editor.move_line_end();
                            handled = true;
                        }
                        KeyCode::PageUp => {
                            editor.move_page_up(page_step);
                            handled = true;
                        }
                        KeyCode::PageDown => {
                            editor.move_page_down(page_step);
                            handled = true;
                        }
                        KeyCode::Tab => {
                            for _ in 0..4 {
                                editor.insert_char(' ');
                            }
                            handled = true;
                        }
                        _ => {}
                    },
                    EditorMode::Normal => match key.code {
                        KeyCode::Esc => handled = true,
                        KeyCode::Enter => {
                            handled = workspace.toggle_current_directory();
                        }
                        KeyCode::Up => {
                            selection_delta = -1;
                            handled = true;
                        }
                        KeyCode::Down => {
                            selection_delta = 1;
                            handled = true;
                        }
                        KeyCode::Left => {
                            editor.move_left();
                            handled = true;
                        }
                        KeyCode::Right => {
                            editor.move_right();
                            handled = true;
                        }
                        KeyCode::Home => {
                            editor.move_line_start();
                            handled = true;
                        }
                        KeyCode::End => {
                            editor.move_line_end();
                            handled = true;
                        }
                        KeyCode::PageUp => {
                            editor.move_page_up(page_step);
                            handled = true;
                        }
                        KeyCode::PageDown => {
                            editor.move_page_down(page_step);
                            handled = true;
                        }
                        KeyCode::Delete => {
                            editor.delete_char();
                            handled = true;
                        }
                        KeyCode::Char('h') => {
                            editor.move_left();
                            handled = true;
                        }
                        KeyCode::Char('j') => {
                            editor.move_down();
                            handled = true;
                        }
                        KeyCode::Char('k') => {
                            editor.move_up();
                            handled = true;
                        }
                        KeyCode::Char('l') => {
                            editor.move_right();
                            handled = true;
                        }
                        KeyCode::Char('0') => {
                            editor.move_line_start();
                            handled = true;
                        }
                        KeyCode::Char('$') => {
                            editor.move_line_end();
                            handled = true;
                        }
                        KeyCode::Char('i') => {
                            editor.enter_insert_mode();
                            handled = true;
                        }
                        KeyCode::Char('a') => {
                            editor.enter_insert_after();
                            handled = true;
                        }
                        KeyCode::Char('o') => {
                            editor.open_below();
                            handled = true;
                        }
                        KeyCode::Char('x') => {
                            editor.delete_char();
                            handled = true;
                        }
                        KeyCode::Char(':') => {
                            editor.start_command();
                            handled = true;
                        }
                        _ => {}
                    },
                }
            }

            if selection_delta < 0 {
                workspace.move_up();
            } else if selection_delta > 0 {
                workspace.move_down();
            }

            if handled {
                if let Some(editor) = workspace.editor.as_mut() {
                    editor.ensure_visible(viewport.width, viewport.height);
                }
            }
        }

        if saved {
            let root = self.agents[agent_index].definition.workspace.clone();
            self.agents[agent_index].git_diff.refresh(&root);
        }

        if close {
            if let Err(error) = self.agents[agent_index].workspace.close_editor() {
                self.agents[agent_index].workspace.error = Some(error.to_string());
            }
        }

        handled || saved || close
    }

    fn handle_git_diff_key(&mut self, key: KeyEvent) -> bool {
        if self.current_tab != AppTab::GitDiff {
            return false;
        }

        match key.code {
            KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.stage_git_diff_changes();
                true
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.unstage_git_diff_changes();
                true
            }
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.discard_git_diff_changes();
                true
            }
            KeyCode::Left if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                if let Some(agent) = self.active_agent_mut() {
                    let root = agent.definition.workspace.clone();
                    agent
                        .git_diff
                        .set_active_section(&root, DiffSection::Changes);
                }
                true
            }
            KeyCode::Right if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                if let Some(agent) = self.active_agent_mut() {
                    let root = agent.definition.workspace.clone();
                    agent
                        .git_diff
                        .set_active_section(&root, DiffSection::Staged);
                }
                true
            }
            KeyCode::Tab => {
                if let Some(agent) = self.active_agent_mut() {
                    let root = agent.definition.workspace.clone();
                    let next = match agent.git_diff.active_section {
                        DiffSection::Changes => DiffSection::Staged,
                        DiffSection::Staged => DiffSection::Changes,
                    };
                    agent.git_diff.set_active_section(&root, next);
                }
                true
            }
            KeyCode::Enter => {
                self.commit_git_diff_changes();
                true
            }
            _ => false,
        }
    }

    fn commit_git_diff_changes(&mut self) {
        let Some(agent) = self.active_agent_mut() else {
            self.status_message = Some("Add an agent before committing changes.".to_string());
            return;
        };

        let root = agent.definition.workspace.clone();
        match agent.git_diff.commit(&root) {
            Ok(()) => agent.workspace.refresh(&root),
            Err(error) => agent.git_diff.error = Some(error.to_string()),
        }
    }

    fn stage_git_diff_changes(&mut self) {
        let Some(agent) = self.active_agent_mut() else {
            self.status_message = Some("Add an agent before staging changes.".to_string());
            return;
        };

        let root = agent.definition.workspace.clone();
        match agent.git_diff.stage_selected(&root) {
            Ok(()) => agent.workspace.refresh(&root),
            Err(error) => agent.git_diff.error = Some(error.to_string()),
        }
    }

    fn unstage_git_diff_changes(&mut self) {
        let Some(agent) = self.active_agent_mut() else {
            self.status_message = Some("Add an agent before unstaging changes.".to_string());
            return;
        };

        let root = agent.definition.workspace.clone();
        match agent.git_diff.unstage_selected(&root) {
            Ok(()) => agent.workspace.refresh(&root),
            Err(error) => agent.git_diff.error = Some(error.to_string()),
        }
    }

    fn discard_git_diff_changes(&mut self) {
        let Some(agent) = self.active_agent_mut() else {
            self.status_message = Some("Add an agent before discarding changes.".to_string());
            return;
        };

        let root = agent.definition.workspace.clone();
        match agent.git_diff.discard_selected(&root) {
            Ok(()) => agent.workspace.refresh(&root),
            Err(error) => agent.git_diff.error = Some(error.to_string()),
        }
    }

    fn push_git_diff_changes(&mut self) {
        let Some(agent) = self.active_agent_mut() else {
            self.status_message = Some("Add an agent before pushing changes.".to_string());
            return;
        };

        let root = agent.definition.workspace.clone();
        match agent.git_diff.push(&root) {
            Ok(()) => agent.workspace.refresh(&root),
            Err(error) => agent.git_diff.error = Some(error.to_string()),
        }
    }

    fn pull_git_diff_changes(&mut self) {
        let Some(agent) = self.active_agent_mut() else {
            self.status_message = Some("Add an agent before pulling changes.".to_string());
            return;
        };

        let root = agent.definition.workspace.clone();
        match agent.git_diff.pull(&root) {
            Ok(()) => agent.workspace.refresh(&root),
            Err(error) => agent.git_diff.error = Some(error.to_string()),
        }
    }

    fn select_chat_sidebar_index(&mut self, index: usize) {
        self.chat_sidebar_index = index.min(self.agents.len());
        if self.chat_sidebar_index > 0 {
            self.current_agent = Some(self.chat_sidebar_index - 1);
            self.add_form.error = None;
        }
    }

    fn submit_new_agent(&mut self, codex: CodexAppServer, ui_tx: mpsc::UnboundedSender<UiEvent>) {
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
                self.chat_sidebar_index = new_index + 1;
                self.add_form = AddAgentForm::default();
                self.status_message = Some("Agent saved to ~/.cmdex.yml".to_string());
                spawn_session_load(
                    codex,
                    ui_tx,
                    new_index,
                    self.agents[new_index].definition.workspace.clone(),
                );
            }
            Err(error) => {
                self.add_form.error = Some(error.to_string());
            }
        }
    }

    fn submit_message(&mut self, codex: CodexAppServer, ui_tx: mpsc::UnboundedSender<UiEvent>) {
        if let Some(command) = shell_command_from_input(&self.chat_input) {
            self.submit_shell_command(command, ui_tx);
            return;
        }

        let text = self.chat_input.trim().to_string();
        if text.is_empty() {
            return;
        }

        let Some(agent_index) = self.current_agent else {
            self.status_message = Some("Add an agent before sending messages.".to_string());
            return;
        };

        let agent = &mut self.agents[agent_index];
        if agent.thinking || agent.shell_running {
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
        let thread_loaded = agent.thread_loaded;
        let workspace = agent.definition.workspace.clone();
        self.chat_input.clear();

        tokio::spawn(async move {
            let thread_id = match existing_thread {
                Some(thread_id) => {
                    if !thread_loaded {
                        match codex.resume_thread(&thread_id).await {
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
                        }
                    } else {
                        thread_id
                    }
                }
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

    fn submit_shell_command(&mut self, command: String, ui_tx: mpsc::UnboundedSender<UiEvent>) {
        let Some(agent_index) = self.current_agent else {
            self.status_message = Some("Add an agent before running shell commands.".to_string());
            return;
        };

        let agent = &mut self.agents[agent_index];
        if agent.thinking || agent.shell_running {
            self.status_message = Some("Wait for the current response to finish.".to_string());
            return;
        }

        agent.messages.push(ChatMessage {
            role: MessageRole::Shell,
            text: format!("> {command}"),
            item_id: None,
        });
        agent.shell_running = true;
        agent.status = None;
        let workspace = agent.definition.workspace.clone();
        self.chat_input.clear();

        tokio::spawn(async move {
            let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
            let result = Command::new(shell)
                .arg("-c")
                .arg(&command)
                .current_dir(&workspace)
                .output()
                .await;

            let (output, success) = match result {
                Ok(output) => {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    (
                        format_shell_output(
                            &command,
                            &stdout,
                            &stderr,
                            output.status.code(),
                            output.status.success(),
                        ),
                        output.status.success(),
                    )
                }
                Err(error) => (
                    format!(
                        "```text\n{}\n```\n\nExit code: unavailable",
                        truncate_shell_text(&error.to_string())
                    ),
                    false,
                ),
            };

            let _ = ui_tx.send(UiEvent::ShellCompleted {
                agent_index,
                output,
                success,
            });
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
                    agent.thread_loaded = true;
                }
            }
            UiEvent::SessionLoaded {
                agent_index,
                session,
            } => {
                if let (Some(agent), Some(session)) = (self.agents.get_mut(agent_index), session) {
                    if agent.thread_id.is_none() && agent.messages.is_empty() {
                        let (thread_id, messages) = session_messages(session);
                        agent.thread_id = Some(thread_id);
                        agent.thread_loaded = false;
                        agent.messages = messages;
                    }
                }
            }
            UiEvent::SubmissionFailed {
                agent_index,
                message,
            } => {
                if let Some(agent) = self.agents.get_mut(agent_index) {
                    agent.thinking = false;
                    agent.shell_running = false;
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
            UiEvent::ShellCompleted {
                agent_index,
                output,
                success,
            } => {
                if let Some(agent) = self.agents.get_mut(agent_index) {
                    agent.shell_running = false;
                    agent.messages.push(ChatMessage {
                        role: MessageRole::Shell,
                        text: output,
                        item_id: None,
                    });
                    agent.status = Some(if success {
                        "Shell command finished".to_string()
                    } else {
                        "Shell command failed".to_string()
                    });
                    let root = agent.definition.workspace.clone();
                    agent.workspace.refresh(&root);
                    agent.git_diff.refresh(&root);
                }
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
        let frame = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(1)])
            .split(area);

        let root = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(SIDEBAR_WIDTH), Constraint::Min(40)])
            .split(frame[0]);

        let sidebar = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(LOGO.len() as u16 + 2),
                Constraint::Min(10),
            ])
            .split(root[0]);

        if self.current_tab == AppTab::Chat && !self.add_agent_selected() {
            let input_height = chat_input_height_for_main_area(&self.chat_input, root[1]);
            let main = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3),
                    Constraint::Min(10),
                    Constraint::Length(input_height),
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
                let outer = rounded_block().title("New Agent");
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

        if self.current_tab == AppTab::GitDiff
            && self.handle_git_diff_click(column, row, layout.body)
        {
            return;
        }

        if self.current_tab == AppTab::Workspace
            && self
                .active_agent()
                .is_some_and(|agent| agent.workspace.editor.is_some())
        {
            self.handle_workspace_editor_click(column, row, layout.body);
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
            if self.current_tab == AppTab::Chat && !self.add_agent_selected() {
                if up {
                    self.scroll_chat_up(layout.body, MOUSE_SCROLL_STEP);
                } else {
                    self.scroll_chat_down(layout.body, MOUSE_SCROLL_STEP);
                }
            } else if up {
                self.scroll_content_up_by(layout.body, MOUSE_SCROLL_STEP);
            } else {
                self.scroll_content_down_by(layout.body, MOUSE_SCROLL_STEP);
            }
        }
    }

    fn handle_sidebar_click(&mut self, column: u16, row: u16, sidebar_list: Rect) {
        match self.current_tab {
            AppTab::Chat => {
                let inner = inner_rect(sidebar_list);
                if inner.height == 0 || !rect_contains(inner, column, row) {
                    return;
                }

                let visible_row = row.saturating_sub(inner.y) as usize;
                let total = self.agents.len() + 1;
                let offset = list_offset(self.chat_sidebar_index, total, inner.height as usize);
                let index = (offset + visible_row).min(total.saturating_sub(1));
                self.select_chat_sidebar_index(index);
            }
            AppTab::Workspace => {
                if let Some(agent) = self.active_agent_mut() {
                    let layout =
                        workspace_sidebar_layout(sidebar_list, agent.workspace.sidebar_tab);
                    if let Some(tab) = workspace_sidebar_tab_from_click(layout.tabs, column, row) {
                        agent.workspace.set_sidebar_tab(tab);
                        return;
                    }

                    match agent.workspace.sidebar_tab {
                        WorkspaceSidebarTab::Files => {
                            let inner = inner_rect(layout.content);
                            if inner.height == 0 || !rect_contains(inner, column, row) {
                                return;
                            }

                            let visible_row = row.saturating_sub(inner.y) as usize;
                            let total = agent.workspace.sidebar_len();
                            if total == 0 {
                                return;
                            }
                            let offset = list_offset(
                                agent.workspace.sidebar_selected_row(),
                                total,
                                inner.height as usize,
                            );
                            let index = (offset + visible_row).min(total.saturating_sub(1));
                            agent.workspace.select_sidebar_row(index);
                        }
                        WorkspaceSidebarTab::Search => {
                            if layout
                                .input
                                .is_some_and(|input| rect_contains(input, column, row))
                            {
                                return;
                            }

                            let inner = inner_rect(layout.content);
                            if inner.height == 0 || !rect_contains(inner, column, row) {
                                return;
                            }

                            let visible_row = row.saturating_sub(inner.y) as usize;
                            let total = agent.workspace.search_total_rows();
                            if total == 0 {
                                return;
                            }
                            let offset = list_offset(
                                agent.workspace.search_selected_row(),
                                total,
                                inner.height as usize,
                            );
                            let index = (offset + visible_row).min(total.saturating_sub(1));
                            agent.workspace.select_search_row(index);
                            if let Err(error) = agent.workspace.open_selected_search_result() {
                                agent.workspace.error = Some(error.to_string());
                            }
                        }
                    }
                }
            }
            AppTab::GitDiff => {
                let inner = inner_rect(sidebar_list);
                if inner.height == 0 || !rect_contains(inner, column, row) {
                    return;
                }

                let visible_row = row.saturating_sub(inner.y) as usize;
                if let Some(agent) = self.active_agent_mut() {
                    let total = agent.git_diff.visible_entries().len();
                    if total == 0 {
                        return;
                    }
                    let offset = list_offset(
                        agent.git_diff.selected_index(),
                        total,
                        inner.height as usize,
                    );
                    let index = (offset + visible_row).min(total.saturating_sub(1));
                    let root = agent.definition.workspace.clone();
                    agent.git_diff.select(&root, index);
                }
            }
        }
    }

    fn handle_workspace_editor_click(&mut self, column: u16, row: u16, area: Rect) {
        let viewport = workspace_editor_viewport(area);
        if !rect_contains(viewport, column, row) {
            return;
        }

        let Some(agent) = self.active_agent_mut() else {
            return;
        };
        let Some(editor) = agent.workspace.editor.as_mut() else {
            return;
        };

        let gutter_width = editor.gutter_width() as u16;
        let target_row =
            usize::from(row.saturating_sub(viewport.y)) + editor.vertical_scroll as usize;
        let content_x = column.saturating_sub(viewport.x);
        let target_col = if content_x <= gutter_width {
            0
        } else {
            usize::from(content_x.saturating_sub(gutter_width)) + editor.horizontal_scroll as usize
        };

        editor.mode = EditorMode::Normal;
        editor.set_cursor(target_row, target_col);
        editor.ensure_visible(viewport.width, viewport.height);
    }

    fn handle_git_diff_click(&mut self, column: u16, row: u16, area: Rect) -> bool {
        let layout = git_diff_layout(area);
        let Some(agent) = self.active_agent() else {
            return false;
        };

        let changes_label = format!("Changes ({})", agent.git_diff.count(DiffSection::Changes));
        let staged_label = format!("Staged ({})", agent.git_diff.count(DiffSection::Staged));
        let active_section = agent.git_diff.active_section;
        if let Some(section) =
            git_diff_section_from_click(layout.sections, &changes_label, &staged_label, column, row)
        {
            if let Some(agent) = self.active_agent_mut() {
                let root = agent.definition.workspace.clone();
                agent.git_diff.set_active_section(&root, section);
            }
            return true;
        }

        if rect_contains(layout.push_button, column, row) {
            self.push_git_diff_changes();
            return true;
        }

        if rect_contains(layout.stage_button, column, row) {
            match active_section {
                DiffSection::Changes => self.stage_git_diff_changes(),
                DiffSection::Staged => self.unstage_git_diff_changes(),
            }
            return true;
        }

        if rect_contains(layout.discard_button, column, row) {
            self.discard_git_diff_changes();
            return true;
        }

        if rect_contains(layout.pull_button, column, row) {
            self.pull_git_diff_changes();
            return true;
        }

        rect_contains(layout.commit_input, column, row)
            || rect_contains(layout.preview, column, row)
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

fn session_messages(session: WorkspaceSession) -> (String, Vec<ChatMessage>) {
    let messages = session
        .entries
        .into_iter()
        .map(|entry| ChatMessage {
            role: match entry.kind {
                HistoryEntryKind::User => MessageRole::User,
                HistoryEntryKind::Assistant => MessageRole::Assistant,
                HistoryEntryKind::Event => MessageRole::Event,
            },
            text: entry.text,
            item_id: None,
        })
        .collect::<Vec<_>>();

    (session.thread.id, messages)
}

fn spawn_session_load(
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
            let (thread_id, messages) = session_messages(session);
            agent.thread_id = Some(thread_id);
            agent.messages = messages;
        }
    }

    Ok(())
}

pub async fn run(terminal: &mut DefaultTerminal) -> Result<()> {
    let config_path = default_config_path()?;
    let config = load_config(&config_path)?;

    let (server_tx, mut server_rx) = mpsc::unbounded_channel();
    let (ui_tx, mut ui_rx) = mpsc::unbounded_channel();
    let codex = CodexAppServer::spawn(server_tx).await?;
    let mut app = App::new(config_path, config);
    hydrate_latest_sessions(&mut app, &codex).await?;

    let mut events = EventStream::new();
    let mut ticker = interval(Duration::from_millis(80));
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
                        let (width, height) = terminal_size()?;
                        app.handle_key(key, &codex, &ui_tx, Rect::new(0, 0, width, height));
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
    frame.render_widget(Block::default().style(app_background_style()), area);
    let frame_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(area);
    let root = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(SIDEBAR_WIDTH), Constraint::Min(40)])
        .split(frame_layout[0]);

    draw_main(frame, app, root[1]);
    draw_sidebar(frame, app, root[0]);
    draw_help_line(frame, frame_layout[1]);
}

fn rounded_block() -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_set(border::ROUNDED)
        .border_style(Style::default().fg(theme().border))
        .title_style(
            Style::default()
                .fg(theme().yellow)
                .add_modifier(Modifier::BOLD),
        )
}

fn theme() -> &'static crate::theme::AppTheme {
    app_theme()
}

fn app_background_style() -> Style {
    Style::default().bg(theme().app_bg).fg(theme().foreground)
}

fn panel_style() -> Style {
    Style::default().bg(theme().panel_bg).fg(theme().foreground)
}

fn sidebar_style() -> Style {
    Style::default()
        .bg(theme().sidebar_bg)
        .fg(theme().foreground)
}

fn input_style() -> Style {
    Style::default().bg(theme().input_bg).fg(theme().foreground)
}

fn editor_style() -> Style {
    Style::default().bg(theme().app_bg).fg(theme().foreground)
}

fn muted_panel_style() -> Style {
    Style::default().bg(theme().panel_bg).fg(theme().muted)
}

fn selection_style() -> Style {
    Style::default()
        .fg(theme().selection_fg)
        .bg(theme().selection_bg)
        .add_modifier(Modifier::BOLD)
}

fn tab_style() -> Style {
    Style::default().fg(theme().tab_fg).bg(theme().tab_bg)
}

fn tab_highlight_style() -> Style {
    Style::default()
        .fg(theme().accent)
        .bg(theme().tab_bg)
        .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
}

fn sidebar_block() -> Block<'static> {
    rounded_block().style(sidebar_style())
}

fn panel_block() -> Block<'static> {
    rounded_block().style(panel_style())
}

fn input_block() -> Block<'static> {
    rounded_block().style(input_style())
}

fn editor_block() -> Block<'static> {
    rounded_block().style(editor_style())
}

fn action_style(color: ratatui::style::Color) -> Style {
    Style::default().bg(theme().panel_bg).fg(color)
}

fn scrollbar_widget() -> Scrollbar<'static> {
    Scrollbar::new(ScrollbarOrientation::VerticalRight)
        .begin_symbol(None)
        .end_symbol(None)
        .track_symbol(None)
        .thumb_symbol("█")
        .thumb_style(Style::default().fg(theme().scrollbar_thumb))
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
        .block(sidebar_block())
        .style(Style::default().fg(theme().accent).bg(theme().sidebar_bg));
    frame.render_widget(logo, chunks[0]);

    if app.current_tab == AppTab::Workspace {
        draw_workspace_sidebar(frame, app, chunks[1]);
        return;
    }

    let title = match app.current_tab {
        AppTab::Chat => "Agents",
        AppTab::GitDiff => "Git Diff",
        AppTab::Workspace => unreachable!("workspace sidebar is rendered separately"),
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
        AppTab::Workspace => state.select(app.active_agent().map(|agent| {
            agent
                .workspace
                .sidebar_selected_row()
                .min(items.len().saturating_sub(1))
        })),
        AppTab::GitDiff => state.select(app.active_agent().map(|agent| {
            agent
                .git_diff
                .selected_index()
                .min(items.len().saturating_sub(1))
        })),
    }

    let list = List::new(items)
        .block(sidebar_block().title(title))
        .style(sidebar_style())
        .highlight_style(selection_style())
        .highlight_symbol("› ");
    frame.render_stateful_widget(list, chunks[1], &mut state);
}

fn draw_help_line(frame: &mut Frame, area: Rect) {
    let help =
        Paragraph::new("Quit: Ctrl+Q").style(Style::default().bg(theme().app_bg).fg(theme().muted));
    frame.render_widget(help, area);
}

fn draw_workspace_sidebar(frame: &mut Frame, app: &App, area: Rect) {
    let Some(agent) = app.active_agent() else {
        let empty = Paragraph::new("Select or create an agent in the Chat tab.")
            .block(sidebar_block().title("Workspace"))
            .style(sidebar_style());
        frame.render_widget(empty, area);
        return;
    };

    let layout = workspace_sidebar_layout(area, agent.workspace.sidebar_tab);
    let tabs = Tabs::new(["Files", "Search"])
        .select(match agent.workspace.sidebar_tab {
            WorkspaceSidebarTab::Files => 0,
            WorkspaceSidebarTab::Search => 1,
        })
        .block(sidebar_block().title("Workspace"))
        .style(sidebar_style())
        .highlight_style(tab_highlight_style());
    frame.render_widget(tabs, layout.tabs);

    match agent.workspace.sidebar_tab {
        WorkspaceSidebarTab::Files => {
            let items = agent
                .workspace
                .sidebar_labels()
                .into_iter()
                .map(ListItem::new)
                .collect::<Vec<_>>();
            let mut state = ListState::default();
            state.select(Some(
                agent
                    .workspace
                    .sidebar_selected_row()
                    .min(items.len().saturating_sub(1)),
            ));

            let list = List::new(items)
                .block(sidebar_block().title("Files"))
                .style(sidebar_style())
                .highlight_style(selection_style())
                .highlight_symbol("› ");
            frame.render_stateful_widget(list, layout.content, &mut state);
        }
        WorkspaceSidebarTab::Search => {
            let input = Paragraph::new(agent.workspace.search_query.as_str())
                .block(panel_block().title("Search"))
                .style(panel_style());
            if let Some(input_area) = layout.input {
                frame.render_widget(input, input_area);
            }

            let labels = if agent.workspace.search_query.trim().is_empty() {
                vec!["Type to search".to_string()]
            } else {
                let labels = agent.workspace.search_rows_labels();
                if labels.is_empty() {
                    vec!["No matches".to_string()]
                } else {
                    labels
                }
            };
            let items = labels.into_iter().map(ListItem::new).collect::<Vec<_>>();
            let mut state = ListState::default();
            if agent.workspace.search_total_rows() > 0 {
                state.select(Some(
                    agent
                        .workspace
                        .search_selected_row()
                        .min(items.len().saturating_sub(1)),
                ));
            }
            let list = List::new(items)
                .block(sidebar_block().title(format!(
                    "Results ({})",
                    agent.workspace.search_match_count()
                )))
                .style(sidebar_style())
                .highlight_style(selection_style())
                .highlight_symbol("› ");
            frame.render_stateful_widget(list, layout.content, &mut state);

            if let Some(input_area) = layout.input {
                let cursor_x = input_area
                    .x
                    .saturating_add(1 + agent.workspace.search_query.chars().count() as u16)
                    .min(input_area.x + input_area.width.saturating_sub(2));
                frame.set_cursor_position((cursor_x, input_area.y + 1));
            }
        }
    }
}

fn workspace_sidebar_layout(
    area: Rect,
    sidebar_tab: WorkspaceSidebarTab,
) -> WorkspaceSidebarLayout {
    match sidebar_tab {
        WorkspaceSidebarTab::Files => {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(3), Constraint::Min(4)])
                .split(area);
            WorkspaceSidebarLayout {
                tabs: chunks[0],
                input: None,
                content: chunks[1],
            }
        }
        WorkspaceSidebarTab::Search => {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3),
                    Constraint::Length(3),
                    Constraint::Min(4),
                ])
                .split(area);
            WorkspaceSidebarLayout {
                tabs: chunks[0],
                input: Some(chunks[1]),
                content: chunks[2],
            }
        }
    }
}

fn workspace_sidebar_tab_from_click(
    area: Rect,
    column: u16,
    row: u16,
) -> Option<WorkspaceSidebarTab> {
    let inner = inner_rect(area);
    if inner.width == 0 || inner.height == 0 {
        return None;
    }

    let files_width = "Files".chars().count() as u16;
    let search_width = "Search".chars().count() as u16;
    let files = Rect::new(inner.x, inner.y, files_width.min(inner.width), 1);
    let search_x = inner.x.saturating_add(files_width).saturating_add(3);
    let search = Rect::new(
        search_x,
        inner.y,
        search_width.min(inner.x.saturating_add(inner.width).saturating_sub(search_x)),
        1,
    );

    if rect_contains(files, column, row) {
        Some(WorkspaceSidebarTab::Files)
    } else if rect_contains(search, column, row) {
        Some(WorkspaceSidebarTab::Search)
    } else {
        None
    }
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
        .block(rounded_block().style(tab_style()))
        .style(tab_style())
        .highlight_style(tab_highlight_style());

    if app.current_tab == AppTab::Chat && !app.add_agent_selected() {
        let input_height = chat_input_height_for_main_area(&app.chat_input, area);
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(10),
                Constraint::Length(input_height),
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
            .block(panel_block().title("Chat"))
            .style(panel_style());
        frame.render_widget(empty, area);
        return;
    };

    let lines = chat_lines(agent);

    let title = if let Some(status) = &agent.status {
        format!("Chat - {} ({status})", agent.definition.name)
    } else {
        format!("Chat - {}", agent.definition.name)
    };

    let inner_height = area.height.saturating_sub(2);
    let text = Text::from(lines);
    let content_height = wrapped_text_height(&text, area.width.saturating_sub(2));
    let max_scroll = content_height.saturating_sub(inner_height as usize) as u16;
    let scroll = if agent.chat_follow_output {
        max_scroll
    } else {
        agent.chat_scroll.min(max_scroll)
    };

    let chat = Paragraph::new(text)
        .block(panel_block().title(title))
        .style(panel_style())
        .scroll((scroll, 0))
        .wrap(Wrap { trim: false });
    frame.render_widget(chat, area);
    render_vertical_scrollbar(frame, area, content_height, scroll);
}

fn draw_chat_input(frame: &mut Frame, app: &App, area: Rect) {
    let shell_mode = chat_input_is_shell(&app.chat_input);
    let thinking = app.active_agent().is_some_and(|agent| agent.thinking);
    let shell_running = app.active_agent().is_some_and(|agent| agent.shell_running);
    let title = if shell_running {
        format!(
            "Shell · {}  {} Running...",
            app.chat_model_label, SPINNER[app.spinner_index]
        )
    } else if shell_mode {
        format!("Shell · {}", app.chat_model_label)
    } else if thinking {
        format!(
            "Message · {}  {} Thinking...",
            app.chat_model_label, SPINNER[app.spinner_index]
        )
    } else {
        format!("Message · {}", app.chat_model_label)
    };

    let wrapped_lines = wrapped_chat_input_lines(&app.chat_input, area.width.saturating_sub(2));
    let input = Paragraph::new(Text::from(
        wrapped_lines
            .iter()
            .cloned()
            .map(Line::from)
            .collect::<Vec<_>>(),
    ))
    .block(panel_block().title(title))
    .style(panel_style())
    .wrap(Wrap { trim: false });
    frame.render_widget(input, area);

    let last_line = wrapped_lines
        .last()
        .map(|line| line.chars().count())
        .unwrap_or(0) as u16;
    let cursor_row = wrapped_lines.len().saturating_sub(1) as u16;
    let x = area
        .x
        .saturating_add(1 + last_line)
        .min(area.x + area.width.saturating_sub(2));
    let y = area
        .y
        .saturating_add(1 + cursor_row)
        .min(area.y + area.height.saturating_sub(2));
    frame.set_cursor_position((x, y));
}

fn draw_add_agent_form(frame: &mut Frame, app: &App, area: Rect) {
    frame.render_widget(Clear, area);
    let outer = panel_block().title("New Agent");
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
        .style(muted_panel_style());
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
        .block(input_block().title(name_title))
        .style(input_style());
    let workspace = Paragraph::new(app.add_form.workspace.as_str())
        .block(input_block().title(workspace_title))
        .style(input_style());
    frame.render_widget(name, chunks[1]);
    frame.render_widget(workspace, chunks[2]);

    let status = app
        .add_form
        .error
        .clone()
        .unwrap_or_else(|| "Saved agents live in ~/.cmdex.yml".to_string());
    let status = Paragraph::new(status).style(Style::default().bg(theme().panel_bg).fg(
        if app.add_form.error.is_some() {
            theme().error
        } else {
            theme().muted
        },
    ));
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
            .block(panel_block().title("Workspace"))
            .style(panel_style());
        frame.render_widget(empty, area);
        return;
    };

    if let Some(editor) = agent.workspace.editor.as_ref() {
        draw_workspace_editor(frame, editor, area);
        return;
    }

    let mut lines = vec![Line::from(format!(
        "Workspace: {}",
        compact_home(&agent.definition.workspace)
    ))];
    lines.push(Line::from(String::new()));
    lines.extend(agent.workspace.preview.iter().cloned());
    if let Some(error) = &agent.workspace.error {
        lines.push(Line::from(String::new()));
        lines.push(Line::from(Span::styled(
            error.clone(),
            Style::default().fg(theme().error).bg(theme().panel_bg),
        )));
    }
    let content_length = preview_content_height(&lines, area.width.saturating_sub(2));

    let widget = Paragraph::new(Text::from(lines))
        .block(panel_block().title(agent.workspace.preview_title.clone()))
        .style(panel_style())
        .scroll((agent.workspace.content_scroll, 0))
        .wrap(Wrap { trim: false });
    frame.render_widget(widget, area);
    render_vertical_scrollbar(frame, area, content_length, agent.workspace.content_scroll);
}

fn draw_workspace_editor(frame: &mut Frame, editor: &WorkspaceEditorState, area: Rect) {
    let mode = match editor.mode {
        EditorMode::Normal => "NORMAL",
        EditorMode::Insert => "INSERT",
        EditorMode::Command => "COMMAND",
    };
    let dirty = if editor.dirty { " [+]" } else { "" };
    let block = editor_block().title(format!("{}{} [{}]", editor.path.display(), dirty, mode));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let (code_area, status_area) = workspace_editor_panes(inner);
    let vertical_scroll = editor.clamped_vertical_scroll(code_area.height);
    let lines = editor.rendered_lines(code_area.height);
    let code = Paragraph::new(Text::from(lines))
        .style(editor_style())
        .scroll((0, editor.horizontal_scroll));
    frame.render_widget(code, code_area);
    render_vertical_scrollbar_with_viewport(
        frame,
        code_area,
        editor.content_height(),
        vertical_scroll,
    );

    if let Some(status_area) = status_area {
        let status = workspace_editor_status(editor);
        let status_widget =
            Paragraph::new(status).style(Style::default().bg(theme().app_bg).fg(theme().muted));
        frame.render_widget(status_widget, status_area);
    }

    match editor.mode {
        EditorMode::Command => {
            if let Some(status_area) = status_area {
                let x = status_area
                    .x
                    .saturating_add(1 + editor.command.chars().count() as u16)
                    .min(status_area.x + status_area.width.saturating_sub(1));
                frame.set_cursor_position((x, status_area.y));
            }
        }
        EditorMode::Normal | EditorMode::Insert => {
            if code_area.width == 0 || code_area.height == 0 {
                return;
            }

            let visible_row = editor.cursor_row.saturating_sub(vertical_scroll as usize) as u16;
            if visible_row >= code_area.height {
                return;
            }

            let gutter_width = editor.gutter_width() as u16;
            let visible_col = editor
                .cursor_col
                .saturating_sub(editor.horizontal_scroll as usize)
                as u16;
            let max_x = code_area.x + code_area.width.saturating_sub(1);
            let x = code_area
                .x
                .saturating_add(gutter_width)
                .saturating_add(visible_col)
                .min(max_x);
            let y = code_area.y.saturating_add(visible_row);
            frame.set_cursor_position((x, y));
        }
    }
}

fn draw_git_diff(frame: &mut Frame, app: &App, area: Rect) {
    let Some(agent) = app.active_agent() else {
        let empty = Paragraph::new("Select or create an agent in the Chat tab.")
            .block(panel_block().title("Git Diff"))
            .style(panel_style());
        frame.render_widget(empty, area);
        return;
    };

    let layout = git_diff_layout(area);
    let tabs = Tabs::new([
        format!("Changes ({})", agent.git_diff.count(DiffSection::Changes)),
        format!("Staged ({})", agent.git_diff.count(DiffSection::Staged)),
    ])
    .select(match agent.git_diff.active_section {
        DiffSection::Changes => 0,
        DiffSection::Staged => 1,
    })
    .block(rounded_block().style(tab_style()).title(format!(
        "Files · {}",
        compact_home(&agent.definition.workspace)
    )))
    .style(tab_style())
    .highlight_style(tab_highlight_style());
    frame.render_widget(tabs, layout.sections);

    let preview_lines = agent.git_diff.preview.clone();
    let content_length =
        preview_content_height(&preview_lines, layout.preview.width.saturating_sub(2));
    let widget = Paragraph::new(Text::from(preview_lines))
        .block(editor_block().title(agent.git_diff.preview_title.clone()))
        .style(editor_style())
        .scroll((agent.git_diff.content_scroll, 0))
        .wrap(Wrap { trim: false });
    frame.render_widget(widget, layout.preview);
    render_vertical_scrollbar(
        frame,
        layout.preview,
        content_length,
        agent.git_diff.content_scroll,
    );

    let commit_title = "Commit Message · Enter commits staged changes";
    let commit_text = git_diff_commit_input_text(
        &agent.git_diff.commit_message,
        layout.commit_input.width.saturating_sub(2),
    );
    let commit_input = Paragraph::new(commit_text.as_str())
        .block(panel_block().title(commit_title))
        .style(panel_style());
    frame.render_widget(commit_input, layout.commit_input);

    let stage_label = match agent.git_diff.active_section {
        DiffSection::Changes => "Stage",
        DiffSection::Staged => "Unstage",
    };
    let stage_button = Paragraph::new(stage_label)
        .alignment(Alignment::Center)
        .style(action_style(theme().foreground))
        .block(panel_block());
    frame.render_widget(stage_button, layout.stage_button);

    let discard_style = match agent.git_diff.active_section {
        DiffSection::Changes => action_style(theme().error),
        DiffSection::Staged => action_style(theme().muted),
    };
    let discard_button = Paragraph::new("Discard")
        .alignment(Alignment::Center)
        .style(discard_style)
        .block(panel_block());
    frame.render_widget(discard_button, layout.discard_button);

    let push_button = Paragraph::new("Push")
        .alignment(Alignment::Center)
        .style(action_style(theme().accent))
        .block(panel_block());
    frame.render_widget(push_button, layout.push_button);

    let pull_button = Paragraph::new("Pull")
        .alignment(Alignment::Center)
        .style(action_style(theme().foreground))
        .block(panel_block());
    frame.render_widget(pull_button, layout.pull_button);

    let status = if let Some(error) = &agent.git_diff.error {
        Paragraph::new(error.as_str())
            .style(Style::default().bg(theme().panel_bg).fg(theme().error))
    } else if let Some(status) = &agent.git_diff.status {
        Paragraph::new(status.as_str())
            .style(Style::default().bg(theme().panel_bg).fg(theme().success))
    } else {
        Paragraph::new(
            "Tab/Left/Right switches sections. Ctrl+S stages, Ctrl+U unstages, Ctrl+D discards changes, Enter commits staged changes.",
        )
            .style(muted_panel_style())
    };
    frame.render_widget(status, layout.status);

    let cursor_x = layout
        .commit_input
        .x
        .saturating_add(1 + commit_text.chars().count() as u16)
        .min(layout.commit_input.x + layout.commit_input.width.saturating_sub(2));
    let cursor_y = layout.commit_input.y.saturating_add(1);
    frame.set_cursor_position((cursor_x, cursor_y));
}

fn git_diff_layout(area: Rect) -> GitDiffLayout {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(5),
        ])
        .split(area);
    let controls = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(1)])
        .split(chunks[2]);
    let controls_row = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Min(18),
            Constraint::Length(12),
            Constraint::Length(12),
            Constraint::Length(12),
            Constraint::Length(12),
        ])
        .split(controls[0]);

    GitDiffLayout {
        sections: chunks[0],
        preview: chunks[1],
        commit_input: controls_row[0],
        stage_button: controls_row[1],
        discard_button: controls_row[2],
        push_button: controls_row[3],
        pull_button: controls_row[4],
        status: controls[1],
    }
}

fn git_diff_section_from_click(
    area: Rect,
    changes_label: &str,
    staged_label: &str,
    column: u16,
    row: u16,
) -> Option<DiffSection> {
    let inner = inner_rect(area);
    if inner.width == 0 || inner.height == 0 {
        return None;
    }

    let changes_width = changes_label.chars().count() as u16;
    let staged_width = staged_label.chars().count() as u16;
    let changes = Rect::new(inner.x, inner.y, changes_width.min(inner.width), 1);
    let staged_x = inner.x.saturating_add(changes_width).saturating_add(3);
    let staged = Rect::new(
        staged_x,
        inner.y,
        staged_width.min(inner.x.saturating_add(inner.width).saturating_sub(staged_x)),
        1,
    );

    if rect_contains(changes, column, row) {
        Some(DiffSection::Changes)
    } else if rect_contains(staged, column, row) {
        Some(DiffSection::Staged)
    } else {
        None
    }
}

fn git_diff_commit_input_text(input: &str, max_width: u16) -> String {
    let max_width = usize::from(max_width.max(1));
    let chars = input.chars().collect::<Vec<_>>();
    if chars.len() <= max_width {
        input.to_string()
    } else {
        chars[chars.len().saturating_sub(max_width)..]
            .iter()
            .collect()
    }
}

fn render_vertical_scrollbar(frame: &mut Frame, area: Rect, content_length: usize, scroll: u16) {
    let inner_width = area.width.saturating_sub(2);
    let inner_height = area.height.saturating_sub(2);
    if inner_width == 0 || inner_height == 0 {
        return;
    }
    if content_length <= inner_height as usize {
        return;
    }

    let mut state = ScrollbarState::new(content_length.max(1))
        .position(scroll as usize)
        .viewport_content_length(inner_height as usize);

    frame.render_stateful_widget(
        scrollbar_widget(),
        area.inner(Margin {
            vertical: 1,
            horizontal: 0,
        }),
        &mut state,
    );
}

fn render_vertical_scrollbar_with_viewport(
    frame: &mut Frame,
    viewport: Rect,
    content_length: usize,
    scroll: u16,
) {
    if viewport.width == 0 || viewport.height == 0 {
        return;
    }
    if content_length <= viewport.height as usize {
        return;
    }

    let mut state = ScrollbarState::new(content_length.max(1))
        .position(scroll as usize)
        .viewport_content_length(viewport.height as usize);

    let scrollbar_area = Rect::new(
        viewport.x.saturating_add(viewport.width.saturating_sub(1)),
        viewport.y,
        1,
        viewport.height,
    );
    frame.render_stateful_widget(scrollbar_widget(), scrollbar_area, &mut state);
}

fn workspace_editor_panes(inner: Rect) -> (Rect, Option<Rect>) {
    if inner.height <= 1 {
        return (inner, None);
    }

    let panes = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);
    (panes[0], Some(panes[1]))
}

fn workspace_editor_viewport(area: Rect) -> Rect {
    let inner = area.inner(Margin {
        vertical: 1,
        horizontal: 1,
    });
    workspace_editor_panes(inner).0
}

fn workspace_editor_status(editor: &WorkspaceEditorState) -> String {
    match editor.mode {
        EditorMode::Command => format!(":{}", editor.command),
        EditorMode::Insert => {
            "-- INSERT --  Esc normal  Enter newline  Backspace delete".to_string()
        }
        EditorMode::Normal => editor.status.clone().unwrap_or_else(|| {
            "NORMAL  ↑/↓ file  h/j/k/l move  i/a/o edit  x delete  :w save  :q preview".to_string()
        }),
    }
}

fn preview_content_height(lines: &[Line<'_>], width: u16) -> usize {
    let width = usize::from(width.max(1));

    lines
        .iter()
        .map(|line| match line.width() {
            0 => 1,
            line_width => line_width.saturating_sub(1) / width + 1,
        })
        .sum()
}

fn wrapped_text_height(text: &Text<'_>, width: u16) -> usize {
    Paragraph::new(text.clone())
        .wrap(Wrap { trim: false })
        .line_count(width.max(1))
}

fn chat_input_height_for_main_area(input: &str, main_area: Rect) -> u16 {
    let available = main_area.height.saturating_sub(3);
    if available == 0 {
        return 0;
    }

    let desired = wrapped_chat_input_lines(input, main_area.width.saturating_sub(2))
        .len()
        .saturating_add(2) as u16;
    let min_height = available.min(3);
    let max_height = available.saturating_sub(1).max(min_height);

    desired.clamp(min_height, max_height)
}

fn wrapped_chat_input_lines(input: &str, width: u16) -> Vec<String> {
    let width = usize::from(width.max(1));
    if input.is_empty() {
        return vec![String::new()];
    }

    let mut wrapped = Vec::new();

    for raw_line in input.split('\n') {
        if raw_line.is_empty() {
            wrapped.push(String::new());
            continue;
        }

        let mut current = String::new();
        let mut current_width = 0;

        for character in raw_line.chars() {
            current.push(character);
            current_width += 1;

            if current_width == width {
                wrapped.push(std::mem::take(&mut current));
                current_width = 0;
            }
        }

        if !current.is_empty() {
            wrapped.push(current);
        }
    }

    if wrapped.is_empty() {
        vec![String::new()]
    } else {
        wrapped
    }
}

fn chat_lines(agent: &AgentState) -> Vec<Line<'static>> {
    if agent.messages.is_empty() {
        vec![Line::from("No messages yet.")]
    } else {
        agent
            .messages
            .iter()
            .flat_map(|message| render_chat_message_lines(message, &agent.definition.name))
            .collect()
    }
}

fn chat_max_scroll(agent: &AgentState, area: Rect) -> u16 {
    let lines = chat_lines(agent);
    let inner_height = area.height.saturating_sub(2) as usize;
    let content_height = wrapped_text_height(&Text::from(lines), area.width.saturating_sub(2));

    content_height.saturating_sub(inner_height) as u16
}

fn render_chat_message_lines(message: &ChatMessage, agent_name: &str) -> Vec<Line<'static>> {
    let role = match message.role {
        MessageRole::User => ("You", theme().yellow),
        MessageRole::Assistant => (agent_name, theme().green),
        MessageRole::Event => ("Event", theme().cyan),
        MessageRole::System => ("System", theme().red),
        MessageRole::Shell => ("Shell", theme().magenta),
    };

    let mut lines = vec![Line::from(vec![Span::styled(
        format!("{}:", role.0),
        Style::default().fg(role.1).add_modifier(Modifier::BOLD),
    )])];
    lines.extend(render_markdown_lines(&message.text));
    lines.push(Line::default());
    lines
}

fn render_markdown_lines(source: &str) -> Vec<Line<'static>> {
    if source.trim().is_empty() {
        return vec![Line::default()];
    }

    let mut options = MarkdownOptions::empty();
    options.insert(MarkdownOptions::ENABLE_STRIKETHROUGH);
    let parser = MarkdownParser::new_ext(source, options);
    let mut renderer = MarkdownRenderer::default();

    for event in parser {
        renderer.handle(event);
    }

    renderer.finish()
}

#[derive(Debug, Clone)]
enum MarkdownListKind {
    Unordered,
    Ordered(usize),
}

#[derive(Default)]
struct MarkdownRenderer {
    lines: Vec<Line<'static>>,
    current_spans: Vec<Span<'static>>,
    emphasis_depth: usize,
    strong_depth: usize,
    strikethrough_depth: usize,
    heading_level: Option<HeadingLevel>,
    code_block_depth: usize,
    blockquote_depth: usize,
    list_stack: Vec<MarkdownListKind>,
    link_targets: Vec<String>,
}

impl MarkdownRenderer {
    fn handle(&mut self, event: MarkdownEvent<'_>) {
        match event {
            MarkdownEvent::Start(tag) => self.start_tag(tag),
            MarkdownEvent::End(tag) => self.end_tag(tag),
            MarkdownEvent::Text(text) => self.push_text(
                text.as_ref(),
                if self.in_code_block() {
                    inline_code_style()
                } else {
                    self.current_style()
                },
            ),
            MarkdownEvent::Code(text) => self.push_text(text.as_ref(), inline_code_style()),
            MarkdownEvent::SoftBreak => {
                if self.in_code_block() {
                    self.push_line();
                } else {
                    self.push_text(" ", self.current_style());
                }
            }
            MarkdownEvent::HardBreak => self.push_line(),
            MarkdownEvent::Rule => {
                self.push_line_if_needed();
                self.lines.push(Line::from("---"));
                self.lines.push(Line::default());
            }
            MarkdownEvent::Html(text) | MarkdownEvent::InlineHtml(text) => {
                self.push_text(text.as_ref(), html_style())
            }
            MarkdownEvent::TaskListMarker(checked) => {
                let marker = if checked { "[x] " } else { "[ ] " };
                self.push_text(marker, task_marker_style());
            }
            MarkdownEvent::InlineMath(text) | MarkdownEvent::DisplayMath(text) => {
                self.push_text(text.as_ref(), inline_code_style())
            }
            MarkdownEvent::FootnoteReference(text) => {
                self.push_text(text.as_ref(), link_style());
            }
        }
    }

    fn start_tag(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Paragraph => {}
            Tag::Heading { level, .. } => {
                self.push_line_if_needed();
                self.heading_level = Some(level);
            }
            Tag::BlockQuote(_) => {
                self.push_line_if_needed();
                self.blockquote_depth += 1;
            }
            Tag::CodeBlock(kind) => {
                self.push_line_if_needed();
                self.code_block_depth += 1;
                if let CodeBlockKind::Fenced(language) = kind {
                    let language = language.trim();
                    if !language.is_empty() {
                        self.push_text(
                            &format!("```{language}"),
                            Style::default()
                                .fg(theme().muted)
                                .add_modifier(Modifier::ITALIC),
                        );
                        self.push_line();
                    }
                }
            }
            Tag::List(start) => {
                self.push_line_if_needed();
                self.list_stack.push(match start {
                    Some(number) => MarkdownListKind::Ordered(number as usize),
                    None => MarkdownListKind::Unordered,
                });
            }
            Tag::Item => {
                self.push_line_if_needed();
                self.ensure_block_prefix();
                let prefix = match self.list_stack.last_mut() {
                    Some(MarkdownListKind::Ordered(number)) => {
                        let current = *number;
                        *number += 1;
                        format!("{current}. ")
                    }
                    _ => "- ".to_string(),
                };
                self.current_spans
                    .push(Span::styled(prefix, task_marker_style()));
            }
            Tag::Emphasis => self.emphasis_depth += 1,
            Tag::Strong => self.strong_depth += 1,
            Tag::Strikethrough => self.strikethrough_depth += 1,
            Tag::Link { dest_url, .. } => self.link_targets.push(dest_url.to_string()),
            Tag::Image { dest_url, .. } => {
                self.push_text(
                    &format!("[image: {}]", dest_url),
                    Style::default().fg(theme().magenta),
                );
            }
            _ => {}
        }
    }

    fn end_tag(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph => {
                self.push_line_if_needed();
                self.trim_trailing_blank_lines();
            }
            TagEnd::Heading(..) => {
                self.push_line_if_needed();
                self.heading_level = None;
                self.trim_trailing_blank_lines();
            }
            TagEnd::BlockQuote(..) => {
                self.push_line_if_needed();
                self.blockquote_depth = self.blockquote_depth.saturating_sub(1);
                self.trim_trailing_blank_lines();
            }
            TagEnd::CodeBlock => {
                self.push_line_if_needed();
                self.code_block_depth = self.code_block_depth.saturating_sub(1);
                self.trim_trailing_blank_lines();
            }
            TagEnd::List(..) => {
                self.push_line_if_needed();
                self.list_stack.pop();
                self.trim_trailing_blank_lines();
            }
            TagEnd::Item => self.push_line_if_needed(),
            TagEnd::Emphasis => {
                self.emphasis_depth = self.emphasis_depth.saturating_sub(1);
            }
            TagEnd::Strong => {
                self.strong_depth = self.strong_depth.saturating_sub(1);
            }
            TagEnd::Strikethrough => {
                self.strikethrough_depth = self.strikethrough_depth.saturating_sub(1);
            }
            TagEnd::Link => {
                if let Some(target) = self.link_targets.pop() {
                    self.push_text(&format!(" ({target})"), link_style());
                }
            }
            _ => {}
        }
    }

    fn finish(mut self) -> Vec<Line<'static>> {
        self.push_line_if_needed();
        self.trim_trailing_blank_lines();
        if self.lines.is_empty() {
            vec![Line::default()]
        } else {
            self.lines
        }
    }

    fn push_text(&mut self, text: &str, style: Style) {
        for (index, segment) in text.split('\n').enumerate() {
            if index > 0 {
                self.push_line();
            }

            if segment.is_empty() {
                continue;
            }

            self.ensure_block_prefix();
            self.current_spans
                .push(Span::styled(segment.to_string(), style));
        }
    }

    fn push_line_if_needed(&mut self) {
        if !self.current_spans.is_empty() {
            self.push_line();
        }
    }

    fn push_line(&mut self) {
        if self.current_spans.is_empty() {
            self.lines.push(Line::default());
        } else {
            self.lines
                .push(Line::from(std::mem::take(&mut self.current_spans)));
        }
    }

    fn trim_trailing_blank_lines(&mut self) {
        while self
            .lines
            .last()
            .is_some_and(|line| line.spans.iter().all(|span| span.content.is_empty()))
        {
            self.lines.pop();
        }
    }

    fn ensure_block_prefix(&mut self) {
        if !self.current_spans.is_empty() || self.blockquote_depth == 0 {
            return;
        }

        for _ in 0..self.blockquote_depth {
            self.current_spans
                .push(Span::styled("> ", Style::default().fg(theme().muted)));
        }
    }

    fn current_style(&self) -> Style {
        let mut style = Style::default();

        if self.strong_depth > 0 || self.heading_level.is_some() {
            style = style.add_modifier(Modifier::BOLD);
        }
        if self.emphasis_depth > 0 {
            style = style.add_modifier(Modifier::ITALIC);
        }
        if self.strikethrough_depth > 0 {
            style = style.add_modifier(Modifier::CROSSED_OUT);
        }
        if self.heading_level.is_some() {
            style = style.fg(theme().accent);
        }

        style
    }

    fn in_code_block(&self) -> bool {
        self.code_block_depth > 0
    }
}

fn inline_code_style() -> Style {
    Style::default()
        .fg(theme().inline_code_fg)
        .bg(theme().inline_code_bg)
}

fn html_style() -> Style {
    Style::default()
        .fg(theme().magenta)
        .add_modifier(Modifier::ITALIC)
}

fn task_marker_style() -> Style {
    Style::default()
        .fg(theme().muted)
        .add_modifier(Modifier::BOLD)
}

fn link_style() -> Style {
    Style::default()
        .fg(theme().blue)
        .add_modifier(Modifier::UNDERLINED)
}

fn chat_input_is_shell(input: &str) -> bool {
    input.starts_with('>')
}

fn shell_command_from_input(input: &str) -> Option<String> {
    if !chat_input_is_shell(input) {
        return None;
    }

    let command = input.strip_prefix('>').unwrap_or(input).trim().to_string();
    if command.is_empty() {
        None
    } else {
        Some(command)
    }
}

fn truncate_shell_text(text: &str) -> String {
    if text.chars().count() <= SHELL_OUTPUT_LIMIT {
        return text.trim_end_matches('\n').to_string();
    }

    let truncated = text.chars().take(SHELL_OUTPUT_LIMIT).collect::<String>();
    format!("{}\n[truncated]", truncated.trim_end_matches('\n'))
}

fn format_shell_output(
    command: &str,
    stdout: &str,
    stderr: &str,
    exit_code: Option<i32>,
    success: bool,
) -> String {
    let mut body = String::new();
    let stdout = truncate_shell_text(stdout);
    let stderr = truncate_shell_text(stderr);

    if !stdout.is_empty() {
        body.push_str(&stdout);
    }
    if !stderr.is_empty() {
        if !body.is_empty() {
            body.push('\n');
        }
        if !stdout.is_empty() {
            body.push_str("[stderr]\n");
        }
        body.push_str(&stderr);
    }
    if body.is_empty() {
        body.push_str("[no output]");
    }

    let exit_code = exit_code
        .map(|code| code.to_string())
        .unwrap_or_else(|| "unavailable".to_string());
    let status = if success { "ok" } else { "failed" };

    format!("Command: `{command}`\n\n```text\n{body}\n```\n\nExit code: {exit_code} ({status})")
}

fn load_codex_chat_model_label() -> Option<String> {
    let config_path = codex_config_path()?;
    let contents = fs::read_to_string(config_path).ok()?;

    let model = parse_codex_top_level_string(&contents, "model")?;
    match parse_codex_top_level_string(&contents, "model_reasoning_effort") {
        Some(effort) => Some(format!("{model} · {effort}")),
        None => Some(model),
    }
}

fn codex_config_path() -> Option<PathBuf> {
    env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".codex")))
        .map(|dir| dir.join("config.toml"))
}

#[cfg(test)]
fn parse_codex_model_from_config(contents: &str) -> Option<String> {
    parse_codex_top_level_string(contents, "model")
}

#[cfg(test)]
fn parse_codex_reasoning_effort_from_config(contents: &str) -> Option<String> {
    parse_codex_top_level_string(contents, "model_reasoning_effort")
}

fn parse_codex_top_level_string(contents: &str, wanted_key: &str) -> Option<String> {
    let mut at_top_level = true;

    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if trimmed.starts_with('[') {
            at_top_level = false;
            continue;
        }
        if !at_top_level {
            continue;
        }

        let Some((key, value)) = trimmed.split_once('=') else {
            continue;
        };
        if key.trim() != wanted_key {
            continue;
        }

        let parsed = value.trim().trim_matches('"').trim_matches('\'').trim();
        if !parsed.is_empty() {
            return Some(parsed.to_string());
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrapped_text_height_matches_paragraph_word_wrapping() {
        let text = Text::from(vec![Line::from("abc def ghi")]);

        assert_eq!(wrapped_text_height(&text, 6), 3);
    }

    #[test]
    fn chat_max_scroll_uses_wrapped_height_for_last_message() {
        let mut agent = AgentState::new(AgentDefinition {
            name: "Test".to_string(),
            workspace: PathBuf::from("/tmp"),
        });
        agent.messages.push(ChatMessage {
            role: MessageRole::Assistant,
            text: "abc def ghi".to_string(),
            item_id: None,
        });

        let area = Rect::new(0, 0, 8, 5);

        assert_eq!(chat_max_scroll(&agent, area), 2);
    }

    #[test]
    fn shell_command_is_detected_from_chat_input() {
        assert_eq!(
            shell_command_from_input("> cargo test"),
            Some("cargo test".to_string())
        );
        assert_eq!(
            shell_command_from_input(">   ls -la"),
            Some("ls -la".to_string())
        );
        assert_eq!(shell_command_from_input("hello > world"), None);
        assert_eq!(shell_command_from_input(">"), None);
    }

    #[test]
    fn shell_output_is_formatted_for_chat() {
        let output = format_shell_output("ls", "file.txt\n", "", Some(0), true);

        assert!(output.contains("Command: `ls`"));
        assert!(output.contains("```text"));
        assert!(output.contains("file.txt"));
        assert!(output.contains("Exit code: 0 (ok)"));
    }

    #[test]
    fn chat_input_wraps_into_multiple_lines() {
        assert_eq!(
            wrapped_chat_input_lines("abcdef", 4),
            vec!["abcd".to_string(), "ef".to_string()]
        );
        assert_eq!(
            wrapped_chat_input_lines("ab\ncd", 4),
            vec!["ab".to_string(), "cd".to_string()]
        );
    }

    #[test]
    fn chat_input_height_grows_with_wrapped_content() {
        let main_area = Rect::new(0, 0, 10, 20);

        assert_eq!(chat_input_height_for_main_area("short", main_area), 3);
        assert_eq!(chat_input_height_for_main_area("abcdefghijk", main_area), 4);
    }

    #[test]
    fn parses_codex_model_from_top_level_config() {
        let config = r#"
model = "gpt-5.4"
model_reasoning_effort = "xhigh"

[projects."/tmp/example"]
trust_level = "trusted"
"#;

        assert_eq!(
            parse_codex_model_from_config(config),
            Some("gpt-5.4".to_string())
        );
        assert_eq!(
            parse_codex_reasoning_effort_from_config(config),
            Some("xhigh".to_string())
        );
    }

    #[test]
    fn ignores_non_top_level_model_keys() {
        let config = r#"
[profiles.fast]
model = "gpt-5.5-mini"
"#;

        assert_eq!(parse_codex_model_from_config(config), None);
        assert_eq!(parse_codex_reasoning_effort_from_config(config), None);
    }

    #[test]
    fn builds_chat_model_label_with_reasoning_effort() {
        let config = r#"
model = "gpt-5.4"
model_reasoning_effort = "xhigh"
"#;

        let model = parse_codex_model_from_config(config).unwrap();
        let effort = parse_codex_reasoning_effort_from_config(config).unwrap();

        assert_eq!(format!("{model} · {effort}"), "gpt-5.4 · xhigh");
    }
}
