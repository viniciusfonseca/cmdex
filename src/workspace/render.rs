use super::*;
use crate::{syntax::SyntaxRegistry, theme::ThemeRegistry};
use syntect::parsing::SyntaxReference;

pub(super) struct WorkspaceRenderer;

impl WorkspaceRenderer {
    pub(super) fn read_text_preview(path: &Path) -> Result<Vec<Line<'static>>> {
        let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
        if bytes.contains(&0) {
            return Ok(Self::plain_preview_lines(
                "Binary file preview is not available.",
            ));
        }

        let truncated = bytes.len() > PREVIEW_LIMIT;
        let preview =
            String::from_utf8_lossy(&bytes[..bytes.len().min(PREVIEW_LIMIT)]).into_owned();
        let preview = Self::normalize_newlines(&preview);
        let mut lines = SyntaxRegistry::highlight_path(path, &preview);
        if !truncated {
            Self::maybe_trim_trailing_empty_line(&mut lines);
        }
        let mut lines = Self::add_line_numbers(lines);
        if truncated {
            lines.push(Line::default());
            lines.push(Line::from(Span::styled(
                "[truncated]".to_string(),
                Style::default()
                    .fg(ThemeRegistry::app().muted)
                    .add_modifier(Modifier::ITALIC),
            )));
        }
        Ok(lines)
    }

    fn syntax_for_editor_lines(
        path: &Path,
        source_lines: &[String],
    ) -> Option<&'static SyntaxReference> {
        SyntaxRegistry::syntax_for_path(path, &source_lines.join("\n"))
    }

    pub(super) fn syntax_for_path(path: &Path, source: &str) -> Option<&'static SyntaxReference> {
        SyntaxRegistry::syntax_for_path(path, source)
    }

    pub(super) fn highlighted_preview_lines(
        source: &str,
        syntax: Option<&'static SyntaxReference>,
    ) -> Vec<Line<'static>> {
        SyntaxRegistry::highlight_source(source, syntax)
    }

    pub(super) fn build_editor_render_lines(
        path: &Path,
        source_lines: &[String],
    ) -> Vec<Line<'static>> {
        let syntax = Self::syntax_for_editor_lines(path, source_lines);
        let lines = SyntaxRegistry::highlight_source(&source_lines.join("\n"), syntax);

        Self::add_line_numbers(lines)
    }

    pub(super) fn plain_preview_lines(source: &str) -> Vec<Line<'static>> {
        Self::split_preserving_lines(source)
            .into_iter()
            .map(Line::from)
            .collect()
    }

    pub(super) fn add_line_numbers(lines: Vec<Line<'static>>) -> Vec<Line<'static>> {
        let gutter_width = lines.len().max(1).to_string().len();

        lines
            .into_iter()
            .enumerate()
            .map(|(index, mut line)| {
                let mut spans = Vec::with_capacity(line.spans.len() + 1);
                spans.push(Span::styled(
                    format!("{:>width$} | ", index + 1, width = gutter_width),
                    Style::default().fg(ThemeRegistry::app().line_number),
                ));
                spans.append(&mut line.spans);
                line.spans = spans;
                line
            })
            .collect()
    }

    pub(super) fn highlight_editor_line(line: &mut Line<'static>) {
        let background = ThemeRegistry::app().line_highlight;
        for span in &mut line.spans {
            span.style = span.style.bg(background);
        }
    }

    pub(super) fn highlight_editor_selection(
        line: &mut Line<'static>,
        gutter_width: usize,
        selection_start: usize,
        selection_end: usize,
    ) {
        if selection_start >= selection_end {
            return;
        }

        let selection_start = gutter_width.saturating_add(selection_start);
        let selection_end = gutter_width.saturating_add(selection_end);
        let mut highlighted = Vec::with_capacity(line.spans.len() + 2);
        let mut offset = 0usize;

        for span in &line.spans {
            let text = span.content.as_ref();
            let span_width = text.chars().count();
            let span_start = offset;
            let span_end = offset.saturating_add(span_width);

            if span_width == 0 || selection_end <= span_start || selection_start >= span_end {
                highlighted.push(span.clone());
                offset = span_end;
                continue;
            }

            let local_start = selection_start.saturating_sub(span_start).min(span_width);
            let local_end = selection_end.min(span_end).saturating_sub(span_start);
            Self::push_span_slice(&mut highlighted, span, 0, local_start, span.style);
            Self::push_span_slice(
                &mut highlighted,
                span,
                local_start,
                local_end,
                span.style.bg(ThemeRegistry::app().selection_bg),
            );
            Self::push_span_slice(&mut highlighted, span, local_end, span_width, span.style);
            offset = span_end;
        }

        line.spans = highlighted;
    }

    pub(super) fn split_preserving_lines(source: &str) -> Vec<String> {
        if source.is_empty() {
            return vec![String::new()];
        }

        source.split('\n').map(ToString::to_string).collect()
    }

    pub(super) fn normalize_newlines(source: &str) -> String {
        source.replace("\r\n", "\n").replace('\r', "\n")
    }

    pub(super) fn maybe_trim_trailing_empty_line(lines: &mut Vec<Line<'static>>) {
        if lines.len() > 1
            && lines
                .last()
                .is_some_and(|line| line.spans.iter().all(|span| span.content.is_empty()))
        {
            lines.pop();
        }
    }

    fn push_span_slice(
        spans: &mut Vec<Span<'static>>,
        original: &Span<'static>,
        start: usize,
        end: usize,
        style: Style,
    ) {
        if start >= end {
            return;
        }

        let content = original
            .content
            .chars()
            .skip(start)
            .take(end.saturating_sub(start))
            .collect::<String>();
        if content.is_empty() {
            return;
        }

        spans.push(Span::styled(content, style));
    }
}
