use super::{components::*, *};

impl App {
    pub(super) fn move_selection_up(&mut self) {
        let _ = ChatComponent::move_selection_up(self)
            || WorkspaceScreen::move_selection_up(self)
            || ShellComponent::move_selection_up(self)
            || GitDiffComponent::move_selection_up(self);
    }

    pub(super) fn move_selection_down(&mut self) {
        let _ = ChatComponent::move_selection_down(self)
            || WorkspaceScreen::move_selection_down(self)
            || ShellComponent::move_selection_down(self)
            || GitDiffComponent::move_selection_down(self);
    }
    pub(super) fn scroll_content_up(&mut self, area: Rect) {
        self.scroll_content_up_by(area, CONTENT_SCROLL_STEP);
    }

    pub(super) fn scroll_content_up_by(&mut self, area: Rect, lines: u16) {
        let _ = WorkspaceScreen::scroll_content(self, area, lines, true)
            || ShellComponent::scroll_output(self, lines, true)
            || GitDiffComponent::scroll_preview(self, lines, true);
    }
    pub(super) fn should_handle_mouse_scroll(
        &mut self,
        direction: ScrollDirection,
        horizontal: bool,
    ) -> bool {
        let axis = if horizontal {
            ScrollAxis::Horizontal
        } else {
            ScrollAxis::Vertical
        };
        self.should_handle_mouse_scroll_at_axis(axis, direction, Instant::now())
    }

    pub(super) fn should_handle_mouse_scroll_at_axis(
        &mut self,
        axis: ScrollAxis,
        direction: ScrollDirection,
        now: Instant,
    ) -> bool {
        if self
            .last_mouse_scroll
            .is_some_and(|(last_axis, last_direction, last_at)| {
                last_axis == axis
                    && last_direction == direction
                    && now
                        .checked_duration_since(last_at)
                        .is_some_and(|elapsed| elapsed < MOUSE_SCROLL_DEBOUNCE)
            })
        {
            return false;
        }

        self.last_mouse_scroll = Some((axis, direction, now));
        true
    }

    pub(super) fn scroll_content_down(&mut self, area: Rect) {
        self.scroll_content_down_by(area, CONTENT_SCROLL_STEP);
    }

    pub(super) fn scroll_content_down_by(&mut self, area: Rect, lines: u16) {
        let _ = WorkspaceScreen::scroll_content(self, area, lines, false)
            || ShellComponent::scroll_output(self, lines, false)
            || GitDiffComponent::scroll_preview(self, lines, false);
    }
}
