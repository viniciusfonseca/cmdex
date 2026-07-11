use super::super::*;
use super::{UiSupport, WorkspaceEditorComponent, WorkspaceScreen};

impl WorkspaceScreen {
    pub(in crate::app) fn scrollbar_drag_target_at(
        app: &App,
        column: u16,
        row: u16,
        layout: UiLayout,
    ) -> Option<ScrollbarDragTarget> {
        if app.current_tab != AppTab::Workspace {
            return None;
        }
        let editor = app.active_agent()?.workspace.editor.as_ref();
        if editor.is_some()
            && let Some(metrics) = Self::scrollbar_metrics(
                app,
                ScrollbarDragTarget::WorkspaceCompletionPopover,
                layout,
            )
            && UiSupport::rect_contains(metrics.track, column, row)
        {
            return Some(ScrollbarDragTarget::WorkspaceCompletionPopover);
        }

        let target = if editor.is_some() {
            ScrollbarDragTarget::WorkspaceEditor
        } else {
            ScrollbarDragTarget::WorkspacePreview
        };
        let metrics = Self::scrollbar_metrics(app, target, layout)?;
        UiSupport::rect_contains(metrics.track, column, row).then_some(target)
    }

    pub(in crate::app) fn scrollbar_metrics(
        app: &App,
        target: ScrollbarDragTarget,
        layout: UiLayout,
    ) -> Option<ScrollbarMetrics> {
        if app.current_tab != AppTab::Workspace {
            return None;
        }
        let agent = app.active_agent()?;
        match target {
            ScrollbarDragTarget::WorkspacePreview => {
                let content_length = UiSupport::scrollable_preview_content_height(
                    &agent.workspace.preview,
                    layout.body,
                );
                UiSupport::vertical_scrollbar_metrics(layout.body, content_length)
            }
            ScrollbarDragTarget::WorkspaceEditor => {
                let editor = agent.workspace.editor.as_ref()?;
                let viewport = WorkspaceEditorComponent::viewport(layout.body);
                UiSupport::vertical_scrollbar_metrics_for_viewport(
                    viewport,
                    editor.content_height(),
                )
            }
            ScrollbarDragTarget::WorkspaceCompletionPopover => {
                let editor = agent.workspace.editor.as_ref()?;
                WorkspaceEditorComponent::completion_popover_scrollbar_metrics(editor, layout.body)
            }
            _ => None,
        }
    }

    pub(in crate::app) fn update_scrollbar_drag(
        app: &mut App,
        target: ScrollbarDragTarget,
        scroll: u16,
        metrics: ScrollbarMetrics,
    ) -> bool {
        if app.current_tab != AppTab::Workspace {
            return false;
        }
        let Some(agent) = app.active_agent_mut() else {
            return false;
        };
        match target {
            ScrollbarDragTarget::WorkspacePreview => agent.workspace.content_scroll = scroll,
            ScrollbarDragTarget::WorkspaceEditor => {
                let Some(editor) = agent.workspace.editor.as_mut() else {
                    return false;
                };
                editor.set_vertical_scroll(scroll, metrics.viewport_length as u16);
            }
            ScrollbarDragTarget::WorkspaceCompletionPopover => {
                let Some(editor) = agent.workspace.editor.as_mut() else {
                    return false;
                };
                editor.set_completion_window_start(scroll as usize, metrics.viewport_length);
            }
            _ => return false,
        }
        true
    }

    pub(in crate::app) fn handle_mouse_move(
        app: &mut App,
        column: u16,
        row: u16,
        area: Rect,
    ) -> bool {
        if app.current_tab != AppTab::Workspace {
            return false;
        }
        if Self::shortcuts_popup_open(app) {
            app.clear_workspace_hover();
            return true;
        }
        if Self::handle_editor_hover(app, column, row, area) {
            return true;
        }
        app.clear_workspace_hover();
        true
    }

    pub(in crate::app) fn handle_popup_click_or_close(
        app: &mut App,
        column: u16,
        row: u16,
        area: Rect,
    ) -> bool {
        if app.current_tab != AppTab::Workspace || !Self::shortcuts_popup_open(app) {
            return false;
        }
        app.clear_workspace_hover();
        if UiSupport::rect_contains(area, column, row) {
            Self::handle_shortcuts_popup_click(app, column, row, area);
        } else if let Some(agent) = app.active_agent_mut()
            && let Some(editor) = agent.workspace.editor.as_mut()
        {
            editor.close_shortcuts_help();
        }
        true
    }

    pub(in crate::app) fn handle_editor_drag_if_active(
        app: &mut App,
        column: u16,
        row: u16,
        area: Rect,
    ) -> bool {
        if app.current_tab != AppTab::Workspace
            || !app.active_workspace_selection_drag
            || app
                .active_agent()
                .is_none_or(|agent| agent.workspace.editor.is_none())
        {
            return false;
        }
        Self::handle_editor_drag(app, column, row, area)
    }

    pub(in crate::app) fn finish_editor_drag(app: &mut App) -> bool {
        if app.current_tab != AppTab::Workspace || !app.active_workspace_selection_drag {
            return false;
        }
        if let Some(agent) = app.active_agent_mut()
            && let Some(editor) = agent.workspace.editor.as_mut()
            && editor.mode.is_visual()
            && !editor.has_selection()
        {
            editor.exit_visual_mode();
        }
        true
    }
}
