use super::super::*;
use super::UiSupport;

pub(in crate::app) struct AddAgentDialogComponent;

impl AddAgentDialogComponent {
    pub(in crate::app) fn draw(frame: &mut Frame, app: &App, area: Rect) {
        frame.render_widget(Clear, area);
        let outer = UiSupport::panel_block().title("New Agent");
        let inner = outer.inner(area);
        frame.render_widget(outer, area);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Min(3),
            ])
            .margin(1)
            .split(inner);

        let help = Paragraph::new("Use Tab to switch fields and Enter to save the agent.")
            .style(UiSupport::muted_panel_style());
        frame.render_widget(help, chunks[0]);

        let name_title = if app.add_form.active_field == AddAgentField::Name {
            "Name *"
        } else {
            "Name"
        };
        let workspace_title = if app.add_form.active_field == AddAgentField::Workspace {
            "Workspace *"
        } else {
            "Workspace"
        };

        let name = Paragraph::new(app.add_form.name.as_str())
            .block(UiSupport::input_block().title(name_title))
            .style(UiSupport::input_style());
        let workspace = Paragraph::new(app.add_form.workspace.as_str())
            .block(UiSupport::input_block().title(workspace_title))
            .style(UiSupport::input_style());
        frame.render_widget(name, chunks[1]);
        frame.render_widget(workspace, chunks[2]);

        let status = app
            .add_form
            .error
            .clone()
            .unwrap_or_else(|| "Saved agents live in ~/.cmdex.yml".to_string());
        let status =
            Paragraph::new(status).style(Style::default().bg(UiSupport::theme().panel_bg).fg(
                if app.add_form.error.is_some() {
                    UiSupport::theme().error
                } else {
                    UiSupport::theme().muted
                },
            ));
        frame.render_widget(status, chunks[3]);

        let target = match app.add_form.active_field {
            AddAgentField::Name => chunks[1],
            AddAgentField::Workspace => chunks[2],
        };
        let content = match app.add_form.active_field {
            AddAgentField::Name => &app.add_form.name,
            AddAgentField::Workspace => &app.add_form.workspace,
        };
        let cursor_x = target
            .x
            .saturating_add(1 + content.chars().count() as u16)
            .min(target.x + target.width.saturating_sub(2));
        frame.set_cursor_position((cursor_x, target.y + 1));
    }

    pub(in crate::app) fn submit(
        app: &mut App,
        codex: CodexAppServer,
        ui_tx: mpsc::UnboundedSender<UiEvent>,
    ) {
        match ConfigStore::validate_agent_input(&app.add_form.name, &app.add_form.workspace) {
            Ok(agent) => {
                app.config.agents.push(agent.clone());
                if let Err(error) = ConfigStore::save(&app.config_path, &app.config) {
                    app.add_form.error = Some(error.to_string());
                    return;
                }

                app.agents.push(AgentState::new(
                    agent,
                    app.default_chat_model.clone(),
                    app.default_chat_reasoning_effort.clone(),
                    &app.chat_model_label,
                ));
                let new_index = app.agents.len().saturating_sub(1);
                app.current_agent = Some(new_index);
                app.chat_sidebar_index = new_index + 1;
                app.add_form = AddAgentForm::default();
                app.status_message = Some("Agent saved to ~/.cmdex.yml".to_string());
                SessionLoader::spawn(
                    codex,
                    ui_tx,
                    new_index,
                    app.agents[new_index].definition.workspace.clone(),
                );
            }
            Err(error) => {
                app.add_form.error = Some(error.to_string());
            }
        }
    }
}
