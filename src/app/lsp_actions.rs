use super::{lsp, *};

impl App {
    fn lsp_command_tx(
        &mut self,
        agent_index: usize,
        server_index: usize,
        ui_tx: &mpsc::UnboundedSender<UiEvent>,
    ) -> anyhow::Result<std::sync::mpsc::Sender<lsp::LspCommand>> {
        let key = LspRuntimeKey {
            agent_index,
            server_index,
        };
        if let Some(runtime) = self.lsp_runtimes.get(&key) {
            return Ok(runtime.command_tx.clone());
        }

        let workspace_root = self.agents[agent_index].definition.workspace.clone();
        let server = self.lsp_servers[server_index].clone();
        let command_tx =
            lsp::LspRuntimeFactory::spawn(&workspace_root, server, agent_index, ui_tx.clone())?;
        self.lsp_runtimes.insert(
            key,
            LspRuntime {
                command_tx: command_tx.clone(),
                server_name: self.lsp_servers[server_index].name.clone(),
                starting: true,
            },
        );
        Ok(command_tx)
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
        ui_tx: &mpsc::UnboundedSender<UiEvent>,
    ) {
        let Some(server_index) = self.lsp_server_index_for_path(&path) else {
            return;
        };
        let server_name = self.lsp_servers[server_index].name.clone();

        let command_tx = match self.lsp_command_tx(agent_index, server_index, ui_tx) {
            Ok(command_tx) => command_tx,
            Err(error) => {
                if let Some(agent) = self.agents.get_mut(agent_index)
                    && let Some(editor) = agent.workspace.editor.as_mut()
                {
                    editor.status = Some(error.to_string());
                }
                return;
            }
        };

        if command_tx
            .send(lsp::LspCommand::Hover {
                agent_index,
                path,
                source,
                position,
            })
            .is_err()
        {
            self.lsp_runtimes.remove(&LspRuntimeKey {
                agent_index,
                server_index,
            });
            if let Some(agent) = self.agents.get_mut(agent_index)
                && let Some(editor) = agent.workspace.editor.as_mut()
            {
                editor.status = Some(format!("Failed to send hover request to {}", server_name));
            }
        }
    }

    pub(super) fn request_lsp_definition(
        &mut self,
        agent_index: usize,
        path: PathBuf,
        source: String,
        position: EditorPosition,
        ui_tx: &mpsc::UnboundedSender<UiEvent>,
    ) {
        let Some(server_index) = self.lsp_server_index_for_path(&path) else {
            let error_message = self.lsp_server_error_for_path(&path);
            if let Some(agent) = self.agents.get_mut(agent_index)
                && let Some(editor) = agent.workspace.editor.as_mut()
            {
                editor.status = Some(error_message);
            }
            return;
        };
        let server_name = self.lsp_servers[server_index].name.clone();

        let command_tx = match self.lsp_command_tx(agent_index, server_index, ui_tx) {
            Ok(command_tx) => command_tx,
            Err(error) => {
                if let Some(agent) = self.agents.get_mut(agent_index)
                    && let Some(editor) = agent.workspace.editor.as_mut()
                {
                    editor.status = Some(error.to_string());
                }
                return;
            }
        };

        if command_tx
            .send(lsp::LspCommand::Definition {
                agent_index,
                path,
                source,
                position,
            })
            .is_err()
        {
            self.lsp_runtimes.remove(&LspRuntimeKey {
                agent_index,
                server_index,
            });
            if let Some(agent) = self.agents.get_mut(agent_index)
                && let Some(editor) = agent.workspace.editor.as_mut()
            {
                editor.status = Some(format!(
                    "Failed to send definition request to {}",
                    server_name
                ));
            }
        }
    }

    pub(super) fn request_lsp_completion(
        &mut self,
        agent_index: usize,
        path: PathBuf,
        source: String,
        position: EditorPosition,
        ui_tx: &mpsc::UnboundedSender<UiEvent>,
    ) {
        let Some(server_index) = self.lsp_server_index_for_path(&path) else {
            let error_message = self.lsp_server_error_for_path(&path);
            if let Some(agent) = self.agents.get_mut(agent_index)
                && let Some(editor) = agent.workspace.editor.as_mut()
            {
                editor.status = Some(error_message);
                editor.clear_completion();
            }
            return;
        };
        let server_name = self.lsp_servers[server_index].name.clone();

        let command_tx = match self.lsp_command_tx(agent_index, server_index, ui_tx) {
            Ok(command_tx) => command_tx,
            Err(error) => {
                if let Some(agent) = self.agents.get_mut(agent_index)
                    && let Some(editor) = agent.workspace.editor.as_mut()
                {
                    editor.status = Some(error.to_string());
                    editor.clear_completion();
                }
                return;
            }
        };

        if command_tx
            .send(lsp::LspCommand::Completion {
                agent_index,
                path,
                source,
                position,
            })
            .is_err()
        {
            self.lsp_runtimes.remove(&LspRuntimeKey {
                agent_index,
                server_index,
            });
            if let Some(agent) = self.agents.get_mut(agent_index)
                && let Some(editor) = agent.workspace.editor.as_mut()
            {
                editor.status = Some(format!(
                    "Failed to send completion request to {}",
                    server_name
                ));
                editor.clear_completion();
            }
        }
    }
}
