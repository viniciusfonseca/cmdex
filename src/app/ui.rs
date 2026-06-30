use super::{components::*, *};

#[allow(unused_imports)]
pub(super) use super::components::{
    UiSupport, chat_input_height_for_main_area, git_diff_layout, git_diff_remote_button_label,
    git_diff_section_from_click, tab_from_click, top_navigation_tabs_rect,
    workspace_editor_viewport, workspace_sidebar_layout, workspace_sidebar_tab_from_click,
    wrapped_chat_input_lines,
};

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

    draw_top_navigation(frame, app, frame_layout[0]);
    draw_main(frame, app, root[1]);
    draw_sidebar(frame, app, root[0]);
    draw_help_line(frame, frame_layout[2]);
}

fn draw_main(frame: &mut Frame, app: &App, area: Rect) {
    if app.current_tab == AppTab::Chat && !app.add_agent_selected() {
        let input_height = chat_input_height_for_main_area(&app.chat_input, area);
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(10), Constraint::Length(input_height)])
            .split(area);
        draw_chat(frame, app, chunks[0]);
        draw_chat_input(frame, app, chunks[1]);
    } else if app.current_tab == AppTab::Shell {
        ShellView::draw(frame, app, area);
    } else {
        match app.current_tab {
            AppTab::Chat => draw_add_agent_form(frame, app, area),
            AppTab::Workspace => draw_workspace(frame, app, area),
            AppTab::Shell => unreachable!("shell with input is rendered separately"),
            AppTab::GitDiff => draw_git_diff(frame, app, area),
        }
    }
}
