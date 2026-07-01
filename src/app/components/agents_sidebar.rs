use super::super::*;
use super::UiSupport;

pub(in crate::app) struct AgentsSidebarComponent;

impl AgentsSidebarComponent {
    pub(in crate::app) fn draw(frame: &mut Frame, app: &App, area: Rect) {
        let items = Self::labels(app)
            .into_iter()
            .map(ListItem::new)
            .collect::<Vec<_>>();
        let mut state = ListState::default();
        state.select(Some(
            app.chat_sidebar_index.min(items.len().saturating_sub(1)),
        ));

        let list = List::new(items)
            .block(UiSupport::sidebar_block().title("Agents"))
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
        let total = app.agents.len() + 1;
        let offset = UiSupport::list_offset(app.chat_sidebar_index, total, inner.height as usize);
        let index = (offset + visible_row).min(total.saturating_sub(1));
        Self::select_index(app, index);
    }

    pub(in crate::app) fn select_index(app: &mut App, index: usize) {
        app.chat_sidebar_index = index.min(app.agents.len());
        if app.chat_sidebar_index > 0 {
            app.current_agent = Some(app.chat_sidebar_index - 1);
            app.add_form.error = None;
        }
    }

    fn labels(app: &App) -> Vec<String> {
        let mut items = vec!["+ Add agent".to_string()];
        items.extend(app.agents.iter().map(|agent| {
            format!(
                "{}  {}",
                agent.definition.name,
                ConfigStore::compact_home(&agent.definition.workspace)
            )
        }));
        items
    }
}
