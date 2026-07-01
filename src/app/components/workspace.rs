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
            WorkspaceEditorComponent::draw(frame, editor, area);
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
        let mut selection_delta = 0i8;

        {
            let agent = &mut app.agents[agent_index];
            let workspace = &mut agent.workspace;

            if workspace.sidebar_tab == WorkspaceSidebarTab::Search {
                match key.code {
                    KeyCode::Esc => {
                        workspace.set_sidebar_tab(WorkspaceSidebarTab::Files);
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

            {
                let editor = workspace.editor.as_mut().expect("editor checked above");
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
                        KeyCode::Enter => {
                            handled = workspace.toggle_current_directory();
                        }
                        KeyCode::Up => {
                            selection_delta = -1;
                            handled = true;
                        }
                        KeyCode::Down => {
                            selection_delta = 1;
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

            if selection_delta < 0 {
                workspace.move_up();
            } else if selection_delta > 0 {
                workspace.move_down();
            }

            if handled {
                if let Some(editor) = workspace.editor.as_mut() {
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

    pub(in crate::app) fn handle_editor_click(app: &mut App, column: u16, row: u16, area: Rect) {
        let viewport = WorkspaceEditorComponent::viewport(area);
        if !UiSupport::rect_contains(viewport, column, row) {
            return;
        }

        let Some(agent) = app.active_agent_mut() else {
            return;
        };
        let Some(editor) = agent.workspace.editor.as_mut() else {
            return;
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

        editor.mode = EditorMode::Normal;
        editor.set_cursor(target_row, target_col);
        editor.ensure_visible(viewport.width, viewport.height);
    }
}
