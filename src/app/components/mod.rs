pub(super) mod add_agent_dialog;
pub(super) mod agents_sidebar;
pub(super) mod chat;
pub(super) mod chat_input;
pub(super) mod git_diff;
pub(super) mod git_diff_sidebar;
pub(super) mod help_bar;
pub(super) mod navigation;
pub(super) mod shared;
pub(super) mod shell;
pub(super) mod shell_sidebar;
pub(super) mod workspace;
pub(super) mod workspace_editor;
pub(super) mod workspace_editor_input;
pub(super) mod workspace_editor_shortcuts;
pub(super) mod workspace_keys;
pub(super) mod workspace_sidebar;
pub(super) mod workspace_sidebar_input;

pub(super) use self::{
    add_agent_dialog::AddAgentDialogComponent,
    agents_sidebar::AgentsSidebarComponent,
    chat::{ChatComponent, ModelPickerAction},
    chat_input::ChatInputComponent,
    git_diff::GitDiffComponent,
    git_diff_sidebar::GitDiffSidebarComponent,
    help_bar::StatusBarComponent,
    navigation::TopNavigationComponent,
    shared::{SelectableListPopover, UiSupport},
    shell::ShellComponent,
    shell_sidebar::ShellSidebarComponent,
    workspace::WorkspaceComponent,
    workspace_editor::WorkspaceEditorComponent,
    workspace_sidebar::WorkspaceSidebarComponent,
};
