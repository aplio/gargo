//! E2E: tree-sitter symbol-index fallback for goto-definition when no LSP is
//! available. Drives the real dispatch path (`lsp.goto_definition` with the
//! LSP plugin disabled) and asserts on resulting editor state.

use std::thread;
use std::time::{Duration, Instant};

use gargo::app::App;
use gargo::config::Config;
use gargo::core::editor::Editor;
use gargo::input::action::{Action, AppAction, IntegrationAction, NavigationAction};
use tempfile::tempdir;

const BUILDING_MESSAGE: &str = "Symbol index is building — try again in a moment";

fn goto_definition_action() -> Action {
    Action::App(AppAction::Integration(
        IntegrationAction::RunPluginCommand {
            id: "lsp.goto_definition".to_string(),
        },
    ))
}

fn app_without_plugins(open_path: &std::path::Path, project_root: &std::path::Path) -> App {
    let mut config = Config::default();
    config.plugins.enabled.clear();
    let editor = Editor::open(&open_path.to_string_lossy());
    App::new(editor, config, Some(project_root))
}

/// Dispatch `action` repeatedly (the symbol index builds lazily in the
/// background, reporting "building" until ready) until `done` holds.
fn dispatch_until(app: &mut App, action: &Action, done: impl Fn(&App) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        app.dispatch_action(action.clone());
        if done(app) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "condition never reached; last message: {:?}",
            app.editor().message
        );
        thread::sleep(Duration::from_millis(10));
    }
}

fn active_path_ends_with(app: &App, suffix: &str) -> bool {
    app.editor()
        .active_buffer()
        .file_path
        .as_deref()
        .is_some_and(|p| p.ends_with(suffix))
}

#[test]
fn goto_definition_jumps_to_definition_in_another_file() {
    let temp = tempdir().expect("temp dir");
    std::fs::create_dir(temp.path().join(".git")).expect("git dir");
    let caller = temp.path().join("a.rs");
    std::fs::write(&caller, "fn main() { helper(); }\n").expect("write a.rs");
    std::fs::write(temp.path().join("b.rs"), "// defs\nfn helper() {}\n").expect("write b.rs");

    let mut app = app_without_plugins(&caller, temp.path());
    // Cursor inside the `helper` call.
    app.editor_mut()
        .active_buffer_mut()
        .set_cursor_line_char(0, 13);

    dispatch_until(&mut app, &goto_definition_action(), |app| {
        active_path_ends_with(app, "b.rs")
    });

    let buffer = app.editor().active_buffer();
    assert_eq!(buffer.cursor_line(), 1);
    assert_eq!(buffer.cursor_col(), 3);
}

#[test]
fn goto_definition_reports_miss_for_unknown_identifier() {
    let temp = tempdir().expect("temp dir");
    std::fs::create_dir(temp.path().join(".git")).expect("git dir");
    let file = temp.path().join("a.rs");
    std::fs::write(&file, "fn main() { undefined_thing(); }\n").expect("write a.rs");

    let mut app = app_without_plugins(&file, temp.path());
    app.editor_mut()
        .active_buffer_mut()
        .set_cursor_line_char(0, 13);

    dispatch_until(&mut app, &goto_definition_action(), |app| {
        app.editor().message.as_deref() != Some(BUILDING_MESSAGE)
    });

    assert_eq!(
        app.editor().message.as_deref(),
        Some("No definition found for 'undefined_thing'")
    );
    assert!(
        active_path_ends_with(&app, "a.rs"),
        "buffer should not change on a miss"
    );
}

#[test]
fn explicit_symbol_goto_definition_command_works_via_navigation_action() {
    let temp = tempdir().expect("temp dir");
    std::fs::create_dir(temp.path().join(".git")).expect("git dir");
    let caller = temp.path().join("a.rs");
    std::fs::write(&caller, "fn main() { helper(); }\n").expect("write a.rs");
    std::fs::write(temp.path().join("b.rs"), "fn helper() {}\n").expect("write b.rs");

    let mut app = app_without_plugins(&caller, temp.path());
    app.editor_mut()
        .active_buffer_mut()
        .set_cursor_line_char(0, 13);

    let action = Action::App(AppAction::Navigation(
        NavigationAction::GotoDefinitionViaSymbolIndex,
    ));
    dispatch_until(&mut app, &action, |app| active_path_ends_with(app, "b.rs"));
}

#[test]
fn goto_definition_tracks_unsaved_edits_in_active_buffer() {
    let temp = tempdir().expect("temp dir");
    std::fs::create_dir(temp.path().join(".git")).expect("git dir");
    let file = temp.path().join("a.rs");
    std::fs::write(&file, "fn local() {}\nfn main() { local(); }\n").expect("write a.rs");

    let mut app = app_without_plugins(&file, temp.path());
    // Simulate an unsaved edit that shifts the definition down one line
    // without saving: the on-disk index still says line 0.
    {
        let doc = app.editor_mut().active_buffer_mut();
        doc.rope.insert(0, "// comment\n");
    }
    app.editor_mut()
        .active_buffer_mut()
        .set_cursor_line_char(2, 13);

    dispatch_until(&mut app, &goto_definition_action(), |app| {
        app.editor().active_buffer().cursor_line() == 1
    });

    assert!(
        active_path_ends_with(&app, "a.rs"),
        "jump should stay in the edited buffer"
    );
    assert_eq!(app.editor().active_buffer().cursor_col(), 3);
}
