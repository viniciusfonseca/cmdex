use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

use ratatui::text::Line;

use super::{
    browser::WorkspaceBrowserSupport,
    diff::{DiffHunk, GitDiffSupport},
    render::WorkspaceRenderer,
    *,
};
use crate::theme::ThemeRegistry;

#[test]
fn skips_git_subtree_entries() {
    assert!(WorkspaceBrowserSupport::contains_git_component(Path::new(
        "/tmp/repo/.git/config"
    )));
    assert!(WorkspaceBrowserSupport::contains_git_component(Path::new(
        "/tmp/repo/.git/objects/aa"
    )));
    assert!(!WorkspaceBrowserSupport::contains_git_component(Path::new(
        "/tmp/repo/src/main.rs"
    )));
}

#[test]
fn preserves_blank_lines_in_plain_preview() {
    let lines = WorkspaceRenderer::plain_preview_lines("first\n\nthird");
    assert_eq!(lines.len(), 3);
    assert_eq!(lines[0].spans[0].content, "first");
    assert!(lines[1].spans.is_empty() || lines[1].spans[0].content.is_empty());
    assert_eq!(lines[2].spans[0].content, "third");
}

#[test]
fn adds_line_numbers_to_blank_and_non_blank_lines() {
    let lines = WorkspaceRenderer::add_line_numbers(WorkspaceRenderer::plain_preview_lines(
        "first\n\nthird",
    ));

    assert_eq!(lines.len(), 3);
    assert_eq!(lines[0].spans[0].content, "1 | ");
    assert_eq!(lines[0].spans[1].content, "first");
    assert_eq!(lines[1].spans[0].content, "2 | ");
    assert_eq!(lines[2].spans[0].content, "3 | ");
    assert_eq!(lines[2].spans[1].content, "third");
}

#[test]
fn editor_inserts_text_and_saves() {
    let path = std::env::temp_dir().join(format!("cmdex-editor-save-{}.txt", std::process::id()));
    fs::write(&path, "hello\n").unwrap();

    let mut editor = WorkspaceEditorState::open(&path).unwrap();
    editor.move_line_end();
    editor.enter_insert_mode();
    editor.insert_char('!');
    editor.save().unwrap();

    assert_eq!(fs::read_to_string(&path).unwrap(), "hello!\n");

    let _ = fs::remove_file(path);
}

#[test]
fn editor_backspace_merges_lines() {
    let path = std::env::temp_dir().join(format!("cmdex-editor-merge-{}.txt", std::process::id()));
    fs::write(&path, "hello\nworld").unwrap();

    let mut editor = WorkspaceEditorState::open(&path).unwrap();
    editor.cursor_row = 1;
    editor.cursor_col = 0;
    editor.backspace();

    assert_eq!(editor.lines, vec!["helloworld".to_string()]);

    let _ = fs::remove_file(path);
}

#[test]
fn editor_refuses_quit_when_dirty_without_bang() {
    let path = std::env::temp_dir().join(format!("cmdex-editor-q-{}.txt", std::process::id()));
    fs::write(&path, "hello").unwrap();

    let mut editor = WorkspaceEditorState::open(&path).unwrap();
    editor.enter_insert_mode();
    editor.insert_char('!');
    editor.start_command();
    editor.push_command_char('q');

    let result = editor.execute_command().unwrap();

    assert!(!result.close);
    assert!(
        editor
            .status
            .as_deref()
            .is_some_and(|status| status.contains("Unsaved"))
    );

    let _ = fs::remove_file(path);
}

#[test]
fn editor_visual_selection_highlights_selected_text() {
    let path = std::env::temp_dir().join(format!("cmdex-editor-visual-{}.txt", std::process::id()));
    fs::write(&path, "hello").unwrap();

    let mut editor = WorkspaceEditorState::open(&path).unwrap();
    editor.enter_visual_mode();
    editor.extend_right();
    editor.extend_right();

    let lines = editor.rendered_lines(1);
    let selected = lines[0]
        .spans
        .iter()
        .filter(|span| span.style.bg == Some(ThemeRegistry::app().selection_bg))
        .map(|span| span.content.as_ref())
        .collect::<String>();

    assert_eq!(line_text(&lines[0]), "1 | hello");
    assert_eq!(selected, "he");

    let _ = fs::remove_file(path);
}

#[test]
fn editor_delete_selection_removes_selected_range_across_lines() {
    let path = std::env::temp_dir().join(format!(
        "cmdex-editor-delete-selection-{}.txt",
        std::process::id()
    ));
    fs::write(&path, "alpha\nbeta\ngamma").unwrap();

    let mut editor = WorkspaceEditorState::open(&path).unwrap();
    editor.move_right();
    editor.move_right();
    editor.enter_visual_mode();
    editor.extend_down();

    assert!(editor.delete_selection());
    assert_eq!(editor.lines, vec!["alta".to_string(), "gamma".to_string()]);
    assert_eq!(editor.cursor_row, 0);
    assert_eq!(editor.cursor_col, 2);
    assert!(matches!(editor.mode, EditorMode::Visual { .. }));

    let _ = fs::remove_file(path);
}

#[test]
fn editor_copy_selection_or_line_prefers_selection() {
    let path = std::env::temp_dir().join(format!(
        "cmdex-editor-copy-selection-{}.txt",
        std::process::id()
    ));
    fs::write(&path, "alpha\nbeta").unwrap();

    let mut editor = WorkspaceEditorState::open(&path).unwrap();
    editor.move_right();
    editor.move_right();
    editor.enter_visual_mode();
    editor.extend_down();

    assert_eq!(editor.copy_selection_or_line(), "pha\nbe");

    let _ = fs::remove_file(path);
}

#[test]
fn editor_copy_selection_or_line_falls_back_to_current_line() {
    let path =
        std::env::temp_dir().join(format!("cmdex-editor-copy-line-{}.txt", std::process::id()));
    fs::write(&path, "alpha\nbeta").unwrap();

    let mut editor = WorkspaceEditorState::open(&path).unwrap();
    editor.move_down();

    assert_eq!(editor.copy_selection_or_line(), "beta");

    let _ = fs::remove_file(path);
}

#[test]
fn editor_paste_text_inserts_multiline_content() {
    let path = std::env::temp_dir().join(format!(
        "cmdex-editor-paste-multi-{}.txt",
        std::process::id()
    ));
    fs::write(&path, "alpha\nomega").unwrap();

    let mut editor = WorkspaceEditorState::open(&path).unwrap();
    editor.move_right();
    editor.move_right();

    assert!(editor.paste_text("X\nY"));
    assert_eq!(
        editor.lines,
        vec!["alX".to_string(), "Ypha".to_string(), "omega".to_string()]
    );
    assert_eq!(editor.cursor_row, 1);
    assert_eq!(editor.cursor_col, 1);
    assert!(matches!(editor.mode, EditorMode::Normal));

    let _ = fs::remove_file(path);
}

#[test]
fn editor_paste_text_replaces_selection_and_exits_visual_mode() {
    let path = std::env::temp_dir().join(format!(
        "cmdex-editor-paste-selection-{}.txt",
        std::process::id()
    ));
    fs::write(&path, "hello").unwrap();

    let mut editor = WorkspaceEditorState::open(&path).unwrap();
    editor.move_right();
    editor.enter_visual_mode();
    editor.extend_right();
    editor.extend_right();
    editor.extend_right();

    assert!(editor.paste_text("abc"));
    assert_eq!(editor.lines, vec!["habco".to_string()]);
    assert_eq!(editor.cursor_row, 0);
    assert_eq!(editor.cursor_col, 4);
    assert!(matches!(editor.mode, EditorMode::Normal));
    assert!(!editor.has_selection());

    let _ = fs::remove_file(path);
}

#[test]
fn editor_completion_applies_selected_item_over_replace_range() {
    let path = std::env::temp_dir().join(format!(
        "cmdex-editor-completion-apply-{}.rs",
        std::process::id()
    ));
    fs::write(&path, "gre\n").unwrap();

    let mut editor = WorkspaceEditorState::open(&path).unwrap();
    editor.enter_insert_mode();
    editor.move_right();
    editor.move_right();
    editor.move_right();

    let position = EditorPosition { row: 0, col: 3 };
    editor.request_completion(position);
    assert!(editor.resolve_completion(
        position,
        vec![EditorCompletionItem {
            label: "greet".to_string(),
            detail: Some("fn(&str) -> String".to_string()),
            insert_text: "greet(name)".to_string(),
            replace_start: EditorPosition { row: 0, col: 0 },
            replace_end: EditorPosition { row: 0, col: 3 },
            preselected: true,
        }]
    ));

    assert!(editor.apply_selected_completion());
    assert_eq!(
        editor.lines,
        vec!["greet(name)".to_string(), "".to_string()]
    );
    assert_eq!(editor.cursor_row, 0);
    assert_eq!(editor.cursor_col, 11);
    assert!(matches!(editor.mode, EditorMode::Insert));
    assert!(editor.completion_popover().is_none());

    let _ = fs::remove_file(path);
}

#[test]
fn editor_completion_prefers_preselected_item() {
    let path = std::env::temp_dir().join(format!(
        "cmdex-editor-completion-preselect-{}.rs",
        std::process::id()
    ));
    fs::write(&path, "gre\n").unwrap();

    let mut editor = WorkspaceEditorState::open(&path).unwrap();
    let position = EditorPosition { row: 0, col: 3 };
    editor.request_completion(position);
    assert!(editor.resolve_completion(
        position,
        vec![
            EditorCompletionItem {
                label: "green".to_string(),
                detail: None,
                insert_text: "green".to_string(),
                replace_start: EditorPosition { row: 0, col: 0 },
                replace_end: EditorPosition { row: 0, col: 3 },
                preselected: false,
            },
            EditorCompletionItem {
                label: "greet".to_string(),
                detail: None,
                insert_text: "greet".to_string(),
                replace_start: EditorPosition { row: 0, col: 0 },
                replace_end: EditorPosition { row: 0, col: 3 },
                preselected: true,
            },
        ]
    ));

    let (_, selected, _) = editor.completion_popover().expect("completion popover");
    assert_eq!(selected, 1);

    let _ = fs::remove_file(path);
}

#[test]
fn editor_undo_reverts_latest_change_and_restores_clean_state() {
    let path = std::env::temp_dir().join(format!(
        "cmdex-editor-undo-clean-{}.txt",
        std::process::id()
    ));
    fs::write(&path, "hello").unwrap();

    let mut editor = WorkspaceEditorState::open(&path).unwrap();
    editor.enter_insert_mode();
    editor.insert_char('!');

    assert!(editor.dirty);
    assert!(editor.undo());
    assert_eq!(editor.lines, vec!["hello".to_string()]);
    assert_eq!(editor.cursor_row, 0);
    assert_eq!(editor.cursor_col, 0);
    assert!(matches!(editor.mode, EditorMode::Normal));
    assert!(!editor.dirty);

    let _ = fs::remove_file(path);
}

#[test]
fn editor_undo_reverts_multiline_paste() {
    let path = std::env::temp_dir().join(format!(
        "cmdex-editor-undo-paste-{}.txt",
        std::process::id()
    ));
    fs::write(&path, "alpha\nomega").unwrap();

    let mut editor = WorkspaceEditorState::open(&path).unwrap();
    editor.move_right();
    editor.move_right();
    editor.paste_text("X\nY");

    assert!(editor.undo());
    assert_eq!(editor.lines, vec!["alpha".to_string(), "omega".to_string()]);
    assert_eq!(editor.cursor_row, 0);
    assert_eq!(editor.cursor_col, 2);
    assert!(matches!(editor.mode, EditorMode::Normal));

    let _ = fs::remove_file(path);
}

#[test]
fn editor_undo_reports_when_history_is_empty() {
    let path = std::env::temp_dir().join(format!(
        "cmdex-editor-undo-empty-{}.txt",
        std::process::id()
    ));
    fs::write(&path, "hello").unwrap();

    let mut editor = WorkspaceEditorState::open(&path).unwrap();

    assert!(!editor.undo());
    assert_eq!(editor.status.as_deref(), Some("Nothing to undo"));

    let _ = fs::remove_file(path);
}

#[test]
fn selecting_a_file_auto_opens_editor_in_normal_mode() {
    let path =
        std::env::temp_dir().join(format!("cmdex-editor-auto-open-{}.txt", std::process::id()));
    fs::write(&path, "hello").unwrap();

    let mut browser = FileBrowserState::with_entries(vec![FileEntry {
        path: path.clone(),
        relative_path: PathBuf::from("hello.txt"),
    }]);

    browser.select(0);

    let editor = browser.editor.as_ref().expect("editor");
    assert_eq!(editor.path, path);
    assert!(matches!(editor.mode, EditorMode::Normal));

    let _ = fs::remove_file(path);
}

#[test]
fn dirty_editor_blocks_switching_selected_file() {
    let first = std::env::temp_dir().join(format!("cmdex-editor-first-{}.txt", std::process::id()));
    let second =
        std::env::temp_dir().join(format!("cmdex-editor-second-{}.txt", std::process::id()));
    fs::write(&first, "first").unwrap();
    fs::write(&second, "second").unwrap();

    let mut browser = FileBrowserState::with_entries(vec![
        FileEntry {
            path: first.clone(),
            relative_path: PathBuf::from("first.txt"),
        },
        FileEntry {
            path: second.clone(),
            relative_path: PathBuf::from("second.txt"),
        },
    ]);

    browser.select(0);
    browser.editor.as_mut().unwrap().insert_char('!');
    browser.select(1);

    assert_eq!(browser.selected, 0);
    assert_eq!(browser.editor.as_ref().unwrap().path, first);
    assert!(
        browser
            .editor
            .as_ref()
            .and_then(|editor| editor.status.as_deref())
            .is_some_and(|status| status.contains("Unsaved changes"))
    );

    let _ = fs::remove_file(first);
    let _ = fs::remove_file(second);
}

#[test]
fn builds_workspace_sidebar_tree_rows_from_files() {
    let entries = vec![
        FileEntry {
            path: PathBuf::from("/tmp/repo/README.md"),
            relative_path: PathBuf::from("README.md"),
        },
        FileEntry {
            path: PathBuf::from("/tmp/repo/src/app.rs"),
            relative_path: PathBuf::from("src/app.rs"),
        },
        FileEntry {
            path: PathBuf::from("/tmp/repo/src/ui/mod.rs"),
            relative_path: PathBuf::from("src/ui/mod.rs"),
        },
    ];

    let (rows, file_rows) =
        WorkspaceBrowserSupport::build_file_tree_rows(&entries, &BTreeSet::new());

    assert_eq!(
        rows.iter()
            .map(|row| row.label.as_str())
            .collect::<Vec<_>>(),
        vec![
            " ├─ ▾ src",
            " │  ├─ ▾ ui",
            " │  │  └─ mod.rs",
            " │  └─ app.rs",
            " └─ README.md",
        ]
    );
    assert_eq!(file_rows, vec![Some(4), Some(3), Some(2)]);
    assert!(rows[1].directory_path().is_some());
    assert_eq!(rows[3].file_index(), Some(1));
}

#[test]
fn hides_descendants_for_collapsed_directory_rows() {
    let entries = vec![
        FileEntry {
            path: PathBuf::from("/tmp/repo/src/app.rs"),
            relative_path: PathBuf::from("src/app.rs"),
        },
        FileEntry {
            path: PathBuf::from("/tmp/repo/src/ui/mod.rs"),
            relative_path: PathBuf::from("src/ui/mod.rs"),
        },
    ];
    let collapsed = BTreeSet::from([PathBuf::from("src")]);

    let (rows, file_rows) = WorkspaceBrowserSupport::build_file_tree_rows(&entries, &collapsed);

    assert_eq!(
        rows.iter()
            .map(|row| row.label.as_str())
            .collect::<Vec<_>>(),
        vec![" └─ ▸ src"]
    );
    assert_eq!(file_rows, vec![None, None]);
}

#[test]
fn workspace_tree_starts_collapsed_by_default() {
    let root = std::env::temp_dir().join(format!(
        "cmdex-workspace-collapsed-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(root.join("src/ui")).unwrap();
    fs::write(root.join("src/app.rs"), "").unwrap();
    fs::write(root.join("src/ui/mod.rs"), "").unwrap();

    let mut browser = FileBrowserState::default();
    browser
        .apply_scanned_entries_without_io(FileBrowserState::scan_entries(&root).unwrap())
        .unwrap();

    assert_eq!(
        browser
            .tree_rows
            .iter()
            .map(|row| row.label.as_str())
            .collect::<Vec<_>>(),
        vec![" └─ ▸ src"]
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn applying_scanned_entries_preserves_selected_file_and_updates_tree_rows() {
    let root = std::env::temp_dir().join(format!(
        "cmdex-workspace-refresh-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&root).unwrap();
    let selected = root.join("b.txt");
    let added = root.join("a.txt");
    fs::write(&selected, "b").unwrap();

    let mut browser = FileBrowserState::default();
    browser
        .apply_scanned_entries_without_io(FileBrowserState::scan_entries(&root).unwrap())
        .unwrap();

    assert_eq!(browser.entries()[browser.selected].path, selected);

    fs::write(&added, "a").unwrap();
    browser
        .apply_scanned_entries_without_io(FileBrowserState::scan_entries(&root).unwrap())
        .unwrap();

    assert_eq!(browser.entries()[browser.selected].path, selected);
    assert!(
        browser
            .tree_rows
            .iter()
            .map(|row| row.label.as_str())
            .any(|label| label.contains("a.txt"))
    );

    fs::remove_file(&added).unwrap();
    browser
        .apply_scanned_entries_without_io(FileBrowserState::scan_entries(&root).unwrap())
        .unwrap();

    assert!(
        !browser
            .tree_rows
            .iter()
            .map(|row| row.label.as_str())
            .any(|label| label.contains("a.txt"))
    );

    let _ = fs::remove_file(selected);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn stale_workspace_search_results_do_not_replace_newer_query() {
    let root = std::env::temp_dir().join(format!(
        "cmdex-workspace-search-generation-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&root).unwrap();
    let path = root.join("source.txt");
    fs::write(&path, "alpha\nbeta\n").unwrap();

    let mut browser = FileBrowserState::with_entries(vec![FileEntry {
        path: path.clone(),
        relative_path: PathBuf::from("source.txt"),
    }]);
    browser.push_search_char('a');
    let first_generation = browser.search_generation();
    browser.push_search_char('b');
    let second_generation = browser.search_generation();

    let first = FileBrowserState::search_entries(browser.entries(), "a").unwrap();
    assert!(!browser.apply_search_snapshot(first_generation, "a", first));
    assert_eq!(browser.search_total_rows(), 0);

    let second = FileBrowserState::search_entries(browser.entries(), "ab").unwrap();
    assert!(browser.apply_search_snapshot(second_generation, "ab", second));
    assert_eq!(browser.search_match_count(), 0);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn editor_rendered_lines_are_limited_to_visible_viewport() {
    let path =
        std::env::temp_dir().join(format!("cmdex-editor-viewport-{}.txt", std::process::id()));
    fs::write(&path, "one\ntwo\nthree\nfour").unwrap();

    let mut editor = WorkspaceEditorState::open(&path).unwrap();
    editor.vertical_scroll = 1;

    let lines = editor.rendered_lines(2);

    assert_eq!(lines.len(), 2);
    assert_eq!(line_text(&lines[0]), "2 | two");
    assert_eq!(line_text(&lines[1]), "3 | three");

    let _ = fs::remove_file(path);
}

#[test]
fn editor_rendered_lines_clamp_out_of_bounds_scroll() {
    let path = std::env::temp_dir().join(format!("cmdex-editor-scroll-{}.txt", std::process::id()));
    fs::write(&path, "one\ntwo\nthree\nfour").unwrap();

    let mut editor = WorkspaceEditorState::open(&path).unwrap();
    editor.vertical_scroll = 99;

    let lines = editor.rendered_lines(2);

    assert_eq!(lines.len(), 2);
    assert_eq!(line_text(&lines[0]), "3 | three");
    assert_eq!(line_text(&lines[1]), "4 | four");

    let _ = fs::remove_file(path);
}

#[test]
fn editor_render_cache_updates_after_insertions() {
    let path = std::env::temp_dir().join(format!("cmdex-editor-cache-{}.txt", std::process::id()));
    fs::write(&path, "hello").unwrap();

    let mut editor = WorkspaceEditorState::open(&path).unwrap();
    editor.move_line_end();
    editor.enter_insert_mode();
    editor.insert_char('!');

    let lines = editor.rendered_lines(1);

    assert_eq!(line_text(&lines[0]), "1 | hello!");

    let _ = fs::remove_file(path);
}

#[test]
fn splits_git_status_into_changes_and_staged_sections() {
    let root = Path::new("/tmp/repo");
    let output = "\
MM src/app.rs
A  src/new.rs
 M src/dirty.rs
?? README.md
R  src/old.rs -> src/new_name.rs
";

    let sections = GitDiffSupport::parse_status_sections(root, output);

    assert_eq!(
        sections
            .changes
            .iter()
            .map(|entry| entry.label.as_str())
            .collect::<Vec<_>>(),
        vec!["[??] README.md", "[M] src/app.rs", "[M] src/dirty.rs"]
    );
    assert_eq!(
        sections
            .staged
            .iter()
            .map(|entry| entry.label.as_str())
            .collect::<Vec<_>>(),
        vec!["[M] src/app.rs", "[A] src/new.rs", "[R] src/new_name.rs"]
    );
}

#[test]
fn git_remote_action_state_blocks_concurrent_actions_and_clears_on_failure() {
    let mut diff = DiffBrowserState::default();

    diff.begin_remote_action(GitRemoteAction::Push).unwrap();
    assert_eq!(diff.remote_action, Some(GitRemoteAction::Push));
    assert!(diff.begin_remote_action(GitRemoteAction::Pull).is_err());

    diff.complete_remote_action(
        Path::new("/tmp"),
        GitRemoteAction::Push,
        false,
        "push failed".to_string(),
    );

    assert_eq!(diff.remote_action, None);
    assert_eq!(diff.error.as_deref(), Some("push failed"));
}

#[test]
fn git_refresh_generation_ignores_stale_snapshots() {
    let mut diff = DiffBrowserState::default();
    let first_generation = diff.begin_refresh();
    let second_generation = diff.begin_refresh();
    let result = GitDiffLoadResult {
        changes: Vec::new(),
        staged: Vec::new(),
        active_section: DiffSection::Changes,
        selected_path: None,
        preview_title: "Git Diff".to_string(),
        preview: vec![Line::from("latest")],
    };

    assert!(!diff.apply_load_result(first_generation, Some(result.clone()), None,));
    assert!(diff.refresh_in_flight);
    assert!(diff.apply_load_result(second_generation, Some(result), None));
    assert!(!diff.refresh_in_flight);
    assert_eq!(diff.preview[0].spans[0].content, "latest");
}

#[test]
fn parses_unified_diff_hunks_and_removed_lines() {
    let diff = "\
@@ -2,2 +2,3 @@
-old one
-old two
+new one
+new two
+new three
";

    let hunks = GitDiffSupport::parse_unified_diff_hunks(diff);

    assert_eq!(hunks.len(), 1);
    assert_eq!(hunks[0].old_start, 2);
    assert_eq!(hunks[0].old_count, 2);
    assert_eq!(hunks[0].new_start, 2);
    assert_eq!(hunks[0].new_count, 3);
    assert_eq!(hunks[0].removed_lines, vec!["old one", "old two"]);
}

#[test]
fn renders_removed_lines_inside_full_file_diff_preview() {
    let hunks = vec![DiffHunk {
        old_start: 2,
        old_count: 1,
        new_start: 2,
        new_count: 1,
        removed_lines: vec!["old value".to_string()],
    }];

    let lines = GitDiffSupport::render_modified_diff_preview(
        Path::new("example.txt"),
        "one\nnew value\nthree",
        &hunks,
    );

    assert_eq!(line_text(&lines[0]), "1   | one");
    assert_eq!(line_text(&lines[1]), "2 - | old value");
    assert_eq!(line_text(&lines[2]), "2 ~ | new value");
    assert_eq!(line_text(&lines[3]), "3   | three");
    assert!(lines[0].style.bg.is_none());
    assert!(lines[1].style.bg.is_some());
    assert!(lines[2].style.bg.is_some());
}

fn line_text(line: &Line<'_>) -> String {
    line.spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>()
}
