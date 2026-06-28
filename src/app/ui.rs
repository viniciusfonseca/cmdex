use super::{
    chat::{chat_input_is_shell, padded_chat_lines},
    *,
};

pub(super) fn draw(frame: &mut Frame, app: &App) {
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

pub(super) fn rounded_block() -> Block<'static> {
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

pub(super) fn theme() -> &'static crate::theme::AppTheme {
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

fn draw_sidebar(frame: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(LOGO_PANEL_HEIGHT), Constraint::Min(10)])
        .split(area);

    let logo = Paragraph::new(format!("\n{}\n", LOGO.join("\n")))
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

pub(super) fn workspace_sidebar_layout(
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

pub(super) fn workspace_sidebar_tab_from_click(
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

pub(super) fn rect_contains(rect: Rect, column: u16, row: u16) -> bool {
    column >= rect.x
        && column < rect.x.saturating_add(rect.width)
        && row >= rect.y
        && row < rect.y.saturating_add(rect.height)
}

pub(super) fn inner_rect(rect: Rect) -> Rect {
    let width = rect.width.saturating_sub(2);
    let height = rect.height.saturating_sub(2);
    Rect::new(
        rect.x.saturating_add(1),
        rect.y.saturating_add(1),
        width,
        height,
    )
}

pub(super) fn list_offset(selected: usize, len: usize, visible_rows: usize) -> usize {
    if len <= visible_rows || visible_rows == 0 {
        0
    } else {
        selected.saturating_add(1).saturating_sub(visible_rows)
    }
}

pub(super) fn tab_from_click(tabs: Rect, column: u16, row: u16) -> Option<AppTab> {
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

    let lines = padded_chat_lines(agent, area);

    let title = if let Some(status) = &agent.status {
        format!("Chat - {} ({status})", agent.definition.name)
    } else {
        format!("Chat - {}", agent.definition.name)
    };

    let inner_height = area.height.saturating_sub(2);
    let text = Text::from(lines);
    let content_height = scrollable_text_height(&text, area);
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
            app.active_chat_model_label(),
            SPINNER[app.spinner_index]
        )
    } else if shell_mode {
        format!("Shell · {}", app.active_chat_model_label())
    } else if thinking {
        format!(
            "Message · {}  {} Thinking...",
            app.active_chat_model_label(),
            SPINNER[app.spinner_index]
        )
    } else {
        format!("Message · {}", app.active_chat_model_label())
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
    let content_length = scrollable_preview_content_height(&lines, area);

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
    let content_length = scrollable_preview_content_height(&preview_lines, layout.preview);
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

pub(super) fn git_diff_layout(area: Rect) -> GitDiffLayout {
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

pub(super) fn git_diff_section_from_click(
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
    let Some(metrics) = vertical_scrollbar_metrics(area, content_length) else {
        return;
    };
    render_scrollbar_thumb(frame, metrics, scroll);
}

fn render_vertical_scrollbar_with_viewport(
    frame: &mut Frame,
    viewport: Rect,
    content_length: usize,
    scroll: u16,
) {
    let Some(metrics) = vertical_scrollbar_metrics_for_viewport(viewport, content_length) else {
        return;
    };
    render_scrollbar_thumb(frame, metrics, scroll);
}

pub(super) fn vertical_scrollbar_metrics(
    area: Rect,
    content_length: usize,
) -> Option<ScrollbarMetrics> {
    vertical_scrollbar_metrics_for_viewport(inner_rect(area), content_length)
}

pub(super) fn vertical_scrollbar_metrics_for_viewport(
    viewport: Rect,
    content_length: usize,
) -> Option<ScrollbarMetrics> {
    if viewport.width == 0 || viewport.height == 0 {
        return None;
    }
    if content_length <= viewport.height as usize {
        return None;
    }

    Some(ScrollbarMetrics {
        track: Rect::new(
            viewport.x.saturating_add(viewport.width.saturating_sub(1)),
            viewport.y,
            1,
            viewport.height,
        ),
        content_length,
        viewport_length: viewport.height as usize,
    })
}

pub(super) fn scroll_position_from_row(metrics: ScrollbarMetrics, row: u16) -> u16 {
    let max_scroll = metrics
        .content_length
        .saturating_sub(metrics.viewport_length)
        .min(u16::MAX as usize);
    if max_scroll == 0 {
        return 0;
    }

    let (_, thumb_height) = scrollbar_thumb_bounds(metrics, 0).unwrap_or((0, 1));
    let track_travel = metrics.track.height.saturating_sub(thumb_height);
    if track_travel == 0 {
        return 0;
    }

    let thumb_half = thumb_height / 2;
    let row_offset = row
        .saturating_sub(metrics.track.y)
        .saturating_sub(thumb_half)
        .min(track_travel) as usize;
    let track_travel = usize::from(track_travel);

    ((row_offset * max_scroll) + track_travel / 2)
        .checked_div(track_travel)
        .unwrap_or(0) as u16
}

fn render_scrollbar_thumb(frame: &mut Frame, metrics: ScrollbarMetrics, scroll: u16) {
    let Some((thumb_top, thumb_height)) = scrollbar_thumb_bounds(metrics, scroll) else {
        return;
    };

    let lines = (0..metrics.track.height)
        .map(|row| {
            if row >= thumb_top && row < thumb_top.saturating_add(thumb_height) {
                Line::from(Span::styled(
                    "█",
                    Style::default().fg(theme().scrollbar_thumb),
                ))
            } else {
                Line::from(" ")
            }
        })
        .collect::<Vec<_>>();

    frame.render_widget(Paragraph::new(Text::from(lines)), metrics.track);
}

pub(super) fn scrollbar_thumb_bounds(metrics: ScrollbarMetrics, scroll: u16) -> Option<(u16, u16)> {
    if metrics.track.height == 0 || metrics.content_length == 0 || metrics.viewport_length == 0 {
        return None;
    }

    let track_height = usize::from(metrics.track.height);
    let proportional_height = ((metrics.viewport_length * track_height)
        + metrics.content_length / 2)
        / metrics.content_length;
    let thumb_height = proportional_height.clamp(1, track_height) as u16;

    let max_scroll = metrics
        .content_length
        .saturating_sub(metrics.viewport_length)
        .min(u16::MAX as usize) as u16;
    let track_travel = metrics.track.height.saturating_sub(thumb_height);
    let thumb_top = if max_scroll == 0 || track_travel == 0 {
        0
    } else {
        let scroll = scroll.min(max_scroll) as usize;
        let track_travel = usize::from(track_travel);
        (((scroll * track_travel) + usize::from(max_scroll) / 2) / usize::from(max_scroll)) as u16
    };

    Some((thumb_top, thumb_height))
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

pub(super) fn workspace_editor_viewport(area: Rect) -> Rect {
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

pub(super) fn preview_content_height(lines: &[Line<'_>], width: u16) -> usize {
    let width = usize::from(width.max(1));

    lines
        .iter()
        .map(|line| match line.width() {
            0 => 1,
            line_width => line_width.saturating_sub(1) / width + 1,
        })
        .sum()
}

pub(super) fn wrapped_text_height(text: &Text<'_>, width: u16) -> usize {
    Paragraph::new(text.clone())
        .wrap(Wrap { trim: false })
        .line_count(width.max(1))
}

pub(super) fn scrollable_preview_content_height(lines: &[Line<'_>], area: Rect) -> usize {
    let viewport = inner_rect(area);
    let base_height = preview_content_height(lines, viewport.width);
    if base_height > viewport.height as usize && viewport.width > 1 {
        preview_content_height(lines, viewport.width.saturating_sub(1))
    } else {
        base_height
    }
}

pub(super) fn scrollable_text_height(text: &Text<'_>, area: Rect) -> usize {
    let viewport = inner_rect(area);
    let base_height = wrapped_text_height(text, viewport.width);
    if base_height > viewport.height as usize && viewport.width > 1 {
        wrapped_text_height(text, viewport.width.saturating_sub(1))
    } else {
        base_height
    }
}

pub(super) fn chat_input_height_for_main_area(input: &str, main_area: Rect) -> u16 {
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

pub(super) fn wrapped_chat_input_lines(input: &str, width: u16) -> Vec<String> {
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
