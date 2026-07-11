use anyhow::{Context, Result, anyhow};
use serde::Deserialize;
use serde_json::Value;
use std::{fs, path::PathBuf};
use url::Url;

use super::{DefinitionTarget, EditorCompletionItem, EditorPosition};

#[derive(Debug, Deserialize)]
struct HoverResponseDto {
    contents: Option<HoverContentsDto>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum HoverContentsDto {
    Text(String),
    Marked(MarkedStringDto),
    Many(Vec<HoverContentsDto>),
}

#[derive(Debug, Deserialize)]
struct MarkedStringDto {
    #[serde(default)]
    language: Option<String>,
    value: String,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum CompletionResponseDto {
    Items(Vec<CompletionItemDto>),
    List(CompletionListDto),
}

#[derive(Debug, Deserialize)]
struct CompletionListDto {
    #[serde(default)]
    items: Vec<CompletionItemDto>,
}

#[derive(Debug, Deserialize)]
struct CompletionItemDto {
    label: String,
    #[serde(default)]
    detail: Option<String>,
    #[serde(default)]
    preselect: bool,
    #[serde(rename = "insertTextFormat", default)]
    insert_text_format: Option<u64>,
    #[serde(rename = "insertText", default)]
    insert_text: Option<String>,
    #[serde(rename = "textEdit", default)]
    text_edit: Option<CompletionTextEditDto>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum CompletionTextEditDto {
    Range {
        range: LspRangeDto,
        #[serde(rename = "newText")]
        new_text: String,
    },
    InsertReplace {
        insert: LspRangeDto,
        replace: LspRangeDto,
        #[serde(rename = "newText")]
        new_text: String,
    },
}

#[derive(Debug, Deserialize)]
struct LspRangeDto {
    start: LspPositionDto,
    end: LspPositionDto,
}

#[derive(Debug, Deserialize)]
struct LspPositionDto {
    line: usize,
    character: usize,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum DefinitionResponseDto {
    One(DefinitionTargetDto),
    Many(Vec<DefinitionTargetDto>),
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum DefinitionTargetDto {
    Location(DefinitionLocationDto),
    Link(DefinitionLinkDto),
}

#[derive(Debug, Deserialize)]
struct DefinitionLocationDto {
    uri: String,
    range: LspRangeDto,
}

#[derive(Debug, Deserialize)]
struct DefinitionLinkDto {
    #[serde(rename = "targetUri")]
    target_uri: String,
    #[serde(rename = "targetSelectionRange", alias = "targetRange")]
    target_selection_range: LspRangeDto,
}

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
    let response: HoverResponseDto = serde_json::from_value(value.clone()).ok()?;
    hover_dto_to_text(response.contents?)
}

fn hover_dto_to_text(value: HoverContentsDto) -> Option<String> {
    match value {
        HoverContentsDto::Text(text) => Some(text),
        HoverContentsDto::Marked(marked) => {
            let language = marked.language.as_deref().unwrap_or_default().trim();
            if language.is_empty() {
                Some(marked.value)
            } else {
                Some(format!("```{language}\n{}\n```", marked.value))
            }
        }
        HoverContentsDto::Many(items) => {
            let joined = items
                .into_iter()
                .filter_map(hover_dto_to_text)
                .filter(|text| !text.trim().is_empty())
                .collect::<Vec<_>>()
                .join("\n\n");
            (!joined.is_empty()).then_some(joined)
        }
    }
}

pub fn parse_completion_response(
    value: &Value,
    source: &str,
    position: EditorPosition,
) -> Vec<EditorCompletionItem> {
    let response: CompletionResponseDto = match serde_json::from_value(value.clone()) {
        Ok(response) => response,
        Err(_) => return Vec::new(),
    };
    let items = match response {
        CompletionResponseDto::Items(items) => items,
        CompletionResponseDto::List(list) => list.items,
    };
    items
        .into_iter()
        .filter_map(|item| parse_completion_item(item, source, position))
        .collect()
}

fn parse_completion_item(
    item: CompletionItemDto,
    source: &str,
    position: EditorPosition,
) -> Option<EditorCompletionItem> {
    let CompletionItemDto {
        label: raw_label,
        detail: raw_detail,
        preselect,
        insert_text_format,
        insert_text,
        text_edit,
    } = item;
    let label = raw_label.trim().to_string();
    if label.is_empty() {
        return None;
    }

    let detail = raw_detail
        .as_deref()
        .map(str::trim)
        .filter(|detail| !detail.is_empty())
        .map(ToString::to_string);
    let (replace_start, replace_end, edit_text) = parse_completion_text_edit(text_edit, source)
        .unwrap_or_else(|| {
            let default_range = default_completion_range(source, position);
            let text = insert_text.unwrap_or_else(|| label.clone());
            (default_range.0, default_range.1, text)
        });
    let insert_text = if insert_text_format == Some(2) {
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
        preselected: preselect,
    })
}

fn parse_completion_text_edit(
    value: Option<CompletionTextEditDto>,
    source: &str,
) -> Option<(EditorPosition, EditorPosition, String)> {
    match value? {
        CompletionTextEditDto::Range { range, new_text } => {
            let (start, end) = parse_lsp_range(source, &range)?;
            Some((start, end, new_text))
        }
        CompletionTextEditDto::InsertReplace {
            insert,
            replace,
            new_text,
        } => {
            let (start, end) =
                parse_lsp_range(source, &replace).or_else(|| parse_lsp_range(source, &insert))?;
            Some((start, end, new_text))
        }
    }
}

fn parse_lsp_range(source: &str, range: &LspRangeDto) -> Option<(EditorPosition, EditorPosition)> {
    Some((
        EditorPosition {
            row: range.start.line,
            col: char_column_from_utf16(source, range.start.line, range.start.character),
        },
        EditorPosition {
            row: range.end.line,
            col: char_column_from_utf16(source, range.end.line, range.end.character),
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
    let Some(location) = parse_definition_location(value)? else {
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
pub(super) struct RawDefinitionLocation {
    pub(super) uri: String,
    pub(super) line: usize,
    pub(super) character: usize,
}

pub(super) fn parse_definition_location(value: &Value) -> Result<Option<RawDefinitionLocation>> {
    let response: DefinitionResponseDto = serde_json::from_value(value.clone())?;
    let target = match response {
        DefinitionResponseDto::One(target) => Some(target),
        DefinitionResponseDto::Many(targets) => targets.into_iter().next(),
    };
    Ok(target.map(|target| match target {
        DefinitionTargetDto::Location(location) => RawDefinitionLocation {
            uri: location.uri,
            line: location.range.start.line,
            character: location.range.start.character,
        },
        DefinitionTargetDto::Link(link) => RawDefinitionLocation {
            uri: link.target_uri,
            line: link.target_selection_range.start.line,
            character: link.target_selection_range.start.character,
        },
    }))
}

fn normalize_newlines(source: &str) -> String {
    source.replace("\r\n", "\n").replace('\r', "\n")
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

fn file_path_from_uri(uri: String) -> Result<PathBuf> {
    Url::parse(&uri)
        .context("invalid LSP file URI")?
        .to_file_path()
        .map_err(|_| anyhow!("definition target is not a local file"))
}
