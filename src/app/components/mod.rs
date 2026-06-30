pub(super) mod chat;
pub(super) mod git_diff;
pub(super) mod navigation;
pub(super) mod shared;
pub(super) mod shell;
pub(super) mod workspace;

#[allow(unused_imports)]
pub(super) use self::{
    chat::{
        chat_input_height_for_main_area, draw_add_agent_form, draw_chat, draw_chat_input,
        wrapped_chat_input_lines,
    },
    git_diff::{
        draw_git_diff, git_diff_layout, git_diff_remote_button_label, git_diff_section_from_click,
    },
    navigation::{
        draw_help_line, draw_sidebar, draw_top_navigation, tab_from_click, top_navigation_tabs_rect,
    },
    shared::UiSupport,
    shell::ShellView,
    workspace::{
        draw_workspace, draw_workspace_sidebar, workspace_editor_viewport,
        workspace_sidebar_layout, workspace_sidebar_tab_from_click,
    },
};
