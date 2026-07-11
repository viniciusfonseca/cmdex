use super::super::effects::AppEffect;
use super::super::*;
use super::{GitDiffComponent, ShellComponent, UiSupport};

const TOP_NAV_PREFIX: &str = "CMDEX ·";

pub(in crate::app) struct TopNavigationComponent;

impl TopNavigationComponent {
    pub(in crate::app) fn draw(frame: &mut Frame, app: &App, area: Rect) {
        frame.render_widget(
            UiSupport::rounded_block().style(UiSupport::tab_style()),
            area,
        );

        let label = Paragraph::new(TOP_NAV_PREFIX).style(
            Style::default()
                .fg(UiSupport::theme().yellow)
                .bg(UiSupport::theme().tab_bg)
                .add_modifier(Modifier::BOLD),
        );
        frame.render_widget(label, Self::label_rect(area));

        let tabs = Tabs::new(TAB_LABELS.map(|(label, _)| label))
            .select(Self::selected_index(app))
            .style(UiSupport::tab_style())
            .highlight_style(UiSupport::tab_highlight_style());
        frame.render_widget(tabs, Self::tabs_rect(area));
    }

    pub(in crate::app) fn selected_index(app: &App) -> usize {
        match app.current_tab {
            AppTab::Chat => 0,
            AppTab::Workspace => 1,
            AppTab::Shell => 2,
            AppTab::GitDiff => 3,
        }
    }

    pub(in crate::app) fn refresh_current_tab(app: &mut App) {
        match app.current_tab {
            AppTab::Chat => {}
            AppTab::Workspace => {
                if let Some(agent_index) = app.current_agent {
                    let root = app.agents[agent_index].definition.workspace.clone();
                    app.workspace_refresh_in_flight = true;
                    app.enqueue_effect(AppEffect::RefreshWorkspace { agent_index, root });
                }
                app.last_workspace_refresh_at = Some(Instant::now());
            }
            AppTab::Shell => {}
            AppTab::GitDiff => {
                GitDiffComponent::request_refresh(app);
            }
        }
    }

    pub(in crate::app) fn set_tab(app: &mut App, tab: AppTab) {
        if app.current_tab != tab {
            app.model_picker = None;
            app.current_tab = tab;
            Self::refresh_current_tab(app);
        }
    }

    pub(in crate::app) fn activate_tab(
        app: &mut App,
        tab: AppTab,
        ui_tx: mpsc::UnboundedSender<UiEvent>,
    ) {
        if tab == AppTab::Shell && app.current_tab != AppTab::Shell {
            ShellComponent::open_tab(app, ui_tx);
        } else {
            Self::set_tab(app, tab);
        }
    }

    pub(in crate::app) fn tab_from_click(tabs: Rect, column: u16, row: u16) -> Option<AppTab> {
        let inner = Self::tabs_rect(tabs);
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
            if UiSupport::rect_contains(clickable, column, row) {
                return Some(tab);
            }

            x = x.saturating_add(label_width).saturating_add(3);
            if x >= inner.x.saturating_add(inner.width) {
                break;
            }
        }

        None
    }

    pub(in crate::app) fn tabs_rect(area: Rect) -> Rect {
        let inner = UiSupport::inner_rect(area);
        if inner.width == 0 || inner.height == 0 {
            return inner;
        }

        let label = Self::label_rect(area);
        let x = label.x.saturating_add(label.width);
        Rect::new(
            x,
            inner.y,
            inner.x.saturating_add(inner.width).saturating_sub(x),
            1,
        )
    }

    fn label_rect(area: Rect) -> Rect {
        let inner = UiSupport::inner_rect(area);
        if inner.width == 0 || inner.height == 0 {
            return inner;
        }

        Rect::new(
            inner.x,
            inner.y,
            (TOP_NAV_PREFIX.chars().count() as u16).min(inner.width),
            1,
        )
    }
}
