use std::path::Path;

use gargo::app::App;
use gargo::command::registry::{CommandContext, CommandEffect, CommandRegistry, register_builtins};
use gargo::config::Config;
use gargo::core::editor::Editor;
use gargo::input::action::{Action, AppAction, LifecycleAction};

fn action_for_builtin_command(command_id: &str) -> Action {
    let mut registry = CommandRegistry::new();
    register_builtins(&mut registry);
    let command = registry
        .commands()
        .iter()
        .find(|entry| entry.id == command_id)
        .expect("builtin command id exists");

    let context_editor = Editor::new();
    let context = CommandContext::new(&context_editor);
    match (command.action)(&context) {
        CommandEffect::Action(action) => action,
        CommandEffect::None => panic!("expected command to return an action"),
        CommandEffect::Message(_) => panic!("expected command to return an action"),
    }
}

#[test]
fn command_palette_toggle_debug_flips_runtime_flag() {
    let mut config = Config::default();
    config.plugins.enabled.clear();
    let mut app = App::new(Editor::new(), config, Some(Path::new(".")));
    assert!(!app.config().debug);

    let first = action_for_builtin_command("config.toggle_debug");
    assert_eq!(
        first,
        Action::App(AppAction::Lifecycle(LifecycleAction::ToggleDebug))
    );
    assert!(!app.dispatch_action(first));
    assert!(app.config().debug);

    let second = action_for_builtin_command("config.toggle_debug");
    assert_eq!(
        second,
        Action::App(AppAction::Lifecycle(LifecycleAction::ToggleDebug))
    );
    assert!(!app.dispatch_action(second));
    assert!(!app.config().debug);
}

#[test]
fn command_palette_toggle_line_numbers_flips_runtime_flag() {
    let mut config = Config::default();
    config.plugins.enabled.clear();
    let mut app = App::new(Editor::new(), config, Some(Path::new(".")));
    assert!(app.config().show_line_number);

    let first = action_for_builtin_command("config.toggle_line_numbers");
    assert_eq!(
        first,
        Action::App(AppAction::Lifecycle(LifecycleAction::ToggleLineNumber))
    );
    assert!(!app.dispatch_action(first));
    assert!(!app.config().show_line_number);

    let second = action_for_builtin_command("config.toggle_line_numbers");
    assert_eq!(
        second,
        Action::App(AppAction::Lifecycle(LifecycleAction::ToggleLineNumber))
    );
    assert!(!app.dispatch_action(second));
    assert!(app.config().show_line_number);
}
