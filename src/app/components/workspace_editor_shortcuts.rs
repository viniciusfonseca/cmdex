use super::super::*;
use super::{UiSupport, WorkspaceEditorComponent};

impl WorkspaceEditorComponent {
    pub(super) fn shortcuts_help_lines() -> Vec<Line<'static>> {
        let header_style = Style::default()
            .fg(UiSupport::theme().accent)
            .add_modifier(Modifier::BOLD);
        let muted_style = Style::default().fg(UiSupport::theme().muted);

        vec![
            Line::from(Span::styled("Global", header_style)),
            Line::from(vec![
                Span::styled("Ctrl+H", Style::default().add_modifier(Modifier::BOLD)),
                Span::styled(" open/close help  ", muted_style),
                Span::styled("Tab", Style::default().add_modifier(Modifier::BOLD)),
                Span::styled(" switch focus", muted_style),
            ]),
            Line::from(vec![
                Span::styled("Ctrl+click", Style::default().add_modifier(Modifier::BOLD)),
                Span::styled(" go to definition  ", muted_style),
                Span::styled("Ctrl+Space", Style::default().add_modifier(Modifier::BOLD)),
                Span::styled(" autocomplete", muted_style),
            ]),
            Line::from(vec![
                Span::styled(
                    "Shift+scroll",
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::styled(" horizontal scroll  ", muted_style),
                Span::styled("mouse drag", Style::default().add_modifier(Modifier::BOLD)),
                Span::styled(" select text", muted_style),
            ]),
            Line::default(),
            Line::from(Span::styled("Normal", header_style)),
            Line::from("arrows / h j k l move, v select, y copy, p paste"),
            Line::from("u undo, i / a / o edit, x delete, : open command"),
            Line::default(),
            Line::from(Span::styled("Visual / Insert", header_style)),
            Line::from("Visual: arrows / h j k l expand, y copy, p paste, x delete"),
            Line::from("Insert: Esc normal, Enter new line, Backspace / Delete remove"),
            Line::from("Home / End / PageUp / PageDown navigate"),
        ]
    }
}
