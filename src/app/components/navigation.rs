use super::super::*;
use super::{draw_workspace_sidebar, shared::UiSupport};

const TOP_NAV_PREFIX: &str = "CMDEX ·";

impl App {
    pub(in crate::app) fn sidebar_labels(&self) -> Vec<String> {
        match self.current_tab {
            AppTab::Chat => {
                let mut items = vec!["+ Add agent".to_string()];
                items.extend(self.agents.iter().map(|agent| {
                    format!(
                        "{}  {}",
                        agent.definition.name,
                        ConfigStore::compact_home(&agent.definition.workspace)
                    )
                }));
                items
            }
            AppTab::Workspace => self
                .active_agent()
                .map(|agent| agent.workspace.sidebar_labels())
                .unwrap_or_default(),
            AppTab::Shell => self
                .active_agent()
                .map(|agent| {
                    let mut items = vec!["+ New session".to_string()];
                    items.extend(agent.shell_tab.labels(self.spinner_index));
                    items
                })
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

    pub(in crate::app) fn selected_tab_index(&self) -> usize {
        match self.current_tab {
            AppTab::Chat => 0,
            AppTab::Workspace => 1,
            AppTab::Shell => 2,
            AppTab::GitDiff => 3,
        }
    }

    pub(in crate::app) fn refresh_current_tab(&mut self) {
        match self.current_tab {
            AppTab::Chat => {}
            AppTab::Workspace => {
                if let Some(agent) = self.active_agent_mut() {
                    agent.workspace.refresh(&agent.definition.workspace);
                }
                self.last_workspace_refresh_at = Some(Instant::now());
            }
            AppTab::Shell => {}
            AppTab::GitDiff => {
                if let Some(agent) = self.active_agent_mut() {
                    agent.git_diff.refresh(&agent.definition.workspace);
                }
            }
        }
    }

    pub(in crate::app) fn set_tab(&mut self, tab: AppTab) {
        if self.current_tab != tab {
            self.current_tab = tab;
            self.refresh_current_tab();
        }
    }

    pub(in crate::app) fn activate_tab(
        &mut self,
        tab: AppTab,
        ui_tx: mpsc::UnboundedSender<UiEvent>,
    ) {
        if tab == AppTab::Shell && self.current_tab != AppTab::Shell {
            self.open_shell_tab(ui_tx);
        } else {
            self.set_tab(tab);
        }
    }
}

pub(in crate::app) fn draw_top_navigation(frame: &mut Frame, app: &App, area: Rect) {
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
    frame.render_widget(label, top_navigation_label_rect(area));

    let tabs = Tabs::new(TAB_LABELS.map(|(label, _)| label))
        .select(app.selected_tab_index())
        .style(UiSupport::tab_style())
        .highlight_style(UiSupport::tab_highlight_style());
    frame.render_widget(tabs, top_navigation_tabs_rect(area));
}

pub(in crate::app) fn draw_sidebar(frame: &mut Frame, app: &App, area: Rect) {
    if app.current_tab == AppTab::Workspace {
        draw_workspace_sidebar(frame, app, area);
        return;
    }

    let title = match app.current_tab {
        AppTab::Chat => "Agents",
        AppTab::Shell => "Shell Sessions",
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
        AppTab::Shell => state.select(app.active_agent().map(|agent| {
            if agent.shell_tab.sessions.is_empty() {
                0
            } else {
                (agent.shell_tab.selected_index() + 1).min(items.len().saturating_sub(1))
            }
        })),
        AppTab::GitDiff => state.select(app.active_agent().map(|agent| {
            agent
                .git_diff
                .selected_index()
                .min(items.len().saturating_sub(1))
        })),
    }

    let list = List::new(items)
        .block(UiSupport::sidebar_block().title(title))
        .style(UiSupport::sidebar_style())
        .highlight_style(UiSupport::selection_style())
        .highlight_symbol("› ");
    frame.render_stateful_widget(list, area, &mut state);
}

pub(in crate::app) fn draw_help_line(frame: &mut Frame, area: Rect) {
    let help = Paragraph::new("Quit: Ctrl+Q  Restart: Alt+R").style(
        Style::default()
            .bg(UiSupport::theme().app_bg)
            .fg(UiSupport::theme().muted),
    );
    frame.render_widget(help, area);
}

pub(in crate::app) fn tab_from_click(tabs: Rect, column: u16, row: u16) -> Option<AppTab> {
    let inner = top_navigation_tabs_rect(tabs);
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

fn top_navigation_label_rect(area: Rect) -> Rect {
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

pub(in crate::app) fn top_navigation_tabs_rect(area: Rect) -> Rect {
    let inner = UiSupport::inner_rect(area);
    if inner.width == 0 || inner.height == 0 {
        return inner;
    }

    let label = top_navigation_label_rect(area);
    let x = label.x.saturating_add(label.width);
    Rect::new(
        x,
        inner.y,
        inner.x.saturating_add(inner.width).saturating_sub(x),
        1,
    )
}
