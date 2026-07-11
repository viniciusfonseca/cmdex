use super::super::*;
use super::{UiSupport, WorkspaceEditorComponent};

pub(in crate::app) struct WorkspaceScreen;

impl WorkspaceScreen {
    pub(in crate::app) fn handle_key_with_context(
        app: &mut App,
        key: KeyEvent,
        ui_tx: &mpsc::UnboundedSender<UiEvent>,
        area: Rect,
    ) -> bool {
        if app.current_tab != AppTab::Workspace {
            return false;
        }
        app.clear_workspace_hover();
        if Self::handle_completion_request(app, key, ui_tx) {
            return true;
        }
        Self::handle_key(app, key, area)
    }

    pub(in crate::app) fn handle_editor_click_with_context(
        app: &mut App,
        column: u16,
        row: u16,
        modifiers: KeyModifiers,
        area: Rect,
        ui_tx: &mpsc::UnboundedSender<UiEvent>,
    ) -> bool {
        if app.current_tab != AppTab::Workspace
            || app
                .active_agent()
                .is_none_or(|agent| agent.workspace.editor.is_none())
        {
            return false;
        }

        app.clear_workspace_hover();
        if Self::handle_shortcuts_popup_click(app, column, row, area) {
            return true;
        }
        if modifiers.contains(KeyModifiers::CONTROL)
            && Self::handle_editor_definition_click(app, column, row, area, ui_tx)
        {
            return true;
        }
        app.active_workspace_selection_drag = Self::handle_editor_click(app, column, row, area);
        true
    }

    pub(in crate::app) fn handle_editor_scroll(
        app: &mut App,
        column: u16,
        row: u16,
        area: Rect,
        footer: Option<Rect>,
        up: bool,
        horizontal: bool,
    ) -> bool {
        if app.current_tab != AppTab::Workspace {
            return false;
        }
        if Self::shortcuts_popup_open(app)
            || Self::consume_shortcuts_popup_scroll(app, column, row, area)
            || (!horizontal && Self::handle_completion_scroll(app, column, row, area, up))
        {
            return true;
        }
        if !(UiSupport::rect_contains(area, column, row)
            || footer.is_some_and(|rect| UiSupport::rect_contains(rect, column, row)))
        {
            return false;
        }
        if !horizontal {
            return false;
        }

        let Some(agent) = app.active_agent_mut() else {
            return false;
        };
        let Some(editor) = agent.workspace.editor.as_mut() else {
            return false;
        };
        editor.clear_hover();
        let viewport = WorkspaceEditorComponent::viewport(area);
        if up {
            editor.scroll_left(MOUSE_SCROLL_STEP);
        } else {
            editor.scroll_right(MOUSE_SCROLL_STEP, viewport.width);
        }
        true
    }

    pub(in crate::app) fn move_selection_up(app: &mut App) -> bool {
        Self::move_selection(app, true)
    }

    pub(in crate::app) fn move_selection_down(app: &mut App) -> bool {
        Self::move_selection(app, false)
    }

    pub(in crate::app) fn scroll_content(app: &mut App, area: Rect, lines: u16, up: bool) -> bool {
        if app.current_tab != AppTab::Workspace {
            return false;
        }
        app.clear_workspace_hover();
        let Some(agent) = app.active_agent_mut() else {
            return true;
        };
        if let Some(editor) = agent.workspace.editor.as_mut() {
            let viewport = WorkspaceEditorComponent::viewport(area);
            if up {
                editor.scroll_up(lines);
            } else {
                editor.scroll_down(lines, viewport.height);
            }
        } else if up {
            agent.workspace.scroll_up(lines);
        } else {
            agent.workspace.scroll_down(lines);
        }
        true
    }

    fn move_selection(app: &mut App, up: bool) -> bool {
        if app.current_tab != AppTab::Workspace {
            return false;
        }
        let Some(agent_index) = app.current_agent else {
            return true;
        };
        let should_open_editor = {
            let workspace = &mut app.agents[agent_index].workspace;
            if workspace.editor_focused() {
                return true;
            }
            if workspace.sidebar_tab == WorkspaceSidebarTab::Search {
                if up {
                    workspace.search_move_up();
                } else {
                    workspace.search_move_down();
                }
            } else if up {
                workspace.move_up_without_io();
            } else {
                workspace.move_down_without_io();
            }
            true
        };
        if should_open_editor {
            Self::request_open_editor(app, agent_index);
        }
        true
    }

    pub(in crate::app) fn handle_text_input(app: &mut App, character: char) -> bool {
        if app.current_tab != AppTab::Workspace {
            return false;
        }
        app.clear_workspace_hover();
        let Some(agent) = app.active_agent_mut() else {
            return true;
        };
        if agent.workspace.sidebar_focused()
            && agent.workspace.sidebar_tab == WorkspaceSidebarTab::Search
        {
            agent.workspace.push_search_char(character);
            return true;
        }
        if let Some(editor) = agent.workspace.editor.as_mut() {
            editor.clear_completion();
            match &editor.mode {
                EditorMode::Insert => editor.insert_char(character),
                EditorMode::Command { .. } => editor.push_command_char(character),
                EditorMode::Normal | EditorMode::Visual { .. } => {}
            }
        }
        true
    }

    pub(in crate::app) fn handle_backspace(app: &mut App) -> bool {
        if app.current_tab != AppTab::Workspace {
            return false;
        }
        app.clear_workspace_hover();
        let Some(agent) = app.active_agent_mut() else {
            return true;
        };
        if agent.workspace.sidebar_focused()
            && agent.workspace.sidebar_tab == WorkspaceSidebarTab::Search
        {
            agent.workspace.pop_search_char();
            return true;
        }
        if let Some(editor) = agent.workspace.editor.as_mut() {
            editor.clear_completion();
            match &editor.mode {
                EditorMode::Insert => editor.backspace(),
                EditorMode::Command { .. } => editor.pop_command_char(),
                EditorMode::Normal | EditorMode::Visual { .. } => {}
            }
        }
        true
    }

    pub(in crate::app) fn draw(frame: &mut Frame, app: &App, area: Rect) {
        let Some(agent) = app.active_agent() else {
            let empty = Paragraph::new("Select or create an agent in the Chat tab.")
                .block(UiSupport::panel_block().title("Workspace"))
                .style(UiSupport::panel_style());
            frame.render_widget(empty, area);
            return;
        };

        if let Some(editor) = agent.workspace.editor.as_ref() {
            let lsp_loading = app.active_workspace_lsp_loading_label();
            WorkspaceEditorComponent::draw(
                frame,
                editor,
                area,
                agent.workspace.editor_focused(),
                lsp_loading.as_deref(),
            );
            return;
        }

        let mut lines = vec![Line::from(format!(
            "Workspace: {}",
            ConfigStore::compact_home(&agent.definition.workspace)
        ))];
        lines.push(Line::from(String::new()));
        lines.extend(agent.workspace.preview.iter().cloned());
        if let Some(error) = &agent.workspace.error {
            lines.push(Line::from(String::new()));
            lines.push(Line::from(Span::styled(
                error.clone(),
                Style::default()
                    .fg(UiSupport::theme().error)
                    .bg(UiSupport::theme().panel_bg),
            )));
        }
        let content_length = UiSupport::scrollable_preview_content_height(&lines, area);

        let widget = Paragraph::new(Text::from(lines))
            .block(UiSupport::panel_block().title(agent.workspace.preview_title.clone()))
            .style(UiSupport::panel_style())
            .scroll((agent.workspace.content_scroll, 0))
            .wrap(Wrap { trim: false });
        frame.render_widget(widget, area);
        UiSupport::render_vertical_scrollbar(
            frame,
            area,
            content_length,
            agent.workspace.content_scroll,
        );
    }

    pub(in crate::app) fn request_refresh_for_agent(app: &mut App, agent_index: usize) -> bool {
        let Some(agent) = app.agents.get(agent_index) else {
            return false;
        };
        let root = agent.definition.workspace.clone();
        if !app.workspace_refresh_in_flight.insert(agent_index) {
            return false;
        }
        app.enqueue_effect(super::super::effects::AppEffect::RefreshWorkspace {
            agent_index,
            root,
        });
        true
    }

    pub(in crate::app) fn maybe_search(app: &mut App) -> bool {
        let Some(agent_index) = app.current_agent else {
            return false;
        };
        let request = {
            let workspace = &mut app.agents[agent_index].workspace;
            workspace.take_search_request()
        };
        let Some((query, generation)) = request else {
            return false;
        };

        let entries = app.agents[agent_index].workspace.entries().to_vec();
        app.enqueue_effect(super::super::effects::AppEffect::SearchWorkspace {
            agent_index,
            entries,
            query,
            generation,
        });
        true
    }

    pub(in crate::app) fn request_open_editor(app: &mut App, agent_index: usize) -> bool {
        Self::request_open_editor_at(app, agent_index, None)
    }

    pub(in crate::app) fn request_open_editor_at(
        app: &mut App,
        agent_index: usize,
        position: Option<EditorPosition>,
    ) -> bool {
        let Some(agent) = app.agents.get_mut(agent_index) else {
            return false;
        };
        let Some(path) = agent
            .workspace
            .entries()
            .get(agent.workspace.selected)
            .map(|entry| entry.path.clone())
        else {
            return false;
        };
        if !agent.workspace.begin_editor_load(&path) {
            return false;
        }
        app.enqueue_effect(super::super::effects::AppEffect::OpenWorkspaceEditor {
            agent_index,
            path,
            position,
        });
        true
    }

    pub(in crate::app) fn request_preview(app: &mut App, agent_index: usize) -> bool {
        let Some(agent) = app.agents.get(agent_index) else {
            return false;
        };
        let Some(path) = agent
            .workspace
            .entries()
            .get(agent.workspace.selected)
            .map(|entry| entry.path.clone())
        else {
            return false;
        };
        app.enqueue_effect(super::super::effects::AppEffect::LoadWorkspacePreview {
            agent_index,
            path,
        });
        true
    }

    pub(in crate::app) fn handle_completion_request(
        app: &mut App,
        key: KeyEvent,
        ui_tx: &mpsc::UnboundedSender<UiEvent>,
    ) -> bool {
        if app.current_tab != AppTab::Workspace
            || !key.modifiers.contains(KeyModifiers::CONTROL)
            || !matches!(key.code, KeyCode::Char(' ') | KeyCode::Null)
        {
            return false;
        }

        let Some(agent_index) = app.current_agent else {
            return false;
        };

        let Some((path, source, position)) = ({
            let agent = &mut app.agents[agent_index];
            if agent.workspace.sidebar_focused() {
                return false;
            }

            let Some(editor) = agent.workspace.editor.as_mut() else {
                return false;
            };
            if editor.shortcuts_help_open()
                || matches!(
                    editor.mode,
                    EditorMode::Command { .. } | EditorMode::Visual { .. }
                )
            {
                return false;
            }

            editor.request_completion(editor.cursor_position());
            Some((
                editor.path.clone(),
                editor.source_text(),
                editor.cursor_position(),
            ))
        }) else {
            return false;
        };

        app.request_lsp_completion(agent_index, path, source, position, ui_tx);
        true
    }

    pub(in crate::app) fn shortcuts_popup_open(app: &App) -> bool {
        app.active_agent()
            .and_then(|agent| agent.workspace.editor.as_ref())
            .is_some_and(WorkspaceEditorState::shortcuts_help_open)
    }

    pub(in crate::app) fn handle_shortcuts_popup_click(
        app: &mut App,
        column: u16,
        row: u16,
        area: Rect,
    ) -> bool {
        let Some(agent) = app.active_agent_mut() else {
            return false;
        };
        let Some(editor) = agent.workspace.editor.as_mut() else {
            return false;
        };
        if !editor.shortcuts_help_open() {
            return false;
        }

        let popup_area = WorkspaceEditorComponent::shortcuts_popup_area(area);
        let close_button_area = WorkspaceEditorComponent::shortcuts_popup_close_button_area(area);
        if !UiSupport::rect_contains(popup_area, column, row)
            || UiSupport::rect_contains(close_button_area, column, row)
        {
            editor.close_shortcuts_help();
        }
        true
    }

    pub(in crate::app) fn consume_shortcuts_popup_scroll(
        app: &App,
        column: u16,
        row: u16,
        area: Rect,
    ) -> bool {
        Self::shortcuts_popup_open(app)
            && UiSupport::rect_contains(
                WorkspaceEditorComponent::shortcuts_popup_area(area),
                column,
                row,
            )
    }

    pub(in crate::app) fn handle_editor_click(
        app: &mut App,
        column: u16,
        row: u16,
        area: Rect,
    ) -> bool {
        let Some(agent) = app.active_agent_mut() else {
            return false;
        };
        agent.workspace.focus_editor();
        let Some(target) = agent
            .workspace
            .editor
            .as_ref()
            .and_then(|editor| Self::editor_position_at(editor, column, row, area, false))
        else {
            return false;
        };
        let Some(editor) = agent.workspace.editor.as_mut() else {
            return false;
        };
        editor.clear_hover();
        editor.clear_completion();
        editor.enter_normal_mode();
        editor.set_cursor(target.row, target.col);
        let viewport = WorkspaceEditorComponent::viewport(area);
        editor.ensure_visible(viewport.width, viewport.height);
        true
    }

    pub(in crate::app) fn handle_editor_drag(
        app: &mut App,
        column: u16,
        row: u16,
        area: Rect,
    ) -> bool {
        let Some(agent) = app.active_agent_mut() else {
            return false;
        };
        agent.workspace.focus_editor();
        let Some(target) = agent
            .workspace
            .editor
            .as_ref()
            .and_then(|editor| Self::editor_position_at(editor, column, row, area, true))
        else {
            return false;
        };
        let Some(editor) = agent.workspace.editor.as_mut() else {
            return false;
        };
        editor.clear_hover();
        editor.clear_completion();
        editor.select_to(target.row, target.col);
        let viewport = WorkspaceEditorComponent::viewport(area);
        editor.ensure_visible(viewport.width, viewport.height);
        true
    }

    pub(in crate::app) fn handle_editor_hover(
        app: &mut App,
        column: u16,
        row: u16,
        area: Rect,
    ) -> bool {
        if Self::shortcuts_popup_open(app) {
            return true;
        }

        let Some(agent_index) = app.current_agent else {
            return false;
        };

        let Some((path, target, already_requested)) = ({
            let agent = &mut app.agents[agent_index];
            let Some(editor) = agent.workspace.editor.as_mut() else {
                return false;
            };
            let Some(target) = Self::editor_symbol_position(editor, column, row, area) else {
                return false;
            };
            let already_requested = editor.hover_request_position() == Some(target);
            Some((editor.path.clone(), target, already_requested))
        }) else {
            return false;
        };
        if app.lsp_server_for_path(&path).is_none() {
            return false;
        }

        let is_same_visible_hover = app
            .active_agent()
            .and_then(|agent| agent.workspace.editor.as_ref())
            .and_then(|editor| editor.hover_popover())
            .is_some_and(|(_, position)| position == target);
        if already_requested || is_same_visible_hover {
            return true;
        }

        app.set_pending_workspace_hover(agent_index, column, row, path, target);
        true
    }

    pub(in crate::app) fn handle_editor_definition_click(
        app: &mut App,
        column: u16,
        row: u16,
        area: Rect,
        ui_tx: &mpsc::UnboundedSender<UiEvent>,
    ) -> bool {
        let Some(agent_index) = app.current_agent else {
            return false;
        };

        let Some((path, source, target)) = ({
            let agent = &mut app.agents[agent_index];
            agent.workspace.focus_editor();
            let Some(editor) = agent.workspace.editor.as_ref() else {
                return false;
            };
            let Some(target) = Self::editor_symbol_position(editor, column, row, area) else {
                return false;
            };
            Some((editor.path.clone(), editor.source_text(), target))
        }) else {
            return false;
        };

        app.request_lsp_definition(agent_index, path, source, target, ui_tx);
        true
    }

    pub(in crate::app) fn handle_completion_scroll(
        app: &mut App,
        column: u16,
        row: u16,
        area: Rect,
        up: bool,
    ) -> bool {
        let Some(agent) = app.active_agent_mut() else {
            return false;
        };
        let Some(editor) = agent.workspace.editor.as_mut() else {
            return false;
        };
        let Some(popup_area) = WorkspaceEditorComponent::completion_popover_area(editor, area)
        else {
            return false;
        };
        if !UiSupport::rect_contains(popup_area, column, row) {
            return false;
        }

        if up {
            editor.select_previous_completion();
        } else {
            editor.select_next_completion();
        }
        editor.clear_hover();
        let viewport = WorkspaceEditorComponent::viewport(area);
        editor.ensure_visible(viewport.width, viewport.height);
        true
    }

    fn editor_position_at(
        editor: &WorkspaceEditorState,
        column: u16,
        row: u16,
        area: Rect,
        clamp_to_viewport: bool,
    ) -> Option<EditorPosition> {
        let viewport = WorkspaceEditorComponent::viewport(area);
        if viewport.width == 0 || viewport.height == 0 {
            return None;
        }

        let (column, row) = if clamp_to_viewport {
            let max_x = viewport.x + viewport.width.saturating_sub(1);
            let max_y = viewport.y + viewport.height.saturating_sub(1);
            (
                column.clamp(viewport.x, max_x),
                row.clamp(viewport.y, max_y),
            )
        } else {
            if !UiSupport::rect_contains(viewport, column, row) {
                return None;
            }
            (column, row)
        };

        let gutter_width = editor.gutter_width() as u16;
        let target_row =
            usize::from(row.saturating_sub(viewport.y)) + editor.vertical_scroll as usize;
        let content_x = column.saturating_sub(viewport.x);
        let target_col = if content_x <= gutter_width {
            0
        } else {
            usize::from(content_x.saturating_sub(gutter_width)) + editor.horizontal_scroll as usize
        };

        Some(EditorPosition {
            row: target_row,
            col: target_col,
        })
    }

    fn editor_symbol_position(
        editor: &WorkspaceEditorState,
        column: u16,
        row: u16,
        area: Rect,
    ) -> Option<EditorPosition> {
        let target = Self::editor_position_at(editor, column, row, area, false)?;
        editor.symbol_position_near(target.row, target.col)
    }

    pub(super) fn request_copy_editor(app: &mut App, agent_index: usize) -> bool {
        let Some(editor) = app
            .agents
            .get(agent_index)
            .and_then(|agent| agent.workspace.editor.as_ref())
        else {
            return false;
        };
        app.enqueue_effect(super::super::effects::AppEffect::CopyToClipboard {
            agent_index,
            path: editor.path.clone(),
            text: editor.copy_selection_or_line(),
        });
        true
    }

    pub(super) fn request_paste_editor(app: &mut App, agent_index: usize) -> bool {
        let Some(path) = app
            .agents
            .get(agent_index)
            .and_then(|agent| agent.workspace.editor.as_ref())
            .map(|editor| editor.path.clone())
        else {
            return false;
        };
        app.enqueue_effect(super::super::effects::AppEffect::PasteFromClipboard {
            agent_index,
            path,
        });
        true
    }
}
