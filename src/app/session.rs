use super::event_types::{self, ChatEvent};
use super::*;

pub struct MessageStore;

impl MessageStore {
    pub(super) fn upsert(agent: &mut AgentState, role: MessageRole, item_id: &str, text: String) {
        if let Some(message) = agent
            .chat
            .messages
            .iter_mut()
            .find(|message| message.item_id.as_deref() == Some(item_id))
        {
            message.set_text(text);
        } else {
            agent
                .chat
                .messages
                .push(ChatMessage::new(role, text, Some(item_id.to_string())));
        }
        agent.chat.invalidate_chat_render_cache();
    }
}

pub struct SessionLoader;

impl SessionLoader {
    pub(super) fn session_messages(session: WorkspaceSession) -> (String, Vec<ChatMessage>) {
        let messages = session
            .entries
            .into_iter()
            .map(|entry| {
                ChatMessage::new(
                    match entry.kind {
                        HistoryEntryKind::User => MessageRole::User,
                        HistoryEntryKind::Assistant => MessageRole::Assistant,
                        HistoryEntryKind::Event => MessageRole::Event,
                    },
                    entry.text,
                    None,
                )
            })
            .collect::<Vec<_>>();

        (session.thread.id, messages)
    }

    pub(super) fn spawn(
        codex: CodexAppServer,
        ui_tx: mpsc::UnboundedSender<UiEvent>,
        agent_index: usize,
        workspace: PathBuf,
    ) {
        tokio::spawn(async move {
            match codex.load_latest_workspace_session(&workspace).await {
                Ok(session) => {
                    event_types::send(
                        &ui_tx,
                        ChatEvent::SessionLoaded {
                            agent_index,
                            session,
                        },
                    );
                }
                Err(error) => {
                    event_types::send(
                        &ui_tx,
                        ChatEvent::SubmissionFailed {
                            agent_index,
                            message: format!(
                                "Failed to load the latest workspace session: {error}"
                            ),
                        },
                    );
                }
            }
        });
    }

    pub(super) async fn hydrate_latest_sessions(
        app: &mut App,
        codex: &CodexAppServer,
    ) -> Result<()> {
        for agent in &mut app.agents {
            if let Some(session) = codex
                .load_latest_workspace_session(&agent.definition.workspace)
                .await?
            {
                let (thread_id, messages) = Self::session_messages(session);
                agent.chat.thread_id = Some(thread_id);
                agent.chat.replace_messages(messages);
            }
        }

        Ok(())
    }
}
