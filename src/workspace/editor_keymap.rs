use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::{EditorCommand, EditorDirection, EditorMode};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EditorKeyAction {
    Apply(EditorCommand),
    Copy,
    Paste,
    CommandCancel,
    CommandBackspace,
    CommandSubmit,
    CommandInsert(char),
    Consume,
}

pub(crate) fn map_key(
    mode: EditorMode,
    key: KeyEvent,
    page_step: usize,
) -> Option<EditorKeyAction> {
    match mode {
        EditorMode::Command => map_command_key(key),
        EditorMode::Visual => map_visual_key(key, page_step),
        EditorMode::Insert => map_insert_key(key, page_step),
        EditorMode::Normal => map_normal_key(key, page_step),
    }
}

fn map_command_key(key: KeyEvent) -> Option<EditorKeyAction> {
    match key.code {
        KeyCode::Esc => Some(EditorKeyAction::CommandCancel),
        KeyCode::Backspace => Some(EditorKeyAction::CommandBackspace),
        KeyCode::Enter => Some(EditorKeyAction::CommandSubmit),
        KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            Some(EditorKeyAction::CommandInsert(character))
        }
        _ => Some(EditorKeyAction::Consume),
    }
}

fn map_visual_key(key: KeyEvent, page_step: usize) -> Option<EditorKeyAction> {
    let action = match key.code {
        KeyCode::Esc | KeyCode::Char('v') => EditorKeyAction::Apply(EditorCommand::ExitVisual),
        KeyCode::Char('y') => EditorKeyAction::Copy,
        KeyCode::Char('p') => EditorKeyAction::Paste,
        KeyCode::Left | KeyCode::Char('h') => movement(EditorDirection::Left, true),
        KeyCode::Right | KeyCode::Char('l') => movement(EditorDirection::Right, true),
        KeyCode::Up | KeyCode::Char('k') => movement(EditorDirection::Up, true),
        KeyCode::Down | KeyCode::Char('j') => movement(EditorDirection::Down, true),
        KeyCode::Home | KeyCode::Char('0') => line_start(true),
        KeyCode::End | KeyCode::Char('$') => line_end(true),
        KeyCode::PageUp => page(true, page_step),
        KeyCode::PageDown => page(false, page_step),
        KeyCode::Delete | KeyCode::Backspace | KeyCode::Char('x') => {
            EditorKeyAction::Apply(EditorCommand::DeleteSelection)
        }
        KeyCode::Char(':') => EditorKeyAction::Apply(EditorCommand::StartCommand),
        _ => return None,
    };
    Some(action)
}

fn map_insert_key(key: KeyEvent, page_step: usize) -> Option<EditorKeyAction> {
    let action = match key.code {
        KeyCode::Esc => EditorKeyAction::Apply(EditorCommand::ExitInsert),
        KeyCode::Enter => EditorKeyAction::Apply(EditorCommand::InsertNewline),
        KeyCode::Backspace => EditorKeyAction::Apply(EditorCommand::Backspace),
        KeyCode::Delete => EditorKeyAction::Apply(EditorCommand::DeleteChar),
        KeyCode::Left => movement(EditorDirection::Left, false),
        KeyCode::Right => movement(EditorDirection::Right, false),
        KeyCode::Up => movement(EditorDirection::Up, false),
        KeyCode::Down => movement(EditorDirection::Down, false),
        KeyCode::Home => line_start(false),
        KeyCode::End => line_end(false),
        KeyCode::PageUp => page(true, page_step),
        KeyCode::PageDown => page(false, page_step),
        KeyCode::Tab => EditorKeyAction::Apply(EditorCommand::InsertTab),
        _ => return None,
    };
    Some(action)
}

fn map_normal_key(key: KeyEvent, page_step: usize) -> Option<EditorKeyAction> {
    if key.modifiers.contains(KeyModifiers::SHIFT) {
        let action = match key.code {
            KeyCode::Left => movement(EditorDirection::Left, true),
            KeyCode::Right => movement(EditorDirection::Right, true),
            KeyCode::Up => movement(EditorDirection::Up, true),
            KeyCode::Down => movement(EditorDirection::Down, true),
            KeyCode::Home => line_start(true),
            KeyCode::End => line_end(true),
            KeyCode::PageUp => page(true, page_step).extend(true),
            KeyCode::PageDown => page(false, page_step).extend(true),
            _ => return None,
        };
        return Some(action);
    }

    let action = match key.code {
        KeyCode::Esc | KeyCode::Enter => EditorKeyAction::Consume,
        KeyCode::Char('y') => EditorKeyAction::Copy,
        KeyCode::Char('p') => EditorKeyAction::Paste,
        KeyCode::Up | KeyCode::Char('k') => movement(EditorDirection::Up, false),
        KeyCode::Down | KeyCode::Char('j') => movement(EditorDirection::Down, false),
        KeyCode::Left | KeyCode::Char('h') => movement(EditorDirection::Left, false),
        KeyCode::Right | KeyCode::Char('l') => movement(EditorDirection::Right, false),
        KeyCode::Home | KeyCode::Char('0') => line_start(false),
        KeyCode::End | KeyCode::Char('$') => line_end(false),
        KeyCode::PageUp => page(true, page_step),
        KeyCode::PageDown => page(false, page_step),
        KeyCode::Delete | KeyCode::Char('x') => EditorKeyAction::Apply(EditorCommand::DeleteChar),
        KeyCode::Char('v') => EditorKeyAction::Apply(EditorCommand::EnterVisual),
        KeyCode::Char('u') => EditorKeyAction::Apply(EditorCommand::Undo),
        KeyCode::Char('i') => EditorKeyAction::Apply(EditorCommand::EnterInsert),
        KeyCode::Char('a') => EditorKeyAction::Apply(EditorCommand::EnterInsertAfter),
        KeyCode::Char('o') => EditorKeyAction::Apply(EditorCommand::OpenBelow),
        KeyCode::Char(':') => EditorKeyAction::Apply(EditorCommand::StartCommand),
        _ => return None,
    };
    Some(action)
}

fn movement(direction: EditorDirection, extend: bool) -> EditorKeyAction {
    EditorKeyAction::Apply(EditorCommand::Move { direction, extend })
}

fn line_start(extend: bool) -> EditorKeyAction {
    EditorKeyAction::Apply(EditorCommand::MoveLineStart { extend })
}

fn line_end(extend: bool) -> EditorKeyAction {
    EditorKeyAction::Apply(EditorCommand::MoveLineEnd { extend })
}

fn page(up: bool, lines: usize) -> EditorKeyAction {
    EditorKeyAction::Apply(EditorCommand::MovePage {
        lines,
        extend: false,
        up,
    })
}

trait ExtendEditorKeyAction {
    fn extend(self, extend: bool) -> Self;
}

impl ExtendEditorKeyAction for EditorKeyAction {
    fn extend(self, extend: bool) -> Self {
        match self {
            EditorKeyAction::Apply(EditorCommand::MovePage { lines, up, .. }) => {
                EditorKeyAction::Apply(EditorCommand::MovePage { lines, extend, up })
            }
            action => action,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arrows_and_vim_keys_share_movement_commands() {
        let arrow = map_key(
            EditorMode::Normal,
            KeyEvent::new(KeyCode::Left, KeyModifiers::NONE),
            4,
        );
        let vim = map_key(
            EditorMode::Normal,
            KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE),
            4,
        );

        assert_eq!(arrow, vim);
    }

    #[test]
    fn shift_page_movement_extends_selection() {
        assert_eq!(
            map_key(
                EditorMode::Normal,
                KeyEvent::new(KeyCode::PageDown, KeyModifiers::SHIFT),
                4,
            ),
            Some(EditorKeyAction::Apply(EditorCommand::MovePage {
                lines: 4,
                extend: true,
                up: false,
            }))
        );
    }
}
