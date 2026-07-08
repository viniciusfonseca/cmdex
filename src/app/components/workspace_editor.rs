use super::super::*;
use super::UiSupport;
use crate::syntax::SyntaxRegistry;
use ratatui::style::Color;
use std::path::Path;
use syntect::{easy::HighlightLines, highlighting::FontStyle, parsing::SyntaxReference};

pub(in crate::app) struct WorkspaceEditorComponent;

const COMPLETION_POPOVER_MAX_ITEMS: usize = 8;

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
        if !Self::render_completion_popover(frame, editor, code_area, vertical_scroll) {
            Self::render_hover_popover(frame, editor, code_area, vertical_scroll);
        }

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

        if editor.completion_popover().is_some() {
            return "COMPLETION  Enter/Tab apply  Esc dismiss  Up/Down select".to_string();
        }

        match editor.mode {
            EditorMode::Command => format!(":{}", editor.command),
            EditorMode::Visual => {
                "-- VISUAL --  Esc normal  y copy  p paste  h/j/k/l move  x delete selection"
                    .to_string()
            }
            EditorMode::Insert => {
                "-- INSERT --  Ctrl+Space autocomplete  Esc normal  Enter newline  Backspace delete"
                    .to_string()
            }
            EditorMode::Normal => editor.status.clone().unwrap_or_else(|| {
                "NORMAL  Tab sidebar  arrows move  v select  y copy  p paste  u undo  Ctrl+Space autocomplete  Ctrl+click definition  i/a/o edit  x delete  :w save  :q preview"
                    .to_string()
            }),
        }
    }

    fn render_completion_popover(
        frame: &mut Frame,
        editor: &WorkspaceEditorState,
        code_area: Rect,
        vertical_scroll: u16,
    ) -> bool {
        let Some((items, selected, position)) = editor.completion_popover() else {
            return false;
        };
        if code_area.width < 16 || code_area.height < 4 {
            return false;
        }

        let visible_row = position.row.saturating_sub(vertical_scroll as usize) as u16;
        if visible_row >= code_area.height {
            return false;
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

        let visible_len = items.len().min(COMPLETION_POPOVER_MAX_ITEMS);
        let start = selected
            .saturating_sub(visible_len.saturating_sub(1))
            .min(items.len().saturating_sub(visible_len));
        let lines = items[start..start + visible_len]
            .iter()
            .enumerate()
            .map(|(offset, item)| Self::completion_line(item, start + offset == selected))
            .collect::<Vec<_>>();
        let natural_content_width = lines
            .iter()
            .map(|line| line.width() as u16)
            .max()
            .unwrap_or(1);
        let max_content_width = code_area.width.saturating_sub(4).clamp(16, 72);
        let content_width = natural_content_width.clamp(16, max_content_width);
        let popup_area = Self::hover_popover_area(
            code_area,
            anchor_x,
            anchor_y,
            content_width.saturating_add(2),
            visible_len as u16 + 2,
        );

        frame.render_widget(Clear, popup_area);
        let popup = Paragraph::new(Text::from(lines))
            .block(
                UiSupport::rounded_block()
                    .style(
                        Style::default()
                            .bg(UiSupport::theme().panel_bg)
                            .fg(UiSupport::theme().foreground),
                    )
                    .border_style(Style::default().fg(UiSupport::theme().accent)),
            )
            .style(
                Style::default()
                    .bg(UiSupport::theme().panel_bg)
                    .fg(UiSupport::theme().foreground),
            );
        frame.render_widget(popup, popup_area);
        true
    }

    fn completion_line(item: &EditorCompletionItem, selected: bool) -> Line<'static> {
        let selected_style = Style::default()
            .bg(UiSupport::theme().selection_bg)
            .fg(UiSupport::theme().selection_fg)
            .add_modifier(Modifier::BOLD);
        let base_style = if selected {
            selected_style
        } else {
            Style::default()
                .bg(UiSupport::theme().panel_bg)
                .fg(UiSupport::theme().foreground)
        };
        let detail_style = if selected {
            selected_style
        } else {
            Style::default()
                .bg(UiSupport::theme().panel_bg)
                .fg(UiSupport::theme().muted)
        };
        let prefix = if selected { "> " } else { "  " };
        let mut spans = vec![
            Span::styled(prefix.to_string(), base_style),
            Span::styled(item.label.clone(), base_style),
        ];
        if let Some(detail) = item.detail.as_deref() {
            spans.push(Span::styled(format!("  {detail}"), detail_style));
        }
        Line::from(spans)
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

        let rendered_lines = Self::hover_lines(hover, &editor.path);
        let text = Text::from(rendered_lines.clone());
        let natural_content_width = rendered_lines
            .iter()
            .map(|line| line.width() as u16)
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

    fn hover_lines(hover: &str, path: &Path) -> Vec<Line<'static>> {
        let mut lines = Vec::new();

        for (index, block) in Self::parse_hover_blocks(hover).into_iter().enumerate() {
            if index > 0 && !lines.is_empty() {
                lines.push(Line::default());
            }

            match block {
                HoverBlock::Text(text) => {
                    lines.extend(text.split('\n').map(|line| {
                        if line.is_empty() {
                            Line::default()
                        } else {
                            Line::from(line.to_string())
                        }
                    }));
                }
                HoverBlock::Code { language, text } => {
                    lines.extend(Self::highlight_hover_code(&text, language.as_deref(), path));
                }
            }
        }

        if lines.is_empty() {
            lines.push(Line::default());
        }

        lines
    }

    fn parse_hover_blocks(hover: &str) -> Vec<HoverBlock> {
        let mut blocks = Vec::new();
        let mut text_lines = Vec::new();
        let mut code_lines = Vec::new();
        let mut code_language = None;
        let mut inside_code = false;

        for line in hover.replace("\r\n", "\n").replace('\r', "\n").split('\n') {
            let trimmed = line.trim_start();
            if trimmed.starts_with("```") {
                if inside_code {
                    Self::push_hover_text_block(&mut blocks, &mut text_lines);
                    Self::push_hover_code_block(&mut blocks, code_language.take(), &mut code_lines);
                } else {
                    Self::push_hover_text_block(&mut blocks, &mut text_lines);
                    let language = trimmed.trim_start_matches("```").trim();
                    code_language = (!language.is_empty()).then(|| language.to_string());
                }
                inside_code = !inside_code;
                continue;
            }

            if inside_code {
                code_lines.push(line.to_string());
            } else {
                text_lines.push(line.to_string());
            }
        }

        if inside_code {
            Self::push_hover_code_block(&mut blocks, code_language, &mut code_lines);
        } else {
            Self::push_hover_text_block(&mut blocks, &mut text_lines);
        }

        blocks
    }

    fn push_hover_text_block(blocks: &mut Vec<HoverBlock>, lines: &mut Vec<String>) {
        let mut normalized = Vec::new();
        let mut previous_blank = false;

        for line in lines.drain(..) {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                if !previous_blank && !normalized.is_empty() {
                    normalized.push(String::new());
                }
                previous_blank = true;
            } else {
                normalized.push(trimmed.to_string());
                previous_blank = false;
            }
        }

        while normalized.last().is_some_and(String::is_empty) {
            normalized.pop();
        }

        if normalized.is_empty() {
            return;
        }

        blocks.push(HoverBlock::Text(normalized.join("\n")));
    }

    fn push_hover_code_block(
        blocks: &mut Vec<HoverBlock>,
        language: Option<String>,
        lines: &mut Vec<String>,
    ) {
        while lines.last().is_some_and(String::is_empty) {
            lines.pop();
        }

        if lines.is_empty() {
            return;
        }

        blocks.push(HoverBlock::Code {
            language,
            text: lines.drain(..).collect::<Vec<_>>().join("\n"),
        });
    }

    fn highlight_hover_code(
        source: &str,
        language: Option<&str>,
        path: &Path,
    ) -> Vec<Line<'static>> {
        let syntax = Self::hover_syntax(language, source, path);
        match syntax {
            Some(syntax) => {
                let mut highlighter = HighlightLines::new(syntax, ThemeRegistry::syntax());
                source
                    .split('\n')
                    .map(|line| {
                        if line.is_empty() {
                            return Line::default();
                        }

                        match highlighter.highlight_line(line, SyntaxRegistry::set()) {
                            Ok(ranges) => Line::from(
                                ranges
                                    .into_iter()
                                    .map(|(style, text)| {
                                        Span::styled(
                                            text.to_string(),
                                            Self::to_ratatui_style(style),
                                        )
                                    })
                                    .collect::<Vec<_>>(),
                            ),
                            Err(_) => Line::from(line.to_string()),
                        }
                    })
                    .collect()
            }
            None => source
                .split('\n')
                .map(|line| {
                    if line.is_empty() {
                        Line::default()
                    } else {
                        Line::from(line.to_string())
                    }
                })
                .collect(),
        }
    }

    fn hover_syntax(
        language: Option<&str>,
        source: &str,
        path: &Path,
    ) -> Option<&'static SyntaxReference> {
        let language = language.unwrap_or_default().trim();
        path.extension()
            .and_then(|extension| extension.to_str())
            .and_then(|extension| SyntaxRegistry::set().find_syntax_by_extension(extension))
            .or_else(|| SyntaxRegistry::set().find_syntax_by_token(language))
            .or_else(|| SyntaxRegistry::set().find_syntax_by_name(language))
            .or_else(|| SyntaxRegistry::set().find_syntax_by_extension(language))
            .or_else(|| {
                source
                    .lines()
                    .next()
                    .and_then(|line| SyntaxRegistry::set().find_syntax_by_first_line(line))
            })
    }

    fn to_ratatui_style(style: syntect::highlighting::Style) -> Style {
        let mut modifiers = Modifier::empty();
        if style.font_style.contains(FontStyle::BOLD) {
            modifiers |= Modifier::BOLD;
        }
        if style.font_style.contains(FontStyle::ITALIC) {
            modifiers |= Modifier::ITALIC;
        }
        if style.font_style.contains(FontStyle::UNDERLINE) {
            modifiers |= Modifier::UNDERLINED;
        }

        Style::default()
            .fg(Color::Rgb(
                style.foreground.r,
                style.foreground.g,
                style.foreground.b,
            ))
            .bg(Color::Rgb(
                style.background.r,
                style.background.g,
                style.background.b,
            ))
            .add_modifier(modifiers)
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

enum HoverBlock {
    Text(String),
    Code {
        language: Option<String>,
        text: String,
    },
}
