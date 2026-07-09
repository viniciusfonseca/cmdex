use super::super::chat::ChatSupport;
use super::super::*;
use super::UiSupport;

pub(in crate::app) struct ChatInputComponent;

impl ChatInputComponent {
    pub(in crate::app) fn draw(frame: &mut Frame, app: &App, area: Rect) {
        let shell_mode = ChatSupport::input_is_shell(&app.chat_input);
        let thinking = app.active_agent().is_some_and(|agent| agent.thinking);
        let shell_running = app.active_agent().is_some_and(|agent| agent.shell_running);
        let queued = app
            .active_agent()
            .map(|agent| agent.queued_chat_count())
            .unwrap_or(0);
        let title = if shell_running {
            format!(
                "Shell · {}  {} Running...",
                app.active_chat_model_label(),
                SPINNER[app.spinner_index]
            )
        } else if shell_mode {
            format!("Shell · {}", app.active_chat_model_label())
        } else if thinking {
            format!(
                "Message · {}  {} Thinking...{}",
                app.active_chat_model_label(),
                SPINNER[app.spinner_index],
                Self::queue_suffix(queued),
            )
        } else {
            format!(
                "Message · {}{}",
                app.active_chat_model_label(),
                Self::queue_suffix(queued)
            )
        };

        let wrapped_lines = Self::wrapped_lines(&app.chat_input, area.width.saturating_sub(2));
        let input = Paragraph::new(Text::from(
            wrapped_lines
                .iter()
                .cloned()
                .map(Line::from)
                .collect::<Vec<_>>(),
        ))
        .block(UiSupport::panel_block().title(title))
        .style(UiSupport::panel_style())
        .wrap(Wrap { trim: false });
        frame.render_widget(input, area);
        Self::draw_queue_popover(frame, app, area);
        Self::draw_model_picker(frame, app, area);

        let last_line = wrapped_lines
            .last()
            .map(|line| line.chars().count())
            .unwrap_or(0) as u16;
        let cursor_row = wrapped_lines.len().saturating_sub(1) as u16;
        let x = area
            .x
            .saturating_add(1 + last_line)
            .min(area.x + area.width.saturating_sub(2));
        let y = area
            .y
            .saturating_add(1 + cursor_row)
            .min(area.y + area.height.saturating_sub(2));
        frame.set_cursor_position((x, y));
    }

    pub(in crate::app) fn height_for_main_area(input: &str, main_area: Rect) -> u16 {
        let available = main_area.height.saturating_sub(3);
        if available == 0 {
            return 0;
        }

        let desired = Self::wrapped_lines(input, main_area.width.saturating_sub(2))
            .len()
            .saturating_add(2) as u16;
        let min_height = available.min(3);
        let max_height = available.saturating_sub(1).max(min_height);

        desired.clamp(min_height, max_height)
    }

    fn queue_suffix(count: usize) -> String {
        if count == 0 {
            String::new()
        } else {
            format!("  ·  {count} queued  ·  Alt+Up/Down browse  Alt+Backspace cancel")
        }
    }

    fn draw_queue_popover(frame: &mut Frame, app: &App, area: Rect) {
        let Some(agent) = app.active_agent() else {
            return;
        };
        if !agent.has_queued_chat_messages() {
            return;
        }

        let items = agent.queued_chat_messages();
        let selected = agent.selected_queued_chat_index().unwrap_or(0);
        let visible_count = items.len().min(5);
        let start = selected
            .saturating_sub(visible_count.saturating_sub(1))
            .min(items.len().saturating_sub(visible_count));
        let max_width = area.width.saturating_sub(2).clamp(1, 72);
        let popup_height = visible_count as u16 + 2;
        let popup_y = area.y.saturating_sub(popup_height.saturating_sub(1));
        let popup_area = Rect::new(area.x, popup_y, max_width, popup_height);

        let lines = items
            .iter()
            .skip(start)
            .take(visible_count)
            .enumerate()
            .map(|(offset, item)| {
                let is_selected = start + offset == selected;
                let style = if is_selected {
                    UiSupport::selection_style()
                } else {
                    Style::default()
                        .bg(UiSupport::theme().panel_bg)
                        .fg(UiSupport::theme().foreground)
                };
                let prefix = if is_selected { "› " } else { "  " };
                Line::from(Span::styled(
                    format!(
                        "{}{}",
                        prefix,
                        Self::queue_preview(&item.text, popup_area.width.saturating_sub(4))
                    ),
                    style,
                ))
            })
            .collect::<Vec<_>>();

        frame.render_widget(Clear, popup_area);
        let popup = Paragraph::new(Text::from(lines))
            .block(
                UiSupport::rounded_block()
                    .title(format!("Queue ({})", items.len()))
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
            )
            .wrap(Wrap { trim: false });
        frame.render_widget(popup, popup_area);
    }

    fn draw_model_picker(frame: &mut Frame, app: &App, area: Rect) {
        let Some(picker) = app.model_picker.as_ref() else {
            return;
        };
        if picker.models.is_empty() {
            return;
        }

        let (title, selected, labels) = match &picker.view {
            super::super::ModelPickerView::Models => (
                "Select model".to_string(),
                picker.selected.min(picker.models.len().saturating_sub(1)),
                picker
                    .models
                    .iter()
                    .map(Self::model_label)
                    .collect::<Vec<_>>(),
            ),
            super::super::ModelPickerView::Efforts {
                model_index,
                selected,
            } => {
                let model = &picker.models[*model_index];
                (
                    "Select effort".to_string(),
                    (*selected).min(model.supported_reasoning_efforts.len().saturating_sub(1)),
                    model
                        .supported_reasoning_efforts
                        .iter()
                        .map(Self::effort_label)
                        .collect::<Vec<_>>(),
                )
            }
        };
        let visible_count = labels.len().min(7);
        let start = UiSupport::list_offset(selected, labels.len(), visible_count);
        let popup_width = area.width.saturating_sub(2).min(72).max(1);
        let popup_height = visible_count as u16 + 2;
        let popup_y = area.y.saturating_sub(popup_height.saturating_sub(1));
        let popup_area = Rect::new(area.x, popup_y, popup_width, popup_height);

        let lines = labels
            .iter()
            .skip(start)
            .take(visible_count)
            .enumerate()
            .map(|(offset, label)| {
                let is_selected = start + offset == selected;
                let style = if is_selected {
                    UiSupport::selection_style()
                } else {
                    Style::default()
                        .bg(UiSupport::theme().panel_bg)
                        .fg(UiSupport::theme().foreground)
                };
                let prefix = if is_selected { "› " } else { "  " };
                Line::from(Span::styled(
                    format!(
                        "{}{}",
                        prefix,
                        Self::queue_preview(label, popup_area.width.saturating_sub(4))
                    ),
                    style,
                ))
            })
            .collect::<Vec<_>>();

        frame.render_widget(Clear, popup_area);
        let popup = Paragraph::new(Text::from(lines))
            .block(
                UiSupport::rounded_block()
                    .title(title)
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
            )
            .wrap(Wrap { trim: false });
        frame.render_widget(popup, popup_area);
    }

    fn model_label(model: &ModelInfo) -> String {
        let mut label = model.model.clone();
        if model.display_name != model.model {
            label.push_str(" - ");
            label.push_str(&model.display_name);
        }
        if model.id != model.model {
            label.push_str(" [");
            label.push_str(&model.id);
            label.push(']');
        }
        label
    }

    fn effort_label(effort: &ModelReasoningEffort) -> String {
        effort
            .description
            .as_deref()
            .filter(|description| !description.trim().is_empty())
            .map(|description| format!("{} - {description}", effort.reasoning_effort))
            .unwrap_or_else(|| effort.reasoning_effort.clone())
    }

    fn queue_preview(text: &str, width: u16) -> String {
        let compact = text.split_whitespace().collect::<Vec<_>>().join(" ");
        let limit = usize::from(width.max(1));
        if compact.chars().count() <= limit {
            compact
        } else {
            let mut truncated = compact
                .chars()
                .take(limit.saturating_sub(1))
                .collect::<String>();
            truncated.push('…');
            truncated
        }
    }

    pub(in crate::app) fn wrapped_lines(input: &str, width: u16) -> Vec<String> {
        let width = usize::from(width.max(1));
        if input.is_empty() {
            return vec![String::new()];
        }

        let mut wrapped = Vec::new();
        for raw_line in input.split('\n') {
            let mut current = String::new();
            let mut count = 0usize;

            for character in raw_line.chars() {
                if count == width {
                    wrapped.push(current);
                    current = String::new();
                    count = 0;
                }

                current.push(character);
                count += 1;
            }

            wrapped.push(current);
        }

        if wrapped.is_empty() {
            wrapped.push(String::new());
        }

        wrapped
    }
}
