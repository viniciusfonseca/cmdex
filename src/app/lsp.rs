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

use super::{EditorCompletionItem, EditorPosition, UiEvent};
use crate::config::LspServerConfig;

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
    stdout: Option<BufReader<ChildStdout>>,
    next_request_id: u64,
    documents: HashMap<PathBuf, TrackedDocument>,
}

impl LspRuntimeFactory {
    pub(super) fn spawn(
        workspace_root: &Path,
        server: LspServerConfig,
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
                let mut session = LspSession::new(workspace_root, server);

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
    fn new(workspace_root: PathBuf, server: LspServerConfig) -> Self {
        Self {
            workspace_root,
            server,
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
        for attempt in 0..2 {
            let id = self.next_request_id;
            self.next_request_id += 1;

            self.send_message(&json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": method,
                "params": params.clone(),
            }))?;

            loop {
                let response = self.read_message()?;
                if response_id(&response) != Some(id) {
                    continue;
                }

                if let Some(error) = response.get("error") {
                    if attempt == 0 && response_error_is_retryable(error) {
                        break;
                    }
                    return Err(anyhow!(response_error_message(error)));
                }

                return Ok(response.get("result").cloned().unwrap_or(Value::Null));
            }
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

    fn send_message(&mut self, payload: &Value) -> Result<()> {
        let stdin = self
            .stdin
            .as_mut()
            .with_context(|| format!("{} stdin is unavailable", self.server.name))?;
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
            .with_context(|| format!("{} stdout is unavailable", self.server.name))?;
        let mut content_length = None;

        loop {
            let mut header = String::new();
            let bytes = stdout.read_line(&mut header)?;
            if bytes == 0 {
                return Err(anyhow!("{} closed the LSP stream", self.server.name));
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

pub(super) fn summarize_hover_text(text: &str) -> Option<String> {
    let mut lines = Vec::new();
    let mut previous_blank = false;
    let mut inside_code_fence = false;

    for line in text.lines() {
        let trimmed_start = line.trim_start();
        if trimmed_start.starts_with("```") {
            lines.push(trimmed_start.trim().to_string());
            inside_code_fence = !inside_code_fence;
            previous_blank = false;
            continue;
        }

        if inside_code_fence {
            lines.push(line.trim_end().to_string());
            previous_blank = false;
            continue;
        }

        let trimmed = line.trim();
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

pub(super) fn parse_completion_response(
    value: &Value,
    source: &str,
    position: EditorPosition,
) -> Vec<EditorCompletionItem> {
    match value {
        Value::Array(items) => items
            .iter()
            .filter_map(|item| parse_completion_item(item, source, position))
            .collect(),
        Value::Object(map) => map
            .get("items")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| parse_completion_item(item, source, position))
                    .collect()
            })
            .unwrap_or_default(),
        _ => Vec::new(),
    }
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
            .map(|(language, value)| {
                let language = language.trim();
                if language.is_empty() {
                    format!("```\n{value}\n```")
                } else {
                    format!("```{language}\n{value}\n```")
                }
            })
            .or_else(|| map.get("value").and_then(Value::as_str).map(str::to_string)),
        _ => None,
    }
}

fn parse_completion_item(
    value: &Value,
    source: &str,
    position: EditorPosition,
) -> Option<EditorCompletionItem> {
    let map = value.as_object()?;
    let label = map.get("label")?.as_str()?.trim().to_string();
    if label.is_empty() {
        return None;
    }

    let detail = map
        .get("detail")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|detail| !detail.is_empty())
        .map(ToString::to_string);
    let preselected = map
        .get("preselect")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let insert_text_format = map
        .get("insertTextFormat")
        .and_then(Value::as_u64)
        .unwrap_or(1);

    let text_edit = map.get("textEdit");
    let (replace_start, replace_end, edit_text) = parse_completion_text_edit(text_edit, source)
        .unwrap_or_else(|| {
            let default_range = default_completion_range(source, position);
            let text = map
                .get("insertText")
                .and_then(Value::as_str)
                .unwrap_or(label.as_str())
                .to_string();
            (default_range.0, default_range.1, text)
        });
    let insert_text = if insert_text_format == 2 {
        normalize_completion_snippet(&edit_text)
    } else {
        edit_text
    };

    Some(EditorCompletionItem {
        label,
        detail,
        insert_text,
        replace_start,
        replace_end,
        preselected,
    })
}

fn parse_completion_text_edit(
    value: Option<&Value>,
    source: &str,
) -> Option<(EditorPosition, EditorPosition, String)> {
    let map = value?.as_object()?;
    let new_text = map.get("newText")?.as_str()?.to_string();
    let range = map
        .get("range")
        .or_else(|| map.get("replace"))
        .or_else(|| map.get("insert"))?;
    let (start, end) = parse_lsp_range(source, range)?;
    Some((start, end, new_text))
}

fn parse_lsp_range(source: &str, range: &Value) -> Option<(EditorPosition, EditorPosition)> {
    let start = range.get("start")?;
    let end = range.get("end")?;

    Some((
        EditorPosition {
            row: start.get("line")?.as_u64()? as usize,
            col: char_column_from_utf16(
                source,
                start.get("line")?.as_u64()? as usize,
                start.get("character")?.as_u64()? as usize,
            ),
        },
        EditorPosition {
            row: end.get("line")?.as_u64()? as usize,
            col: char_column_from_utf16(
                source,
                end.get("line")?.as_u64()? as usize,
                end.get("character")?.as_u64()? as usize,
            ),
        },
    ))
}

fn default_completion_range(
    source: &str,
    position: EditorPosition,
) -> (EditorPosition, EditorPosition) {
    let line = source.split('\n').nth(position.row).unwrap_or_default();
    let characters = line.chars().collect::<Vec<_>>();
    let mut start = position.col.min(characters.len());
    let mut end = position.col.min(characters.len());

    while start > 0 && is_completion_symbol_char(characters[start - 1]) {
        start -= 1;
    }
    while end < characters.len() && is_completion_symbol_char(characters[end]) {
        end += 1;
    }

    (
        EditorPosition {
            row: position.row,
            col: start,
        },
        EditorPosition {
            row: position.row,
            col: end,
        },
    )
}

fn is_completion_symbol_char(character: char) -> bool {
    character == '_' || character.is_alphanumeric()
}

fn normalize_completion_snippet(source: &str) -> String {
    let mut output = String::new();
    let mut characters = source.chars().peekable();

    while let Some(character) = characters.next() {
        match character {
            '\\' => {
                if let Some(next) = characters.next() {
                    output.push(next);
                }
            }
            '$' => match characters.peek().copied() {
                Some('{') => {
                    let _ = characters.next();
                    let mut placeholder = String::new();
                    while let Some(next) = characters.next() {
                        if next == '}' {
                            break;
                        }
                        placeholder.push(next);
                    }
                    output.push_str(&snippet_placeholder_text(&placeholder));
                }
                Some(next) if next.is_ascii_digit() => {
                    while characters
                        .peek()
                        .copied()
                        .is_some_and(|digit| digit.is_ascii_digit())
                    {
                        let _ = characters.next();
                    }
                }
                _ => output.push(character),
            },
            _ => output.push(character),
        }
    }

    output
}

fn snippet_placeholder_text(source: &str) -> String {
    if let Some((_, rest)) = source.split_once(':') {
        return normalize_completion_snippet(rest);
    }

    if let Some((_, rest)) = source.split_once('|') {
        let options = rest.trim_end_matches('|');
        return options.split(',').next().unwrap_or_default().to_string();
    }

    String::new()
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
