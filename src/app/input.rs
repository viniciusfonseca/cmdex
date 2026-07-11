use crossterm::event::{Event, KeyEvent, KeyEventKind, MouseEvent};

pub(super) enum AppInput {
    Key(KeyEvent),
    Mouse(MouseEvent),
    Paste(String),
    Tick,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AppOutcome {
    Handled,
    Redraw,
    Exit(super::AppExit),
}

impl AppOutcome {
    pub(super) fn needs_redraw(self) -> bool {
        matches!(self, Self::Redraw)
    }

    pub(super) fn exit(self) -> Option<super::AppExit> {
        match self {
            Self::Exit(exit) => Some(exit),
            Self::Handled | Self::Redraw => None,
        }
    }
}

impl AppInput {
    pub(super) fn from_terminal_event(event: Event) -> Option<Self> {
        match event {
            Event::Key(key) if key.kind == KeyEventKind::Press => Some(Self::Key(key)),
            Event::Mouse(mouse) => Some(Self::Mouse(mouse)),
            Event::Paste(text) => Some(Self::Paste(text)),
            _ => None,
        }
    }
}
