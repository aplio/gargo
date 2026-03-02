use std::fs;

use gargo::core::editor::Editor;
use tempfile::tempdir;

#[test]
fn test_open_write_save_and_close_flow() {
    let temp = tempdir().expect("create temp dir");
    let file_path = temp.path().join("note.txt");
    fs::write(&file_path, "hello\n").expect("seed file");

    let mut editor = Editor::open(file_path.to_str().expect("utf-8 path"));

    // Open existing file and append text.
    editor.active_buffer_mut().move_to_line_end();
    editor.active_buffer_mut().insert_text(" world");

    let save_msg = editor.active_buffer_mut().save().expect("save edited file");
    assert!(save_msg.contains("Wrote"));

    let contents = fs::read_to_string(&file_path).expect("read back saved file");
    assert_eq!(contents, "hello world\n");

    // Close clean file buffer and ensure editor returns to a scratch buffer.
    editor.close_active_buffer().expect("close active buffer");
    assert_eq!(editor.buffer_count(), 1);
    assert_eq!(editor.active_buffer().display_name(), "[scratch]");
}
