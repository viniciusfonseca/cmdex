use super::super::*;
use super::UiSupport;

pub(in crate::app) struct HelpBarComponent;

impl HelpBarComponent {
    pub(in crate::app) fn draw(frame: &mut Frame, area: Rect) {
        let help = Paragraph::new("Quit: Ctrl+Q  Restart: Alt+R").style(
            Style::default()
                .bg(UiSupport::theme().app_bg)
                .fg(UiSupport::theme().muted),
        );
        frame.render_widget(help, area);
    }
}
