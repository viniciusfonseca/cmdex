use super::*;
use serde_json::json;

#[test]
fn lsp_hover_parses_marked_string_objects() {
    let hover = json!({
        "contents": {
            "language": "rust",
            "value": "fn greet()"
        }
    });

    assert_eq!(
        super::lsp::parse_hover_response(&hover).as_deref(),
        Some("```rust\nfn greet()\n```")
    );
}

#[test]
fn lsp_hover_parses_hover_arrays() {
    let hover = json!({
        "contents": [
            { "value": "fn greet()" },
            "Returns a greeting"
        ]
    });

    assert_eq!(
        super::lsp::parse_hover_response(&hover).as_deref(),
        Some("fn greet()\n\nReturns a greeting")
    );
}

#[test]
fn lsp_hover_summary_preserves_line_breaks_and_strips_code_fences() {
    let hover = "```rust\nfn greet()\n```\n\nReturns a greeting";

    assert_eq!(
        super::lsp::summarize_hover_text(hover).as_deref(),
        Some("```rust\nfn greet()\n```\n\nReturns a greeting")
    );
}

#[test]
fn lsp_hover_summary_preserves_indented_code_blocks() {
    let hover = "Example:\n\n    fn greet(name: &str) -> String\n";

    assert_eq!(
        super::lsp::summarize_hover_text(hover).as_deref(),
        Some("Example:\n\n    fn greet(name: &str) -> String")
    );
}

#[test]
fn lsp_completion_parses_text_edits_and_snippets() {
    let completion = json!({
        "items": [{
            "label": "greet",
            "detail": "fn(&str) -> String",
            "preselect": true,
            "textEdit": {
                "range": {
                    "start": { "line": 0, "character": 1 },
                    "end": { "line": 0, "character": 4 }
                },
                "newText": "greet(${1:name})$0"
            },
            "insertTextFormat": 2
        }]
    });

    let items = super::lsp::parse_completion_response(
        &completion,
        "agre\n",
        EditorPosition { row: 0, col: 4 },
    );

    assert_eq!(items.len(), 1);
    assert_eq!(items[0].label, "greet");
    assert_eq!(items[0].detail.as_deref(), Some("fn(&str) -> String"));
    assert_eq!(items[0].insert_text, "greet(name)");
    assert_eq!(items[0].replace_start, EditorPosition { row: 0, col: 1 });
    assert_eq!(items[0].replace_end, EditorPosition { row: 0, col: 4 });
    assert!(items[0].preselected);
}

#[test]
fn lsp_utf16_columns_round_trip_through_unicode_text() {
    let source = "a😀b\n";
    let position = EditorPosition { row: 0, col: 2 };

    let utf16 = super::lsp::utf16_column(source, position);

    assert_eq!(utf16, 3);
    assert_eq!(super::lsp::char_column_from_utf16(source, 0, utf16), 2);
}

#[test]
fn lsp_definition_parser_converts_utf16_offsets_into_editor_columns() {
    let path = std::env::temp_dir().join(format!(
        "cmdex-lsp-definition-{}-{}.rs",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::write(&path, "a😀b\n").unwrap();

    let response = json!([{
        "uri": url::Url::from_file_path(&path).unwrap().to_string(),
        "range": {
            "start": {
                "line": 0,
                "character": 3
            },
            "end": {
                "line": 0,
                "character": 4
            }
        }
    }]);

    let target = super::lsp::parse_definition_response(&response)
        .unwrap()
        .unwrap();

    assert_eq!(target.path, path);
    assert_eq!(target.position, EditorPosition { row: 0, col: 2 });

    let _ = fs::remove_file(path);
}
