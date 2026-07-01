use super::{components::*, *};

pub(super) struct AppUi;

impl AppUi {
    pub(super) fn draw(frame: &mut Frame, app: &App) {
        let area = frame.area();
        frame.render_widget(
            Block::default().style(UiSupport::app_background_style()),
            area,
        );

        let frame_layout = Layout::default()
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
            .split(frame_layout[1]);

        TopNavigationComponent::draw(frame, app, frame_layout[0]);
        Self::draw_main(frame, app, root[1]);
        Self::draw_sidebar(frame, app, root[0]);
        HelpBarComponent::draw(frame, frame_layout[2]);
    }

    fn draw_main(frame: &mut Frame, app: &App, area: Rect) {
        if app.current_tab == AppTab::Chat && !app.add_agent_selected() {
            let input_height = ChatInputComponent::height_for_main_area(&app.chat_input, area);
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(10), Constraint::Length(input_height)])
                .split(area);
            ChatComponent::draw(frame, app, chunks[0]);
            ChatInputComponent::draw(frame, app, chunks[1]);
            return;
        }

        match app.current_tab {
            AppTab::Chat => AddAgentDialogComponent::draw(frame, app, area),
            AppTab::Workspace => WorkspaceComponent::draw(frame, app, area),
            AppTab::Shell => ShellComponent::draw(frame, app, area),
            AppTab::GitDiff => GitDiffComponent::draw(frame, app, area),
        }
    }

    fn draw_sidebar(frame: &mut Frame, app: &App, area: Rect) {
        match app.current_tab {
            AppTab::Chat => AgentsSidebarComponent::draw(frame, app, area),
            AppTab::Workspace => WorkspaceSidebarComponent::draw(frame, app, area),
            AppTab::Shell => ShellSidebarComponent::draw(frame, app, area),
            AppTab::GitDiff => GitDiffSidebarComponent::draw(frame, app, area),
        }
    }
}
