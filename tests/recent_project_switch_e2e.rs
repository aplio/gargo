use std::collections::HashMap;
use std::path::Path;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use gargo::command::recent_projects::RecentProjectEntry;
use gargo::command::registry::{CommandRegistry, register_builtins};
use gargo::config::Config;
use gargo::input::action::{Action, AppAction, NavigationAction, ProjectAction};
use gargo::syntax::language::LanguageRegistry;
use gargo::ui::framework::component::EventResult;
use gargo::ui::overlays::palette::picker::Palette;
use gargo::ui::overlays::project::recent_picker::RecentProjectPopup;

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

#[test]
fn switch_recent_project_command_is_executable_from_command_palette() {
    let mut registry = CommandRegistry::new();
    register_builtins(&mut registry);
    let lang_registry = LanguageRegistry::new();
    let config = Config::default();

    let mut palette = Palette::new(vec![], Path::new(""), &HashMap::new(), None, vec![], vec![]);
    palette.set_input(">switch recent project".to_string());
    palette.update_candidates(&registry, &lang_registry, &config);

    let result = palette.handle_key_event(key(KeyCode::Enter), &registry, &lang_registry, &config);

    let idx = match result {
        EventResult::Action(Action::App(AppAction::Navigation(
            NavigationAction::ExecutePaletteCommand(idx),
        ))) => idx,
        other => panic!("expected ExecutePaletteCommand, got {:?}", other),
    };
    assert_eq!(registry.commands()[idx].id, "project.switch_recent");
}

#[test]
fn smart_copy_command_is_executable_from_command_palette() {
    let mut registry = CommandRegistry::new();
    register_builtins(&mut registry);
    let lang_registry = LanguageRegistry::new();
    let config = Config::default();

    let mut palette = Palette::new(vec![], Path::new(""), &HashMap::new(), None, vec![], vec![]);
    palette.set_input(">smart copy".to_string());
    palette.update_candidates(&registry, &lang_registry, &config);

    let result = palette.handle_key_event(key(KeyCode::Enter), &registry, &lang_registry, &config);

    let idx = match result {
        EventResult::Action(Action::App(AppAction::Navigation(
            NavigationAction::ExecutePaletteCommand(idx),
        ))) => idx,
        other => panic!("expected ExecutePaletteCommand, got {:?}", other),
    };
    assert_eq!(registry.commands()[idx].id, "symbol.smart_copy");
}

#[test]
fn recent_project_popup_ctrl_n_p_and_enter_select_candidate() {
    let entries = vec![
        RecentProjectEntry {
            project_path: "/tmp/repo_a".to_string(),
            last_open_at: 20,
            last_edit_at: 10,
            last_open_file: None,
            last_edit_file: None,
        },
        RecentProjectEntry {
            project_path: "/tmp/repo_b".to_string(),
            last_open_at: 10,
            last_edit_at: 30,
            last_open_file: None,
            last_edit_file: None,
        },
    ];
    let mut popup = RecentProjectPopup::new(entries);

    popup.handle_key(ctrl('n'));
    popup.handle_key(ctrl('p'));
    popup.handle_key(ctrl('n'));

    let result = popup.handle_key(key(KeyCode::Enter));
    assert_eq!(
        result,
        EventResult::Action(Action::App(AppAction::Project(
            ProjectAction::SwitchToRecentProject("/tmp/repo_b".to_string())
        )))
    );
}

#[test]
fn recent_project_popup_filters_with_fzf_query() {
    let entries = vec![
        RecentProjectEntry {
            project_path: "/tmp/gargo2".to_string(),
            last_open_at: 20,
            last_edit_at: 10,
            last_open_file: None,
            last_edit_file: None,
        },
        RecentProjectEntry {
            project_path: "/tmp/another".to_string(),
            last_open_at: 10,
            last_edit_at: 30,
            last_open_file: None,
            last_edit_file: None,
        },
    ];
    let mut popup = RecentProjectPopup::new(entries);
    popup.handle_key(key(KeyCode::Char('g')));
    popup.handle_key(key(KeyCode::Char('2')));

    let result = popup.handle_key(key(KeyCode::Enter));
    assert_eq!(
        result,
        EventResult::Action(Action::App(AppAction::Project(
            ProjectAction::SwitchToRecentProject("/tmp/gargo2".to_string())
        )))
    );
}
