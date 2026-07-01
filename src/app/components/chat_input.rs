use super::super::chat::ChatSupport;
use super::super::*;
use super::UiSupport;

pub(in crate::app) struct ChatInputComponent;

impl ChatInputComponent {
    pub(in crate::app) fn draw(frame: &mut Frame, app: &App, area: Rect) {
        let shell_mode = ChatSupport::input_is_shell(&app.chat_input);
        let thinking = app.active_agent().is_some_and(|agent| agent.thinking);
        let shell_running = app.active_agent().is_some_and(|agent| agent.shell_running);
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
                "Message · {}  {} Thinking...",
                app.active_chat_model_label(),
                SPINNER[app.spinner_index]
            )
        } else {
            format!("Message · {}", app.active_chat_model_label())
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
