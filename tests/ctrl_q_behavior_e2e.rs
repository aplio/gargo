use std::path::Path;

use gargo::app::App;
use gargo::config::Config;
use gargo::core::editor::Editor;
use gargo::input::action::{Action, AppAction, BufferAction, WindowAction, WindowSplitAxis};

fn test_config() -> Config {
    let mut config = Config::default();
    config.plugins.enabled.clear();
    config
}

#[test]
fn ctrl_q_closes_split_window_before_quit_when_only_clean_scratch_remains() {
    let mut app = App::new(Editor::new(), test_config(), Some(Path::new(".")));

    assert!(
        !app.dispatch_action(Action::App(AppAction::Window(WindowAction::WindowSplit(
            WindowSplitAxis::Vertical,
        ))))
    );

    // First close removes one scratch buffer, leaving a single clean scratch in two windows.
    assert!(!app.dispatch_action(Action::App(AppAction::Buffer(BufferAction::CloseBuffer))));
    // Second close should close the focused split window, not quit.
    assert!(!app.dispatch_action(Action::App(AppAction::Buffer(BufferAction::CloseBuffer))));
    // Third close now matches single-window single-clean-scratch and quits.
    assert!(app.dispatch_action(Action::App(AppAction::Buffer(BufferAction::CloseBuffer))));
}

#[test]
fn ctrl_q_quits_immediately_with_single_clean_scratch_in_single_window() {
    let mut app = App::new(Editor::new(), test_config(), Some(Path::new(".")));
    assert!(app.dispatch_action(Action::App(AppAction::Buffer(BufferAction::CloseBuffer))));
}
