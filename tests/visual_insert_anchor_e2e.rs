use std::fs;
use std::path::Path;

use gargo::app::App;
use gargo::config::Config;
use gargo::core::editor::Editor;
use gargo::core::mode;
use gargo::input::action::{Action, AppAction, BufferAction, CoreAction};
use tempfile::tempdir;

fn test_config() -> Config {
    let mut config = Config::default();
    config.plugins.enabled.clear();
    config
}

#[test]
fn visual_to_insert_does_not_keep_selection_anchor() {
    let tmp = tempdir().expect("create temp dir");
    let file = tmp.path().join("anchor.txt");
    fs::write(&file, "abcd").expect("seed file");

    let editor = Editor::open(file.to_str().expect("utf-8 path"));
    let mut app = App::new(editor, test_config(), Some(Path::new(".")));

    // Build a visual selection from column 0 to 1.
    app.dispatch_action(Action::Core(CoreAction::ChangeMode(mode::Mode::Visual)));
    app.dispatch_action(Action::Core(CoreAction::MoveRight));

    // Enter insert, move once, then leave insert.
    app.dispatch_action(Action::Core(CoreAction::ChangeMode(mode::Mode::Insert)));
    app.dispatch_action(Action::Core(CoreAction::MoveRight));
    app.dispatch_action(Action::Core(CoreAction::ChangeMode(mode::Mode::Normal)));

    // If anchor leaked into insert mode, this would delete a range ("ab").
    app.dispatch_action(Action::Core(CoreAction::DeleteSelection));
    app.dispatch_action(Action::App(AppAction::Buffer(BufferAction::Save)));

    let saved = fs::read_to_string(&file).expect("read saved file");
    assert_eq!(saved, "acd");
}
