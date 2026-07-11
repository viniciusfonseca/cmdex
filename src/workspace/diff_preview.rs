use super::*;
use ratatui::style::Color;

impl GitDiffSupport {
    pub(crate) fn read_diff_preview(
        root: &Path,
        entry: &DiffEntry,
        section: DiffSection,
    ) -> Result<Vec<Line<'static>>> {
        let snapshot = Self::read_diff_snapshot(root, entry, section)?;
        Ok(Self::render_diff_snapshot_preview(
            entry.path.as_path(),
            &snapshot,
        ))
    }

    fn read_diff_snapshot(
        root: &Path,
        entry: &DiffEntry,
        section: DiffSection,
    ) -> Result<DiffSnapshot> {
        if entry.status.contains('?') {
            let bytes = fs::read(&entry.path)
                .with_context(|| format!("failed to read {}", entry.path.display()))?;
            return Self::snapshot_from_bytes(bytes, Some(DiffPreviewKind::Added));
        }

        let repository = super::super::git_repository::GitRepository::new(root);
        let relative = repository.relative_path(&entry.path)?;
        let diff = match section {
            DiffSection::Changes => repository.output(&[
                "diff",
                "--no-ext-diff",
                "--no-color",
                "--unified=0",
                "--",
                &relative,
            ])?,
            DiffSection::Staged => repository.output(&[
                "diff",
                "--no-ext-diff",
                "--no-color",
                "--cached",
                "--unified=0",
                "--",
                &relative,
            ])?,
        };

        let force_removed = entry.status.contains('D');
        let default_kind = if force_removed {
            Some(DiffPreviewKind::Removed)
        } else {
            None
        };

        let bytes = match section {
            DiffSection::Changes => {
                if force_removed {
                    repository.output_bytes(&["show", &format!(":{relative}")])?
                } else {
                    fs::read(&entry.path)
                        .with_context(|| format!("failed to read {}", entry.path.display()))?
                }
            }
            DiffSection::Staged => {
                if force_removed {
                    repository.output_bytes(&["show", &format!("HEAD:{relative}")])?
                } else {
                    repository.output_bytes(&["show", &format!(":{relative}")])?
                }
            }
        };

        let mut snapshot = Self::snapshot_from_bytes(bytes, default_kind)?;
        snapshot.hunks = Self::parse_unified_diff_hunks_impl(&diff);

        if snapshot.default_kind.is_none() && snapshot.hunks.is_empty() {
            snapshot.message = Some("No diff available for this file.".to_string());
        }

        Ok(snapshot)
    }

    fn snapshot_from_bytes(
        bytes: Vec<u8>,
        default_kind: Option<DiffPreviewKind>,
    ) -> Result<DiffSnapshot> {
        if bytes.contains(&0) {
            return Ok(DiffSnapshot {
                source: String::new(),
                default_kind,
                hunks: Vec::new(),
                message: Some("Binary file preview is not available.".to_string()),
            });
        }

        let truncated = bytes.len() > PREVIEW_LIMIT;
        let source = String::from_utf8_lossy(&bytes[..bytes.len().min(PREVIEW_LIMIT)]).into_owned();
        let mut snapshot = DiffSnapshot {
            source: WorkspaceRenderer::normalize_newlines(&source),
            default_kind,
            hunks: Vec::new(),
            message: None,
        };
        if truncated {
            snapshot.message = Some("[truncated]".to_string());
        }
        Ok(snapshot)
    }

    fn render_diff_snapshot_preview(path: &Path, snapshot: &DiffSnapshot) -> Vec<Line<'static>> {
        if snapshot.source.is_empty() && snapshot.message.is_some() {
            return WorkspaceRenderer::plain_preview_lines(
                snapshot.message.as_deref().unwrap_or_default(),
            );
        }

        let mut lines = Self::render_diff_preview_rows(Self::build_diff_preview_rows(
            path,
            &snapshot.source,
            snapshot.default_kind,
            &snapshot.hunks,
        ));

        if let Some(message) = &snapshot.message {
            lines.push(Line::default());
            lines.push(Line::from(Span::styled(
                message.clone(),
                Style::default()
                    .fg(ThemeRegistry::app().muted)
                    .add_modifier(Modifier::ITALIC),
            )));
        }

        lines
    }

    #[cfg(test)]
    pub(in crate::workspace) fn render_modified_diff_preview(
        path: &Path,
        source: &str,
        hunks: &[DiffHunk],
    ) -> Vec<Line<'static>> {
        Self::render_diff_preview_rows(Self::build_diff_preview_rows(path, source, None, hunks))
    }

    fn build_diff_preview_rows(
        path: &Path,
        source: &str,
        default_kind: Option<DiffPreviewKind>,
        hunks: &[DiffHunk],
    ) -> Vec<DiffPreviewRow> {
        let mut highlighted = WorkspaceRenderer::highlighted_preview_lines(
            source,
            WorkspaceRenderer::syntax_for_path(path, source),
        );
        WorkspaceRenderer::maybe_trim_trailing_empty_line(&mut highlighted);

        let mut rows = highlighted
            .into_iter()
            .enumerate()
            .map(|(index, line)| DiffPreviewRow {
                line_number: Some(index + 1),
                kind: default_kind.unwrap_or_else(|| Self::diff_kind_for_line(index + 1, hunks)),
                line,
            })
            .collect::<Vec<_>>();

        if default_kind.is_none() {
            Self::insert_removed_diff_rows(&mut rows, hunks);
        }

        rows
    }

    fn diff_kind_for_line(line_number: usize, hunks: &[DiffHunk]) -> DiffPreviewKind {
        for hunk in hunks {
            if hunk.new_count == 0 {
                continue;
            }

            let start = hunk.new_start.max(1);
            let end = start + hunk.new_count.saturating_sub(1);
            if (start..=end).contains(&line_number) {
                return if hunk.old_count == 0 {
                    DiffPreviewKind::Added
                } else {
                    DiffPreviewKind::Modified
                };
            }
        }

        DiffPreviewKind::Context
    }

    fn insert_removed_diff_rows(rows: &mut Vec<DiffPreviewRow>, hunks: &[DiffHunk]) {
        let mut inserted_rows = 0usize;

        for hunk in hunks {
            if hunk.removed_lines.is_empty() {
                continue;
            }

            let insert_at = hunk
                .new_start
                .saturating_sub(1)
                .saturating_add(inserted_rows)
                .min(rows.len());

            let removed_rows = hunk
                .removed_lines
                .iter()
                .enumerate()
                .map(|(offset, line)| DiffPreviewRow {
                    line_number: Some(hunk.old_start + offset),
                    kind: DiffPreviewKind::Removed,
                    line: Line::from(line.clone()),
                })
                .collect::<Vec<_>>();

            inserted_rows += removed_rows.len();
            rows.splice(insert_at..insert_at, removed_rows);
        }
    }

    fn render_diff_preview_rows(rows: Vec<DiffPreviewRow>) -> Vec<Line<'static>> {
        let gutter_width = rows
            .iter()
            .filter_map(|row| row.line_number)
            .max()
            .unwrap_or(1)
            .to_string()
            .len();

        rows.into_iter()
            .map(|row| {
                let marker = match row.kind {
                    DiffPreviewKind::Context => ' ',
                    DiffPreviewKind::Added => '+',
                    DiffPreviewKind::Modified => '~',
                    DiffPreviewKind::Removed => '-',
                };
                let number = row
                    .line_number
                    .map(|value| format!("{value:>width$}", width = gutter_width))
                    .unwrap_or_else(|| " ".repeat(gutter_width));

                let mut line = row.line;
                let mut spans = Vec::with_capacity(line.spans.len() + 1);
                spans.push(Span::styled(
                    format!("{number} {marker} | "),
                    Style::default().fg(Self::diff_gutter_color(row.kind)),
                ));
                spans.append(&mut line.spans);
                line.spans = spans;
                Self::apply_diff_preview_kind(&mut line, row.kind);
                line
            })
            .collect()
    }

    fn diff_gutter_color(kind: DiffPreviewKind) -> Color {
        match kind {
            DiffPreviewKind::Context => ThemeRegistry::app().line_number,
            DiffPreviewKind::Added => ThemeRegistry::app().success,
            DiffPreviewKind::Modified => ThemeRegistry::app().warning,
            DiffPreviewKind::Removed => ThemeRegistry::app().error,
        }
    }

    fn apply_diff_preview_kind(line: &mut Line<'static>, kind: DiffPreviewKind) {
        let Some(background) = Self::diff_preview_background(kind) else {
            return;
        };

        line.style = line.style.bg(background);
        for span in &mut line.spans {
            span.style = span.style.bg(background);
        }
    }

    fn diff_preview_background(kind: DiffPreviewKind) -> Option<Color> {
        match kind {
            DiffPreviewKind::Context => None,
            DiffPreviewKind::Added => Some(Self::blend_tui_colors(
                ThemeRegistry::app().panel_bg,
                ThemeRegistry::app().success,
                0.22,
            )),
            DiffPreviewKind::Modified => Some(Self::blend_tui_colors(
                ThemeRegistry::app().panel_bg,
                ThemeRegistry::app().warning,
                0.18,
            )),
            DiffPreviewKind::Removed => Some(Self::blend_tui_colors(
                ThemeRegistry::app().panel_bg,
                ThemeRegistry::app().error,
                0.22,
            )),
        }
    }

    fn blend_tui_colors(base: Color, overlay: Color, alpha: f32) -> Color {
        let (Color::Rgb(base_r, base_g, base_b), Color::Rgb(overlay_r, overlay_g, overlay_b)) =
            (base, overlay)
        else {
            return overlay;
        };

        let blend = |background: u8, foreground: u8| -> u8 {
            ((background as f32 * (1.0 - alpha)) + (foreground as f32 * alpha)).round() as u8
        };

        Color::Rgb(
            blend(base_r, overlay_r),
            blend(base_g, overlay_g),
            blend(base_b, overlay_b),
        )
    }

    pub(super) fn parse_unified_diff_hunks_impl(diff: &str) -> Vec<DiffHunk> {
        let mut hunks = Vec::new();
        let mut current: Option<DiffHunk> = None;

        for line in WorkspaceRenderer::normalize_newlines(diff).lines() {
            if line.starts_with("@@") {
                if let Some(hunk) = current.take() {
                    hunks.push(hunk);
                }
                current = Self::parse_diff_hunk_header(line).map(
                    |(old_start, old_count, new_start, new_count)| DiffHunk {
                        old_start,
                        old_count,
                        new_start,
                        new_count,
                        removed_lines: Vec::new(),
                    },
                );
                continue;
            }

            if let Some(hunk) = current.as_mut()
                && let Some(removed) = line.strip_prefix('-')
            {
                hunk.removed_lines.push(removed.to_string());
            }
        }

        if let Some(hunk) = current {
            hunks.push(hunk);
        }

        hunks
    }

    fn parse_diff_hunk_header(header: &str) -> Option<(usize, usize, usize, usize)> {
        let mut parts = header.split_whitespace();
        let _ = parts.next()?;
        let old_range = parts.next()?;
        let new_range = parts.next()?;
        let (old_start, old_count) = Self::parse_diff_range(old_range, '-')?;
        let (new_start, new_count) = Self::parse_diff_range(new_range, '+')?;
        Some((old_start, old_count, new_start, new_count))
    }

    fn parse_diff_range(value: &str, prefix: char) -> Option<(usize, usize)> {
        let value = value.strip_prefix(prefix)?;
        let (start, count) = match value.split_once(',') {
            Some((start, count)) => (start, count.parse().ok()?),
            None => (value, 1),
        };
        Some((start.parse().ok()?, count))
    }
}
