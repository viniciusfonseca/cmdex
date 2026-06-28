use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

use ratatui::text::Line;

use super::{
    browser::{build_file_tree_rows, contains_git_component},
    diff::{
        DiffHunk, parse_status_sections, parse_unified_diff_hunks, render_modified_diff_preview,
    },
    render::{add_line_numbers, plain_preview_lines},
    *,
};

#[test]
fn skips_git_subtree_entries() {
    assert!(contains_git_component(Path::new("/tmp/repo/.git/config")));
    assert!(contains_git_component(Path::new(
        "/tmp/repo/.git/objects/aa"
    )));
    assert!(!contains_git_component(Path::new("/tmp/repo/src/main.rs")));
}

#[test]
fn preserves_blank_lines_in_plain_preview() {
    let lines = plain_preview_lines("first\n\nthird");
    assert_eq!(lines.len(), 3);
    assert_eq!(lines[0].spans[0].content, "first");
    assert!(lines[1].spans.is_empty() || lines[1].spans[0].content.is_empty());
    assert_eq!(lines[2].spans[0].content, "third");
}

#[test]
fn adds_line_numbers_to_blank_and_non_blank_lines() {
    let lines = add_line_numbers(plain_preview_lines("first\n\nthird"));

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
    editor.mode = EditorMode::Command;
    editor.command = "q".to_string();

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
fn selecting_a_file_auto_opens_editor_in_normal_mode() {
    let path =
        std::env::temp_dir().join(format!("cmdex-editor-auto-open-{}.txt", std::process::id()));
    fs::write(&path, "hello").unwrap();

    let mut browser = FileBrowserState {
        entries: vec![FileEntry {
            path: path.clone(),
            relative_path: PathBuf::from("hello.txt"),
        }],
        ..Default::default()
    };

    browser.select(0);

    let editor = browser.editor.as_ref().expect("editor");
    assert_eq!(editor.path, path);
    assert_eq!(editor.mode, EditorMode::Normal);

    let _ = fs::remove_file(path);
}

#[test]
fn dirty_editor_blocks_switching_selected_file() {
    let first = std::env::temp_dir().join(format!("cmdex-editor-first-{}.txt", std::process::id()));
    let second =
        std::env::temp_dir().join(format!("cmdex-editor-second-{}.txt", std::process::id()));
    fs::write(&first, "first").unwrap();
    fs::write(&second, "second").unwrap();

    let mut browser = FileBrowserState {
        entries: vec![
            FileEntry {
                path: first.clone(),
                relative_path: PathBuf::from("first.txt"),
            },
            FileEntry {
                path: second.clone(),
                relative_path: PathBuf::from("second.txt"),
            },
        ],
        ..Default::default()
    };

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

    let (rows, file_rows) = build_file_tree_rows(&entries, &BTreeSet::new());

    assert_eq!(
        rows.iter()
            .map(|row| row.label.as_str())
            .collect::<Vec<_>>(),
        vec![
            "├── ▾ src",
            "│   ├── ▾ ui",
            "│   │   └── mod.rs",
            "│   └── app.rs",
            "└── README.md",
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

    let (rows, file_rows) = build_file_tree_rows(&entries, &collapsed);

    assert_eq!(
        rows.iter()
            .map(|row| row.label.as_str())
            .collect::<Vec<_>>(),
        vec!["└── ▸ src"]
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
    browser.refresh(&root);

    assert_eq!(
        browser
            .tree_rows
            .iter()
            .map(|row| row.label.as_str())
            .collect::<Vec<_>>(),
        vec!["└── ▸ src"]
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn refresh_if_changed_preserves_selected_file_and_updates_tree_rows() {
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
    browser.refresh(&root);

    assert_eq!(browser.entries[browser.selected].path, selected);

    fs::write(&added, "a").unwrap();
    browser.refresh_if_changed(&root);

    assert_eq!(browser.entries[browser.selected].path, selected);
    assert!(
        browser
            .sidebar_labels()
            .iter()
            .any(|label| label.contains("a.txt"))
    );

    fs::remove_file(&added).unwrap();
    browser.refresh_if_changed(&root);

    assert!(
        !browser
            .sidebar_labels()
            .iter()
            .any(|label| label.contains("a.txt"))
    );

    let _ = fs::remove_file(selected);
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

    let sections = parse_status_sections(root, output);

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
fn parses_unified_diff_hunks_and_removed_lines() {
    let diff = "\
@@ -2,2 +2,3 @@
-old one
-old two
+new one
+new two
+new three
";

    let hunks = parse_unified_diff_hunks(diff);

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

    let lines =
        render_modified_diff_preview(Path::new("example.txt"), "one\nnew value\nthree", &hunks);

    assert_eq!(line_text(&lines[0]), "1   | one");
    assert_eq!(line_text(&lines[1]), "2 - | old value");
    assert_eq!(line_text(&lines[2]), "2 ~ | new value");
    assert_eq!(line_text(&lines[3]), "3   | three");
}

fn line_text(line: &Line<'_>) -> String {
    line.spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>()
}
