use super::{
    ui::{scrollable_text_height, theme},
    *,
};

pub(super) fn chat_lines(agent: &AgentState) -> Vec<Line<'static>> {
    if agent.messages.is_empty() {
        vec![Line::from("No messages yet.")]
    } else {
        agent
            .messages
            .iter()
            .flat_map(|message| render_chat_message_lines(message, &agent.definition.name))
            .collect()
    }
}

pub(super) fn chat_max_scroll(agent: &AgentState, area: Rect) -> u16 {
    let lines = chat_lines(agent);
    let inner_height = area.height.saturating_sub(2) as usize;
    let content_height = scrollable_text_height(&Text::from(lines), area);

    content_height.saturating_sub(inner_height) as u16
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ChatCommand {
    Model(ModelCommand),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ModelCommand {
    List,
    ResetDefault,
    Set {
        model: Option<String>,
        effort: Option<String>,
    },
}

pub(super) fn chat_command_from_input(input: &str) -> Option<ChatCommand> {
    let trimmed = input.trim();
    let remainder = trimmed.strip_prefix("/model")?;
    if !remainder.is_empty() && !remainder.starts_with(char::is_whitespace) {
        return None;
    }

    Some(ChatCommand::Model(parse_model_command(remainder.trim())))
}

fn parse_model_command(input: &str) -> ModelCommand {
    let input = input.trim();
    if input.is_empty() {
        return ModelCommand::List;
    }

    let parts = input.split_whitespace().collect::<Vec<_>>();
    let first = parts[0];

    if parts.len() == 1 && first.eq_ignore_ascii_case("default") {
        return ModelCommand::ResetDefault;
    }

    if parts.len() == 1 && is_reasoning_effort_value(first) {
        return ModelCommand::Set {
            model: None,
            effort: Some(first.to_string()),
        };
    }

    let effort = (parts.len() > 1).then(|| parts[1..].join(" "));
    ModelCommand::Set {
        model: Some(first.to_string()),
        effort,
    }
}

fn is_reasoning_effort_value(value: &str) -> bool {
    matches!(
        value.to_ascii_lowercase().as_str(),
        "minimal" | "low" | "medium" | "high" | "xhigh" | "none"
    )
}

pub(super) fn padded_chat_lines(agent: &AgentState, area: Rect) -> Vec<Line<'static>> {
    let mut lines = chat_lines(agent);
    let inner_height = area.height.saturating_sub(2) as usize;
    if inner_height == 0 {
        return lines;
    }

    let content_height = scrollable_text_height(&Text::from(lines.clone()), area);
    if content_height >= inner_height {
        return lines;
    }

    let mut padded = vec![Line::default(); inner_height - content_height];
    padded.append(&mut lines);
    padded
}

fn render_chat_message_lines(message: &ChatMessage, agent_name: &str) -> Vec<Line<'static>> {
    let role = match message.role {
        MessageRole::User => ("You", theme().yellow),
        MessageRole::Assistant => (agent_name, theme().green),
        MessageRole::Event => ("Event", theme().cyan),
        MessageRole::System => ("System", theme().red),
        MessageRole::Shell => ("Shell", theme().magenta),
    };

    let mut lines = vec![Line::from(vec![Span::styled(
        format!("{}:", role.0),
        Style::default().fg(role.1).add_modifier(Modifier::BOLD),
    )])];
    lines.extend(render_markdown_lines(&message.text));
    lines.push(Line::default());
    lines
}

fn render_markdown_lines(source: &str) -> Vec<Line<'static>> {
    if source.trim().is_empty() {
        return vec![Line::default()];
    }

    let mut options = MarkdownOptions::empty();
    options.insert(MarkdownOptions::ENABLE_STRIKETHROUGH);
    let parser = MarkdownParser::new_ext(source, options);
    let mut renderer = MarkdownRenderer::default();

    for event in parser {
        renderer.handle(event);
    }

    renderer.finish()
}

#[derive(Debug, Clone)]
enum MarkdownListKind {
    Unordered,
    Ordered(usize),
}

#[derive(Default)]
struct MarkdownRenderer {
    lines: Vec<Line<'static>>,
    current_spans: Vec<Span<'static>>,
    emphasis_depth: usize,
    strong_depth: usize,
    strikethrough_depth: usize,
    heading_level: Option<HeadingLevel>,
    code_block_depth: usize,
    blockquote_depth: usize,
    list_stack: Vec<MarkdownListKind>,
    link_targets: Vec<String>,
}

impl MarkdownRenderer {
    fn handle(&mut self, event: MarkdownEvent<'_>) {
        match event {
            MarkdownEvent::Start(tag) => self.start_tag(tag),
            MarkdownEvent::End(tag) => self.end_tag(tag),
            MarkdownEvent::Text(text) => self.push_text(
                text.as_ref(),
                if self.in_code_block() {
                    inline_code_style()
                } else {
                    self.current_style()
                },
            ),
            MarkdownEvent::Code(text) => self.push_text(text.as_ref(), inline_code_style()),
            MarkdownEvent::SoftBreak => {
                if self.in_code_block() {
                    self.push_line();
                } else {
                    self.push_text(" ", self.current_style());
                }
            }
            MarkdownEvent::HardBreak => self.push_line(),
            MarkdownEvent::Rule => {
                self.push_line_if_needed();
                self.lines.push(Line::from("---"));
                self.lines.push(Line::default());
            }
            MarkdownEvent::Html(text) | MarkdownEvent::InlineHtml(text) => {
                self.push_text(text.as_ref(), html_style())
            }
            MarkdownEvent::TaskListMarker(checked) => {
                let marker = if checked { "[x] " } else { "[ ] " };
                self.push_text(marker, task_marker_style());
            }
            MarkdownEvent::InlineMath(text) | MarkdownEvent::DisplayMath(text) => {
                self.push_text(text.as_ref(), inline_code_style())
            }
            MarkdownEvent::FootnoteReference(text) => {
                self.push_text(text.as_ref(), link_style());
            }
        }
    }

    fn start_tag(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Paragraph => {}
            Tag::Heading { level, .. } => {
                self.push_line_if_needed();
                self.heading_level = Some(level);
            }
            Tag::BlockQuote(_) => {
                self.push_line_if_needed();
                self.blockquote_depth += 1;
            }
            Tag::CodeBlock(kind) => {
                self.push_line_if_needed();
                self.code_block_depth += 1;
                if let CodeBlockKind::Fenced(language) = kind {
                    let language = language.trim();
                    if !language.is_empty() {
                        self.push_text(
                            &format!("```{language}"),
                            Style::default()
                                .fg(theme().muted)
                                .add_modifier(Modifier::ITALIC),
                        );
                        self.push_line();
                    }
                }
            }
            Tag::List(start) => {
                self.push_line_if_needed();
                self.list_stack.push(match start {
                    Some(number) => MarkdownListKind::Ordered(number as usize),
                    None => MarkdownListKind::Unordered,
                });
            }
            Tag::Item => {
                self.push_line_if_needed();
                self.ensure_block_prefix();
                let prefix = match self.list_stack.last_mut() {
                    Some(MarkdownListKind::Ordered(number)) => {
                        let current = *number;
                        *number += 1;
                        format!("{current}. ")
                    }
                    _ => "- ".to_string(),
                };
                self.current_spans
                    .push(Span::styled(prefix, task_marker_style()));
            }
            Tag::Emphasis => self.emphasis_depth += 1,
            Tag::Strong => self.strong_depth += 1,
            Tag::Strikethrough => self.strikethrough_depth += 1,
            Tag::Link { dest_url, .. } => self.link_targets.push(dest_url.to_string()),
            Tag::Image { dest_url, .. } => {
                self.push_text(
                    &format!("[image: {}]", dest_url),
                    Style::default().fg(theme().magenta),
                );
            }
            _ => {}
        }
    }

    fn end_tag(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph => {
                self.push_line_if_needed();
                self.trim_trailing_blank_lines();
            }
            TagEnd::Heading(..) => {
                self.push_line_if_needed();
                self.heading_level = None;
                self.trim_trailing_blank_lines();
            }
            TagEnd::BlockQuote(..) => {
                self.push_line_if_needed();
                self.blockquote_depth = self.blockquote_depth.saturating_sub(1);
                self.trim_trailing_blank_lines();
            }
            TagEnd::CodeBlock => {
                self.push_line_if_needed();
                self.code_block_depth = self.code_block_depth.saturating_sub(1);
                self.trim_trailing_blank_lines();
            }
            TagEnd::List(..) => {
                self.push_line_if_needed();
                self.list_stack.pop();
                self.trim_trailing_blank_lines();
            }
            TagEnd::Item => self.push_line_if_needed(),
            TagEnd::Emphasis => {
                self.emphasis_depth = self.emphasis_depth.saturating_sub(1);
            }
            TagEnd::Strong => {
                self.strong_depth = self.strong_depth.saturating_sub(1);
            }
            TagEnd::Strikethrough => {
                self.strikethrough_depth = self.strikethrough_depth.saturating_sub(1);
            }
            TagEnd::Link => {
                if let Some(target) = self.link_targets.pop() {
                    self.push_text(&format!(" ({target})"), link_style());
                }
            }
            _ => {}
        }
    }

    fn finish(mut self) -> Vec<Line<'static>> {
        self.push_line_if_needed();
        self.trim_trailing_blank_lines();
        if self.lines.is_empty() {
            vec![Line::default()]
        } else {
            self.lines
        }
    }

    fn push_text(&mut self, text: &str, style: Style) {
        for (index, segment) in text.split('\n').enumerate() {
            if index > 0 {
                self.push_line();
            }

            if segment.is_empty() {
                continue;
            }

            self.ensure_block_prefix();
            self.current_spans
                .push(Span::styled(segment.to_string(), style));
        }
    }

    fn push_line_if_needed(&mut self) {
        if !self.current_spans.is_empty() {
            self.push_line();
        }
    }

    fn push_line(&mut self) {
        if self.current_spans.is_empty() {
            self.lines.push(Line::default());
        } else {
            self.lines
                .push(Line::from(std::mem::take(&mut self.current_spans)));
        }
    }

    fn trim_trailing_blank_lines(&mut self) {
        while self
            .lines
            .last()
            .is_some_and(|line| line.spans.iter().all(|span| span.content.is_empty()))
        {
            self.lines.pop();
        }
    }

    fn ensure_block_prefix(&mut self) {
        if !self.current_spans.is_empty() || self.blockquote_depth == 0 {
            return;
        }

        for _ in 0..self.blockquote_depth {
            self.current_spans
                .push(Span::styled("> ", Style::default().fg(theme().muted)));
        }
    }

    fn current_style(&self) -> Style {
        let mut style = Style::default();

        if self.strong_depth > 0 || self.heading_level.is_some() {
            style = style.add_modifier(Modifier::BOLD);
        }
        if self.emphasis_depth > 0 {
            style = style.add_modifier(Modifier::ITALIC);
        }
        if self.strikethrough_depth > 0 {
            style = style.add_modifier(Modifier::CROSSED_OUT);
        }
        if self.heading_level.is_some() {
            style = style.fg(theme().accent);
        }

        style
    }

    fn in_code_block(&self) -> bool {
        self.code_block_depth > 0
    }
}

fn inline_code_style() -> Style {
    Style::default()
        .fg(theme().inline_code_fg)
        .bg(theme().inline_code_bg)
}

fn html_style() -> Style {
    Style::default()
        .fg(theme().magenta)
        .add_modifier(Modifier::ITALIC)
}

fn task_marker_style() -> Style {
    Style::default()
        .fg(theme().muted)
        .add_modifier(Modifier::BOLD)
}

fn link_style() -> Style {
    Style::default()
        .fg(theme().blue)
        .add_modifier(Modifier::UNDERLINED)
}

pub(super) fn chat_input_is_shell(input: &str) -> bool {
    input.starts_with('>')
}

pub(super) fn shell_command_from_input(input: &str) -> Option<String> {
    if !chat_input_is_shell(input) {
        return None;
    }

    let command = input.strip_prefix('>').unwrap_or(input).trim().to_string();
    if command.is_empty() {
        None
    } else {
        Some(command)
    }
}

pub(super) fn truncate_shell_text(text: &str) -> String {
    if text.chars().count() <= SHELL_OUTPUT_LIMIT {
        return text.trim_end_matches('\n').to_string();
    }

    let truncated = text.chars().take(SHELL_OUTPUT_LIMIT).collect::<String>();
    format!("{}\n[truncated]", truncated.trim_end_matches('\n'))
}

pub(super) fn format_shell_output(
    command: &str,
    stdout: &str,
    stderr: &str,
    exit_code: Option<i32>,
    success: bool,
) -> String {
    let mut body = String::new();
    let stdout = truncate_shell_text(stdout);
    let stderr = truncate_shell_text(stderr);

    if !stdout.is_empty() {
        body.push_str(&stdout);
    }
    if !stderr.is_empty() {
        if !body.is_empty() {
            body.push('\n');
        }
        if !stdout.is_empty() {
            body.push_str("[stderr]\n");
        }
        body.push_str(&stderr);
    }
    if body.is_empty() {
        body.push_str("[no output]");
    }

    let exit_code = exit_code
        .map(|code| code.to_string())
        .unwrap_or_else(|| "unavailable".to_string());
    let status = if success { "ok" } else { "failed" };

    format!("Command: `{command}`\n\n```text\n{body}\n```\n\nExit code: {exit_code} ({status})")
}

pub(super) fn load_codex_chat_model_label() -> Option<String> {
    let (model, reasoning_effort) = load_codex_chat_model_config()?;
    Some(format_chat_model_label(&model, reasoning_effort.as_deref()))
}

pub(super) fn load_codex_chat_model() -> Option<String> {
    let (model, _) = load_codex_chat_model_config()?;
    Some(model)
}

pub(super) fn load_codex_chat_reasoning_effort() -> Option<String> {
    let (_, reasoning_effort) = load_codex_chat_model_config()?;
    reasoning_effort
}

pub(super) fn format_chat_model_label(model: &str, reasoning_effort: Option<&str>) -> String {
    match reasoning_effort {
        Some(effort) if !effort.trim().is_empty() => format!("{model} · {effort}"),
        _ => model.to_string(),
    }
}

pub(super) fn resolve_chat_model_label(
    selected_model: Option<&str>,
    selected_effort: Option<&str>,
    default_model: Option<&str>,
    default_label: &str,
) -> String {
    match selected_model {
        Some(model) => format_chat_model_label(model, selected_effort),
        None => match selected_effort
            .map(str::trim)
            .filter(|effort| !effort.is_empty())
        {
            Some(effort) => default_model
                .map(|model| format_chat_model_label(model, Some(effort)))
                .unwrap_or_else(|| format!("default · {effort}")),
            None => default_label.to_string(),
        },
    }
}

fn load_codex_chat_model_config() -> Option<(String, Option<String>)> {
    let config_path = codex_config_path()?;
    let contents = fs::read_to_string(config_path).ok()?;

    let model = parse_codex_top_level_string(&contents, "model")?;
    let reasoning_effort = parse_codex_top_level_string(&contents, "model_reasoning_effort");
    Some((model, reasoning_effort))
}

fn codex_config_path() -> Option<PathBuf> {
    env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".codex")))
        .map(|dir| dir.join("config.toml"))
}

#[cfg(test)]
pub(super) fn parse_codex_model_from_config(contents: &str) -> Option<String> {
    parse_codex_top_level_string(contents, "model")
}

#[cfg(test)]
pub(super) fn parse_codex_reasoning_effort_from_config(contents: &str) -> Option<String> {
    parse_codex_top_level_string(contents, "model_reasoning_effort")
}

fn parse_codex_top_level_string(contents: &str, wanted_key: &str) -> Option<String> {
    let mut at_top_level = true;

    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if trimmed.starts_with('[') {
            at_top_level = false;
            continue;
        }
        if !at_top_level {
            continue;
        }

        let Some((key, value)) = trimmed.split_once('=') else {
            continue;
        };
        if key.trim() != wanted_key {
            continue;
        }

        let parsed = value.trim().trim_matches('"').trim_matches('\'').trim();
        if !parsed.is_empty() {
            return Some(parsed.to_string());
        }
    }

    None
}
