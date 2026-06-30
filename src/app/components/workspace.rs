use super::super::*;
use super::shared::UiSupport;

pub(in crate::app) fn draw_workspace_sidebar(frame: &mut Frame, app: &App, area: Rect) {
    let Some(agent) = app.active_agent() else {
        let empty = Paragraph::new("Select or create an agent in the Chat tab.")
            .block(UiSupport::sidebar_block().title("Workspace"))
            .style(UiSupport::sidebar_style());
        frame.render_widget(empty, area);
        return;
    };

    let layout = workspace_sidebar_layout(area, agent.workspace.sidebar_tab);
    let tabs = Tabs::new(["Files", "Search"])
        .select(match agent.workspace.sidebar_tab {
            WorkspaceSidebarTab::Files => 0,
            WorkspaceSidebarTab::Search => 1,
        })
        .block(UiSupport::sidebar_block().title("Workspace"))
        .style(UiSupport::sidebar_style())
        .highlight_style(UiSupport::tab_highlight_style());
    frame.render_widget(tabs, layout.tabs);

    match agent.workspace.sidebar_tab {
        WorkspaceSidebarTab::Files => {
            let items = agent
                .workspace
                .sidebar_items()
                .into_iter()
                .map(ListItem::new)
                .collect::<Vec<_>>();
            let selected = agent
                .workspace
                .sidebar_selected_row()
                .min(items.len().saturating_sub(1));
            let visible_rows = UiSupport::inner_rect(layout.content).height as usize;
            let offset = UiSupport::list_offset(selected, items.len(), visible_rows);
            let mut state = ListState::default()
                .with_offset(offset)
                .with_selected(Some(selected));

            let list = List::new(items)
                .block(UiSupport::sidebar_block().title("Files"))
                .style(UiSupport::sidebar_style())
                .highlight_style(UiSupport::selection_style())
                .highlight_symbol("› ");
            frame.render_stateful_widget(list, layout.content, &mut state);
            UiSupport::render_vertical_scrollbar(
                frame,
                layout.content,
                agent.workspace.sidebar_len(),
                offset as u16,
            );
        }
        WorkspaceSidebarTab::Search => {
            let input = Paragraph::new(agent.workspace.search_query.as_str())
                .block(UiSupport::panel_block().title("Search"))
                .style(UiSupport::panel_style());
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
                .block(UiSupport::sidebar_block().title(format!(
                    "Results ({})",
                    agent.workspace.search_match_count()
                )))
                .style(UiSupport::sidebar_style())
                .highlight_style(UiSupport::selection_style())
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

pub(in crate::app) fn workspace_sidebar_layout(
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

pub(in crate::app) fn workspace_sidebar_tab_from_click(
    area: Rect,
    column: u16,
    row: u16,
) -> Option<WorkspaceSidebarTab> {
    let inner = UiSupport::inner_rect(area);
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

    if UiSupport::rect_contains(files, column, row) {
        Some(WorkspaceSidebarTab::Files)
    } else if UiSupport::rect_contains(search, column, row) {
        Some(WorkspaceSidebarTab::Search)
    } else {
        None
    }
}

pub(in crate::app) fn draw_workspace(frame: &mut Frame, app: &App, area: Rect) {
    let Some(agent) = app.active_agent() else {
        let empty = Paragraph::new("Select or create an agent in the Chat tab.")
            .block(UiSupport::panel_block().title("Workspace"))
            .style(UiSupport::panel_style());
        frame.render_widget(empty, area);
        return;
    };

    if let Some(editor) = agent.workspace.editor.as_ref() {
        draw_workspace_editor(frame, editor, area);
        return;
    }

    let mut lines = vec![Line::from(format!(
        "Workspace: {}",
        ConfigStore::compact_home(&agent.definition.workspace)
    ))];
    lines.push(Line::from(String::new()));
    lines.extend(agent.workspace.preview.iter().cloned());
    if let Some(error) = &agent.workspace.error {
        lines.push(Line::from(String::new()));
        lines.push(Line::from(Span::styled(
            error.clone(),
            Style::default()
                .fg(UiSupport::theme().error)
                .bg(UiSupport::theme().panel_bg),
        )));
    }
    let content_length = UiSupport::scrollable_preview_content_height(&lines, area);

    let widget = Paragraph::new(Text::from(lines))
        .block(UiSupport::panel_block().title(agent.workspace.preview_title.clone()))
        .style(UiSupport::panel_style())
        .scroll((agent.workspace.content_scroll, 0))
        .wrap(Wrap { trim: false });
    frame.render_widget(widget, area);
    UiSupport::render_vertical_scrollbar(
        frame,
        area,
        content_length,
        agent.workspace.content_scroll,
    );
}

pub(in crate::app) fn draw_workspace_editor(
    frame: &mut Frame,
    editor: &WorkspaceEditorState,
    area: Rect,
) {
    let mode = match editor.mode {
        EditorMode::Normal => "NORMAL",
        EditorMode::Insert => "INSERT",
        EditorMode::Command => "COMMAND",
    };
    let dirty = if editor.dirty { " [+]" } else { "" };
    let block =
        UiSupport::editor_block().title(format!("{}{} [{}]", editor.path.display(), dirty, mode));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let (code_area, status_area) = workspace_editor_panes(inner);
    let vertical_scroll = editor.clamped_vertical_scroll(code_area.height);
    let lines = editor.rendered_lines(code_area.height);
    let code = Paragraph::new(Text::from(lines))
        .style(UiSupport::editor_style())
        .scroll((0, editor.horizontal_scroll));
    frame.render_widget(code, code_area);
    UiSupport::render_vertical_scrollbar_with_viewport(
        frame,
        code_area,
        editor.content_height(),
        vertical_scroll,
    );

    if let Some(status_area) = status_area {
        let status = workspace_editor_status(editor);
        let status_widget = Paragraph::new(status).style(
            Style::default()
                .bg(UiSupport::theme().app_bg)
                .fg(UiSupport::theme().muted),
        );
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

pub(in crate::app) fn workspace_editor_viewport(area: Rect) -> Rect {
    let inner = area.inner(Margin {
        vertical: 1,
        horizontal: 1,
    });
    workspace_editor_panes(inner).0
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

impl App {
    pub(in crate::app) fn maybe_refresh_workspace(&mut self) {
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

    pub(in crate::app) fn handle_workspace_sidebar_click(
        &mut self,
        column: u16,
        row: u16,
        sidebar_list: Rect,
    ) {
        if let Some(agent) = self.active_agent_mut() {
            let layout = workspace_sidebar_layout(sidebar_list, agent.workspace.sidebar_tab);
            if let Some(tab) = workspace_sidebar_tab_from_click(layout.tabs, column, row) {
                agent.workspace.set_sidebar_tab(tab);
                return;
            }

            match agent.workspace.sidebar_tab {
                WorkspaceSidebarTab::Files => {
                    let inner = UiSupport::inner_rect(layout.content);
                    if inner.height == 0 || !UiSupport::rect_contains(inner, column, row) {
                        return;
                    }

                    let visible_row = row.saturating_sub(inner.y) as usize;
                    let total = agent.workspace.sidebar_len();
                    if total == 0 {
                        return;
                    }
                    let offset = UiSupport::list_offset(
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
                        .is_some_and(|input| UiSupport::rect_contains(input, column, row))
                    {
                        return;
                    }

                    let inner = UiSupport::inner_rect(layout.content);
                    if inner.height == 0 || !UiSupport::rect_contains(inner, column, row) {
                        return;
                    }

                    let visible_row = row.saturating_sub(inner.y) as usize;
                    let total = agent.workspace.search_total_rows();
                    if total == 0 {
                        return;
                    }
                    let offset = UiSupport::list_offset(
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

    pub(in crate::app) fn handle_workspace_key(&mut self, key: KeyEvent, area: Rect) -> bool {
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

    pub(in crate::app) fn handle_workspace_editor_click(
        &mut self,
        column: u16,
        row: u16,
        area: Rect,
    ) {
        let viewport = workspace_editor_viewport(area);
        if !UiSupport::rect_contains(viewport, column, row) {
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
}
