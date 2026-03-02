use std::fs;

use gargo::core::editor::Editor;
use gargo::core::mode::Mode;
use tempfile::tempdir;

#[test]
fn test_visual_yank_then_paste_preserves_lf_as_line_breaks() {
    let temp = tempdir().expect("create temp dir");
    let file_path = temp.path().join("visual_yank.txt");

    // 4 lines so starting at line 2 and pressing x 3 times still selects 3 lines.
    fs::write(&file_path, "one\ntwo\nthree\nfour\n").expect("seed file");

    let mut editor = Editor::open(file_path.to_str().expect("utf-8 path"));

    // 1) Go to second line.
    editor.active_buffer_mut().move_down();

    // 2) Enter visual mode, then x x x (extend line selection downward).
    editor.mode = Mode::Visual;
    editor.active_buffer_mut().set_anchor();
    editor.active_buffer_mut().extend_line_selection_down();
    editor.active_buffer_mut().extend_line_selection_down();
    editor.active_buffer_mut().extend_line_selection_down();

    // 3) Yank to clipboard/register.
    let yanked = editor
        .active_buffer()
        .selection_text()
        .expect("selection should exist");
    editor.register = Some(yanked.clone());
    editor.active_buffer_mut().clear_anchor();
    editor.mode = Mode::Normal;

    // 4) Go to end of file.
    editor.active_buffer_mut().move_to_file_end();

    // 5) Paste from clipboard/register.
    let pasted = editor
        .register
        .clone()
        .expect("register should be populated");
    let pos = editor.active_buffer().cursors[0];
    editor.active_buffer_mut().insert_text_at(pos, &pasted);

    // 6) Assert LF is preserved as line breaks (not collapsed).
    editor.active_buffer_mut().save().expect("save file");
    let contents = fs::read_to_string(&file_path).expect("read saved file");

    let expected = "one\ntwo\nthree\nfour\ntwo\nthree\nfour\n";
    assert_eq!(contents, expected);
    assert!(contents.contains("\ntwo\nthree\nfour"));
    assert!(!contents.contains("twothreefour"));
}
