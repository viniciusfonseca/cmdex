use super::{
    App, AppExit, AppInput, AppRuntime, ChatComponent, CodexAppServer, ConfigStore, Rect, Result,
    UiEvent, effects, mpsc, sleep, terminal_size, ui,
};
use crossterm::event::EventStream;
use futures_util::StreamExt;
use ratatui::DefaultTerminal;

impl AppRuntime {
    pub async fn run(terminal: &mut DefaultTerminal) -> Result<AppExit> {
        let config_path = ConfigStore::default_path()?;
        let config = ConfigStore::load(&config_path)?;

        let (server_tx, mut server_rx) = mpsc::unbounded_channel();
        let (ui_tx, mut ui_rx) = mpsc::unbounded_channel();
        let codex = CodexAppServer::spawn(server_tx).await?;
        let mut app = App::new(config_path, config);
        super::SessionLoader::hydrate_latest_sessions(&mut app, &codex).await?;

        let mut events = EventStream::new();
        super::components::TopNavigationComponent::refresh_current_tab(&mut app);
        let mut needs_redraw = true;

        let exit = loop {
            if needs_redraw {
                terminal.draw(|frame| ui::AppUi::draw(frame, &app))?;
            }

            let tick = sleep(app.tick_interval());
            tokio::pin!(tick);

            tokio::select! {
                maybe_event = events.next() => {
                    match maybe_event {
                        Some(Ok(event)) => {
                            if let Some(input) = AppInput::from_terminal_event(event) {
                                let (width, height) = terminal_size()?;
                                let outcome = app.handle_input(
                                    input,
                                    &codex,
                                    &ui_tx,
                                    Rect::new(0, 0, width, height),
                                );
                                if let Some(exit) = outcome.exit() {
                                    break exit;
                                }
                                needs_redraw = outcome.needs_redraw();
                                Self::spawn_pending_effects(&mut app, &codex, &ui_tx);
                                ChatComponent::maybe_dispatch_queued_messages(&mut app);
                            } else {
                                needs_redraw = true;
                            }
                        }
                        Some(Err(error)) => {
                            app.status_message = Some(error.to_string());
                            needs_redraw = true;
                        }
                        None => break AppExit::Quit,
                    }
                }
                Some(server_event) = server_rx.recv() => {
                    app.handle_server_event(server_event);
                    ChatComponent::maybe_dispatch_queued_messages(&mut app);
                    Self::spawn_pending_effects(&mut app, &codex, &ui_tx);
                    needs_redraw = true;
                }
                Some(ui_event) = ui_rx.recv() => {
                    app.handle_ui_event(ui_event);
                    ChatComponent::maybe_dispatch_queued_messages(&mut app);
                    Self::spawn_pending_effects(&mut app, &codex, &ui_tx);
                    needs_redraw = true;
                }
                _ = &mut tick => {
                    needs_redraw = app
                        .handle_input(AppInput::Tick, &codex, &ui_tx, Rect::default())
                        .needs_redraw();
                    Self::spawn_pending_effects(&mut app, &codex, &ui_tx);
                }
            }
        };

        app.shutdown_lsp_sessions();
        app.shutdown_shell_sessions();

        Ok(exit)
    }

    fn spawn_pending_effects(
        app: &mut App,
        codex: &CodexAppServer,
        ui_tx: &mpsc::UnboundedSender<UiEvent>,
    ) {
        super::components::WorkspaceComponent::maybe_search(app);
        for effect in app.take_effects() {
            effects::spawn(effect, codex.clone(), ui_tx.clone());
        }
    }
}
