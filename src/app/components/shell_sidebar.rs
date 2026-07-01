use super::super::*;
use super::{ShellComponent, UiSupport};

pub(in crate::app) struct ShellSidebarComponent;

impl ShellSidebarComponent {
    pub(in crate::app) fn labels(app: &App) -> Vec<String> {
        app.active_agent()
            .map(|agent| {
                let mut items = vec!["+ New session".to_string()];
                items.extend(agent.shell_tab.labels(app.spinner_index));
                items
            })
            .unwrap_or_default()
    }

    pub(in crate::app) fn draw(frame: &mut Frame, app: &App, area: Rect) {
        let items = Self::labels(app)
            .into_iter()
            .map(ListItem::new)
            .collect::<Vec<_>>();

        let mut state = ListState::default();
        state.select(app.active_agent().map(|agent| {
            if agent.shell_tab.sessions.is_empty() {
                0
            } else {
                (agent.shell_tab.selected_index() + 1).min(items.len().saturating_sub(1))
            }
        }));

        let list = List::new(items)
            .block(UiSupport::sidebar_block().title("Shell Sessions"))
            .style(UiSupport::sidebar_style())
            .highlight_style(UiSupport::selection_style())
            .highlight_symbol("› ");
        frame.render_stateful_widget(list, area, &mut state);
    }

    pub(in crate::app) fn handle_click(
        app: &mut App,
        column: u16,
        row: u16,
        sidebar_list: Rect,
        ui_tx: &mpsc::UnboundedSender<UiEvent>,
    ) {
        let inner = UiSupport::inner_rect(sidebar_list);
        if inner.height == 0 || !UiSupport::rect_contains(inner, column, row) {
            return;
        }

        let visible_row = row.saturating_sub(inner.y) as usize;
        let clicked_index = app.active_agent().map(|agent| {
            let total = agent.shell_tab.sessions.len() + 1;
            let selected = if agent.shell_tab.sessions.is_empty() {
                0
            } else {
                agent.shell_tab.selected_index() + 1
            };
            let offset = UiSupport::list_offset(selected, total, inner.height as usize);
            (offset + visible_row).min(total.saturating_sub(1))
        });

        match clicked_index {
            Some(0) => ShellComponent::open_tab_and_create_session(app, ui_tx.clone()),
            Some(index) => {
                if let Some(agent) = app.active_agent_mut() {
                    agent.shell_tab.select(index - 1);
                }
            }
            None => {}
        }
    }
}
