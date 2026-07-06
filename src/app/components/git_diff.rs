use super::super::*;
use super::UiSupport;
use crate::workspace::GitDiffSupport;
use tokio::task;

pub(in crate::app) struct GitDiffComponent;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::app) struct GitDiffLayout {
    pub(in crate::app) sections: Rect,
    pub(in crate::app) preview: Rect,
    pub(in crate::app) commit_input: Rect,
    pub(in crate::app) stage_button: Rect,
    pub(in crate::app) discard_button: Rect,
    pub(in crate::app) push_button: Rect,
    pub(in crate::app) pull_button: Rect,
    pub(in crate::app) status: Rect,
}

impl GitDiffComponent {
    pub(in crate::app) fn draw(frame: &mut Frame, app: &App, area: Rect) {
        let Some(agent) = app.active_agent() else {
            let empty = Paragraph::new("Select or create an agent in the Chat tab.")
                .block(UiSupport::panel_block().title("Git Diff"))
                .style(UiSupport::panel_style());
            frame.render_widget(empty, area);
            return;
        };

        let layout = Self::layout(area);
        let tabs = Tabs::new([
            format!("Changes ({})", agent.git_diff.count(DiffSection::Changes)),
            format!("Staged ({})", agent.git_diff.count(DiffSection::Staged)),
        ])
        .select(match agent.git_diff.active_section {
            DiffSection::Changes => 0,
            DiffSection::Staged => 1,
        })
        .block(
            UiSupport::rounded_block()
                .style(UiSupport::tab_style())
                .title(format!(
                    "Files · {}",
                    ConfigStore::compact_home(&agent.definition.workspace)
                )),
        )
        .style(UiSupport::tab_style())
        .highlight_style(UiSupport::tab_highlight_style());
        frame.render_widget(tabs, layout.sections);

        let preview_lines = agent.git_diff.preview.clone();
        let content_length =
            UiSupport::scrollable_preview_content_height(&preview_lines, layout.preview);
        let viewport = UiSupport::inner_rect(layout.preview);
        let render_width = if content_length > viewport.height as usize && viewport.width > 1 {
            viewport.width.saturating_sub(1)
        } else {
            viewport.width
        };
        let preview_lines = Self::pad_preview_lines(&preview_lines, render_width);
        let widget = Paragraph::new(Text::from(preview_lines))
            .block(UiSupport::editor_block().title(agent.git_diff.preview_title.clone()))
            .style(UiSupport::editor_style())
            .scroll((agent.git_diff.content_scroll, 0))
            .wrap(Wrap { trim: false });
        frame.render_widget(widget, layout.preview);
        UiSupport::render_vertical_scrollbar(
            frame,
            layout.preview,
            content_length,
            agent.git_diff.content_scroll,
        );

        let commit_title = "Commit Message · Enter commits staged changes";
        let commit_text = Self::commit_input_text(
            &agent.git_diff.commit_message,
            layout.commit_input.width.saturating_sub(2),
        );
        let commit_input = Paragraph::new(commit_text.as_str())
            .block(UiSupport::panel_block().title(commit_title))
            .style(UiSupport::panel_style());
        frame.render_widget(commit_input, layout.commit_input);

        let stage_label = match agent.git_diff.active_section {
            DiffSection::Changes => "Stage",
            DiffSection::Staged => "Unstage",
        };
        let stage_button = Paragraph::new(stage_label)
            .alignment(Alignment::Center)
            .style(UiSupport::action_style(UiSupport::theme().foreground))
            .block(UiSupport::panel_block());
        frame.render_widget(stage_button, layout.stage_button);

        let discard_style = match agent.git_diff.active_section {
            DiffSection::Changes => UiSupport::action_style(UiSupport::theme().error),
            DiffSection::Staged => UiSupport::action_style(UiSupport::theme().muted),
        };
        let discard_button = Paragraph::new("Discard")
            .alignment(Alignment::Center)
            .style(discard_style)
            .block(UiSupport::panel_block());
        frame.render_widget(discard_button, layout.discard_button);

        let push_button = Paragraph::new(Self::remote_button_label(
            "Push",
            agent.git_diff.remote_action,
            GitRemoteAction::Push,
            app.spinner_index,
        ))
        .alignment(Alignment::Center)
        .style(UiSupport::action_style(UiSupport::theme().accent))
        .block(UiSupport::panel_block());
        frame.render_widget(push_button, layout.push_button);

        let pull_button = Paragraph::new(Self::remote_button_label(
            "Pull",
            agent.git_diff.remote_action,
            GitRemoteAction::Pull,
            app.spinner_index,
        ))
        .alignment(Alignment::Center)
        .style(UiSupport::action_style(UiSupport::theme().foreground))
        .block(UiSupport::panel_block());
        frame.render_widget(pull_button, layout.pull_button);

        let status = if let Some(error) = &agent.git_diff.error {
            Paragraph::new(error.as_str()).style(
                Style::default()
                    .bg(UiSupport::theme().panel_bg)
                    .fg(UiSupport::theme().error),
            )
        } else if let Some(status) = &agent.git_diff.status {
            Paragraph::new(status.as_str()).style(
                Style::default()
                    .bg(UiSupport::theme().panel_bg)
                    .fg(UiSupport::theme().success),
            )
        } else {
            Paragraph::new(
                "Tab/Left/Right switches sections. Ctrl+S stages, Ctrl+U unstages, Ctrl+D discards changes, Enter commits staged changes.",
            )
            .style(UiSupport::muted_panel_style())
        };
        frame.render_widget(status, layout.status);

        let cursor_x = layout
            .commit_input
            .x
            .saturating_add(1 + commit_text.chars().count() as u16)
            .min(layout.commit_input.x + layout.commit_input.width.saturating_sub(2));
        let cursor_y = layout.commit_input.y.saturating_add(1);
        frame.set_cursor_position((cursor_x, cursor_y));
    }

    pub(in crate::app) fn layout(area: Rect) -> GitDiffLayout {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(8),
                Constraint::Length(5),
            ])
            .split(area);
        let controls = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(1)])
            .split(chunks[2]);
        let controls_row = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Min(18),
                Constraint::Length(12),
                Constraint::Length(12),
                Constraint::Length(12),
                Constraint::Length(12),
            ])
            .split(controls[0]);

        GitDiffLayout {
            sections: chunks[0],
            preview: chunks[1],
            commit_input: controls_row[0],
            stage_button: controls_row[1],
            discard_button: controls_row[2],
            push_button: controls_row[3],
            pull_button: controls_row[4],
            status: controls[1],
        }
    }

    pub(in crate::app) fn handle_key(
        app: &mut App,
        key: KeyEvent,
        _ui_tx: &mpsc::UnboundedSender<UiEvent>,
    ) -> bool {
        if app.current_tab != AppTab::GitDiff {
            return false;
        }

        match key.code {
            KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                Self::stage_changes(app);
                true
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                Self::unstage_changes(app);
                true
            }
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                Self::discard_changes(app);
                true
            }
            KeyCode::Left if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                if let Some(agent) = app.active_agent_mut() {
                    let root = agent.definition.workspace.clone();
                    agent
                        .git_diff
                        .set_active_section(&root, DiffSection::Changes);
                }
                true
            }
            KeyCode::Right if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                if let Some(agent) = app.active_agent_mut() {
                    let root = agent.definition.workspace.clone();
                    agent
                        .git_diff
                        .set_active_section(&root, DiffSection::Staged);
                }
                true
            }
            KeyCode::Tab => {
                if let Some(agent) = app.active_agent_mut() {
                    let root = agent.definition.workspace.clone();
                    let next = match agent.git_diff.active_section {
                        DiffSection::Changes => DiffSection::Staged,
                        DiffSection::Staged => DiffSection::Changes,
                    };
                    agent.git_diff.set_active_section(&root, next);
                }
                true
            }
            KeyCode::Enter => {
                Self::commit_changes(app);
                true
            }
            _ => false,
        }
    }

    pub(in crate::app) fn handle_click(
        app: &mut App,
        column: u16,
        row: u16,
        area: Rect,
        ui_tx: &mpsc::UnboundedSender<UiEvent>,
    ) -> bool {
        let layout = Self::layout(area);
        let Some(agent) = app.active_agent() else {
            return false;
        };

        let changes_label = format!("Changes ({})", agent.git_diff.count(DiffSection::Changes));
        let staged_label = format!("Staged ({})", agent.git_diff.count(DiffSection::Staged));
        let active_section = agent.git_diff.active_section;
        if let Some(section) =
            Self::section_from_click(layout.sections, &changes_label, &staged_label, column, row)
        {
            if let Some(agent) = app.active_agent_mut() {
                let root = agent.definition.workspace.clone();
                agent.git_diff.set_active_section(&root, section);
            }
            return true;
        }

        if UiSupport::rect_contains(layout.push_button, column, row) {
            Self::push_changes(app, ui_tx);
            return true;
        }

        if UiSupport::rect_contains(layout.stage_button, column, row) {
            match active_section {
                DiffSection::Changes => Self::stage_changes(app),
                DiffSection::Staged => Self::unstage_changes(app),
            }
            return true;
        }

        if UiSupport::rect_contains(layout.discard_button, column, row) {
            Self::discard_changes(app);
            return true;
        }

        if UiSupport::rect_contains(layout.pull_button, column, row) {
            Self::pull_changes(app, ui_tx);
            return true;
        }

        UiSupport::rect_contains(layout.commit_input, column, row)
            || UiSupport::rect_contains(layout.preview, column, row)
    }

    pub(in crate::app) fn section_from_click(
        area: Rect,
        changes_label: &str,
        staged_label: &str,
        column: u16,
        row: u16,
    ) -> Option<DiffSection> {
        let inner = UiSupport::inner_rect(area);
        if inner.width == 0 || inner.height == 0 {
            return None;
        }

        let changes_width = changes_label.chars().count() as u16;
        let staged_width = staged_label.chars().count() as u16;
        let changes = Rect::new(inner.x, inner.y, changes_width.min(inner.width), 1);
        let staged_x = inner.x.saturating_add(changes_width).saturating_add(3);
        let staged = Rect::new(
            staged_x,
            inner.y,
            staged_width.min(inner.x.saturating_add(inner.width).saturating_sub(staged_x)),
            1,
        );

        if UiSupport::rect_contains(changes, column, row) {
            Some(DiffSection::Changes)
        } else if UiSupport::rect_contains(staged, column, row) {
            Some(DiffSection::Staged)
        } else {
            None
        }
    }

    pub(in crate::app) fn remote_button_label(
        label: &str,
        active_action: Option<GitRemoteAction>,
        button_action: GitRemoteAction,
        spinner_index: usize,
    ) -> String {
        if active_action == Some(button_action) {
            format!("{} {}", SPINNER[spinner_index % SPINNER.len()], label)
        } else {
            label.to_string()
        }
    }

    pub(in crate::app) fn pad_preview_lines(
        lines: &[Line<'static>],
        width: u16,
    ) -> Vec<Line<'static>> {
        let width = usize::from(width.max(1));

        lines.iter()
            .cloned()
            .map(|mut line| {
                let Some(background) = line.style.bg else {
                    return line;
                };

                let remainder = line.width() % width;
                let padding = if remainder == 0 {
                    if line.width() == 0 { width } else { 0 }
                } else {
                    width - remainder
                };

                if padding > 0 {
                    line.spans.push(Span::styled(
                        " ".repeat(padding),
                        Style::default().bg(background),
                    ));
                }

                line
            })
            .collect()
    }

    fn commit_changes(app: &mut App) {
        let Some(agent) = app.active_agent_mut() else {
            app.status_message = Some("Add an agent before committing changes.".to_string());
            return;
        };
        if agent.git_diff.remote_action.is_some() {
            agent.git_diff.error = Some("Wait for git push/pull to finish.".to_string());
            return;
        }

        let root = agent.definition.workspace.clone();
        match agent.git_diff.commit(&root) {
            Ok(()) => agent.workspace.refresh(&root),
            Err(error) => agent.git_diff.error = Some(error.to_string()),
        }
    }

    fn stage_changes(app: &mut App) {
        let Some(agent) = app.active_agent_mut() else {
            app.status_message = Some("Add an agent before staging changes.".to_string());
            return;
        };
        if agent.git_diff.remote_action.is_some() {
            agent.git_diff.error = Some("Wait for git push/pull to finish.".to_string());
            return;
        }

        let root = agent.definition.workspace.clone();
        match agent.git_diff.stage_selected(&root) {
            Ok(()) => agent.workspace.refresh(&root),
            Err(error) => agent.git_diff.error = Some(error.to_string()),
        }
    }

    fn unstage_changes(app: &mut App) {
        let Some(agent) = app.active_agent_mut() else {
            app.status_message = Some("Add an agent before unstaging changes.".to_string());
            return;
        };
        if agent.git_diff.remote_action.is_some() {
            agent.git_diff.error = Some("Wait for git push/pull to finish.".to_string());
            return;
        }

        let root = agent.definition.workspace.clone();
        match agent.git_diff.unstage_selected(&root) {
            Ok(()) => agent.workspace.refresh(&root),
            Err(error) => agent.git_diff.error = Some(error.to_string()),
        }
    }

    fn discard_changes(app: &mut App) {
        let Some(agent) = app.active_agent_mut() else {
            app.status_message = Some("Add an agent before discarding changes.".to_string());
            return;
        };
        if agent.git_diff.remote_action.is_some() {
            agent.git_diff.error = Some("Wait for git push/pull to finish.".to_string());
            return;
        }

        let root = agent.definition.workspace.clone();
        match agent.git_diff.discard_selected(&root) {
            Ok(()) => agent.workspace.refresh(&root),
            Err(error) => agent.git_diff.error = Some(error.to_string()),
        }
    }

    fn push_changes(app: &mut App, ui_tx: &mpsc::UnboundedSender<UiEvent>) {
        let Some(agent_index) = app.current_agent else {
            app.status_message = Some("Add an agent before pushing changes.".to_string());
            return;
        };

        let root = app.agents[agent_index].definition.workspace.clone();
        match app.agents[agent_index]
            .git_diff
            .begin_remote_action(GitRemoteAction::Push)
        {
            Ok(()) => {}
            Err(error) => {
                app.agents[agent_index].git_diff.error = Some(error.to_string());
                return;
            }
        }

        let ui_tx = ui_tx.clone();
        task::spawn(async move {
            let result = task::spawn_blocking(move || {
                GitDiffSupport::run_remote_action(&root, GitRemoteAction::Push)
            })
            .await;
            let (success, message) = match result {
                Ok(Ok(output)) => (true, output),
                Ok(Err(error)) => (false, error.to_string()),
                Err(error) => (false, format!("git push task failed: {error}")),
            };
            let _ = ui_tx.send(UiEvent::GitDiffRemoteCompleted {
                agent_index,
                action: GitRemoteAction::Push,
                success,
                message,
            });
        });
    }

    fn pull_changes(app: &mut App, ui_tx: &mpsc::UnboundedSender<UiEvent>) {
        let Some(agent_index) = app.current_agent else {
            app.status_message = Some("Add an agent before pulling changes.".to_string());
            return;
        };

        let root = app.agents[agent_index].definition.workspace.clone();
        match app.agents[agent_index]
            .git_diff
            .begin_remote_action(GitRemoteAction::Pull)
        {
            Ok(()) => {}
            Err(error) => {
                app.agents[agent_index].git_diff.error = Some(error.to_string());
                return;
            }
        }

        let ui_tx = ui_tx.clone();
        task::spawn(async move {
            let result = task::spawn_blocking(move || {
                GitDiffSupport::run_remote_action(&root, GitRemoteAction::Pull)
            })
            .await;
            let (success, message) = match result {
                Ok(Ok(output)) => (true, output),
                Ok(Err(error)) => (false, error.to_string()),
                Err(error) => (false, format!("git pull task failed: {error}")),
            };
            let _ = ui_tx.send(UiEvent::GitDiffRemoteCompleted {
                agent_index,
                action: GitRemoteAction::Pull,
                success,
                message,
            });
        });
    }

    fn commit_input_text(input: &str, max_width: u16) -> String {
        let max_width = usize::from(max_width.max(1));
        let chars = input.chars().collect::<Vec<_>>();
        if chars.len() <= max_width {
            input.to_string()
        } else {
            chars[chars.len().saturating_sub(max_width)..]
                .iter()
                .collect()
        }
    }
}
