use super::super::*;
use super::UiSupport;

pub(in crate::app) struct WorkspaceEditorComponent;

impl WorkspaceEditorComponent {
    pub(in crate::app) fn draw(
        frame: &mut Frame,
        editor: &WorkspaceEditorState,
        area: Rect,
        focused: bool,
    ) {
        let mode = match editor.mode {
            EditorMode::Normal => "NORMAL",
            EditorMode::Visual => "VISUAL",
            EditorMode::Insert => "INSERT",
            EditorMode::Command => "COMMAND",
        };
        let dirty = if editor.dirty { " [+]" } else { "" };
        let block = UiSupport::focus_block(
            UiSupport::editor_block().title(format!(
                "{}{} [{}]",
                editor.path.display(),
                dirty,
                mode
            )),
            focused,
        );
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let (code_area, status_area) = Self::panes(inner);
        let vertical_scroll = editor.clamped_vertical_scroll(code_area.height);
        let lines = editor.rendered_lines(code_area.height);
        let code = Paragraph::new(Text::from(lines))
            .style(UiSupport::editor_style())
            .scroll((0, editor.horizontal_scroll));
        frame.render_widget(code, code_area);
        UiSupport::render_vertical_scrollbar_with_viewport(
            frame,
            code_area,
            editor.content_height(),
            vertical_scroll,
        );
        Self::render_hover_popover(frame, editor, code_area, vertical_scroll);

        if let Some(status_area) = status_area {
            let status_widget = Paragraph::new(Self::status(editor, focused)).style(
                Style::default()
                    .bg(UiSupport::theme().app_bg)
                    .fg(UiSupport::theme().muted),
            );
            frame.render_widget(status_widget, status_area);
        }

        if !focused {
            return;
        }

        match editor.mode {
            EditorMode::Command => {
                if let Some(status_area) = status_area {
                    let x = status_area
                        .x
                        .saturating_add(1 + editor.command.chars().count() as u16)
                        .min(status_area.x + status_area.width.saturating_sub(1));
                    frame.set_cursor_position((x, status_area.y));
                }
            }
            EditorMode::Normal | EditorMode::Visual | EditorMode::Insert => {
                if code_area.width == 0 || code_area.height == 0 {
                    return;
                }

                let visible_row = editor.cursor_row.saturating_sub(vertical_scroll as usize) as u16;
                if visible_row >= code_area.height {
                    return;
                }

                let gutter_width = editor.gutter_width() as u16;
                let visible_col = editor
                    .cursor_col
                    .saturating_sub(editor.horizontal_scroll as usize)
                    as u16;
                let max_x = code_area.x + code_area.width.saturating_sub(1);
                let x = code_area
                    .x
                    .saturating_add(gutter_width)
                    .saturating_add(visible_col)
                    .min(max_x);
                let y = code_area.y.saturating_add(visible_row);
                frame.set_cursor_position((x, y));
            }
        }
    }

    pub(in crate::app) fn viewport(area: Rect) -> Rect {
        let inner = area.inner(Margin {
            vertical: 1,
            horizontal: 1,
        });
        Self::panes(inner).0
    }

    fn panes(inner: Rect) -> (Rect, Option<Rect>) {
        if inner.height <= 1 {
            return (inner, None);
        }

        let panes = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(1)])
            .split(inner);
        (panes[0], Some(panes[1]))
    }

    fn status(editor: &WorkspaceEditorState, focused: bool) -> String {
        if !focused {
            return editor
                .status
                .clone()
                .unwrap_or_else(|| "SIDEBAR FOCUSED  Tab editor".to_string());
        }

        match editor.mode {
            EditorMode::Command => format!(":{}", editor.command),
            EditorMode::Visual => {
                "-- VISUAL --  Esc normal  y copy  p paste  h/j/k/l move  x delete selection"
                    .to_string()
            }
            EditorMode::Insert => {
                "-- INSERT --  Esc normal  Enter newline  Backspace delete".to_string()
            }
            EditorMode::Normal => editor.status.clone().unwrap_or_else(|| {
                "NORMAL  Tab sidebar  arrows move  v select  y copy  p paste  u undo  Ctrl+click definition  i/a/o edit  x delete  :w save  :q preview"
                    .to_string()
            }),
        }
    }

    fn render_hover_popover(
        frame: &mut Frame,
        editor: &WorkspaceEditorState,
        code_area: Rect,
        vertical_scroll: u16,
    ) {
        let Some((hover, position)) = editor.hover_popover() else {
            return;
        };
        if code_area.width < 12 || code_area.height < 4 {
            return;
        }

        let visible_row = position.row.saturating_sub(vertical_scroll as usize) as u16;
        if visible_row >= code_area.height {
            return;
        }

        let gutter_width = editor.gutter_width() as u16;
        let visible_col = position
            .col
            .saturating_sub(editor.horizontal_scroll as usize) as u16;
        let anchor_x = code_area
            .x
            .saturating_add(gutter_width)
            .saturating_add(visible_col)
            .min(code_area.x + code_area.width.saturating_sub(1));
        let anchor_y = code_area.y.saturating_add(visible_row);

        let text = Text::from(hover.to_string());
        let natural_content_width = hover
            .lines()
            .map(|line| Line::from(line.to_string()).width() as u16)
            .max()
            .unwrap_or(1);
        let max_content_width = code_area.width.saturating_sub(4).clamp(8, 64);
        let content_width = natural_content_width.clamp(8, max_content_width);
        let content_height = UiSupport::wrapped_text_height(&text, content_width) as u16;
        let popup_width = content_width.saturating_add(2);
        let popup_height = content_height
            .saturating_add(2)
            .min(code_area.height.max(3));
        let popup_area = Self::hover_popover_area(
            code_area,
            anchor_x,
            anchor_y,
            popup_width,
            popup_height.max(3),
        );

        frame.render_widget(Clear, popup_area);
        let popup = Paragraph::new(text)
            .block(
                UiSupport::rounded_block()
                    .style(
                        Style::default()
                            .bg(UiSupport::theme().panel_bg)
                            .fg(UiSupport::theme().foreground),
                    )
                    .border_style(Style::default().fg(UiSupport::theme().info))
                    .title_style(
                        Style::default()
                            .fg(UiSupport::theme().info)
                            .add_modifier(Modifier::BOLD),
                    ),
            )
            .style(
                Style::default()
                    .bg(UiSupport::theme().panel_bg)
                    .fg(UiSupport::theme().foreground),
            )
            .wrap(Wrap { trim: false });
        frame.render_widget(popup, popup_area);
    }

    pub(in crate::app) fn hover_popover_area(
        code_area: Rect,
        anchor_x: u16,
        anchor_y: u16,
        popup_width: u16,
        popup_height: u16,
    ) -> Rect {
        let popup_width = popup_width.min(code_area.width.max(1));
        let popup_height = popup_height.min(code_area.height.max(1));

        let desired_x = anchor_x.saturating_add(1);
        let desired_y = anchor_y.saturating_add(1);
        let right_limit = code_area
            .x
            .saturating_add(code_area.width.saturating_sub(popup_width));
        let bottom_limit = code_area
            .y
            .saturating_add(code_area.height.saturating_sub(popup_height));

        let x = if desired_x.saturating_add(popup_width) <= code_area.x + code_area.width {
            desired_x.min(right_limit)
        } else {
            anchor_x
                .saturating_sub(popup_width.saturating_sub(1))
                .clamp(code_area.x, right_limit)
        };

        let y = if desired_y.saturating_add(popup_height) <= code_area.y + code_area.height {
            desired_y.min(bottom_limit)
        } else {
            anchor_y
                .saturating_sub(popup_height.saturating_sub(1))
                .clamp(code_area.y, bottom_limit)
        };

        Rect::new(x, y, popup_width, popup_height)
    }
}
