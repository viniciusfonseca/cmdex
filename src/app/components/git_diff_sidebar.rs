use super::super::*;
use super::UiSupport;

pub(in crate::app) struct GitDiffSidebarComponent;

impl GitDiffSidebarComponent {
    pub(in crate::app) fn draw(frame: &mut Frame, app: &App, area: Rect) {
        let items = app
            .active_agent()
            .map(|agent| {
                agent
                    .git_diff
                    .visible_entries()
                    .iter()
                    .map(|entry| entry.label.clone())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
            .into_iter()
            .map(ListItem::new)
            .collect::<Vec<_>>();

        let mut state = ListState::default();
        state.select(app.active_agent().map(|agent| {
            agent
                .git_diff
                .selected_index()
                .min(items.len().saturating_sub(1))
        }));

        let list = List::new(items)
            .block(UiSupport::sidebar_block().title("Git Diff"))
            .style(UiSupport::sidebar_style())
            .highlight_style(UiSupport::selection_style())
            .highlight_symbol("› ");
        frame.render_stateful_widget(list, area, &mut state);
    }

    pub(in crate::app) fn handle_click(app: &mut App, column: u16, row: u16, sidebar_list: Rect) {
        let inner = UiSupport::inner_rect(sidebar_list);
        if inner.height == 0 || !UiSupport::rect_contains(inner, column, row) {
            return;
        }

        let visible_row = row.saturating_sub(inner.y) as usize;
        if let Some(agent) = app.active_agent_mut() {
            let total = agent.git_diff.visible_entries().len();
            if total == 0 {
                return;
            }
            let offset = UiSupport::list_offset(
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
