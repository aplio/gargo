use std::fs;
use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use gargo::input::action::{Action, AppAction, ProjectAction};
use gargo::ui::framework::component::EventResult;
use gargo::ui::overlays::project::root_picker::ProjectRootPopup;
use tempfile::tempdir;

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

#[test]
fn enter_on_default_input_submits_current_project_root() {
    let tmp = tempdir().expect("create temp dir");
    let repo = tmp.path().join("repo");
    fs::create_dir_all(&repo).expect("create repo");

    let mut popup = ProjectRootPopup::new(repo.clone());
    let result = popup.handle_key(key(KeyCode::Enter));

    match result {
        EventResult::Action(Action::App(AppAction::Project(ProjectAction::ChangeProjectRoot(
            path,
        )))) => {
            assert_eq!(PathBuf::from(path), fs::canonicalize(repo).unwrap());
        }
        _ => panic!("expected ChangeProjectRoot from default input"),
    }
}

#[test]
fn ctrl_nav_tab_and_enter_switch_to_candidate_repo() {
    let tmp = tempdir().expect("create temp dir");
    let workspace = tmp.path().join("workspace");
    let repo_a = workspace.join("repo_a");
    let repo_b = workspace.join("repo_b");
    fs::create_dir_all(&repo_a).expect("create repo_a");
    fs::create_dir_all(&repo_b).expect("create repo_b");

    let mut popup = ProjectRootPopup::new(repo_a.clone());
    popup.handle_key(ctrl('w')); // -> workspace/
    popup.handle_key(ctrl('n')); // select repo_b
    popup.handle_key(key(KeyCode::Tab)); // complete selected candidate
    let result = popup.handle_key(key(KeyCode::Enter));

    match result {
        EventResult::Action(Action::App(AppAction::Project(ProjectAction::ChangeProjectRoot(
            path,
        )))) => {
            assert_eq!(PathBuf::from(path), fs::canonicalize(repo_b).unwrap());
        }
        _ => panic!("expected ChangeProjectRoot after candidate completion"),
    }
}

#[test]
fn enter_without_candidate_selection_uses_raw_absolute_input() {
    let tmp = tempdir().expect("create temp dir");
    let workspace = tmp.path().join("workspace");
    let repo_a = workspace.join("repo_a");
    let repo_b = workspace.join("repo_b");
    fs::create_dir_all(&repo_a).expect("create repo_a");
    fs::create_dir_all(&repo_b).expect("create repo_b");

    let mut popup = ProjectRootPopup::new(repo_a);
    popup.handle_key(ctrl('w')); // -> workspace/
    for c in "repo_b".chars() {
        popup.handle_key(key(KeyCode::Char(c)));
    }
    let result = popup.handle_key(key(KeyCode::Enter));

    match result {
        EventResult::Action(Action::App(AppAction::Project(ProjectAction::ChangeProjectRoot(
            path,
        )))) => {
            assert_eq!(PathBuf::from(path), fs::canonicalize(repo_b).unwrap());
        }
        _ => panic!("expected ChangeProjectRoot from raw input"),
    }
}
