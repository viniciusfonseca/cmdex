use std::{path::PathBuf, sync::mpsc as std_mpsc, thread};

use anyhow::{Context, Result};
use notify::{EventKind, RecursiveMode, Watcher};
use tokio::sync::mpsc;

use super::{
    UiEvent,
    event_types::{self, WorkspaceEvent},
};

pub(super) struct WorkspaceWatcher;

impl WorkspaceWatcher {
    pub(super) fn spawn(
        agent_index: usize,
        root: PathBuf,
        ui_tx: mpsc::UnboundedSender<UiEvent>,
    ) -> Result<()> {
        let (stop_tx, stop_rx) = std_mpsc::channel();
        let callback_tx = ui_tx.clone();
        thread::Builder::new()
            .name(format!("cmdex-workspace-watcher-{agent_index}"))
            .spawn(move || {
                let mut watcher = match notify::recommended_watcher(
                    move |result: notify::Result<notify::Event>| match result {
                        Ok(event) if Self::is_relevant(&event) => event_types::send(
                            &callback_tx,
                            WorkspaceEvent::FilesystemChanged { agent_index },
                        ),
                        Ok(_) => {}
                        Err(error) => event_types::send(
                            &callback_tx,
                            WorkspaceEvent::WatcherError {
                                agent_index,
                                message: format!("workspace watcher failed: {error}"),
                            },
                        ),
                    },
                ) {
                    Ok(watcher) => watcher,
                    Err(error) => {
                        event_types::send(
                            &ui_tx,
                            WorkspaceEvent::WatcherFailed {
                                agent_index,
                                message: format!("workspace watcher startup failed: {error}"),
                            },
                        );
                        return;
                    }
                };

                if let Err(error) = watcher.watch(&root, RecursiveMode::Recursive) {
                    event_types::send(
                        &ui_tx,
                        WorkspaceEvent::WatcherFailed {
                            agent_index,
                            message: format!("workspace watcher registration failed: {error}"),
                        },
                    );
                    return;
                }

                event_types::send(
                    &ui_tx,
                    WorkspaceEvent::WatcherReady {
                        agent_index,
                        stop_tx: stop_tx.clone(),
                    },
                );
                let _ = stop_rx.recv();
            })
            .context("failed to spawn workspace watcher thread")?;
        Ok(())
    }

    fn is_relevant(event: &notify::Event) -> bool {
        if !matches!(
            event.kind,
            EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_) | EventKind::Any
        ) {
            return false;
        }

        !event.paths.iter().any(|path| {
            path.components()
                .any(|component| component.as_os_str() == ".git")
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::event::CreateKind;

    #[test]
    fn ignores_git_events_but_accepts_workspace_changes() {
        let source = notify::Event::new(EventKind::Create(CreateKind::File))
            .add_path(PathBuf::from("/tmp/project/src/main.rs"));
        let git = notify::Event::new(EventKind::Create(CreateKind::File))
            .add_path(PathBuf::from("/tmp/project/.git/index"));

        assert!(WorkspaceWatcher::is_relevant(&source));
        assert!(!WorkspaceWatcher::is_relevant(&git));
    }
}
