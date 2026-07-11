use anyhow::Result;
use serde_json::json;

use super::{
    DefinitionTarget, EditorCompletionItem, EditorPosition, LspSession, parse_completion_response,
    parse_definition_response, parse_hover_response, summarize_hover_text, utf16_column,
};

pub(super) fn hover(
    session: &mut LspSession,
    path: &std::path::Path,
    source: &str,
    position: EditorPosition,
) -> Result<Option<String>> {
    session.ensure_document_synced(path, source)?;
    let uri = super::file_uri(path)?;
    let result = session.request_value(
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

pub(super) fn definition(
    session: &mut LspSession,
    path: &std::path::Path,
    source: &str,
    position: EditorPosition,
) -> Result<Option<DefinitionTarget>> {
    session.ensure_document_synced(path, source)?;
    let uri = super::file_uri(path)?;
    let result = session.request_value(
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

pub(super) fn completion(
    session: &mut LspSession,
    path: &std::path::Path,
    source: &str,
    position: EditorPosition,
) -> Result<Vec<EditorCompletionItem>> {
    session.ensure_document_synced(path, source)?;
    let uri = super::file_uri(path)?;
    let result = session.request_value(
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
