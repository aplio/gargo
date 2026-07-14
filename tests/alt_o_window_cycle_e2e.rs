use std::path::Path;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use gargo::app::App;
use gargo::config::Config;
use gargo::core::editor::Editor;
use gargo::core::mode::Mode;
use gargo::input::action::{Action, AppAction, WindowAction, WindowSplitAxis};
use gargo::input::chord::KeyState;
use gargo::input::keymap::resolve;

fn test_config() -> Config {
    let mut config = Config::default();
    config.plugins.enabled.clear();
    config
}

fn alt_o() -> KeyEvent {
    KeyEvent::new(KeyCode::Char('o'), KeyModifiers::ALT)
}

fn dispatch_key(app: &mut App, state: &mut KeyState, mode: Mode, key: KeyEvent) {
    let action = resolve(key, state, &mode, false);
    app.dispatch_action(action);
}

#[test]
fn alt_o_cycles_focus_between_split_windows() {
    let mut app = App::new(Editor::new(), test_config(), Some(Path::new(".")));
    let mut state = KeyState::Normal;

    app.dispatch_action(Action::App(AppAction::Window(WindowAction::WindowSplit(
        WindowSplitAxis::Vertical,
    ))));
    let after_split = app.editor().active_buffer().id;

    dispatch_key(&mut app, &mut state, Mode::Normal, alt_o());
    let after_first_cycle = app.editor().active_buffer().id;
    assert_ne!(
        after_split, after_first_cycle,
        "Alt+O should move focus to the other window"
    );

    dispatch_key(&mut app, &mut state, Mode::Normal, alt_o());
    let after_second_cycle = app.editor().active_buffer().id;
    assert_eq!(
        after_split, after_second_cycle,
        "Alt+O should cycle back to the original window"
    );
}

#[test]
fn alt_o_cycles_focus_in_insert_mode_without_inserting() {
    let mut app = App::new(Editor::new(), test_config(), Some(Path::new(".")));
    let mut state = KeyState::Normal;

    app.dispatch_action(Action::App(AppAction::Window(WindowAction::WindowSplit(
        WindowSplitAxis::Vertical,
    ))));
    let after_split = app.editor().active_buffer().id;

    dispatch_key(&mut app, &mut state, Mode::Insert, alt_o());
    assert_ne!(
        after_split,
        app.editor().active_buffer().id,
        "Alt+O should cycle focus even in insert mode"
    );
    assert_eq!(
        app.editor().active_buffer().rope.to_string(),
        "",
        "Alt+O in insert mode must not insert a literal 'o'"
    );
}

#[test]
fn alt_o_with_single_window_keeps_focus() {
    let mut app = App::new(Editor::new(), test_config(), Some(Path::new(".")));
    let mut state = KeyState::Normal;

    let before = app.editor().active_buffer().id;
    dispatch_key(&mut app, &mut state, Mode::Normal, alt_o());
    assert_eq!(before, app.editor().active_buffer().id);
}
