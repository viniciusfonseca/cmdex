use super::{chat::ChatSupport, components::*, lsp, shell, *};

impl App {
    pub(super) fn on_tick(&mut self, ui_tx: &mpsc::UnboundedSender<UiEvent>) -> bool {
        let mut needs_redraw = false;

        if self.has_active_animation() {
            self.spinner_index = (self.spinner_index + 1) % SPINNER.len();
            needs_redraw = true;
        }

        if self.dispatch_pending_workspace_hover(ui_tx) {
            needs_redraw = true;
        }

        WorkspaceComponent::maybe_refresh(self) || needs_redraw
    }

    pub(super) fn tick_interval(&self) -> Duration {
        if self.has_active_animation() {
            FAST_TICK_INTERVAL
        } else if self.current_tab == AppTab::Workspace {
            WORKSPACE_TICK_INTERVAL
        } else {
            IDLE_TICK_INTERVAL
        }
    }

    fn has_active_animation(&self) -> bool {
        let Some(agent) = self.active_agent() else {
            return false;
        };

        match self.current_tab {
            AppTab::Chat => agent.thinking || agent.shell_running,
            AppTab::Workspace => false,
            AppTab::Shell => agent
                .shell_tab
                .sessions
                .iter()
                .any(|session| !session.ready || session.running),
            AppTab::GitDiff => agent.git_diff.remote_action.is_some(),
        }
    }

    pub(super) fn shutdown_shell_sessions(&mut self) {
        let pids = self
            .shell_runtimes
            .drain()
            .map(|(_, runtime)| runtime.pid)
            .filter(|pid| *pid != 0)
            .collect::<Vec<_>>();

        for pid in pids {
            let _ = std::process::Command::new("kill")
                .args(["-TERM", &pid.to_string()])
                .status();
        }
    }

    pub(super) fn shutdown_lsp_sessions(&mut self) {
        for (_, runtime) in self.lsp_runtimes.drain() {
            let _ = runtime.command_tx.send(lsp::LspCommand::Shutdown);
        }
    }

    fn move_selection_up(&mut self) {
        match self.current_tab {
            AppTab::Chat => {
                AgentsSidebarComponent::select_index(
                    self,
                    self.chat_sidebar_index.saturating_sub(1),
                );
            }
            AppTab::Workspace => {
                if let Some(agent) = self.active_agent_mut() {
                    if agent.workspace.editor_focused() {
                        return;
                    }
                    if agent.workspace.sidebar_tab == WorkspaceSidebarTab::Search {
                        agent.workspace.search_move_up();
                    } else {
                        agent.workspace.move_up();
                    }
                }
            }
            AppTab::Shell => {
                if let Some(agent) = self.active_agent_mut() {
                    agent.shell_tab.move_up();
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
                AgentsSidebarComponent::select_index(
                    self,
                    (self.chat_sidebar_index + 1).min(max_index),
                );
            }
            AppTab::Workspace => {
                if let Some(agent) = self.active_agent_mut() {
                    if agent.workspace.editor_focused() {
                        return;
                    }
                    if agent.workspace.sidebar_tab == WorkspaceSidebarTab::Search {
                        agent.workspace.search_move_down();
                    } else {
                        agent.workspace.move_down();
                    }
                }
            }
            AppTab::Shell => {
                if let Some(agent) = self.active_agent_mut() {
                    agent.shell_tab.move_down();
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
                let has_editor = self
                    .active_agent()
                    .and_then(|agent| agent.workspace.editor.as_ref())
                    .is_some();
                if has_editor {
                    self.clear_workspace_hover();
                    if let Some(agent) = self.active_agent_mut() {
                        if let Some(editor) = agent.workspace.editor.as_mut() {
                            editor.scroll_up(lines);
                            return;
                        }
                    }
                }
                if let Some(agent) = self.active_agent_mut() {
                    agent.workspace.scroll_up(lines);
                }
            }
            AppTab::Shell => {
                if let Some(agent) = self.active_agent_mut() {
                    if let Some(session) = agent.shell_tab.selected_session_mut() {
                        session.scroll = session.scroll.saturating_sub(lines);
                    }
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

    pub(super) fn handle_mouse(
        &mut self,
        mouse: MouseEvent,
        area: Rect,
        ui_tx: &mpsc::UnboundedSender<UiEvent>,
    ) {
        let layout = self.compute_layout(area);

        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                self.active_workspace_selection_drag = false;
                if self.handle_scrollbar_press(mouse.column, mouse.row, layout) {
                    return;
                }
                self.handle_left_click(mouse.column, mouse.row, mouse.modifiers, layout, ui_tx);
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                if self.handle_scrollbar_drag(mouse.row, layout) {
                    return;
                }
                if self.active_workspace_selection_drag
                    && self.current_tab == AppTab::Workspace
                    && self
                        .active_agent()
                        .is_some_and(|agent| agent.workspace.editor.is_some())
                {
                    WorkspaceComponent::handle_editor_drag(
                        self,
                        mouse.column,
                        mouse.row,
                        layout.body,
                    );
                }
            }
            MouseEventKind::Up(MouseButton::Left) => {
                self.active_scrollbar_drag = None;
                if self.active_workspace_selection_drag {
                    if let Some(agent) = self.active_agent_mut() {
                        if let Some(editor) = agent.workspace.editor.as_mut() {
                            if editor.mode == EditorMode::Visual && !editor.has_selection() {
                                editor.exit_visual_mode();
                            }
                        }
                    }
                }
                self.active_workspace_selection_drag = false;
            }
            MouseEventKind::Moved if !self.active_workspace_selection_drag => {
                self.handle_mouse_move(mouse.column, mouse.row, layout)
            }
            MouseEventKind::ScrollUp => {
                let horizontal = mouse.modifiers.contains(KeyModifiers::SHIFT);
                if self.should_handle_mouse_scroll(ScrollDirection::Up, horizontal) {
                    self.handle_scroll(mouse.column, mouse.row, layout, true, horizontal);
                }
            }
            MouseEventKind::ScrollDown => {
                let horizontal = mouse.modifiers.contains(KeyModifiers::SHIFT);
                if self.should_handle_mouse_scroll(ScrollDirection::Down, horizontal) {
                    self.handle_scroll(mouse.column, mouse.row, layout, false, horizontal);
                }
            }
            _ => {}
        }
    }

    fn handle_mouse_move(&mut self, column: u16, row: u16, layout: UiLayout) {
        if self.current_tab == AppTab::Workspace
            && WorkspaceComponent::handle_editor_hover(self, column, row, layout.body)
        {
            return;
        }

        self.clear_workspace_hover();
    }

    fn handle_scrollbar_press(&mut self, column: u16, row: u16, layout: UiLayout) -> bool {
        self.active_scrollbar_drag = None;

        let Some(target) = self.scrollbar_drag_target_at(column, row, layout) else {
            return false;
        };

        self.active_scrollbar_drag = Some(target);
        self.update_scrollbar_drag(target, row, layout)
    }

    fn handle_scrollbar_drag(&mut self, row: u16, layout: UiLayout) -> bool {
        let Some(target) = self.active_scrollbar_drag else {
            return false;
        };

        if self.update_scrollbar_drag(target, row, layout) {
            true
        } else {
            self.active_scrollbar_drag = None;
            false
        }
    }

    fn scrollbar_drag_target_at(
        &self,
        column: u16,
        row: u16,
        layout: UiLayout,
    ) -> Option<ScrollbarDragTarget> {
        let target = match self.current_tab {
            AppTab::Chat if !self.add_agent_selected() => ScrollbarDragTarget::Chat,
            AppTab::Workspace => {
                if self
                    .active_agent()
                    .is_some_and(|agent| agent.workspace.editor.is_some())
                {
                    ScrollbarDragTarget::WorkspaceEditor
                } else {
                    ScrollbarDragTarget::WorkspacePreview
                }
            }
            AppTab::Shell => ScrollbarDragTarget::ShellOutput,
            AppTab::GitDiff => ScrollbarDragTarget::GitDiffPreview,
            AppTab::Chat => return None,
        };

        let metrics = self.scrollbar_metrics(target, layout)?;
        UiSupport::rect_contains(metrics.track, column, row).then_some(target)
    }

    fn update_scrollbar_drag(
        &mut self,
        target: ScrollbarDragTarget,
        row: u16,
        layout: UiLayout,
    ) -> bool {
        let Some(metrics) = self.scrollbar_metrics(target, layout) else {
            return false;
        };
        let scroll = UiSupport::scroll_position_from_row(metrics, row);

        match target {
            ScrollbarDragTarget::Chat => {
                let Some(agent) = self.active_agent_mut() else {
                    return false;
                };
                agent.chat_follow_output = false;
                agent.chat_scroll = scroll;
            }
            ScrollbarDragTarget::WorkspacePreview => {
                let Some(agent) = self.active_agent_mut() else {
                    return false;
                };
                agent.workspace.content_scroll = scroll;
            }
            ScrollbarDragTarget::WorkspaceEditor => {
                let Some(agent) = self.active_agent_mut() else {
                    return false;
                };
                let Some(editor) = agent.workspace.editor.as_mut() else {
                    return false;
                };
                editor.set_vertical_scroll(scroll, metrics.viewport_length as u16);
            }
            ScrollbarDragTarget::ShellOutput => {
                let Some(agent) = self.active_agent_mut() else {
                    return false;
                };
                let Some(session) = agent.shell_tab.selected_session_mut() else {
                    return false;
                };
                session.scroll = scroll;
            }
            ScrollbarDragTarget::GitDiffPreview => {
                let Some(agent) = self.active_agent_mut() else {
                    return false;
                };
                agent.git_diff.content_scroll = scroll;
            }
        }

        true
    }

    fn scrollbar_metrics(
        &self,
        target: ScrollbarDragTarget,
        layout: UiLayout,
    ) -> Option<ScrollbarMetrics> {
        let agent = self.active_agent()?;

        match target {
            ScrollbarDragTarget::Chat => {
                let content_length = ChatSupport::content_height(agent, layout.body);
                UiSupport::vertical_scrollbar_metrics(layout.body, content_length)
            }
            ScrollbarDragTarget::WorkspacePreview => {
                let content_length = UiSupport::scrollable_preview_content_height(
                    &agent.workspace.preview,
                    layout.body,
                );
                UiSupport::vertical_scrollbar_metrics(layout.body, content_length)
            }
            ScrollbarDragTarget::WorkspaceEditor => {
                let editor = agent.workspace.editor.as_ref()?;
                let viewport = WorkspaceEditorComponent::viewport(layout.body);
                UiSupport::vertical_scrollbar_metrics_for_viewport(
                    viewport,
                    editor.content_height(),
                )
            }
            ScrollbarDragTarget::ShellOutput => {
                let session = agent.shell_tab.selected_session()?;
                let lines = shell::ShellPresenter::display_lines(session, &agent.shell_tab.input);
                let content_length =
                    UiSupport::scrollable_preview_content_height(&lines, layout.body);
                UiSupport::vertical_scrollbar_metrics(layout.body, content_length)
            }
            ScrollbarDragTarget::GitDiffPreview => {
                let diff_layout = GitDiffComponent::layout(layout.body);
                let content_length = UiSupport::scrollable_preview_content_height(
                    &agent.git_diff.preview,
                    diff_layout.preview,
                );
                UiSupport::vertical_scrollbar_metrics(diff_layout.preview, content_length)
            }
        }
    }

    fn should_handle_mouse_scroll(&mut self, direction: ScrollDirection, horizontal: bool) -> bool {
        let axis = if horizontal {
            ScrollAxis::Horizontal
        } else {
            ScrollAxis::Vertical
        };
        self.should_handle_mouse_scroll_at_axis(axis, direction, Instant::now())
    }

    pub(super) fn should_handle_mouse_scroll_at(
        &mut self,
        direction: ScrollDirection,
        now: Instant,
    ) -> bool {
        self.should_handle_mouse_scroll_at_axis(ScrollAxis::Vertical, direction, now)
    }

    pub(super) fn should_handle_mouse_scroll_at_axis(
        &mut self,
        axis: ScrollAxis,
        direction: ScrollDirection,
        now: Instant,
    ) -> bool {
        if self
            .last_mouse_scroll
            .is_some_and(|(last_axis, last_direction, last_at)| {
                last_axis == axis
                    && last_direction == direction
                    && now
                        .checked_duration_since(last_at)
                        .is_some_and(|elapsed| elapsed < MOUSE_SCROLL_DEBOUNCE)
            })
        {
            return false;
        }

        self.last_mouse_scroll = Some((axis, direction, now));
        true
    }

    fn scroll_content_down(&mut self, area: Rect) {
        self.scroll_content_down_by(area, CONTENT_SCROLL_STEP);
    }

    fn scroll_content_down_by(&mut self, area: Rect, lines: u16) {
        match self.current_tab {
            AppTab::Workspace => {
                let has_editor = self
                    .active_agent()
                    .and_then(|agent| agent.workspace.editor.as_ref())
                    .is_some();
                if has_editor {
                    self.clear_workspace_hover();
                    if let Some(agent) = self.active_agent_mut() {
                        if let Some(editor) = agent.workspace.editor.as_mut() {
                            let viewport = WorkspaceEditorComponent::viewport(area);
                            editor.scroll_down(lines, viewport.height);
                            return;
                        }
                    }
                }
                if let Some(agent) = self.active_agent_mut() {
                    agent.workspace.scroll_down(lines);
                }
            }
            AppTab::Shell => {
                if let Some(agent) = self.active_agent_mut() {
                    if let Some(session) = agent.shell_tab.selected_session_mut() {
                        session.scroll = session.scroll.saturating_add(lines);
                    }
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

        let max_scroll = ChatSupport::max_scroll(agent, area);
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

        let max_scroll = ChatSupport::max_scroll(agent, area);
        let current = if agent.chat_follow_output {
            max_scroll
        } else {
            agent.chat_scroll.min(max_scroll)
        };
        let next = current.saturating_add(lines).min(max_scroll);

        agent.chat_scroll = next;
        agent.chat_follow_output = next >= max_scroll;
    }

    pub(super) fn handle_key(
        &mut self,
        key: KeyEvent,
        codex: &CodexAppServer,
        ui_tx: &mpsc::UnboundedSender<UiEvent>,
        area: Rect,
    ) {
        if self.current_tab == AppTab::Workspace {
            self.clear_workspace_hover();
        }
        if WorkspaceComponent::handle_key(self, key, area) {
            return;
        }
        if GitDiffComponent::handle_key(self, key, ui_tx) {
            return;
        }

        match key.code {
            KeyCode::Char('q') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.should_quit = true;
            }
            KeyCode::Char('r') if key.modifiers.contains(KeyModifiers::ALT) => {
                self.should_restart = true;
                self.should_quit = true;
            }
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.should_quit = true;
            }
            KeyCode::Char('t') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                ShellComponent::open_tab_and_create_session(self, ui_tx.clone());
            }
            KeyCode::Left if key.modifiers.contains(KeyModifiers::CONTROL) => {
                let tab = match self.current_tab {
                    AppTab::Chat => AppTab::GitDiff,
                    AppTab::Workspace => AppTab::Chat,
                    AppTab::Shell => AppTab::Workspace,
                    AppTab::GitDiff => AppTab::Shell,
                };
                TopNavigationComponent::activate_tab(self, tab, ui_tx.clone());
            }
            KeyCode::Right if key.modifiers.contains(KeyModifiers::CONTROL) => {
                let tab = match self.current_tab {
                    AppTab::Chat => AppTab::Workspace,
                    AppTab::Workspace => AppTab::Shell,
                    AppTab::Shell => AppTab::GitDiff,
                    AppTab::GitDiff => AppTab::Chat,
                };
                TopNavigationComponent::activate_tab(self, tab, ui_tx.clone());
            }
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
            KeyCode::F(5) => TopNavigationComponent::refresh_current_tab(self),
            KeyCode::Esc if self.add_agent_selected() => {
                self.add_form = AddAgentForm::default();
                if !self.agents.is_empty() {
                    self.chat_sidebar_index =
                        self.current_agent.map(|index| index + 1).unwrap_or(0);
                }
            }
            KeyCode::Esc if self.current_tab == AppTab::Chat => {
                ChatComponent::interrupt_active_turn(self, codex.clone(), ui_tx.clone());
            }
            KeyCode::Tab if self.add_agent_selected() => {
                self.add_form.active_field = match self.add_form.active_field {
                    AddAgentField::Name => AddAgentField::Workspace,
                    AddAgentField::Workspace => AddAgentField::Name,
                };
            }
            KeyCode::Enter if self.add_agent_selected() => {
                AddAgentDialogComponent::submit(self, codex.clone(), ui_tx.clone())
            }
            KeyCode::Enter if self.current_tab == AppTab::Chat => {
                ChatComponent::submit_message(self, codex.clone(), ui_tx.clone());
            }
            KeyCode::Enter if self.current_tab == AppTab::Shell => {
                ShellComponent::submit_command(self, ui_tx.clone());
            }
            KeyCode::Backspace => self.handle_backspace(),
            KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.handle_text_input(character);
            }
            _ => {}
        }
    }

    pub(super) fn handle_text_input(&mut self, character: char) {
        if self.add_agent_selected() {
            self.add_form.error = None;
            match self.add_form.active_field {
                AddAgentField::Name => self.add_form.name.push(character),
                AddAgentField::Workspace => self.add_form.workspace.push(character),
            }
        } else if self.current_tab == AppTab::Workspace {
            self.clear_workspace_hover();
            if let Some(agent) = self.active_agent_mut() {
                if agent.workspace.sidebar_focused()
                    && agent.workspace.sidebar_tab == WorkspaceSidebarTab::Search
                {
                    agent.workspace.push_search_char(character);
                    return;
                }
                if let Some(editor) = agent.workspace.editor.as_mut() {
                    match editor.mode {
                        EditorMode::Insert => editor.insert_char(character),
                        EditorMode::Command => editor.command.push(character),
                        EditorMode::Normal | EditorMode::Visual => {}
                    }
                }
            }
        } else if self.current_tab == AppTab::Shell {
            if let Some(agent) = self.active_agent_mut() {
                agent.shell_tab.input.push(character);
                if let Some(session) = agent.shell_tab.selected_session_mut() {
                    session.scroll = u16::MAX;
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
            self.clear_workspace_hover();
            if let Some(agent) = self.active_agent_mut() {
                if agent.workspace.sidebar_focused()
                    && agent.workspace.sidebar_tab == WorkspaceSidebarTab::Search
                {
                    agent.workspace.pop_search_char();
                    return;
                }
                if let Some(editor) = agent.workspace.editor.as_mut() {
                    match editor.mode {
                        EditorMode::Insert => editor.backspace(),
                        EditorMode::Command => {
                            editor.command.pop();
                        }
                        EditorMode::Normal | EditorMode::Visual => {}
                    }
                }
            }
        } else if self.current_tab == AppTab::Shell {
            if let Some(agent) = self.active_agent_mut() {
                agent.shell_tab.input.pop();
                if let Some(session) = agent.shell_tab.selected_session_mut() {
                    session.scroll = u16::MAX;
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

    pub(super) fn handle_server_event(&mut self, event: ServerEvent) {
        match event {
            ServerEvent::ThreadStatusChanged { thread_id, active } => {
                if let Some(agent) = self.find_agent_by_thread_mut(&thread_id) {
                    agent.thinking = active;
                }
            }
            ServerEvent::TurnStarted { thread_id, turn_id } => {
                if let Some(agent) = self.find_agent_by_thread_mut(&thread_id) {
                    agent.active_turn_id = Some(turn_id);
                    agent.thinking = true;
                }
            }
            ServerEvent::ItemStarted { thread_id, item } => {
                if let Some(agent) = self.find_agent_by_thread_mut(&thread_id) {
                    if let ThreadItem::AgentMessage { id, text } = item {
                        agent.streaming_item_id = Some(id.clone());
                        agent.push_message(ChatMessage::new(
                            MessageRole::Assistant,
                            text,
                            Some(id),
                        ));
                    }
                }
            }
            ServerEvent::ItemCompleted { thread_id, item } => {
                if let Some(agent) = self.find_agent_by_thread_mut(&thread_id) {
                    match item {
                        ThreadItem::AgentMessage { id, text } => {
                            MessageStore::upsert(agent, MessageRole::Assistant, &id, text);
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
                        message.append_text(&delta);
                        agent.invalidate_chat_render_cache();
                    } else {
                        agent.push_message(ChatMessage::new(
                            MessageRole::Assistant,
                            delta,
                            Some(item_id),
                        ));
                    }
                }
            }
            ServerEvent::TurnCompleted {
                thread_id,
                turn_id,
                interrupted,
            } => {
                if let Some(agent) = self.find_agent_by_thread_mut(&thread_id) {
                    if agent.active_turn_id.as_deref() == Some(turn_id.as_str()) {
                        agent.active_turn_id = None;
                    }
                    agent.thinking = false;
                    agent.streaming_item_id = None;
                    if interrupted {
                        agent.status = Some("Response canceled".to_string());
                    }
                }
                if interrupted {
                    self.status_message = Some("Response canceled".to_string());
                }
            }
            ServerEvent::Warning(message)
            | ServerEvent::Error(message)
            | ServerEvent::TransportError(message) => {
                self.status_message = Some(message);
            }
        }
    }

    pub(super) fn handle_ui_event(&mut self, event: UiEvent) {
        match event {
            UiEvent::ThreadReady {
                agent_index,
                thread,
            } => {
                if let Some(agent) = self.agents.get_mut(agent_index) {
                    agent.thread_id = Some(thread.id);
                    agent.thread_loaded = true;
                    if !agent.chat_settings_explicit {
                        agent.chat_model = thread.model;
                        agent.chat_reasoning_effort = thread.reasoning_effort;
                    }
                    agent.chat_model_label = ChatSupport::resolve_chat_model_label(
                        agent.chat_model.as_deref(),
                        agent.chat_reasoning_effort.as_deref(),
                        self.default_chat_model.as_deref(),
                        &self.chat_model_label,
                    );
                }
            }
            UiEvent::ModelCommandResult {
                agent_index,
                message,
            } => {
                if let Some(agent) = self.agents.get_mut(agent_index) {
                    agent.status = Some("Model command finished".to_string());
                    agent.push_message(ChatMessage::new(
                        MessageRole::System,
                        message.clone(),
                        None,
                    ));
                }
                self.status_message = Some("Model command finished".to_string());
            }
            UiEvent::SessionLoaded {
                agent_index,
                session,
            } => {
                if let (Some(agent), Some(session)) = (self.agents.get_mut(agent_index), session) {
                    if agent.thread_id.is_none() && agent.messages.is_empty() {
                        let (thread_id, messages) = SessionLoader::session_messages(session);
                        agent.thread_id = Some(thread_id);
                        agent.thread_loaded = false;
                        agent.replace_messages(messages);
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
                    agent.active_turn_id = None;
                    agent.streaming_item_id = None;
                    agent.status = Some(message.clone());
                    agent.push_message(ChatMessage::new(
                        MessageRole::System,
                        message.clone(),
                        None,
                    ));
                }
                self.status_message = Some(message);
            }
            UiEvent::TurnStartedLocal {
                agent_index,
                turn_id,
            } => {
                if let Some(agent) = self.agents.get_mut(agent_index) {
                    agent.active_turn_id = Some(turn_id);
                    agent.thinking = true;
                }
            }
            UiEvent::ShellCompleted {
                agent_index,
                output,
                success,
            } => {
                if let Some(agent) = self.agents.get_mut(agent_index) {
                    agent.shell_running = false;
                    agent.push_message(ChatMessage::new(MessageRole::Shell, output, None));
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
            UiEvent::TurnInterruptFailed {
                agent_index,
                message,
            } => {
                if let Some(agent) = self.agents.get_mut(agent_index) {
                    agent.status = Some(message.clone());
                    agent.push_message(ChatMessage::new(
                        MessageRole::System,
                        message.clone(),
                        None,
                    ));
                }
                self.status_message = Some(message);
            }
            UiEvent::GitDiffRemoteCompleted {
                agent_index,
                action,
                success,
                message,
            } => {
                if let Some(agent) = self.agents.get_mut(agent_index) {
                    let root = agent.definition.workspace.clone();
                    agent
                        .git_diff
                        .complete_remote_action(&root, action, success, message.clone());
                    agent.workspace.refresh(&root);
                }
                self.status_message = Some(if success {
                    format!("Git {} finished", action.label().to_lowercase())
                } else {
                    message
                });
            }
            UiEvent::ShellSessionReady {
                agent_index,
                session_id,
            } => {
                if let Some(agent) = self.agents.get_mut(agent_index) {
                    if let Some(session) = agent.shell_tab.session_by_id_mut(session_id) {
                        session.mark_ready();
                    }
                }
            }
            UiEvent::ShellSessionOutput {
                agent_index,
                session_id,
                line,
                stderr,
            } => {
                if let Some(agent) = self.agents.get_mut(agent_index) {
                    if let Some(session) = agent.shell_tab.session_by_id_mut(session_id) {
                        if stderr {
                            session.append_stderr_line(&line);
                        } else {
                            session.append_stdout_line(&line);
                        }
                    }
                }
            }
            UiEvent::ShellSessionCommandFinished {
                agent_index,
                session_id,
                exit_code,
            } => {
                if let Some(agent) = self.agents.get_mut(agent_index) {
                    if let Some(session) = agent.shell_tab.session_by_id_mut(session_id) {
                        session.finish_command(exit_code);
                    }
                    let root = agent.definition.workspace.clone();
                    agent.workspace.refresh(&root);
                    agent.git_diff.refresh(&root);
                }
            }
            UiEvent::ShellSessionExited {
                agent_index,
                session_id,
                message,
            } => {
                self.shell_runtimes.remove(&ShellSessionKey {
                    agent_index,
                    session_id,
                });
                if let Some(agent) = self.agents.get_mut(agent_index) {
                    agent.shell_tab.remove_session_by_id(session_id);
                    if agent.shell_tab.sessions.is_empty() {
                        agent.shell_tab.input.clear();
                    }
                }
                self.status_message = Some(message);
            }
            UiEvent::LspHoverResult {
                agent_index,
                path,
                position,
                contents,
                error,
            } => {
                let Some(agent) = self.agents.get_mut(agent_index) else {
                    return;
                };
                let Some(editor) = agent.workspace.editor.as_mut() else {
                    return;
                };
                if editor.path != path {
                    return;
                }
                if !editor.resolve_hover(position, contents) {
                    return;
                }
                if let Some(error) = error {
                    editor.status = Some(error);
                }
            }
            UiEvent::LspDefinitionResult {
                agent_index,
                source_path,
                _source_position: _,
                target,
                error,
            } => {
                let Some(agent) = self.agents.get_mut(agent_index) else {
                    return;
                };
                let Some(editor) = agent.workspace.editor.as_ref() else {
                    return;
                };
                if editor.path != source_path {
                    return;
                }

                if let Some(error) = error {
                    if let Some(editor) = agent.workspace.editor.as_mut() {
                        editor.status = Some(error);
                    }
                    return;
                }

                let Some(target) = target else {
                    if let Some(editor) = agent.workspace.editor.as_mut() {
                        editor.status = Some("Definition not found".to_string());
                    }
                    return;
                };

                if !agent
                    .workspace
                    .entries
                    .iter()
                    .any(|entry| entry.path == target.path)
                {
                    if let Some(editor) = agent.workspace.editor.as_mut() {
                        editor.status = Some("Definition is outside the workspace.".to_string());
                    }
                    return;
                }

                if let Err(error) = agent.workspace.open_path_at_position(
                    &target.path,
                    target.position.row,
                    target.position.col,
                ) {
                    if let Some(editor) = agent.workspace.editor.as_mut() {
                        editor.status = Some(format!("Definition lookup failed: {error}"));
                    }
                }
            }
        }
    }

    fn clear_workspace_hover(&mut self) {
        self.pending_workspace_hover = None;
        let Some(agent) = self.active_agent_mut() else {
            return;
        };
        let Some(editor) = agent.workspace.editor.as_mut() else {
            return;
        };
        editor.clear_hover();
    }

    pub(in crate::app) fn set_pending_workspace_hover(
        &mut self,
        agent_index: usize,
        column: u16,
        row: u16,
        path: PathBuf,
        position: EditorPosition,
    ) {
        let is_same_pending = self
            .pending_workspace_hover
            .as_ref()
            .is_some_and(|pending| {
                pending.agent_index == agent_index
                    && pending.column == column
                    && pending.row == row
                    && pending.path == path
                    && pending.position == position
            });
        if is_same_pending {
            return;
        }

        self.clear_workspace_hover();
        self.pending_workspace_hover = Some(PendingWorkspaceHover {
            agent_index,
            column,
            row,
            path,
            position,
            started_at: Instant::now(),
        });
    }

    fn dispatch_pending_workspace_hover(&mut self, ui_tx: &mpsc::UnboundedSender<UiEvent>) -> bool {
        let Some(pending) = self.pending_workspace_hover.clone() else {
            return false;
        };
        if pending.started_at.elapsed() < HOVER_POPOVER_DELAY {
            return false;
        }

        let Some((path, source, position)) = ({
            let Some(agent) = self.agents.get_mut(pending.agent_index) else {
                self.pending_workspace_hover = None;
                return false;
            };
            let Some(editor) = agent.workspace.editor.as_mut() else {
                self.pending_workspace_hover = None;
                return false;
            };
            if editor.path != pending.path {
                self.pending_workspace_hover = None;
                return false;
            }
            if !editor.request_hover(pending.position) {
                self.pending_workspace_hover = None;
                return false;
            }

            Some((editor.path.clone(), editor.source_text(), pending.position))
        }) else {
            return false;
        };

        self.pending_workspace_hover = None;
        self.request_lsp_hover(pending.agent_index, path, source, position, ui_tx);
        true
    }

    fn lsp_command_tx(
        &mut self,
        agent_index: usize,
        server_index: usize,
        ui_tx: &mpsc::UnboundedSender<UiEvent>,
    ) -> anyhow::Result<std::sync::mpsc::Sender<lsp::LspCommand>> {
        let key = LspRuntimeKey {
            agent_index,
            server_index,
        };
        if let Some(runtime) = self.lsp_runtimes.get(&key) {
            return Ok(runtime.command_tx.clone());
        }

        let workspace_root = self.agents[agent_index].definition.workspace.clone();
        let server = self.lsp_servers[server_index].clone();
        let command_tx = lsp::LspRuntimeFactory::spawn(&workspace_root, server, ui_tx.clone())?;
        self.lsp_runtimes.insert(
            key,
            LspRuntime {
                command_tx: command_tx.clone(),
            },
        );
        Ok(command_tx)
    }

    fn lsp_server_index_for_path(&self, path: &std::path::Path) -> Option<usize> {
        self.lsp_server_for_path(path)
            .map(|(server_index, _)| server_index)
    }

    fn lsp_server_error_for_path(&self, path: &std::path::Path) -> String {
        if let Some(extension) = path.extension().and_then(|extension| extension.to_str()) {
            format!("No LSP server configured for .{} files.", extension)
        } else {
            "No LSP server configured for files without extension.".to_string()
        }
    }

    pub(super) fn request_lsp_hover(
        &mut self,
        agent_index: usize,
        path: PathBuf,
        source: String,
        position: EditorPosition,
        ui_tx: &mpsc::UnboundedSender<UiEvent>,
    ) {
        let Some(server_index) = self.lsp_server_index_for_path(&path) else {
            return;
        };
        let server_name = self.lsp_servers[server_index].name.clone();

        let command_tx = match self.lsp_command_tx(agent_index, server_index, ui_tx) {
            Ok(command_tx) => command_tx,
            Err(error) => {
                if let Some(agent) = self.agents.get_mut(agent_index) {
                    if let Some(editor) = agent.workspace.editor.as_mut() {
                        editor.status = Some(error.to_string());
                    }
                }
                return;
            }
        };

        if command_tx
            .send(lsp::LspCommand::Hover {
                agent_index,
                path,
                source,
                position,
            })
            .is_err()
        {
            self.lsp_runtimes.remove(&LspRuntimeKey {
                agent_index,
                server_index,
            });
            if let Some(agent) = self.agents.get_mut(agent_index) {
                if let Some(editor) = agent.workspace.editor.as_mut() {
                    editor.status =
                        Some(format!("Failed to send hover request to {}", server_name));
                }
            }
        }
    }

    pub(super) fn request_lsp_definition(
        &mut self,
        agent_index: usize,
        path: PathBuf,
        source: String,
        position: EditorPosition,
        ui_tx: &mpsc::UnboundedSender<UiEvent>,
    ) {
        let Some(server_index) = self.lsp_server_index_for_path(&path) else {
            let error_message = self.lsp_server_error_for_path(&path);
            if let Some(agent) = self.agents.get_mut(agent_index) {
                if let Some(editor) = agent.workspace.editor.as_mut() {
                    editor.status = Some(error_message);
                }
            }
            return;
        };
        let server_name = self.lsp_servers[server_index].name.clone();

        let command_tx = match self.lsp_command_tx(agent_index, server_index, ui_tx) {
            Ok(command_tx) => command_tx,
            Err(error) => {
                if let Some(agent) = self.agents.get_mut(agent_index) {
                    if let Some(editor) = agent.workspace.editor.as_mut() {
                        editor.status = Some(error.to_string());
                    }
                }
                return;
            }
        };

        if command_tx
            .send(lsp::LspCommand::Definition {
                agent_index,
                path,
                source,
                position,
            })
            .is_err()
        {
            self.lsp_runtimes.remove(&LspRuntimeKey {
                agent_index,
                server_index,
            });
            if let Some(agent) = self.agents.get_mut(agent_index) {
                if let Some(editor) = agent.workspace.editor.as_mut() {
                    editor.status = Some(format!(
                        "Failed to send definition request to {}",
                        server_name
                    ));
                }
            }
        }
    }

    fn find_agent_by_thread_mut(&mut self, thread_id: &str) -> Option<&mut AgentState> {
        self.agents
            .iter_mut()
            .find(|agent| agent.thread_id.as_deref() == Some(thread_id))
    }

    pub(in crate::app) fn compute_layout(&self, area: Rect) -> UiLayout {
        let frame = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(1),
                Constraint::Length(1),
            ])
            .split(area);

        let root = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(SIDEBAR_WIDTH), Constraint::Min(40)])
            .split(frame[1]);

        if self.current_tab == AppTab::Chat && !self.add_agent_selected() {
            let input_height = ChatInputComponent::height_for_main_area(&self.chat_input, root[1]);
            let main = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(10), Constraint::Length(input_height)])
                .split(root[1]);

            UiLayout {
                sidebar_list: root[0],
                tabs: frame[0],
                body: main[0],
                footer: Some(main[1]),
                add_name: None,
                add_workspace: None,
            }
        } else if self.current_tab == AppTab::Shell {
            UiLayout {
                sidebar_list: root[0],
                tabs: frame[0],
                body: root[1],
                footer: None,
                add_name: None,
                add_workspace: None,
            }
        } else if self.current_tab == AppTab::Chat {
            let outer = UiSupport::rounded_block().title("New Agent");
            let inner = outer.inner(root[1]);
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
                sidebar_list: root[0],
                tabs: frame[0],
                body: root[1],
                footer: None,
                add_name: Some(fields[1]),
                add_workspace: Some(fields[2]),
            }
        } else {
            UiLayout {
                sidebar_list: root[0],
                tabs: frame[0],
                body: root[1],
                footer: None,
                add_name: None,
                add_workspace: None,
            }
        }
    }

    fn handle_left_click(
        &mut self,
        column: u16,
        row: u16,
        modifiers: KeyModifiers,
        layout: UiLayout,
        ui_tx: &mpsc::UnboundedSender<UiEvent>,
    ) {
        if UiSupport::rect_contains(layout.tabs, column, row) {
            if let Some(tab) = TopNavigationComponent::tab_from_click(layout.tabs, column, row) {
                TopNavigationComponent::activate_tab(self, tab, ui_tx.clone());
            }
            return;
        }

        if UiSupport::rect_contains(layout.sidebar_list, column, row) {
            self.handle_sidebar_click(column, row, layout.sidebar_list, ui_tx);
            return;
        }

        if self.current_tab == AppTab::GitDiff
            && GitDiffComponent::handle_click(self, column, row, layout.body, ui_tx)
        {
            return;
        }

        if self.current_tab == AppTab::Workspace
            && self
                .active_agent()
                .is_some_and(|agent| agent.workspace.editor.is_some())
        {
            self.clear_workspace_hover();
            if modifiers.contains(KeyModifiers::CONTROL)
                && WorkspaceComponent::handle_editor_definition_click(
                    self,
                    column,
                    row,
                    layout.body,
                    ui_tx,
                )
            {
                return;
            }
            self.active_workspace_selection_drag =
                WorkspaceComponent::handle_editor_click(self, column, row, layout.body);
            return;
        }

        if self.add_agent_selected() {
            if layout
                .add_name
                .is_some_and(|rect| UiSupport::rect_contains(rect, column, row))
            {
                self.add_form.active_field = AddAgentField::Name;
                return;
            }
            if layout
                .add_workspace
                .is_some_and(|rect| UiSupport::rect_contains(rect, column, row))
            {
                self.add_form.active_field = AddAgentField::Workspace;
            }
        }
    }

    fn handle_scroll(
        &mut self,
        column: u16,
        row: u16,
        layout: UiLayout,
        up: bool,
        horizontal: bool,
    ) {
        if UiSupport::rect_contains(layout.sidebar_list, column, row) {
            if up {
                self.move_selection_up();
            } else {
                self.move_selection_down();
            }
            return;
        }

        if UiSupport::rect_contains(layout.body, column, row)
            || layout
                .footer
                .is_some_and(|rect| UiSupport::rect_contains(rect, column, row))
        {
            if horizontal
                && self.current_tab == AppTab::Workspace
                && self
                    .active_agent()
                    .is_some_and(|agent| agent.workspace.editor.is_some())
            {
                self.clear_workspace_hover();
                if let Some(agent) = self.active_agent_mut() {
                    if let Some(editor) = agent.workspace.editor.as_mut() {
                        let viewport = WorkspaceEditorComponent::viewport(layout.body);
                        if up {
                            editor.scroll_left(MOUSE_SCROLL_STEP);
                        } else {
                            editor.scroll_right(MOUSE_SCROLL_STEP, viewport.width);
                        }
                        return;
                    }
                }
            }

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

    fn handle_sidebar_click(
        &mut self,
        column: u16,
        row: u16,
        sidebar_list: Rect,
        ui_tx: &mpsc::UnboundedSender<UiEvent>,
    ) {
        match self.current_tab {
            AppTab::Chat => AgentsSidebarComponent::handle_click(self, column, row, sidebar_list),
            AppTab::Workspace => {
                WorkspaceSidebarComponent::handle_click(self, column, row, sidebar_list)
            }
            AppTab::Shell => {
                ShellSidebarComponent::handle_click(self, column, row, sidebar_list, ui_tx)
            }
            AppTab::GitDiff => {
                GitDiffSidebarComponent::handle_click(self, column, row, sidebar_list)
            }
        }
    }
}
