use super::super::*;

pub(in crate::app) struct UiSupport;

pub(in crate::app) struct SelectableListPopover;

impl SelectableListPopover {
    pub(in crate::app) fn window(
        selected: usize,
        item_count: usize,
        max_visible: usize,
    ) -> (usize, usize) {
        let visible = item_count.min(max_visible);
        let start = selected
            .saturating_sub(visible.saturating_sub(1))
            .min(item_count.saturating_sub(visible));
        (start, visible)
    }

    pub(in crate::app) fn area(anchor: Rect, width: u16, visible: usize) -> Rect {
        let height = visible as u16 + 2;
        Rect::new(
            anchor.x,
            anchor.y.saturating_sub(height.saturating_sub(1)),
            width,
            height,
        )
    }

    pub(in crate::app) fn label_lines(
        labels: &[String],
        selected: usize,
        start: usize,
        visible: usize,
        width: u16,
    ) -> Vec<Line<'static>> {
        labels
            .iter()
            .skip(start)
            .take(visible)
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
                        Self::truncate_label(label, width.saturating_sub(4))
                    ),
                    style,
                ))
            })
            .collect()
    }

    fn truncate_label(label: &str, width: u16) -> String {
        let compact = label.split_whitespace().collect::<Vec<_>>().join(" ");
        let limit = usize::from(width.max(1));
        if compact.chars().count() <= limit {
            compact
        } else {
            let mut truncated = compact
                .chars()
                .take(limit.saturating_sub(3))
                .collect::<String>();
            truncated.push_str("...");
            truncated
        }
    }
}

impl UiSupport {
    pub(in crate::app) fn rounded_block() -> Block<'static> {
        Block::default()
            .borders(Borders::ALL)
            .border_set(border::ROUNDED)
            .border_style(Style::default().fg(Self::theme().border))
            .title_style(
                Style::default()
                    .fg(Self::theme().yellow)
                    .add_modifier(Modifier::BOLD),
            )
    }

    pub(in crate::app) fn theme() -> &'static crate::theme::AppTheme {
        ThemeRegistry::app()
    }

    pub(in crate::app) fn app_background_style() -> Style {
        Style::default()
            .bg(Self::theme().app_bg)
            .fg(Self::theme().foreground)
    }

    pub(in crate::app) fn panel_style() -> Style {
        Style::default()
            .bg(Self::theme().panel_bg)
            .fg(Self::theme().foreground)
    }

    pub(in crate::app) fn sidebar_style() -> Style {
        Style::default()
            .bg(Self::theme().sidebar_bg)
            .fg(Self::theme().foreground)
    }

    pub(in crate::app) fn input_style() -> Style {
        Style::default()
            .bg(Self::theme().input_bg)
            .fg(Self::theme().foreground)
    }

    pub(in crate::app) fn editor_style() -> Style {
        Style::default()
            .bg(Self::theme().app_bg)
            .fg(Self::theme().foreground)
    }

    pub(in crate::app) fn muted_panel_style() -> Style {
        Style::default()
            .bg(Self::theme().panel_bg)
            .fg(Self::theme().muted)
    }

    pub(in crate::app) fn selection_style() -> Style {
        Style::default()
            .fg(Self::theme().selection_fg)
            .bg(Self::theme().selection_bg)
            .add_modifier(Modifier::BOLD)
    }

    pub(in crate::app) fn tab_style() -> Style {
        Style::default()
            .fg(Self::theme().tab_fg)
            .bg(Self::theme().tab_bg)
    }

    pub(in crate::app) fn tab_highlight_style() -> Style {
        Style::default()
            .fg(Self::theme().accent)
            .bg(Self::theme().tab_bg)
            .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
    }

    pub(in crate::app) fn sidebar_block() -> Block<'static> {
        Self::rounded_block().style(Self::sidebar_style())
    }

    pub(in crate::app) fn panel_block() -> Block<'static> {
        Self::rounded_block().style(Self::panel_style())
    }

    pub(in crate::app) fn input_block() -> Block<'static> {
        Self::rounded_block().style(Self::input_style())
    }

    pub(in crate::app) fn editor_block() -> Block<'static> {
        Self::rounded_block().style(Self::editor_style())
    }

    pub(in crate::app) fn focus_block(block: Block<'static>, focused: bool) -> Block<'static> {
        if focused {
            block
                .border_style(Style::default().fg(Self::theme().accent))
                .title_style(
                    Style::default()
                        .fg(Self::theme().accent)
                        .add_modifier(Modifier::BOLD),
                )
        } else {
            block
        }
    }

    pub(in crate::app) fn action_style(color: ratatui::style::Color) -> Style {
        Style::default().bg(Self::theme().panel_bg).fg(color)
    }

    pub(in crate::app) fn rect_contains(rect: Rect, column: u16, row: u16) -> bool {
        column >= rect.x
            && column < rect.x.saturating_add(rect.width)
            && row >= rect.y
            && row < rect.y.saturating_add(rect.height)
    }

    pub(in crate::app) fn inner_rect(rect: Rect) -> Rect {
        let width = rect.width.saturating_sub(2);
        let height = rect.height.saturating_sub(2);
        Rect::new(
            rect.x.saturating_add(1),
            rect.y.saturating_add(1),
            width,
            height,
        )
    }

    pub(in crate::app) fn list_offset(selected: usize, len: usize, visible_rows: usize) -> usize {
        if len <= visible_rows || visible_rows == 0 {
            0
        } else {
            selected.saturating_add(1).saturating_sub(visible_rows)
        }
    }

    pub(in crate::app) fn render_vertical_scrollbar(
        frame: &mut Frame,
        area: Rect,
        content_length: usize,
        scroll: u16,
    ) {
        let Some(metrics) = Self::vertical_scrollbar_metrics(area, content_length) else {
            return;
        };
        Self::render_scrollbar_thumb(frame, metrics, scroll);
    }

    pub(in crate::app) fn render_vertical_scrollbar_with_viewport(
        frame: &mut Frame,
        viewport: Rect,
        content_length: usize,
        scroll: u16,
    ) {
        let Some(metrics) = Self::vertical_scrollbar_metrics_for_viewport(viewport, content_length)
        else {
            return;
        };
        Self::render_scrollbar_thumb(frame, metrics, scroll);
    }

    pub(in crate::app) fn vertical_scrollbar_metrics(
        area: Rect,
        content_length: usize,
    ) -> Option<ScrollbarMetrics> {
        Self::vertical_scrollbar_metrics_for_viewport(Self::inner_rect(area), content_length)
    }

    pub(in crate::app) fn vertical_scrollbar_metrics_for_viewport(
        viewport: Rect,
        content_length: usize,
    ) -> Option<ScrollbarMetrics> {
        if viewport.width == 0 || viewport.height == 0 {
            return None;
        }
        if content_length <= viewport.height as usize {
            return None;
        }

        Some(ScrollbarMetrics {
            track: Rect::new(
                viewport.x.saturating_add(viewport.width.saturating_sub(1)),
                viewport.y,
                1,
                viewport.height,
            ),
            content_length,
            viewport_length: viewport.height as usize,
        })
    }

    pub(in crate::app) fn scroll_position_from_row(metrics: ScrollbarMetrics, row: u16) -> u16 {
        let max_scroll = metrics
            .content_length
            .saturating_sub(metrics.viewport_length)
            .min(u16::MAX as usize);
        if max_scroll == 0 {
            return 0;
        }

        let (_, thumb_height) = Self::scrollbar_thumb_bounds(metrics, 0).unwrap_or((0, 1));
        let track_travel = metrics.track.height.saturating_sub(thumb_height);
        if track_travel == 0 {
            return 0;
        }

        let thumb_half = thumb_height / 2;
        let row_offset = row
            .saturating_sub(metrics.track.y)
            .saturating_sub(thumb_half)
            .min(track_travel) as usize;
        let track_travel = usize::from(track_travel);

        ((row_offset * max_scroll) + track_travel / 2)
            .checked_div(track_travel)
            .unwrap_or(0) as u16
    }

    pub(in crate::app) fn scrollbar_thumb_bounds(
        metrics: ScrollbarMetrics,
        scroll: u16,
    ) -> Option<(u16, u16)> {
        if metrics.track.height == 0 || metrics.content_length == 0 || metrics.viewport_length == 0
        {
            return None;
        }

        let track_height = usize::from(metrics.track.height);
        let proportional_height = ((metrics.viewport_length * track_height)
            + metrics.content_length / 2)
            / metrics.content_length;
        let thumb_height = proportional_height.clamp(1, track_height) as u16;

        let max_scroll = metrics
            .content_length
            .saturating_sub(metrics.viewport_length)
            .min(u16::MAX as usize) as u16;
        let track_travel = metrics.track.height.saturating_sub(thumb_height);
        let thumb_top = if max_scroll == 0 || track_travel == 0 {
            0
        } else {
            let scroll = scroll.min(max_scroll) as usize;
            let track_travel = usize::from(track_travel);
            (((scroll * track_travel) + usize::from(max_scroll) / 2) / usize::from(max_scroll))
                as u16
        };

        Some((thumb_top, thumb_height))
    }

    fn render_scrollbar_thumb(frame: &mut Frame, metrics: ScrollbarMetrics, scroll: u16) {
        let Some((thumb_top, thumb_height)) = Self::scrollbar_thumb_bounds(metrics, scroll) else {
            return;
        };

        let lines = (0..metrics.track.height)
            .map(|row| {
                if row >= thumb_top && row < thumb_top.saturating_add(thumb_height) {
                    Line::from(Span::styled(
                        "█",
                        Style::default().fg(Self::theme().scrollbar_thumb),
                    ))
                } else {
                    Line::from(" ")
                }
            })
            .collect::<Vec<_>>();

        frame.render_widget(Paragraph::new(Text::from(lines)), metrics.track);
    }

    pub(in crate::app) fn preview_content_height(lines: &[Line<'_>], width: u16) -> usize {
        let width = usize::from(width.max(1));

        lines
            .iter()
            .map(|line| match line.width() {
                0 => 1,
                line_width => line_width.saturating_sub(1) / width + 1,
            })
            .sum()
    }

    pub(in crate::app) fn wrapped_text_height(text: &Text<'_>, width: u16) -> usize {
        Paragraph::new(text.clone())
            .wrap(Wrap { trim: false })
            .line_count(width.max(1))
    }

    pub(in crate::app) fn scrollable_preview_content_height(
        lines: &[Line<'_>],
        area: Rect,
    ) -> usize {
        let viewport = Self::inner_rect(area);
        let base_height = Self::preview_content_height(lines, viewport.width);
        if base_height > viewport.height as usize && viewport.width > 1 {
            Self::preview_content_height(lines, viewport.width.saturating_sub(1))
        } else {
            base_height
        }
    }

    #[cfg(test)]
    pub(in crate::app) fn scrollable_text_height(text: &Text<'_>, area: Rect) -> usize {
        let viewport = Self::inner_rect(area);
        let base_height = Self::wrapped_text_height(text, viewport.width);
        if base_height > viewport.height as usize && viewport.width > 1 {
            Self::wrapped_text_height(text, viewport.width.saturating_sub(1))
        } else {
            base_height
        }
    }
}
