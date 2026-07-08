use super::super::*;
use super::{UiSupport, WorkspaceEditorComponent};

pub(in crate::app) struct WorkspaceComponent;

impl WorkspaceComponent {
    pub(in crate::app) fn draw(frame: &mut Frame, app: &App, area: Rect) {
        let Some(agent) = app.active_agent() else {
            let empty = Paragraph::new("Select or create an agent in the Chat tab.")
                .block(UiSupport::panel_block().title("Workspace"))
                .style(UiSupport::panel_style());
            frame.render_widget(empty, area);
            return;
        };

        if let Some(editor) = agent.workspace.editor.as_ref() {
            WorkspaceEditorComponent::draw(frame, editor, area, agent.workspace.editor_focused());
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

    pub(in crate::app) fn maybe_refresh(app: &mut App) -> bool {
        if app.current_tab != AppTab::Workspace {
            return false;
        }

        let now = Instant::now();
        if app
            .last_workspace_refresh_at
            .is_some_and(|last| now.duration_since(last) < WORKSPACE_AUTO_REFRESH_INTERVAL)
        {
            return false;
        }

        app.last_workspace_refresh_at = Some(now);
        if let Some(agent) = app.active_agent_mut() {
            return agent
                .workspace
                .refresh_if_changed(&agent.definition.workspace);
        }

        false
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
            if matches!(editor.mode, EditorMode::Command | EditorMode::Visual) {
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

    pub(in crate::app) fn handle_key(app: &mut App, key: KeyEvent, area: Rect) -> bool {
        if app.current_tab != AppTab::Workspace {
            return false;
        }

        let Some(agent_index) = app.current_agent else {
            return false;
        };

        let viewport = WorkspaceEditorComponent::viewport(App::compute_layout(app, area).body);
        let page_step = usize::from(CONTENT_SCROLL_STEP);
        let mut saved = false;
        let mut close = false;
        let mut handled = false;

        {
            let agent = &mut app.agents[agent_index];
            let workspace = &mut agent.workspace;
            let editor_mode = workspace.editor.as_ref().map(|editor| editor.mode);
            let completion_visible = workspace
                .editor
                .as_ref()
                .and_then(|editor| editor.completion_popover())
                .is_some();

            if workspace.editor.is_some()
                && key.code == KeyCode::Tab
                && !completion_visible
                && matches!(editor_mode, Some(EditorMode::Normal | EditorMode::Visual))
            {
                workspace.toggle_focus();
                return true;
            }

            if workspace.sidebar_focused() && workspace.sidebar_tab == WorkspaceSidebarTab::Search {
                match key.code {
                    KeyCode::Esc => {
                        workspace.set_sidebar_tab(WorkspaceSidebarTab::Files);
                        workspace.focus_sidebar();
                        return true;
                    }
                    KeyCode::Up => {
                        workspace.search_move_up();
                        return true;
                    }
                    KeyCode::Down => {
                        workspace.search_move_down();
                        return true;
                    }
                    KeyCode::Enter => {
                        match workspace.open_selected_search_result() {
                            Ok(true) => {
                                if let Some(editor) = workspace.editor.as_mut() {
                                    editor.ensure_visible(viewport.width, viewport.height);
                                }
                            }
                            Ok(false) => {}
                            Err(error) => workspace.error = Some(error.to_string()),
                        }
                        return true;
                    }
                    KeyCode::Backspace => {
                        workspace.pop_search_char();
                        return true;
                    }
                    KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                        workspace.push_search_char(character);
                        return true;
                    }
                    _ => {}
                }
            }

            if workspace.editor.is_none() {
                if key.code == KeyCode::Enter {
                    if workspace.toggle_current_directory() {
                        return true;
                    }
                    match workspace.open_editor() {
                        Ok(()) => {}
                        Err(error) => workspace.error = Some(error.to_string()),
                    }
                    return true;
                }
                return false;
            }

            if workspace.sidebar_focused() {
                match key.code {
                    KeyCode::Up => {
                        workspace.move_up();
                        handled = true;
                    }
                    KeyCode::Down => {
                        workspace.move_down();
                        handled = true;
                    }
                    KeyCode::Enter => {
                        if workspace.toggle_current_directory() {
                            handled = true;
                        } else {
                            match workspace.open_editor() {
                                Ok(()) => {
                                    handled = true;
                                }
                                Err(error) => {
                                    workspace.error = Some(error.to_string());
                                    handled = true;
                                }
                            }
                        }
                    }
                    KeyCode::Esc => handled = true,
                    _ => {}
                }

                if handled {
                    if let Some(editor) = workspace.editor.as_mut() {
                        editor.clear_hover();
                        editor.ensure_visible(viewport.width, viewport.height);
                    }
                }
                return handled;
            }

            {
                let editor = workspace.editor.as_mut().expect("editor checked above");
                if editor.completion_popover().is_some() {
                    match key.code {
                        KeyCode::Esc => {
                            editor.clear_completion();
                            editor.clear_hover();
                            editor.ensure_visible(viewport.width, viewport.height);
                            return true;
                        }
                        KeyCode::Up => {
                            editor.select_previous_completion();
                            editor.clear_hover();
                            editor.ensure_visible(viewport.width, viewport.height);
                            return true;
                        }
                        KeyCode::Down => {
                            editor.select_next_completion();
                            editor.clear_hover();
                            editor.ensure_visible(viewport.width, viewport.height);
                            return true;
                        }
                        KeyCode::Enter | KeyCode::Tab => {
                            editor.apply_selected_completion();
                            editor.clear_hover();
                            editor.ensure_visible(viewport.width, viewport.height);
                            return true;
                        }
                        _ => editor.clear_completion(),
                    }
                }

                match editor.mode {
                    EditorMode::Command => match key.code {
                        KeyCode::Esc => {
                            editor.cancel_command();
                            handled = true;
                        }
                        KeyCode::Backspace => {
                            editor.command.pop();
                            handled = true;
                        }
                        KeyCode::Enter => {
                            match editor.execute_command() {
                                Ok(result) => {
                                    saved = result.saved;
                                    close = result.close;
                                }
                                Err(error) => editor.status = Some(error.to_string()),
                            }
                            handled = true;
                        }
                        KeyCode::Char(character)
                            if !key.modifiers.contains(KeyModifiers::CONTROL) =>
                        {
                            editor.command.push(character);
                            handled = true;
                        }
                        _ => handled = true,
                    },
                    EditorMode::Visual => match key.code {
                        KeyCode::Esc | KeyCode::Char('v') => {
                            editor.exit_visual_mode();
                            handled = true;
                        }
                        KeyCode::Char('y') => {
                            Self::copy_editor(editor);
                            handled = true;
                        }
                        KeyCode::Char('p') => {
                            Self::paste_editor(editor);
                            handled = true;
                        }
                        KeyCode::Left => {
                            editor.extend_left();
                            handled = true;
                        }
                        KeyCode::Char('h') => {
                            editor.extend_left();
                            handled = true;
                        }
                        KeyCode::Right => {
                            editor.extend_right();
                            handled = true;
                        }
                        KeyCode::Char('l') => {
                            editor.extend_right();
                            handled = true;
                        }
                        KeyCode::Up => {
                            editor.extend_up();
                            handled = true;
                        }
                        KeyCode::Char('k') => {
                            editor.extend_up();
                            handled = true;
                        }
                        KeyCode::Down => {
                            editor.extend_down();
                            handled = true;
                        }
                        KeyCode::Char('j') => {
                            editor.extend_down();
                            handled = true;
                        }
                        KeyCode::Home | KeyCode::Char('0') => {
                            editor.extend_line_start();
                            handled = true;
                        }
                        KeyCode::End | KeyCode::Char('$') => {
                            editor.extend_line_end();
                            handled = true;
                        }
                        KeyCode::PageUp => {
                            editor.extend_page_up(page_step);
                            handled = true;
                        }
                        KeyCode::PageDown => {
                            editor.extend_page_down(page_step);
                            handled = true;
                        }
                        KeyCode::Delete | KeyCode::Backspace | KeyCode::Char('x') => {
                            if editor.has_selection() {
                                editor.delete_selection();
                            }
                            editor.mode = EditorMode::Normal;
                            handled = true;
                        }
                        KeyCode::Char(':') => {
                            editor.start_command();
                            handled = true;
                        }
                        _ => {}
                    },
                    EditorMode::Insert => match key.code {
                        KeyCode::Esc => {
                            editor.mode = EditorMode::Normal;
                            handled = true;
                        }
                        KeyCode::Enter => {
                            editor.insert_newline();
                            handled = true;
                        }
                        KeyCode::Backspace => {
                            editor.backspace();
                            handled = true;
                        }
                        KeyCode::Delete => {
                            editor.delete_char();
                            handled = true;
                        }
                        KeyCode::Left => {
                            editor.move_left();
                            handled = true;
                        }
                        KeyCode::Right => {
                            editor.move_right();
                            handled = true;
                        }
                        KeyCode::Up => {
                            editor.move_up();
                            handled = true;
                        }
                        KeyCode::Down => {
                            editor.move_down();
                            handled = true;
                        }
                        KeyCode::Home => {
                            editor.move_line_start();
                            handled = true;
                        }
                        KeyCode::End => {
                            editor.move_line_end();
                            handled = true;
                        }
                        KeyCode::PageUp => {
                            editor.move_page_up(page_step);
                            handled = true;
                        }
                        KeyCode::PageDown => {
                            editor.move_page_down(page_step);
                            handled = true;
                        }
                        KeyCode::Tab => {
                            for _ in 0..4 {
                                editor.insert_char(' ');
                            }
                            handled = true;
                        }
                        _ => {}
                    },
                    EditorMode::Normal => match key.code {
                        KeyCode::Esc => handled = true,
                        KeyCode::Enter => handled = true,
                        KeyCode::Char('y') => {
                            Self::copy_editor(editor);
                            handled = true;
                        }
                        KeyCode::Char('p') => {
                            Self::paste_editor(editor);
                            handled = true;
                        }
                        KeyCode::Left if key.modifiers.contains(KeyModifiers::SHIFT) => {
                            editor.extend_left();
                            handled = true;
                        }
                        KeyCode::Right if key.modifiers.contains(KeyModifiers::SHIFT) => {
                            editor.extend_right();
                            handled = true;
                        }
                        KeyCode::Up if key.modifiers.contains(KeyModifiers::SHIFT) => {
                            editor.extend_up();
                            handled = true;
                        }
                        KeyCode::Down if key.modifiers.contains(KeyModifiers::SHIFT) => {
                            editor.extend_down();
                            handled = true;
                        }
                        KeyCode::Home if key.modifiers.contains(KeyModifiers::SHIFT) => {
                            editor.extend_line_start();
                            handled = true;
                        }
                        KeyCode::End if key.modifiers.contains(KeyModifiers::SHIFT) => {
                            editor.extend_line_end();
                            handled = true;
                        }
                        KeyCode::PageUp if key.modifiers.contains(KeyModifiers::SHIFT) => {
                            editor.extend_page_up(page_step);
                            handled = true;
                        }
                        KeyCode::PageDown if key.modifiers.contains(KeyModifiers::SHIFT) => {
                            editor.extend_page_down(page_step);
                            handled = true;
                        }
                        KeyCode::Up => {
                            editor.move_up();
                            handled = true;
                        }
                        KeyCode::Down => {
                            editor.move_down();
                            handled = true;
                        }
                        KeyCode::Left => {
                            editor.move_left();
                            handled = true;
                        }
                        KeyCode::Right => {
                            editor.move_right();
                            handled = true;
                        }
                        KeyCode::Home => {
                            editor.move_line_start();
                            handled = true;
                        }
                        KeyCode::End => {
                            editor.move_line_end();
                            handled = true;
                        }
                        KeyCode::PageUp => {
                            editor.move_page_up(page_step);
                            handled = true;
                        }
                        KeyCode::PageDown => {
                            editor.move_page_down(page_step);
                            handled = true;
                        }
                        KeyCode::Delete => {
                            editor.delete_char();
                            handled = true;
                        }
                        KeyCode::Char('h') => {
                            editor.move_left();
                            handled = true;
                        }
                        KeyCode::Char('j') => {
                            editor.move_down();
                            handled = true;
                        }
                        KeyCode::Char('k') => {
                            editor.move_up();
                            handled = true;
                        }
                        KeyCode::Char('l') => {
                            editor.move_right();
                            handled = true;
                        }
                        KeyCode::Char('0') => {
                            editor.move_line_start();
                            handled = true;
                        }
                        KeyCode::Char('$') => {
                            editor.move_line_end();
                            handled = true;
                        }
                        KeyCode::Char('v') => {
                            editor.enter_visual_mode();
                            handled = true;
                        }
                        KeyCode::Char('u') => {
                            editor.undo();
                            handled = true;
                        }
                        KeyCode::Char('i') => {
                            editor.enter_insert_mode();
                            handled = true;
                        }
                        KeyCode::Char('a') => {
                            editor.enter_insert_after();
                            handled = true;
                        }
                        KeyCode::Char('o') => {
                            editor.open_below();
                            handled = true;
                        }
                        KeyCode::Char('x') => {
                            editor.delete_char();
                            handled = true;
                        }
                        KeyCode::Char(':') => {
                            editor.start_command();
                            handled = true;
                        }
                        _ => {}
                    },
                }
            }

            if handled {
                if let Some(editor) = workspace.editor.as_mut() {
                    editor.clear_hover();
                    editor.clear_completion();
                    editor.ensure_visible(viewport.width, viewport.height);
                }
            }
        }

        if saved {
            let root = app.agents[agent_index].definition.workspace.clone();
            app.agents[agent_index].git_diff.refresh(&root);
        }

        if close {
            if let Err(error) = app.agents[agent_index].workspace.close_editor() {
                app.agents[agent_index].workspace.error = Some(error.to_string());
            }
        }

        handled || saved || close
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
        editor.mode = EditorMode::Normal;
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

    fn copy_editor(editor: &mut WorkspaceEditorState) {
        let text = editor.copy_selection_or_line();
        let copied_chars = text.chars().count();

        match Clipboard::new().and_then(|mut clipboard| clipboard.set_text(text)) {
            Ok(()) => {
                if editor.mode == EditorMode::Visual {
                    editor.exit_visual_mode();
                }
                editor.status = Some(format!("Copied {copied_chars} chars"));
            }
            Err(error) => {
                editor.status = Some(format!("Copy failed: {error}"));
            }
        }
    }

    fn paste_editor(editor: &mut WorkspaceEditorState) {
        match Clipboard::new().and_then(|mut clipboard| clipboard.get_text()) {
            Ok(text) => {
                let pasted_chars = text.chars().count();
                if !editor.paste_text(&text) {
                    editor.status = Some("Clipboard is empty".to_string());
                    return;
                }
                editor.status = Some(format!("Pasted {pasted_chars} chars"));
            }
            Err(error) => {
                editor.status = Some(format!("Paste failed: {error}"));
            }
        }
    }
}
