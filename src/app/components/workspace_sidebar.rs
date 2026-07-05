use super::super::*;
use super::UiSupport;

pub(in crate::app) struct WorkspaceSidebarComponent;

impl WorkspaceSidebarComponent {
    pub(in crate::app) fn draw(frame: &mut Frame, app: &App, area: Rect) {
        let Some(agent) = app.active_agent() else {
            let empty = Paragraph::new("Select or create an agent in the Chat tab.")
                .block(UiSupport::sidebar_block().title("Workspace"))
                .style(UiSupport::sidebar_style());
            frame.render_widget(empty, area);
            return;
        };

        let layout = Self::layout(area, agent.workspace.sidebar_tab);
        let tabs = Tabs::new(["Files", "Search"])
            .select(match agent.workspace.sidebar_tab {
                WorkspaceSidebarTab::Files => 0,
                WorkspaceSidebarTab::Search => 1,
            })
            .block(UiSupport::focus_block(
                UiSupport::sidebar_block().title("Workspace"),
                agent.workspace.sidebar_focused(),
            ))
            .style(UiSupport::sidebar_style())
            .highlight_style(UiSupport::tab_highlight_style());
        frame.render_widget(tabs, layout.tabs);

        match agent.workspace.sidebar_tab {
            WorkspaceSidebarTab::Files => Self::draw_files(frame, agent, layout.content),
            WorkspaceSidebarTab::Search => Self::draw_search(frame, agent, layout),
        }
    }

    pub(in crate::app) fn handle_click(app: &mut App, column: u16, row: u16, sidebar_list: Rect) {
        if let Some(agent) = app.active_agent_mut() {
            let layout = Self::layout(sidebar_list, agent.workspace.sidebar_tab);
            if let Some(tab) = Self::tab_from_click(layout.tabs, column, row) {
                agent.workspace.focus_sidebar();
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
                    agent.workspace.focus_sidebar();
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
                        agent.workspace.focus_sidebar();
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
                    agent.workspace.focus_sidebar();
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

    pub(in crate::app) fn layout(
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

    pub(in crate::app) fn tab_from_click(
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

    fn draw_files(frame: &mut Frame, agent: &AgentState, area: Rect) {
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
        let visible_rows = UiSupport::inner_rect(area).height as usize;
        let offset = UiSupport::list_offset(selected, items.len(), visible_rows);
        let mut state = ListState::default()
            .with_offset(offset)
            .with_selected(Some(selected));

        let list = List::new(items)
            .block(UiSupport::focus_block(
                UiSupport::sidebar_block().title("Files"),
                agent.workspace.sidebar_focused(),
            ))
            .style(UiSupport::sidebar_style())
            .highlight_style(UiSupport::selection_style())
            .highlight_symbol("› ");
        frame.render_stateful_widget(list, area, &mut state);
        UiSupport::render_vertical_scrollbar(
            frame,
            area,
            agent.workspace.sidebar_len(),
            offset as u16,
        );
    }

    fn draw_search(frame: &mut Frame, agent: &AgentState, layout: WorkspaceSidebarLayout) {
        let sidebar_focused = agent.workspace.sidebar_focused();
        let input = Paragraph::new(agent.workspace.search_query.as_str())
            .block(UiSupport::focus_block(
                UiSupport::panel_block().title("Search"),
                sidebar_focused,
            ))
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
            .block(UiSupport::focus_block(
                UiSupport::sidebar_block().title(format!(
                    "Results ({})",
                    agent.workspace.search_match_count()
                )),
                sidebar_focused,
            ))
            .style(UiSupport::sidebar_style())
            .highlight_style(UiSupport::selection_style())
            .highlight_symbol("› ");
        frame.render_stateful_widget(list, layout.content, &mut state);

        if sidebar_focused {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::app) struct WorkspaceSidebarLayout {
    pub(super) tabs: Rect,
    pub(super) input: Option<Rect>,
    pub(super) content: Rect,
}
