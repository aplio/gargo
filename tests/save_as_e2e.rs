use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use gargo::app::App;
use gargo::command::registry::{CommandContext, CommandEffect, CommandRegistry, register_builtins};
use gargo::config::Config;
use gargo::core::editor::Editor;
use gargo::input::action::{Action, AppAction, BufferAction, CoreAction};
use gargo::ui::framework::component::EventResult;
use gargo::ui::overlays::project::save_as_popup::SaveAsPopup;
use std::fs;
use tempfile::tempdir;

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

#[test]
fn save_as_popup_enter_dispatches_default_path() {
    let tmp = tempdir().expect("create temp dir");
    let root = tmp.path().to_path_buf();
    let default = root.join("note.txt").to_string_lossy().to_string();

    let mut popup = SaveAsPopup::new(default.clone(), root);
    let result = popup.handle_key(key(KeyCode::Enter));
    match result {
        EventResult::Action(Action::App(AppAction::Buffer(BufferAction::SaveBufferAs(path)))) => {
            assert_eq!(path, default);
        }
        _ => panic!("expected SaveBufferAs action"),
    }
}

#[test]
fn save_as_command_maps_to_open_popup_action() {
    let mut registry = CommandRegistry::new();
    register_builtins(&mut registry);
    let command = registry
        .commands()
        .iter()
        .find(|entry| entry.id == "file.save_as")
        .expect("file.save_as exists");

    let context_editor = Editor::new();
    let context = CommandContext::new(&context_editor);
    let result = (command.action)(&context);
    assert!(matches!(
        result,
        CommandEffect::Action(Action::App(AppAction::Buffer(
            BufferAction::OpenSaveBufferAsPopup
        )))
    ));
}

#[test]
fn save_as_action_creates_parent_dirs_and_writes_file() {
    let tmp = tempdir().expect("create temp dir");
    fs::create_dir(tmp.path().join(".git")).expect("create git marker");

    let mut config = Config::default();
    config.plugins.enabled.clear();
    let mut app = App::new(Editor::new(), config, Some(tmp.path()));
    assert!(!app.dispatch_action(Action::Core(CoreAction::InsertText(
        "hello save as".to_string(),
    ))));

    assert!(
        !app.dispatch_action(Action::App(AppAction::Buffer(BufferAction::SaveBufferAs(
            "nested/dir/file.txt".to_string(),
        ))))
    );

    let written = tmp.path().join("nested").join("dir").join("file.txt");
    assert_eq!(
        fs::read_to_string(written).expect("read saved file"),
        "hello save as"
    );
}

#[test]
fn save_as_action_overwrites_existing_file() {
    let tmp = tempdir().expect("create temp dir");
    fs::create_dir(tmp.path().join(".git")).expect("create git marker");
    let target = tmp.path().join("overwrite.txt");
    fs::write(&target, "old").expect("write seed file");

    let mut config = Config::default();
    config.plugins.enabled.clear();
    let mut app = App::new(Editor::new(), config, Some(tmp.path()));
    assert!(!app.dispatch_action(Action::Core(CoreAction::InsertText("new".to_string(),))));

    assert!(
        !app.dispatch_action(Action::App(AppAction::Buffer(BufferAction::SaveBufferAs(
            target.to_string_lossy().to_string(),
        ))))
    );

    assert_eq!(fs::read_to_string(target).expect("read target"), "new");
}

#[test]
fn save_as_popup_accepts_relative_input() {
    let tmp = tempdir().expect("create temp dir");
    let root = tmp.path().to_path_buf();
    let mut popup = SaveAsPopup::new(String::new(), root);

    popup.handle_key(key(KeyCode::Char('n')));
    popup.handle_key(key(KeyCode::Char('o')));
    popup.handle_key(key(KeyCode::Char('t')));
    popup.handle_key(key(KeyCode::Char('e')));
    popup.handle_key(key(KeyCode::Char('s')));
    popup.handle_key(key(KeyCode::Char('/')));
    popup.handle_key(key(KeyCode::Char('a')));
    popup.handle_key(key(KeyCode::Char('.')));
    popup.handle_key(key(KeyCode::Char('m')));
    popup.handle_key(key(KeyCode::Char('d')));

    let result = popup.handle_key(key(KeyCode::Enter));
    match result {
        EventResult::Action(Action::App(AppAction::Buffer(BufferAction::SaveBufferAs(path)))) => {
            assert_eq!(path, "notes/a.md");
        }
        _ => panic!("expected SaveBufferAs action"),
    }
}

#[test]
fn save_as_popup_rejects_directory_path() {
    let tmp = tempdir().expect("create temp dir");
    let root = tmp.path().to_path_buf();
    let mut popup = SaveAsPopup::new(root.to_string_lossy().to_string(), root.clone());

    let result = popup.handle_key(key(KeyCode::Enter));
    assert!(matches!(result, EventResult::Consumed));
}
