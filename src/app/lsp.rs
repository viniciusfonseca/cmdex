use std::{
    collections::HashMap,
    env, fs,
    io::{BufRead, BufReader, BufWriter, Read, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    sync::mpsc as std_mpsc,
    thread,
};

use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};
use tokio::sync::mpsc;
use url::Url;

use super::{EditorPosition, UiEvent};

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
    child: Option<Child>,
    stdin: Option<BufWriter<ChildStdin>>,
    stdout: Option<BufReader<ChildStdout>>,
    next_request_id: u64,
    documents: HashMap<PathBuf, TrackedDocument>,
}

impl LspRuntimeFactory {
    pub(super) fn spawn(
        workspace_root: &Path,
        ui_tx: mpsc::UnboundedSender<UiEvent>,
    ) -> Result<std_mpsc::Sender<LspCommand>> {
        let workspace_root = workspace_root.to_path_buf();
        let (command_tx, command_rx) = std_mpsc::channel::<LspCommand>();

        thread::Builder::new()
            .name(format!(
                "cmdex-lsp-{}",
                workspace_root
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("workspace")
            ))
            .spawn(move || {
                let mut session = LspSession::new(workspace_root);

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
    fn new(workspace_root: PathBuf) -> Self {
        Self {
            workspace_root,
            child: None,
            stdin: None,
            stdout: None,
            next_request_id: 1,
            documents: HashMap::new(),
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

    fn ensure_document_synced(&mut self, path: &Path, source: &str) -> Result<()> {
        self.ensure_started()?;
        let language_id = language_id_for_path(path)?;
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
                    "languageId": language_id,
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

        let binary = resolve_rust_analyzer_binary()?;
        let mut child = Command::new(&binary)
            .current_dir(&self.workspace_root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .with_context(|| format!("failed to start rust-analyzer from {}", binary.display()))?;

        let stdin = child
            .stdin
            .take()
            .context("failed to open rust-analyzer stdin")?;
        let stdout = child
            .stdout
            .take()
            .context("failed to open rust-analyzer stdout")?;

        self.stdin = Some(BufWriter::new(stdin));
        self.stdout = Some(BufReader::new(stdout));
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
        let id = self.next_request_id;
        self.next_request_id += 1;

        self.send_message(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        }))?;

        loop {
            let response = self.read_message()?;
            if response_id(&response) != Some(id) {
                continue;
            }

            if let Some(error) = response.get("error") {
                return Err(anyhow!(response_error_message(error)));
            }

            return Ok(response.get("result").cloned().unwrap_or(Value::Null));
        }
    }

    fn notify(&mut self, method: &str, params: Value) -> Result<()> {
        self.send_message(&json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        }))
    }

    fn send_message(&mut self, payload: &Value) -> Result<()> {
        let stdin = self
            .stdin
            .as_mut()
            .context("rust-analyzer stdin is unavailable")?;
        let body = serde_json::to_vec(payload)?;
        write!(stdin, "Content-Length: {}\r\n\r\n", body.len())?;
        stdin.write_all(&body)?;
        stdin.flush()?;
        Ok(())
    }

    fn read_message(&mut self) -> Result<Value> {
        let stdout = self
            .stdout
            .as_mut()
            .context("rust-analyzer stdout is unavailable")?;
        let mut content_length = None;

        loop {
            let mut header = String::new();
            let bytes = stdout.read_line(&mut header)?;
            if bytes == 0 {
                return Err(anyhow!("rust-analyzer closed the LSP stream"));
            }

            if header == "\r\n" || header == "\n" {
                break;
            }

            if let Some((name, value)) = header.split_once(':') {
                if name.eq_ignore_ascii_case("Content-Length") {
                    content_length = Some(
                        value
                            .trim()
                            .parse::<usize>()
                            .context("invalid LSP Content-Length header")?,
                    );
                }
            }
        }

        let length = content_length.context("missing LSP Content-Length header")?;
        let mut payload = vec![0_u8; length];
        stdout.read_exact(&mut payload)?;
        Ok(serde_json::from_slice(&payload).context("invalid LSP JSON payload")?)
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
        self.stdout = None;
        self.documents.clear();
    }
}

fn resolve_rust_analyzer_binary() -> Result<PathBuf> {
    if let Some(path) = env::var_os("CMDEX_RUST_ANALYZER").or_else(|| env::var_os("RUST_ANALYZER"))
    {
        return Ok(PathBuf::from(path));
    }

    if Command::new("rust-analyzer")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
    {
        return Ok(PathBuf::from("rust-analyzer"));
    }

    if let Ok(output) = Command::new("rustup")
        .args(["which", "rust-analyzer"])
        .output()
    {
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !path.is_empty() {
                return Ok(PathBuf::from(path));
            }
        }
    }

    Err(anyhow!(
        "rust-analyzer is not available. Install it with `rustup component add rust-analyzer` or set CMDEX_RUST_ANALYZER."
    ))
}

fn language_id_for_path(path: &Path) -> Result<&'static str> {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("rs") => Ok("rust"),
        _ => Err(anyhow!("LSP is available only for Rust files.")),
    }
}

fn file_uri(path: &Path) -> Result<String> {
    Url::from_file_path(path)
        .map(|url| url.to_string())
        .map_err(|_| anyhow!("failed to convert {} into a file URI", path.display()))
}

pub(super) fn summarize_hover_text(text: &str) -> Option<String> {
    let mut lines = Vec::new();
    let mut previous_blank = false;

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            continue;
        }

        if trimmed.is_empty() {
            if !previous_blank && !lines.is_empty() {
                lines.push(String::new());
            }
            previous_blank = true;
            continue;
        }

        lines.push(trimmed.to_string());
        previous_blank = false;
    }

    if lines.is_empty() {
        return None;
    }

    const MAX_LINES: usize = 10;
    const MAX_CHARS: usize = 320;

    let truncated_by_lines = lines.len() > MAX_LINES;
    if truncated_by_lines {
        lines.truncate(MAX_LINES);
    }

    let mut summarized = lines.join("\n");
    let total_chars = summarized.chars().count();
    if total_chars <= MAX_CHARS && !truncated_by_lines {
        return Some(summarized);
    }

    if total_chars > MAX_CHARS {
        summarized = summarized
            .chars()
            .take(MAX_CHARS.saturating_sub(3))
            .collect::<String>();
    }

    if !summarized.ends_with("...") {
        summarized.push_str("...");
    }

    Some(summarized)
}

pub(super) fn parse_hover_response(value: &Value) -> Option<String> {
    hover_value_to_text(value.get("contents")?)
}

fn hover_value_to_text(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::String(text) => Some(text.clone()),
        Value::Array(items) => {
            let joined = items
                .iter()
                .filter_map(hover_value_to_text)
                .filter(|text| !text.trim().is_empty())
                .collect::<Vec<_>>()
                .join("\n\n");
            (!joined.is_empty()).then_some(joined)
        }
        Value::Object(map) => map
            .get("language")
            .and_then(Value::as_str)
            .zip(map.get("value").and_then(Value::as_str))
            .map(|(language, value)| format!("{language}\n{value}"))
            .or_else(|| map.get("value").and_then(Value::as_str).map(str::to_string)),
        _ => None,
    }
}

pub(super) fn parse_definition_response(value: &Value) -> Result<Option<DefinitionTarget>> {
    let Some(location) = first_definition_location(value) else {
        return Ok(None);
    };

    let path = file_path_from_uri(location.uri)?;
    let source = fs::read_to_string(&path)
        .map(|contents| normalize_newlines(&contents))
        .unwrap_or_default();
    let row = location.line;
    let col = char_column_from_utf16(&source, row, location.character);

    Ok(Some(DefinitionTarget {
        path,
        position: EditorPosition { row, col },
    }))
}

#[derive(Debug, Clone)]
struct RawDefinitionLocation {
    uri: String,
    line: usize,
    character: usize,
}

fn first_definition_location(value: &Value) -> Option<RawDefinitionLocation> {
    match value {
        Value::Null => None,
        Value::Array(items) => items.iter().find_map(first_definition_location),
        Value::Object(map) => {
            if let Some(uri) = map.get("uri").and_then(Value::as_str) {
                let range = map.get("range")?;
                return extract_definition_location(uri, range);
            }

            let uri = map.get("targetUri").and_then(Value::as_str)?;
            let range = map
                .get("targetSelectionRange")
                .or_else(|| map.get("targetRange"))?;
            extract_definition_location(uri, range)
        }
        _ => None,
    }
}

fn extract_definition_location(uri: &str, range: &Value) -> Option<RawDefinitionLocation> {
    let start = range.get("start")?;
    Some(RawDefinitionLocation {
        uri: uri.to_string(),
        line: start.get("line")?.as_u64()? as usize,
        character: start.get("character")?.as_u64()? as usize,
    })
}

fn file_path_from_uri(uri: String) -> Result<PathBuf> {
    Url::parse(&uri)
        .context("invalid LSP file URI")?
        .to_file_path()
        .map_err(|_| anyhow!("definition target is not a local file"))
}

pub(super) fn utf16_column(source: &str, position: EditorPosition) -> usize {
    source
        .split('\n')
        .nth(position.row)
        .unwrap_or_default()
        .chars()
        .take(position.col)
        .map(char::len_utf16)
        .sum()
}

pub(super) fn char_column_from_utf16(source: &str, row: usize, utf16_col: usize) -> usize {
    let mut units = 0;
    let mut chars = 0;

    for character in source.split('\n').nth(row).unwrap_or_default().chars() {
        let width = character.len_utf16();
        if units + width > utf16_col {
            break;
        }
        units += width;
        chars += 1;
    }

    chars
}

fn normalize_newlines(source: &str) -> String {
    source.replace("\r\n", "\n").replace('\r', "\n")
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
