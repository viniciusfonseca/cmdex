use super::super::*;
use super::UiSupport;

pub(in crate::app) struct StatusBarComponent;

impl StatusBarComponent {
    pub(in crate::app) fn draw(frame: &mut Frame, app: &App, area: Rect) {
        let text = app
            .status_message
            .as_deref()
            .unwrap_or("Quit: Ctrl+Q  Restart: Alt+R");
        let help = Paragraph::new(text).style(Style::default().bg(UiSupport::theme().app_bg).fg(
            if app.status_message.is_some() {
                UiSupport::theme().info
            } else {
                UiSupport::theme().muted
            },
        ));
        frame.render_widget(help, area);
    }
}
