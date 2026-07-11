use anyhow::{Context, Result, anyhow};
use serde_json::Value;
use std::{fs, path::PathBuf};
use url::Url;

use super::{DefinitionTarget, EditorCompletionItem, EditorPosition};

pub fn summarize_hover_text(text: &str) -> Option<String> {
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

        let trimmed_end = line.trim_end();
        if trimmed_end.trim().is_empty() {
            if !previous_blank && !lines.is_empty() {
                lines.push(String::new());
            }
            previous_blank = true;
            continue;
        }

        lines.push(trimmed_end.to_string());
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

pub fn parse_hover_response(value: &Value) -> Option<String> {
    hover_value_to_text(value.get("contents")?)
}

pub fn parse_completion_response(
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
                    for next in characters.by_ref() {
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

pub fn parse_definition_response(value: &Value) -> Result<Option<DefinitionTarget>> {
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

pub fn utf16_column(source: &str, position: EditorPosition) -> usize {
    source
        .split('\n')
        .nth(position.row)
        .unwrap_or_default()
        .chars()
        .take(position.col)
        .map(char::len_utf16)
        .sum()
}

pub fn char_column_from_utf16(source: &str, row: usize, utf16_col: usize) -> usize {
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
