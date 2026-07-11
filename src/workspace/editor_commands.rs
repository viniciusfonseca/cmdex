use super::*;

impl WorkspaceEditorState {
    pub fn apply(&mut self, command: EditorCommand) {
        match command {
            EditorCommand::Move { direction, extend } => match (direction, extend) {
                (EditorDirection::Left, false) => self.move_left(),
                (EditorDirection::Right, false) => self.move_right(),
                (EditorDirection::Up, false) => self.move_up(),
                (EditorDirection::Down, false) => self.move_down(),
                (EditorDirection::Left, true) => self.extend_left(),
                (EditorDirection::Right, true) => self.extend_right(),
                (EditorDirection::Up, true) => self.extend_up(),
                (EditorDirection::Down, true) => self.extend_down(),
            },
            EditorCommand::MoveLineStart { extend } => {
                if extend {
                    self.extend_line_start();
                } else {
                    self.move_line_start();
                }
            }
            EditorCommand::MoveLineEnd { extend } => {
                if extend {
                    self.extend_line_end();
                } else {
                    self.move_line_end();
                }
            }
            EditorCommand::MovePage { lines, extend, up } => match (up, extend) {
                (true, false) => self.move_page_up(lines),
                (false, false) => self.move_page_down(lines),
                (true, true) => self.extend_page_up(lines),
                (false, true) => self.extend_page_down(lines),
            },
            EditorCommand::EnterInsert => self.enter_insert_mode(),
            EditorCommand::EnterInsertAfter => self.enter_insert_after(),
            EditorCommand::EnterVisual => self.enter_visual_mode(),
            EditorCommand::ExitVisual => self.exit_visual_mode(),
            EditorCommand::ExitInsert => {
                self.mode = EditorMode::Normal;
                self.clear_selection();
            }
            EditorCommand::StartCommand => self.start_command(),
            EditorCommand::DeleteChar => self.delete_char(),
            EditorCommand::DeleteSelection => {
                self.delete_selection();
                self.mode = EditorMode::Normal;
            }
            EditorCommand::Backspace => self.backspace(),
            EditorCommand::InsertNewline => self.insert_newline(),
            EditorCommand::InsertTab => {
                for _ in 0..4 {
                    self.insert_char(' ');
                }
            }
            EditorCommand::OpenBelow => self.open_below(),
            EditorCommand::Undo => {
                let _ = self.undo();
            }
        }
    }
}
