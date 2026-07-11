use super::*;

impl App {
    pub(super) fn clear_workspace_hover(&mut self) {
        self.pending_workspace_hover = None;
        let Some(agent) = self.active_agent_mut() else {
            return;
        };
        let Some(editor) = agent.workspace.editor.as_mut() else {
            return;
        };
        editor.clear_hover();
    }

    pub(in crate::app) fn set_pending_workspace_hover(
        &mut self,
        agent_index: usize,
        column: u16,
        row: u16,
        path: PathBuf,
        position: EditorPosition,
    ) {
        let is_same_pending = self
            .pending_workspace_hover
            .as_ref()
            .is_some_and(|pending| {
                pending.agent_index == agent_index
                    && pending.column == column
                    && pending.row == row
                    && pending.path == path
                    && pending.position == position
            });
        if is_same_pending {
            return;
        }

        self.clear_workspace_hover();
        self.pending_workspace_hover = Some(PendingWorkspaceHover {
            agent_index,
            column,
            row,
            path,
            position,
            started_at: Instant::now(),
        });
    }

    pub(super) fn dispatch_pending_workspace_hover(
        &mut self,
        ui_tx: &mpsc::UnboundedSender<UiEvent>,
    ) -> bool {
        let Some(pending) = self.pending_workspace_hover.clone() else {
            return false;
        };
        if pending.started_at.elapsed() < HOVER_POPOVER_DELAY {
            return false;
        }

        let Some((path, source, position)) = ({
            let Some(agent) = self.agents.get_mut(pending.agent_index) else {
                self.pending_workspace_hover = None;
                return false;
            };
            let Some(editor) = agent.workspace.editor.as_mut() else {
                self.pending_workspace_hover = None;
                return false;
            };
            if editor.path != pending.path {
                self.pending_workspace_hover = None;
                return false;
            }
            if !editor.request_hover(pending.position) {
                self.pending_workspace_hover = None;
                return false;
            }

            Some((editor.path.clone(), editor.source_text(), pending.position))
        }) else {
            return false;
        };

        self.pending_workspace_hover = None;
        self.request_lsp_hover(pending.agent_index, path, source, position, ui_tx);
        true
    }

    pub(super) fn find_agent_by_thread_mut(&mut self, thread_id: &str) -> Option<&mut AgentState> {
        self.agents
            .iter_mut()
            .find(|agent| agent.chat.thread_id.as_deref() == Some(thread_id))
    }
}
