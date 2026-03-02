use std::fs;

use gargo::core::editor::Editor;
use gargo::core::mode::Mode;
use tempfile::tempdir;

#[test]
fn test_paste_with_lf_in_insert_mode_creates_multiple_lines() {
    let temp = tempdir().expect("create temp dir");
    let file_path = temp.path().join("paste_target.txt");
    fs::write(&file_path, "").expect("seed empty file");

    let mut editor = Editor::open(file_path.to_str().expect("utf-8 path"));

    // Match App::run paste behavior: Event::Paste(text) only inserts in Insert mode.
    editor.mode = Mode::Insert;
    editor.active_buffer_mut().insert_text("a\nb");

    editor.active_buffer_mut().save().expect("save pasted text");

    let contents = fs::read_to_string(&file_path).expect("read saved file");
    assert_eq!(contents, "a\nb");
    assert!(contents.contains('\n'));
    assert_ne!(contents, "ab");
}

#[test]
fn test_paste_with_crlf_in_insert_mode_normalizes_to_lf() {
    let temp = tempdir().expect("create temp dir");
    let file_path = temp.path().join("paste_target_crlf.txt");
    fs::write(&file_path, "").expect("seed empty file");

    let mut editor = Editor::open(file_path.to_str().expect("utf-8 path"));
    editor.mode = Mode::Insert;
    editor
        .active_buffer_mut()
        .insert_text("flowchart TB\r\n    Start[hourly_flow start]");
    editor.active_buffer_mut().save().expect("save pasted text");

    let contents = fs::read_to_string(&file_path).expect("read saved file");
    assert_eq!(contents, "flowchart TB\n    Start[hourly_flow start]");
    assert!(contents.contains('\n'));
    assert!(!contents.contains('\r'));
}

#[test]
fn test_paste_with_bare_cr_in_insert_mode_normalizes_to_lf() {
    let temp = tempdir().expect("create temp dir");
    let file_path = temp.path().join("paste_target_cr.txt");
    fs::write(&file_path, "").expect("seed empty file");

    let mut editor = Editor::open(file_path.to_str().expect("utf-8 path"));
    editor.mode = Mode::Insert;
    editor.active_buffer_mut().insert_text("a\rb");
    editor.active_buffer_mut().save().expect("save pasted text");

    let contents = fs::read_to_string(&file_path).expect("read saved file");
    assert_eq!(contents, "a\nb");
    assert!(contents.contains('\n'));
    assert!(!contents.contains('\r'));
}
