use super::*;

impl WorkspaceEditorState {
    pub fn shortcuts_help_open(&self) -> bool {
        matches!(&self.overlay, EditorOverlay::ShortcutsHelp)
    }

    pub fn close_shortcuts_help(&mut self) {
        if self.shortcuts_help_open() {
            self.overlay = EditorOverlay::None;
        }
    }

    pub fn toggle_shortcuts_help(&mut self) {
        self.overlay = if self.shortcuts_help_open() {
            EditorOverlay::None
        } else {
            EditorOverlay::ShortcutsHelp
        };
    }

    pub fn clear_completion(&mut self) {
        if matches!(self.overlay, EditorOverlay::Completion(_)) {
            self.overlay = EditorOverlay::None;
        }
    }

    pub fn request_hover(&mut self, position: EditorPosition) -> bool {
        if self.hover_request == Some(position) {
            return false;
        }

        self.hover = None;
        self.hover_request = Some(position);
        true
    }

    pub fn resolve_hover(&mut self, position: EditorPosition, hover: Option<String>) -> bool {
        if self.hover_request != Some(position) {
            return false;
        }

        self.hover = hover;
        true
    }

    pub fn hover_popover(&self) -> Option<(&str, EditorPosition)> {
        self.hover.as_deref().zip(self.hover_request)
    }

    pub fn hover_request_position(&self) -> Option<EditorPosition> {
        self.hover_request
    }

    pub fn request_completion(&mut self, position: EditorPosition) {
        self.overlay = EditorOverlay::Completion(EditorCompletionState {
            items: Vec::new(),
            request_position: position,
            selected: 0,
            scroll: 0,
        });
    }

    pub fn resolve_completion(
        &mut self,
        position: EditorPosition,
        items: Vec<EditorCompletionItem>,
    ) -> bool {
        let is_current_request = matches!(
            &self.overlay,
            EditorOverlay::Completion(EditorCompletionState {
                request_position,
                ..
            }) if *request_position == position
        );
        if !is_current_request {
            return false;
        }

        if items.is_empty() {
            self.clear_completion();
            return true;
        }

        if let EditorOverlay::Completion(completion) = &mut self.overlay {
            completion.selected = items.iter().position(|item| item.preselected).unwrap_or(0);
            completion.items = items;
            completion.scroll = 0;
        }
        self.ensure_completion_selection_visible(COMPLETION_POPOVER_MAX_ITEMS);
        true
    }

    pub fn completion_popover(&self) -> Option<(&[EditorCompletionItem], usize, EditorPosition)> {
        let EditorOverlay::Completion(completion) = &self.overlay else {
            return None;
        };
        if completion.items.is_empty() {
            return None;
        }

        Some((
            completion.items.as_slice(),
            completion
                .selected
                .min(completion.items.len().saturating_sub(1)),
            completion.request_position,
        ))
    }

    pub fn completion_window_start(&self, max_items: usize) -> usize {
        let visible_len = self.completion_visible_len(max_items);
        self.clamped_completion_scroll(visible_len)
    }

    pub fn set_completion_window_start(&mut self, start: usize, max_items: usize) {
        let visible_len = self.completion_visible_len(max_items);
        if visible_len == 0 {
            if let EditorOverlay::Completion(completion) = &mut self.overlay {
                completion.scroll = 0;
                completion.selected = 0;
            }
            return;
        }

        let start = self.clamp_completion_scroll(start, visible_len);
        let end = start + visible_len.saturating_sub(1);
        if let EditorOverlay::Completion(completion) = &mut self.overlay {
            completion.scroll = start;
            completion.selected = completion
                .selected
                .min(completion.items.len().saturating_sub(1))
                .clamp(start, end);
        }
    }

    pub fn select_previous_completion(&mut self) {
        let Some(completion) = self.completion_state_mut() else {
            return;
        };
        if completion.items.is_empty() {
            return;
        }

        completion.selected = if completion.selected == 0 {
            completion.items.len().saturating_sub(1)
        } else {
            completion.selected - 1
        };
        self.ensure_completion_selection_visible(COMPLETION_POPOVER_MAX_ITEMS);
    }

    pub fn select_next_completion(&mut self) {
        let Some(completion) = self.completion_state_mut() else {
            return;
        };
        if completion.items.is_empty() {
            return;
        }

        completion.selected = (completion.selected + 1) % completion.items.len();
        self.ensure_completion_selection_visible(COMPLETION_POPOVER_MAX_ITEMS);
    }

    pub fn apply_selected_completion(&mut self) -> bool {
        let Some(completion) = self.completion_state() else {
            return false;
        };
        let Some(item) = completion.items.get(completion.selected).cloned() else {
            return false;
        };

        let start = self.clamp_position(item.replace_start);
        let end = self.clamp_position(item.replace_end);
        let (start, end) = if start <= end {
            (start, end)
        } else {
            (end, start)
        };

        self.clear_selection();
        self.cursor_row = start.row;
        self.cursor_col = start.col;
        self.selection_anchor = Some(start);
        self.cursor_row = end.row;
        self.cursor_col = end.col;
        self.preferred_col = self.cursor_col;

        let applied = self.paste_text(&item.insert_text);
        self.clear_completion();
        applied
    }

    fn completion_visible_len(&self, max_items: usize) -> usize {
        self.completion_state()
            .map(|completion| completion.items.len().min(max_items))
            .unwrap_or(0)
    }

    fn clamped_completion_scroll(&self, visible_len: usize) -> usize {
        if visible_len == 0 {
            return 0;
        }

        self.completion_state()
            .map(|completion| self.clamp_completion_scroll(completion.scroll, visible_len))
            .unwrap_or(0)
    }

    fn clamp_completion_scroll(&self, scroll: usize, visible_len: usize) -> usize {
        if visible_len == 0 {
            return 0;
        }

        self.completion_state()
            .map(|completion| completion.items.len().saturating_sub(visible_len))
            .map(|max_scroll| scroll.min(max_scroll))
            .unwrap_or(0)
    }

    fn ensure_completion_selection_visible(&mut self, max_items: usize) {
        let visible_len = self.completion_visible_len(max_items);
        if visible_len == 0 {
            if let EditorOverlay::Completion(completion) = &mut self.overlay {
                completion.scroll = 0;
            }
            return;
        }

        let Some(completion) = self.completion_state() else {
            return;
        };
        let selected = completion.selected;
        let mut start = self.clamped_completion_scroll(visible_len);
        if selected < start {
            start = selected;
        } else if selected >= start + visible_len {
            start = selected + 1 - visible_len;
        }
        let start = self.clamp_completion_scroll(start, visible_len);
        if let EditorOverlay::Completion(completion) = &mut self.overlay {
            completion.scroll = start;
        }
    }

    fn completion_state(&self) -> Option<&EditorCompletionState> {
        match &self.overlay {
            EditorOverlay::Completion(completion) => Some(completion),
            EditorOverlay::None | EditorOverlay::ShortcutsHelp => None,
        }
    }

    fn completion_state_mut(&mut self) -> Option<&mut EditorCompletionState> {
        match &mut self.overlay {
            EditorOverlay::Completion(completion) => Some(completion),
            EditorOverlay::None | EditorOverlay::ShortcutsHelp => None,
        }
    }
}
