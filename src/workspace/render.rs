use super::*;

pub(super) fn read_text_preview(path: &Path) -> Result<Vec<Line<'static>>> {
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    if bytes.contains(&0) {
        return Ok(plain_preview_lines("Binary file preview is not available."));
    }

    let truncated = bytes.len() > PREVIEW_LIMIT;
    let preview = String::from_utf8_lossy(&bytes[..bytes.len().min(PREVIEW_LIMIT)]).into_owned();
    let preview = normalize_newlines(&preview);
    let mut lines = highlighted_preview_lines(&preview, syntax_for_path(path, &preview));
    if !truncated {
        maybe_trim_trailing_empty_line(&mut lines);
    }
    let mut lines = add_line_numbers(lines);
    if truncated {
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(
            "[truncated]".to_string(),
            Style::default()
                .fg(app_theme().muted)
                .add_modifier(Modifier::ITALIC),
        )));
    }
    Ok(lines)
}

fn syntax_for_editor_lines(
    path: &Path,
    source_lines: &[String],
) -> Option<&'static SyntaxReference> {
    path.extension()
        .and_then(|extension| extension.to_str())
        .and_then(|extension| SYNTAX_SET.find_syntax_by_extension(extension))
        .or_else(|| {
            source_lines
                .first()
                .and_then(|line| SYNTAX_SET.find_syntax_by_first_line(line))
        })
}

pub(super) fn syntax_for_path<'a>(
    path: &Path,
    source: &'a str,
) -> Option<&'static SyntaxReference> {
    path.extension()
        .and_then(|extension| extension.to_str())
        .and_then(|extension| SYNTAX_SET.find_syntax_by_extension(extension))
        .or_else(|| {
            source
                .lines()
                .next()
                .and_then(|line| SYNTAX_SET.find_syntax_by_first_line(line))
        })
}

pub(super) fn highlighted_preview_lines(
    source: &str,
    syntax: Option<&'static SyntaxReference>,
) -> Vec<Line<'static>> {
    match syntax {
        Some(syntax) => {
            let mut highlighter = HighlightLines::new(syntax, syntax_theme());
            split_preserving_lines(source)
                .into_iter()
                .map(|line| {
                    if line.is_empty() {
                        return Line::default();
                    }

                    match highlighter.highlight_line(&line, &SYNTAX_SET) {
                        Ok(ranges) => Line::from(
                            ranges
                                .into_iter()
                                .map(|(style, text)| {
                                    Span::styled(text.to_string(), to_ratatui_style(style))
                                })
                                .collect::<Vec<_>>(),
                        ),
                        Err(_) => Line::from(line),
                    }
                })
                .collect()
        }
        None => plain_preview_lines(source),
    }
}

pub(super) fn build_editor_render_lines(
    path: &Path,
    source_lines: &[String],
) -> Vec<Line<'static>> {
    let syntax = syntax_for_editor_lines(path, source_lines);
    let lines = match syntax {
        Some(syntax) => {
            let mut highlighter = HighlightLines::new(syntax, syntax_theme());
            source_lines
                .iter()
                .map(|line| {
                    if line.is_empty() {
                        return Line::default();
                    }

                    match highlighter.highlight_line(line, &SYNTAX_SET) {
                        Ok(ranges) => Line::from(
                            ranges
                                .into_iter()
                                .map(|(style, text)| {
                                    Span::styled(text.to_string(), to_ratatui_style(style))
                                })
                                .collect::<Vec<_>>(),
                        ),
                        Err(_) => Line::from(line.clone()),
                    }
                })
                .collect::<Vec<_>>()
        }
        None => source_lines
            .iter()
            .cloned()
            .map(Line::from)
            .collect::<Vec<_>>(),
    };

    add_line_numbers(lines)
}

pub(super) fn plain_preview_lines(source: &str) -> Vec<Line<'static>> {
    split_preserving_lines(source)
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
                Style::default().fg(app_theme().line_number),
            ));
            spans.append(&mut line.spans);
            line.spans = spans;
            line
        })
        .collect()
}

pub(super) fn highlight_editor_line(line: &mut Line<'static>) {
    let background = app_theme().line_highlight;
    for span in &mut line.spans {
        span.style = span.style.bg(background);
    }
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
