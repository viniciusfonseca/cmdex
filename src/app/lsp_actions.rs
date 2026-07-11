use super::{effects::AppEffect, lsp, *};

impl App {
    fn enqueue_lsp_command(
        &mut self,
        agent_index: usize,
        server_index: usize,
        command: lsp::LspCommand,
    ) {
        let key = LspRuntimeKey {
            agent_index,
            server_index,
        };

        if let Some(runtime) = self.lsp_runtimes.get(&key) {
            self.enqueue_effect(AppEffect::SendLspCommand {
                agent_index,
                server_index,
                command_tx: runtime.command_tx.clone(),
                command,
            });
            return;
        }

        if !self.lsp_starting.insert(key) {
            self.pending_lsp_commands
                .entry(key)
                .or_default()
                .push(command);
            return;
        }

        let workspace = self.agents[agent_index].definition.workspace.clone();
        let server = self.lsp_servers[server_index].clone();
        self.enqueue_effect(AppEffect::StartLspSession {
            agent_index,
            server_index,
            workspace,
            server,
            command,
        });
    }

    fn lsp_server_index_for_path(&self, path: &std::path::Path) -> Option<usize> {
        self.lsp_server_for_path(path)
            .map(|(server_index, _)| server_index)
    }

    fn lsp_server_error_for_path(&self, path: &std::path::Path) -> String {
        if let Some(extension) = path.extension().and_then(|extension| extension.to_str()) {
            format!("No LSP server configured for .{} files.", extension)
        } else {
            "No LSP server configured for files without extension.".to_string()
        }
    }

    pub(super) fn mark_lsp_runtime_ready_for_path(
        &mut self,
        agent_index: usize,
        path: &std::path::Path,
    ) {
        let Some(server_index) = self.lsp_server_index_for_path(path) else {
            return;
        };
        if let Some(runtime) = self.lsp_runtimes.get_mut(&LspRuntimeKey {
            agent_index,
            server_index,
        }) {
            runtime.starting = false;
        }
    }

    pub(super) fn request_lsp_hover(
        &mut self,
        agent_index: usize,
        path: PathBuf,
        source: String,
        position: EditorPosition,
        _ui_tx: &mpsc::UnboundedSender<UiEvent>,
    ) {
        let Some(server_index) = self.lsp_server_index_for_path(&path) else {
            return;
        };
        self.enqueue_lsp_command(
            agent_index,
            server_index,
            lsp::LspCommand::Hover {
                agent_index,
                path,
                source,
                position,
            },
        );
    }

    pub(super) fn request_lsp_definition(
        &mut self,
        agent_index: usize,
        path: PathBuf,
        source: String,
        position: EditorPosition,
        _ui_tx: &mpsc::UnboundedSender<UiEvent>,
    ) {
        let Some(server_index) = self.lsp_server_index_for_path(&path) else {
            self.set_lsp_editor_status(agent_index, self.lsp_server_error_for_path(&path));
            return;
        };
        self.enqueue_lsp_command(
            agent_index,
            server_index,
            lsp::LspCommand::Definition {
                agent_index,
                path,
                source,
                position,
            },
        );
    }

    pub(super) fn request_lsp_completion(
        &mut self,
        agent_index: usize,
        path: PathBuf,
        source: String,
        position: EditorPosition,
        _ui_tx: &mpsc::UnboundedSender<UiEvent>,
    ) {
        let Some(server_index) = self.lsp_server_index_for_path(&path) else {
            self.set_lsp_editor_status(agent_index, self.lsp_server_error_for_path(&path));
            if let Some(agent) = self.agents.get_mut(agent_index)
                && let Some(editor) = agent.workspace.editor.as_mut()
            {
                editor.clear_completion();
            }
            return;
        };
        self.enqueue_lsp_command(
            agent_index,
            server_index,
            lsp::LspCommand::Completion {
                agent_index,
                path,
                source,
                position,
            },
        );
    }

    pub(super) fn set_lsp_editor_status(&mut self, agent_index: usize, message: String) {
        if let Some(agent) = self.agents.get_mut(agent_index)
            && let Some(editor) = agent.workspace.editor.as_mut()
        {
            editor.status = Some(message);
        }
    }
}
