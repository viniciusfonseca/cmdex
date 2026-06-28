use super::{chat::*, ui::*, *};

impl App {
    pub(super) fn sidebar_labels(&self) -> Vec<String> {
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

    pub(super) fn on_tick(&mut self) {
        self.spinner_index = (self.spinner_index + 1) % SPINNER.len();
        self.maybe_refresh_workspace();
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

    pub(super) fn refresh_current_tab(&mut self) {
        match self.current_tab {
            AppTab::Chat => {}
            AppTab::Workspace => {
                if let Some(agent) = self.active_agent_mut() {
                    agent.workspace.refresh(&agent.definition.workspace);
                }
                self.last_workspace_refresh_at = Some(Instant::now());
            }
            AppTab::GitDiff => {
                if let Some(agent) = self.active_agent_mut() {
                    agent.git_diff.refresh(&agent.definition.workspace);
                }
            }
        }
    }

    fn maybe_refresh_workspace(&mut self) {
        if self.current_tab != AppTab::Workspace {
            return;
        }

        let now = Instant::now();
        if self
            .last_workspace_refresh_at
            .is_some_and(|last| now.duration_since(last) < WORKSPACE_AUTO_REFRESH_INTERVAL)
        {
            return;
        }

        self.last_workspace_refresh_at = Some(now);
        if let Some(agent) = self.active_agent_mut() {
            agent
                .workspace
                .refresh_if_changed(&agent.definition.workspace);
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

    pub(super) fn handle_mouse(&mut self, mouse: MouseEvent, area: Rect) {
        let layout = self.compute_layout(area);

        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if self.handle_scrollbar_press(mouse.column, mouse.row, layout) {
                    return;
                }
                self.handle_left_click(mouse.column, mouse.row, layout);
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                if self.handle_scrollbar_drag(mouse.row, layout) {
                    return;
                }
            }
            MouseEventKind::Up(MouseButton::Left) => {
                self.active_scrollbar_drag = None;
            }
            MouseEventKind::ScrollUp if self.should_handle_mouse_scroll(ScrollDirection::Up) => {
                self.handle_scroll(mouse.column, mouse.row, layout, true)
            }
            MouseEventKind::ScrollDown
                if self.should_handle_mouse_scroll(ScrollDirection::Down) =>
            {
                self.handle_scroll(mouse.column, mouse.row, layout, false)
            }
            _ => {}
        }
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
            AppTab::GitDiff => ScrollbarDragTarget::GitDiffPreview,
            AppTab::Chat => return None,
        };

        let metrics = self.scrollbar_metrics(target, layout)?;
        rect_contains(metrics.track, column, row).then_some(target)
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
        let scroll = scroll_position_from_row(metrics, row);

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
                let text = Text::from(chat_lines(agent));
                let content_length = scrollable_text_height(&text, layout.body);
                vertical_scrollbar_metrics(layout.body, content_length)
            }
            ScrollbarDragTarget::WorkspacePreview => {
                let content_length =
                    scrollable_preview_content_height(&agent.workspace.preview, layout.body);
                vertical_scrollbar_metrics(layout.body, content_length)
            }
            ScrollbarDragTarget::WorkspaceEditor => {
                let editor = agent.workspace.editor.as_ref()?;
                let viewport = workspace_editor_viewport(layout.body);
                vertical_scrollbar_metrics_for_viewport(viewport, editor.content_height())
            }
            ScrollbarDragTarget::GitDiffPreview => {
                let diff_layout = git_diff_layout(layout.body);
                let content_length =
                    scrollable_preview_content_height(&agent.git_diff.preview, diff_layout.preview);
                vertical_scrollbar_metrics(diff_layout.preview, content_length)
            }
        }
    }

    fn should_handle_mouse_scroll(&mut self, direction: ScrollDirection) -> bool {
        self.should_handle_mouse_scroll_at(direction, Instant::now())
    }

    pub(super) fn should_handle_mouse_scroll_at(
        &mut self,
        direction: ScrollDirection,
        now: Instant,
    ) -> bool {
        if self
            .last_mouse_scroll
            .is_some_and(|(last_direction, last_at)| {
                last_direction == direction
                    && now
                        .checked_duration_since(last_at)
                        .is_some_and(|elapsed| elapsed < MOUSE_SCROLL_DEBOUNCE)
            })
        {
            return false;
        }

        self.last_mouse_scroll = Some((direction, now));
        true
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

    pub(super) fn handle_key(
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

    pub(super) fn handle_text_input(&mut self, character: char) {
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

                self.agents.push(AgentState::new(
                    agent,
                    self.default_chat_model.clone(),
                    self.default_chat_reasoning_effort.clone(),
                    &self.chat_model_label,
                ));
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
        if let Some(command) = chat_command_from_input(&self.chat_input) {
            self.submit_chat_command(command, codex, ui_tx);
            return;
        }

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
        let selected_model = agent.chat_model.clone();
        let selected_effort = agent.chat_reasoning_effort.clone();
        let workspace = agent.definition.workspace.clone();
        self.chat_input.clear();

        tokio::spawn(async move {
            let thread_id = match existing_thread {
                Some(thread_id) => {
                    if !thread_loaded {
                        match codex
                            .resume_thread(&thread_id, selected_model.as_deref())
                            .await
                        {
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
                None => match codex
                    .start_thread(&workspace, selected_model.as_deref())
                    .await
                {
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

            if let Err(error) = codex
                .start_turn(
                    &thread_id,
                    &text,
                    selected_model.as_deref(),
                    selected_effort.as_deref(),
                )
                .await
            {
                let _ = ui_tx.send(UiEvent::SubmissionFailed {
                    agent_index,
                    message: error.to_string(),
                });
            }
        });
    }

    fn submit_chat_command(
        &mut self,
        command: ChatCommand,
        codex: CodexAppServer,
        ui_tx: mpsc::UnboundedSender<UiEvent>,
    ) {
        match command {
            ChatCommand::Model(command) => self.submit_model_command(command, codex, ui_tx),
        }
    }

    fn submit_model_command(
        &mut self,
        command: ModelCommand,
        codex: CodexAppServer,
        ui_tx: mpsc::UnboundedSender<UiEvent>,
    ) {
        let Some(agent_index) = self.current_agent else {
            self.status_message = Some("Add an agent before changing the model.".to_string());
            return;
        };

        self.chat_input.clear();

        match command {
            ModelCommand::List => {
                let current_label = self.agents[agent_index].chat_model_label.clone();
                self.agents[agent_index].status = Some("Loading available models...".to_string());
                tokio::spawn(async move {
                    let message = match codex.list_models().await {
                        Ok(models) => format_model_list_message(&current_label, &models),
                        Err(error) => format!("Unable to load available models.\n\n{error}"),
                    };
                    let _ = ui_tx.send(UiEvent::ModelCommandResult {
                        agent_index,
                        message,
                    });
                });
            }
            ModelCommand::ResetDefault => {
                let agent = &mut self.agents[agent_index];
                agent.chat_model = self.default_chat_model.clone();
                agent.chat_reasoning_effort = self.default_chat_reasoning_effort.clone();
                agent.chat_model_label = self.chat_model_label.clone();
                agent.chat_settings_explicit = false;
                let message = format!("Model set to `{}`.", agent.chat_model_label);
                agent.status = Some(message.clone());
                agent.messages.push(ChatMessage {
                    role: MessageRole::System,
                    text: message,
                    item_id: None,
                });
                self.status_message = Some("Model updated".to_string());
            }
            ModelCommand::Set { model, effort } => {
                let default_model = self.default_chat_model.clone();
                let default_label = self.chat_model_label.clone();
                let agent = &mut self.agents[agent_index];
                if let Some(model) = model {
                    agent.chat_model = Some(model);
                }
                if let Some(effort) = effort {
                    agent.chat_reasoning_effort = Some(effort);
                }
                agent.chat_settings_explicit = true;
                agent.chat_model_label = resolve_chat_model_label(
                    agent.chat_model.as_deref(),
                    agent.chat_reasoning_effort.as_deref(),
                    default_model.as_deref(),
                    &default_label,
                );
                let message = format!("Model set to `{}`.", agent.chat_model_label);
                agent.status = Some(message.clone());
                agent.messages.push(ChatMessage {
                    role: MessageRole::System,
                    text: message,
                    item_id: None,
                });
                self.status_message = Some("Model updated".to_string());
            }
        }
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

    pub(super) fn handle_server_event(&mut self, event: ServerEvent) {
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
                    agent.chat_model_label = resolve_chat_model_label(
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
                    agent.messages.push(ChatMessage {
                        role: MessageRole::System,
                        text: message.clone(),
                        item_id: None,
                    });
                }
                self.status_message = Some("Model command finished".to_string());
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

    pub(super) fn selected_tab_index(&self) -> usize {
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
            .constraints([Constraint::Length(LOGO_PANEL_HEIGHT), Constraint::Min(10)])
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

fn format_model_list_message(current_label: &str, models: &[ModelInfo]) -> String {
    if models.is_empty() {
        return format!(
            "Current model: `{current_label}`\n\nNo visible models were returned by the app server."
        );
    }

    let mut lines = vec![
        format!("Current model: `{current_label}`"),
        String::new(),
        "Available models:".to_string(),
    ];

    for model in models {
        let mut line = format!("- `{}`", model.model);
        if model.id != model.model {
            line.push_str(&format!(" [{}]", model.id));
        }
        if model.display_name != model.model {
            line.push_str(&format!(" - {}", model.display_name));
        }
        if model.is_default {
            line.push_str(" (default)");
        }
        lines.push(line);
    }

    lines.push(String::new());
    lines.push("Use `/model <id>` to switch models.".to_string());
    lines.push("Use `/model <id> <effort>` to switch model and effort together.".to_string());
    lines.push("Use `/model <effort>` to change only the effort.".to_string());
    lines.push("Use `/model default` to go back to the configured default.".to_string());
    lines.join("\n")
}
