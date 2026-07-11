use std::{
    collections::{HashMap, VecDeque},
    env,
    io::{BufReader, BufWriter},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc as std_mpsc,
    thread,
};

use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};
use tokio::sync::mpsc;
use url::Url;

use super::{EditorCompletionItem, EditorPosition, UiEvent};
use crate::config::LspServerConfig;

#[path = "lsp_protocol.rs"]
mod lsp_protocol;
#[cfg(test)]
pub(super) use lsp_protocol::char_column_from_utf16;
pub(super) use lsp_protocol::{
    parse_completion_response, parse_definition_response, parse_hover_response,
    summarize_hover_text, utf16_column,
};

pub(super) struct LspRuntimeFactory;

#[derive(Debug)]
pub(super) enum LspCommand {
    Hover {
        agent_index: usize,
        path: PathBuf,
        source: String,
        position: EditorPosition,
    },
    Definition {
        agent_index: usize,
        path: PathBuf,
        source: String,
        position: EditorPosition,
    },
    Completion {
        agent_index: usize,
        path: PathBuf,
        source: String,
        position: EditorPosition,
    },
    Shutdown,
}

#[derive(Debug, Clone)]
struct TrackedDocument {
    version: i32,
    source: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct DefinitionTarget {
    pub(super) path: PathBuf,
    pub(super) position: EditorPosition,
}

struct LspSession {
    workspace_root: PathBuf,
    server: LspServerConfig,
    child: Option<Child>,
    stdin: Option<BufWriter<ChildStdin>>,
    incoming: Option<std_mpsc::Receiver<LspIncoming>>,
    reader: Option<thread::JoinHandle<()>>,
    next_request_id: u64,
    documents: HashMap<PathBuf, TrackedDocument>,
    pending_responses: HashMap<u64, Value>,
    pending_notifications: VecDeque<Value>,
    agent_index: usize,
    ui_tx: mpsc::UnboundedSender<UiEvent>,
}

enum LspIncoming {
    Message(Value),
    Error(String),
}

impl LspRuntimeFactory {
    pub(super) fn spawn(
        workspace_root: &Path,
        server: LspServerConfig,
        agent_index: usize,
        ui_tx: mpsc::UnboundedSender<UiEvent>,
    ) -> Result<std_mpsc::Sender<LspCommand>> {
        let workspace_root = workspace_root.to_path_buf();
        let (command_tx, command_rx) = std_mpsc::channel::<LspCommand>();

        thread::Builder::new()
            .name(format!(
                "cmdex-lsp-{}-{}",
                server.name.clone(),
                workspace_root
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("workspace")
            ))
            .spawn(move || {
                let mut session =
                    LspSession::new(workspace_root, server, agent_index, ui_tx.clone());

                while let Ok(command) = command_rx.recv() {
                    match command {
                        LspCommand::Hover {
                            agent_index,
                            path,
                            source,
                            position,
                        } => match session.hover(&path, &source, position) {
                            Ok(contents) => {
                                let _ = ui_tx.send(UiEvent::LspHoverResult {
                                    agent_index,
                                    path,
                                    position,
                                    contents,
                                    error: None,
                                });
                            }
                            Err(error) => {
                                let _ = ui_tx.send(UiEvent::LspHoverResult {
                                    agent_index,
                                    path,
                                    position,
                                    contents: None,
                                    error: Some(error.to_string()),
                                });
                            }
                        },
                        LspCommand::Definition {
                            agent_index,
                            path,
                            source,
                            position,
                        } => match session.definition(&path, &source, position) {
                            Ok(target) => {
                                let _ = ui_tx.send(UiEvent::LspDefinitionResult {
                                    agent_index,
                                    source_path: path,
                                    _source_position: position,
                                    target,
                                    error: None,
                                });
                            }
                            Err(error) => {
                                let _ = ui_tx.send(UiEvent::LspDefinitionResult {
                                    agent_index,
                                    source_path: path,
                                    _source_position: position,
                                    target: None,
                                    error: Some(error.to_string()),
                                });
                            }
                        },
                        LspCommand::Completion {
                            agent_index,
                            path,
                            source,
                            position,
                        } => match session.completion(&path, &source, position) {
                            Ok(items) => {
                                let _ = ui_tx.send(UiEvent::LspCompletionResult {
                                    agent_index,
                                    path,
                                    position,
                                    items,
                                    error: None,
                                });
                            }
                            Err(error) => {
                                let _ = ui_tx.send(UiEvent::LspCompletionResult {
                                    agent_index,
                                    path,
                                    position,
                                    items: Vec::new(),
                                    error: Some(error.to_string()),
                                });
                            }
                        },
                        LspCommand::Shutdown => break,
                    }
                }

                session.shutdown();
            })
            .context("failed to spawn LSP worker thread")?;

        Ok(command_tx)
    }
}

impl LspSession {
    fn new(
        workspace_root: PathBuf,
        server: LspServerConfig,
        agent_index: usize,
        ui_tx: mpsc::UnboundedSender<UiEvent>,
    ) -> Self {
        Self {
            workspace_root,
            server,
            child: None,
            stdin: None,
            incoming: None,
            reader: None,
            next_request_id: 1,
            documents: HashMap::new(),
            pending_responses: HashMap::new(),
            pending_notifications: VecDeque::new(),
            agent_index,
            ui_tx,
        }
    }

    fn hover(
        &mut self,
        path: &Path,
        source: &str,
        position: EditorPosition,
    ) -> Result<Option<String>> {
        self.ensure_document_synced(path, source)?;
        let uri = file_uri(path)?;
        let result = self.request_value(
            "textDocument/hover",
            json!({
                "textDocument": { "uri": uri },
                "position": {
                    "line": position.row,
                    "character": utf16_column(source, position),
                }
            }),
        )?;
        Ok(parse_hover_response(&result).and_then(|text| summarize_hover_text(&text)))
    }

    fn definition(
        &mut self,
        path: &Path,
        source: &str,
        position: EditorPosition,
    ) -> Result<Option<DefinitionTarget>> {
        self.ensure_document_synced(path, source)?;
        let uri = file_uri(path)?;
        let result = self.request_value(
            "textDocument/definition",
            json!({
                "textDocument": { "uri": uri },
                "position": {
                    "line": position.row,
                    "character": utf16_column(source, position),
                }
            }),
        )?;
        parse_definition_response(&result)
    }

    fn completion(
        &mut self,
        path: &Path,
        source: &str,
        position: EditorPosition,
    ) -> Result<Vec<EditorCompletionItem>> {
        self.ensure_document_synced(path, source)?;
        let uri = file_uri(path)?;
        let result = self.request_value(
            "textDocument/completion",
            json!({
                "textDocument": { "uri": uri },
                "position": {
                    "line": position.row,
                    "character": utf16_column(source, position),
                }
            }),
        )?;
        Ok(parse_completion_response(&result, source, position))
    }

    fn ensure_document_synced(&mut self, path: &Path, source: &str) -> Result<()> {
        self.ensure_started()?;
        let uri = file_uri(path)?;

        if let Some((version, changed)) = self
            .documents
            .get(path)
            .map(|document| (document.version + 1, document.source.as_str() != source))
        {
            if changed {
                self.notify(
                    "textDocument/didChange",
                    json!({
                        "textDocument": {
                            "uri": uri,
                            "version": version,
                        },
                        "contentChanges": [{
                            "text": source,
                        }]
                    }),
                )?;
                if let Some(document) = self.documents.get_mut(path) {
                    document.version = version;
                    document.source = source.to_string();
                }
            }
            return Ok(());
        }

        self.notify(
            "textDocument/didOpen",
            json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": self.server.language_id.clone(),
                    "version": 1,
                    "text": source,
                }
            }),
        )?;
        self.documents.insert(
            path.to_path_buf(),
            TrackedDocument {
                version: 1,
                source: source.to_string(),
            },
        );
        Ok(())
    }

    fn ensure_started(&mut self) -> Result<()> {
        if self.child.is_some() {
            return Ok(());
        }

        let command = resolve_command_path(&self.server.command)?;
        let mut process = Command::new(&command);
        process
            .current_dir(&self.workspace_root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        process.args(&self.server.args);
        process.envs(self.server.env.iter());

        let mut child = process.spawn().with_context(|| {
            format!(
                "failed to start LSP server '{}' from {}",
                self.server.name,
                command.display()
            )
        })?;

        let stdin = child
            .stdin
            .take()
            .with_context(|| format!("failed to open {} stdin", self.server.name))?;
        let stdout = child
            .stdout
            .take()
            .with_context(|| format!("failed to open {} stdout", self.server.name))?;

        self.stdin = Some(BufWriter::new(stdin));
        let (incoming_tx, incoming_rx) = std_mpsc::channel();
        let server_name = self.server.name.clone();
        let reader = thread::Builder::new()
            .name(format!("cmdex-lsp-reader-{}", server_name))
            .spawn(move || {
                let mut stdout = BufReader::new(stdout);
                loop {
                    match super::lsp_framing::read_message(&mut stdout, &server_name) {
                        Ok(message) => {
                            if incoming_tx.send(LspIncoming::Message(message)).is_err() {
                                break;
                            }
                        }
                        Err(error) => {
                            let _ = incoming_tx.send(LspIncoming::Error(error.to_string()));
                            break;
                        }
                    }
                }
            })
            .context("failed to spawn LSP reader thread")?;
        self.incoming = Some(incoming_rx);
        self.reader = Some(reader);
        self.child = Some(child);

        let root_uri = file_uri(&self.workspace_root)?;
        let _ = self.request_value(
            "initialize",
            json!({
                "processId": std::process::id(),
                "rootUri": root_uri,
                "workspaceFolders": [{
                    "uri": root_uri,
                    "name": self.workspace_root
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or("workspace"),
                }],
                "capabilities": {},
            }),
        )?;
        self.notify("initialized", json!({}))?;
        Ok(())
    }

    fn request_value(&mut self, method: &str, params: Value) -> Result<Value> {
        for attempt in 0..2 {
            let id = self.next_request_id;
            self.next_request_id += 1;

            self.send_message(&json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": method,
                "params": params.clone(),
            }))?;

            let response = self.read_response(id)?;

            if let Some(error) = response.get("error") {
                if attempt == 0 && response_error_is_retryable(error) {
                    continue;
                }
                return Err(anyhow!(response_error_message(error)));
            }

            return Ok(response.get("result").cloned().unwrap_or(Value::Null));
        }

        Err(anyhow!("LSP request failed after retry"))
    }

    fn notify(&mut self, method: &str, params: Value) -> Result<()> {
        self.send_message(&json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        }))
    }

    fn read_response(&mut self, request_id: u64) -> Result<Value> {
        if let Some(response) = self.pending_responses.remove(&request_id) {
            return Ok(response);
        }

        loop {
            let message = self.read_message()?;
            if response_id(&message) == Some(request_id) {
                return Ok(message);
            }

            self.route_message(message)?;
        }
    }

    fn route_message(&mut self, message: Value) -> Result<()> {
        if let Some(method) = message
            .get("method")
            .and_then(Value::as_str)
            .map(str::to_string)
        {
            if let Some(id) = message.get("id") {
                self.send_message(&json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "error": {
                        "code": -32601,
                        "message": format!("Unsupported server request: {method}"),
                    },
                }))?;
            } else {
                let params = message.get("params").cloned().unwrap_or(Value::Null);
                self.pending_notifications.push_back(message);
                let _ = self.ui_tx.send(UiEvent::LspNotification {
                    agent_index: self.agent_index,
                    server_name: self.server.name.clone(),
                    method,
                    params,
                });
            }
            return Ok(());
        }

        if let Some(id) = response_id(&message) {
            self.pending_responses.insert(id, message);
        }
        Ok(())
    }

    fn send_message(&mut self, payload: &Value) -> Result<()> {
        let stdin = self
            .stdin
            .as_mut()
            .with_context(|| format!("{} stdin is unavailable", self.server.name))?;
        super::lsp_framing::write_message(stdin, payload)
    }

    fn read_message(&mut self) -> Result<Value> {
        let incoming = self
            .incoming
            .as_ref()
            .with_context(|| format!("{} reader is unavailable", self.server.name))?;
        match incoming.recv() {
            Ok(LspIncoming::Message(message)) => Ok(message),
            Ok(LspIncoming::Error(error)) => Err(anyhow!(error)),
            Err(error) => Err(anyhow!(
                "{} reader stopped before a response arrived: {}",
                self.server.name,
                error
            )),
        }
    }

    fn shutdown(&mut self) {
        if self.child.is_none() {
            return;
        }

        let _ = self.request_value("shutdown", Value::Null);
        let _ = self.notify("exit", Value::Null);

        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }

        self.child = None;
        self.stdin = None;
        self.incoming = None;
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
        self.documents.clear();
        self.pending_responses.clear();
        self.pending_notifications.clear();
    }
}

fn resolve_command_path(command: &str) -> Result<PathBuf> {
    let trimmed = command.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("LSP command cannot be empty"));
    }

    if trimmed == "~" {
        return env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or_else(|| anyhow!("HOME is not set"));
    }

    if let Some(rest) = trimmed.strip_prefix("~/") {
        return env::var_os("HOME")
            .map(PathBuf::from)
            .map(|home| home.join(rest))
            .ok_or_else(|| anyhow!("HOME is not set"));
    }

    Ok(PathBuf::from(trimmed))
}

fn file_uri(path: &Path) -> Result<String> {
    Url::from_file_path(path)
        .map(|url| url.to_string())
        .map_err(|_| anyhow!("failed to convert {} into a file URI", path.display()))
}

fn response_id(value: &Value) -> Option<u64> {
    value
        .get("id")
        .and_then(Value::as_u64)
        .or_else(|| value.get("id").and_then(Value::as_i64).map(|id| id as u64))
}

fn response_error_message(error: &Value) -> String {
    error
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("unknown LSP error")
        .to_string()
}

fn response_error_is_retryable(error: &Value) -> bool {
    error
        .get("code")
        .and_then(Value::as_i64)
        .is_some_and(|code| code == -32801)
        || error
            .get("message")
            .and_then(Value::as_str)
            .is_some_and(|message| message.eq_ignore_ascii_case("content modified"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lsp_response_wait_keeps_interleaved_notifications_and_other_responses() {
        let mut session = LspSession::new(
            PathBuf::from("/tmp/workspace"),
            LspServerConfig {
                name: "test".to_string(),
                command: "test-server".to_string(),
                args: Vec::new(),
                language_id: "rust".to_string(),
                extensions: vec!["rs".to_string()],
                env: Default::default(),
            },
            0,
            mpsc::unbounded_channel().0,
        );

        session
            .route_message(json!({
                "jsonrpc": "2.0",
                "method": "window/logMessage",
                "params": { "message": "indexing" },
            }))
            .unwrap();
        session
            .route_message(json!({
                "jsonrpc": "2.0",
                "id": 9,
                "result": { "ready": true },
            }))
            .unwrap();

        let response = session.pending_responses.remove(&9).unwrap();

        assert_eq!(response.get("id").and_then(Value::as_u64), Some(9));
        assert_eq!(session.pending_notifications.len(), 1);
    }
}
