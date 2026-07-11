use super::super::*;
use super::UiSupport;
use crate::workspace::COMPLETION_POPOVER_MAX_ITEMS;
use std::path::Path;

pub(in crate::app) struct WorkspaceEditorComponent;

const COMPLETION_POPOVER_CONTENT_WIDTH: u16 = 28;
const SHORTCUTS_POPUP_WIDTH: u16 = 60;
const SHORTCUTS_POPUP_HEIGHT: u16 = 16;
const SHORTCUTS_CLOSE_BUTTON_WIDTH: u16 = 12;

struct CompletionPopoverLayout {
    popup_area: Rect,
    content_area: Rect,
    text_area: Rect,
    lines: Vec<Line<'static>>,
    start: usize,
    needs_scrollbar: bool,
    item_count: usize,
}

struct ShortcutsPopupLayout {
    popup_area: Rect,
    content_area: Rect,
    close_button_area: Rect,
}

impl WorkspaceEditorComponent {
    pub(in crate::app) fn draw(
        frame: &mut Frame,
        editor: &WorkspaceEditorState,
        area: Rect,
        focused: bool,
        lsp_loading: Option<&str>,
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
        if !editor.shortcuts_help_open()
            && !Self::render_completion_popover(frame, editor, code_area, vertical_scroll)
        {
            Self::render_hover_popover(frame, editor, code_area, vertical_scroll);
        }

        if let Some(status_area) = status_area {
            let status_widget = Paragraph::new(Self::status(editor, focused)).style(
                Style::default()
                    .bg(UiSupport::theme().app_bg)
                    .fg(UiSupport::theme().muted),
            );
            frame.render_widget(status_widget, status_area);
            if let Some(lsp_loading) = lsp_loading {
                let loading_widget = Paragraph::new(lsp_loading)
                    .alignment(Alignment::Right)
                    .style(
                        Style::default()
                            .bg(UiSupport::theme().app_bg)
                            .fg(UiSupport::theme().accent)
                            .add_modifier(Modifier::BOLD),
                    );
                frame.render_widget(loading_widget, status_area);
            }
        }

        if editor.shortcuts_help_open() {
            Self::render_shortcuts_popup(frame, area);
            return;
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

    pub(in crate::app) fn completion_popover_area(
        editor: &WorkspaceEditorState,
        area: Rect,
    ) -> Option<Rect> {
        let code_area = Self::viewport(area);
        let vertical_scroll = editor.clamped_vertical_scroll(code_area.height);
        Self::completion_popover_layout(editor, code_area, vertical_scroll)
            .map(|layout| layout.popup_area)
    }

    pub(in crate::app) fn completion_popover_scrollbar_metrics(
        editor: &WorkspaceEditorState,
        area: Rect,
    ) -> Option<ScrollbarMetrics> {
        let code_area = Self::viewport(area);
        let vertical_scroll = editor.clamped_vertical_scroll(code_area.height);
        let layout = Self::completion_popover_layout(editor, code_area, vertical_scroll)?;
        if !layout.needs_scrollbar {
            return None;
        }

        UiSupport::vertical_scrollbar_metrics_for_viewport(layout.content_area, layout.item_count)
    }

    pub(in crate::app) fn shortcuts_popup_area(area: Rect) -> Rect {
        Self::shortcuts_popup_layout(area).popup_area
    }

    pub(in crate::app) fn shortcuts_popup_close_button_area(area: Rect) -> Rect {
        Self::shortcuts_popup_layout(area).close_button_area
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
        if editor.mode == EditorMode::Command {
            return format!(":{}", editor.command);
        }

        if let Some(status) = editor.status.as_deref() {
            return status.to_string();
        }

        if !focused {
            return "SIDEBAR FOCUSED".to_string();
        }

        if editor.completion_popover().is_some() {
            return "COMPLETION".to_string();
        }

        match editor.mode {
            EditorMode::Visual => "VISUAL".to_string(),
            EditorMode::Insert => "INSERT".to_string(),
            EditorMode::Normal => "NORMAL".to_string(),
            EditorMode::Command => unreachable!("command mode handled above"),
        }
    }

    fn render_shortcuts_popup(frame: &mut Frame, area: Rect) {
        let layout = Self::shortcuts_popup_layout(area);
        frame.render_widget(Clear, layout.popup_area);

        let popup_block = UiSupport::rounded_block()
            .title("Shortcuts")
            .style(
                Style::default()
                    .bg(UiSupport::theme().panel_bg)
                    .fg(UiSupport::theme().foreground),
            )
            .border_style(Style::default().fg(UiSupport::theme().accent));
        frame.render_widget(popup_block, layout.popup_area);

        let content = Paragraph::new(Text::from(Self::shortcuts_help_lines()))
            .style(
                Style::default()
                    .bg(UiSupport::theme().panel_bg)
                    .fg(UiSupport::theme().foreground),
            )
            .wrap(Wrap { trim: false });
        frame.render_widget(content, layout.content_area);

        let close_button = Paragraph::new("Close")
            .alignment(Alignment::Center)
            .style(UiSupport::action_style(UiSupport::theme().foreground))
            .block(UiSupport::panel_block());
        frame.render_widget(close_button, layout.close_button_area);
    }

    fn render_completion_popover(
        frame: &mut Frame,
        editor: &WorkspaceEditorState,
        code_area: Rect,
        vertical_scroll: u16,
    ) -> bool {
        let Some(layout) = Self::completion_popover_layout(editor, code_area, vertical_scroll)
        else {
            return false;
        };

        frame.render_widget(Clear, layout.popup_area);
        let popup_block = UiSupport::rounded_block()
            .style(
                Style::default()
                    .bg(UiSupport::theme().panel_bg)
                    .fg(UiSupport::theme().foreground),
            )
            .border_style(Style::default().fg(UiSupport::theme().accent));
        frame.render_widget(popup_block, layout.popup_area);

        let popup = Paragraph::new(Text::from(layout.lines)).style(
            Style::default()
                .bg(UiSupport::theme().panel_bg)
                .fg(UiSupport::theme().foreground),
        );
        frame.render_widget(popup, layout.text_area);
        if layout.needs_scrollbar {
            UiSupport::render_vertical_scrollbar_with_viewport(
                frame,
                layout.content_area,
                layout.item_count,
                layout.start as u16,
            );
        }
        true
    }

    fn completion_popover_layout(
        editor: &WorkspaceEditorState,
        code_area: Rect,
        vertical_scroll: u16,
    ) -> Option<CompletionPopoverLayout> {
        let (items, selected, position) = editor.completion_popover()?;
        if code_area.width < 16 || code_area.height < 4 {
            return None;
        }

        let visible_row = position.row.saturating_sub(vertical_scroll as usize) as u16;
        if visible_row >= code_area.height {
            return None;
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
        let start = editor.completion_window_start(COMPLETION_POPOVER_MAX_ITEMS);
        let needs_scrollbar = items.len() > visible_len;
        let lines = items[start..start + visible_len]
            .iter()
            .enumerate()
            .map(|(offset, item)| Self::completion_line(item, start + offset == selected))
            .collect::<Vec<_>>();
        let max_content_width = code_area
            .width
            .saturating_sub(if needs_scrollbar { 5 } else { 4 })
            .max(1);
        let content_width = COMPLETION_POPOVER_CONTENT_WIDTH.min(max_content_width);
        let popup_area = Self::hover_popover_area(
            code_area,
            anchor_x,
            anchor_y,
            content_width.saturating_add(if needs_scrollbar { 3 } else { 2 }),
            visible_len as u16 + 2,
        );
        let content_area = popup_area.inner(Margin {
            vertical: 1,
            horizontal: 1,
        });
        let text_area = if needs_scrollbar && content_area.width > 1 {
            Rect::new(
                content_area.x,
                content_area.y,
                content_area.width.saturating_sub(1),
                content_area.height,
            )
        } else {
            content_area
        };

        Some(CompletionPopoverLayout {
            popup_area,
            content_area,
            text_area,
            lines,
            start,
            needs_scrollbar,
            item_count: items.len(),
        })
    }

    fn shortcuts_popup_layout(area: Rect) -> ShortcutsPopupLayout {
        let viewport = area.inner(Margin {
            vertical: 1,
            horizontal: 1,
        });
        let popup_width = SHORTCUTS_POPUP_WIDTH.min(viewport.width.max(1));
        let popup_height = SHORTCUTS_POPUP_HEIGHT.min(viewport.height.max(1));
        let popup_x = viewport
            .x
            .saturating_add(viewport.width.saturating_sub(popup_width) / 2);
        let popup_y = viewport
            .y
            .saturating_add(viewport.height.saturating_sub(popup_height) / 2);
        let popup_area = Rect::new(popup_x, popup_y, popup_width, popup_height);
        let inner = popup_area.inner(Margin {
            vertical: 1,
            horizontal: 1,
        });
        let sections = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(3)])
            .split(inner);
        let close_button_area = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Min(1),
                Constraint::Length(SHORTCUTS_CLOSE_BUTTON_WIDTH),
            ])
            .split(sections[1])[1];

        ShortcutsPopupLayout {
            popup_area,
            content_area: sections[0],
            close_button_area,
        }
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
        let mut text_buffer = String::new();
        let mut code_buffer = None::<String>;
        let mut code_language = None;

        for event in MarkdownParser::new_ext(hover, MarkdownOptions::all()) {
            match event {
                MarkdownEvent::Start(Tag::CodeBlock(kind)) => {
                    Self::push_hover_text_block(&mut blocks, &mut text_buffer);
                    code_language = match kind {
                        CodeBlockKind::Fenced(language) => {
                            let language = language.trim().to_string();
                            (!language.is_empty()).then_some(language)
                        }
                        CodeBlockKind::Indented => None,
                    };
                    code_buffer = Some(String::new());
                }
                MarkdownEvent::End(TagEnd::CodeBlock) => {
                    if let Some(mut code) = code_buffer.take() {
                        while code.ends_with(['\n', '\r']) {
                            code.pop();
                        }
                        Self::push_hover_code_block(&mut blocks, code_language.take(), &mut code);
                    }
                }
                MarkdownEvent::Text(text) => {
                    if let Some(code) = code_buffer.as_mut() {
                        code.push_str(&text);
                    } else {
                        Self::append_hover_text(&mut text_buffer, &text);
                    }
                }
                MarkdownEvent::Code(text) => {
                    if let Some(code) = code_buffer.as_mut() {
                        code.push_str(&text);
                    } else {
                        Self::append_hover_text(&mut text_buffer, &text);
                    }
                }
                MarkdownEvent::SoftBreak | MarkdownEvent::HardBreak => {
                    if let Some(code) = code_buffer.as_mut() {
                        code.push('\n');
                    } else {
                        Self::append_hover_newline(&mut text_buffer);
                    }
                }
                MarkdownEvent::Rule => {
                    if code_buffer.is_none() {
                        Self::append_hover_paragraph_break(&mut text_buffer);
                    }
                }
                MarkdownEvent::Start(Tag::Item) => {
                    if code_buffer.is_none() {
                        Self::append_hover_item_prefix(&mut text_buffer);
                    }
                }
                MarkdownEvent::End(
                    TagEnd::Paragraph
                    | TagEnd::Heading(_)
                    | TagEnd::BlockQuote(_)
                    | TagEnd::HtmlBlock
                    | TagEnd::FootnoteDefinition
                    | TagEnd::DefinitionList
                    | TagEnd::DefinitionListTitle
                    | TagEnd::DefinitionListDefinition
                    | TagEnd::Table
                    | TagEnd::TableHead
                    | TagEnd::TableRow,
                ) => {
                    if code_buffer.is_none() {
                        Self::append_hover_paragraph_break(&mut text_buffer);
                    }
                }
                MarkdownEvent::End(TagEnd::Item) => {
                    if code_buffer.is_none() {
                        Self::append_hover_newline(&mut text_buffer);
                    }
                }
                MarkdownEvent::End(TagEnd::TableCell) => {
                    if code_buffer.is_none() {
                        Self::append_hover_text(&mut text_buffer, " ");
                    }
                }
                MarkdownEvent::Html(text)
                | MarkdownEvent::InlineHtml(text)
                | MarkdownEvent::InlineMath(text)
                | MarkdownEvent::DisplayMath(text) => {
                    if let Some(code) = code_buffer.as_mut() {
                        code.push_str(&text);
                    } else {
                        Self::append_hover_text(&mut text_buffer, &text);
                    }
                }
                MarkdownEvent::TaskListMarker(checked) => {
                    if code_buffer.is_none() {
                        Self::append_hover_text(
                            &mut text_buffer,
                            if checked { "[x] " } else { "[ ] " },
                        );
                    }
                }
                _ => {}
            }
        }

        if let Some(mut code) = code_buffer.take() {
            while code.ends_with(['\n', '\r']) {
                code.pop();
            }
            Self::push_hover_code_block(&mut blocks, code_language, &mut code);
        }
        Self::push_hover_text_block(&mut blocks, &mut text_buffer);

        blocks
    }

    fn append_hover_text(buffer: &mut String, text: &str) {
        buffer.push_str(text);
    }

    fn append_hover_newline(buffer: &mut String) {
        if !buffer.ends_with('\n') {
            buffer.push('\n');
        }
    }

    fn append_hover_paragraph_break(buffer: &mut String) {
        if buffer.trim().is_empty() {
            buffer.clear();
            return;
        }

        while buffer.ends_with(['\n', ' ']) {
            buffer.pop();
        }
        if !buffer.ends_with("\n\n") {
            buffer.push_str("\n\n");
        }
    }

    fn append_hover_item_prefix(buffer: &mut String) {
        if !buffer.is_empty() && !buffer.ends_with(['\n', ' ']) {
            buffer.push('\n');
        }
        buffer.push_str("- ");
    }

    fn push_hover_text_block(blocks: &mut Vec<HoverBlock>, text: &mut String) {
        let mut normalized = Vec::new();
        let mut previous_blank = false;

        for line in text
            .replace("\r\n", "\n")
            .replace('\r', "\n")
            .split('\n')
            .map(str::to_string)
            .collect::<Vec<_>>()
        {
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
            text.clear();
            return;
        }

        blocks.push(HoverBlock::Text(normalized.join("\n")));
        text.clear();
    }

    fn push_hover_code_block(
        blocks: &mut Vec<HoverBlock>,
        language: Option<String>,
        text: &mut String,
    ) {
        let trimmed = text.trim_end_matches(['\n', '\r']);
        if trimmed.is_empty() {
            text.clear();
            return;
        }

        blocks.push(HoverBlock::Code {
            language,
            text: trimmed.to_string(),
        });
        text.clear();
    }

    fn highlight_hover_code(
        source: &str,
        language: Option<&str>,
        path: &Path,
    ) -> Vec<Line<'static>> {
        crate::syntax::SyntaxRegistry::highlight_path_or_language(path, language, source)
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
