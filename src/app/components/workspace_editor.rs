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
                "NORMAL  Tab sidebar  arrows move  v select  y copy  p paste  u undo  i/a/o edit  x delete  :w save  :q preview"
                    .to_string()
            }),
        }
    }
}
