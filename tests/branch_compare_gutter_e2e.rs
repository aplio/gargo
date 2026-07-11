//! E2E: the branch-compare sidebar preview renders a git gutter that marks
//! lines changed against the compare base, through the real compositor
//! render path (sidebar + preview pane + async preview loading).

use gargo::command::git::GitFileEntry;
use gargo::config::Config;
use gargo::core::editor::Editor;
use gargo::input::chord::KeyState;
use gargo::syntax::theme::Theme;
use gargo::ui::framework::component::RenderContext;
use gargo::ui::framework::compositor::Compositor;
use gargo::ui::overlays::explorer::Explorer;
use std::path::{Path, PathBuf};

mod support;

fn run_git(repo: &Path, args: &[&str]) {
    let status = std::process::Command::new("git")
        .current_dir(repo)
        .args(args)
        .status()
        .expect("run git");
    assert!(status.success(), "git {:?} failed", args);
}

fn setup_repo(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("gargo_e2e_{}", name));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    run_git(&dir, &["init"]);
    run_git(&dir, &["config", "user.name", "gargo-test"]);
    run_git(&dir, &["config", "user.email", "gargo-test@example.com"]);
    dir
}

fn render_frame(
    compositor: &mut Compositor,
    editor: &Editor,
    project_root: &Path,
    cols: usize,
    rows: usize,
) -> Vec<u8> {
    let config = Config::default();
    let theme = Theme::dark();
    let key_state = KeyState::Normal;
    let ctx = RenderContext::new(
        cols,
        rows,
        editor,
        &theme,
        &key_state,
        &config,
        project_root,
        false,
        false,
    );
    let mut out = Vec::new();
    compositor
        .render(&ctx, &mut out)
        .expect("render frame to memory");
    out
}

#[test]
fn branch_compare_preview_shows_git_gutter_for_changed_lines() {
    let dir = setup_repo("branch_compare_gutter");

    let base: String = (1..=8).map(|i| format!("alpha{}\n", i)).collect();
    std::fs::write(dir.join("file.txt"), &base).unwrap();
    run_git(&dir, &["add", "."]);
    run_git(&dir, &["commit", "-m", "base"]);
    run_git(&dir, &["branch", "base"]);
    let modified = base.replace("alpha2\n", "alpha2 CHANGED\n") + "alpha9 ADDED\n";
    std::fs::write(dir.join("file.txt"), &modified).unwrap();
    run_git(&dir, &["add", "."]);
    run_git(&dir, &["commit", "-m", "change alpha2, add alpha9"]);

    let files = vec![GitFileEntry {
        path: "file.txt".to_string(),
        status_char: 'M',
        staged: false,
        additions: 2,
        deletions: 1,
    }];
    let mut explorer = Explorer::new_branch_compare(dir.clone(), "base".to_string(), files);
    explorer.select_by_name("file.txt");
    explorer.set_preview_mode(true);

    let cols = 100;
    let rows = 24;
    let mut screen = vec![vec![' '; cols]; rows];
    let editor = Editor::new();
    let mut compositor = Compositor::new();
    compositor.open_explorer(explorer);

    // The preview loads on a worker thread; keep rendering frames (each
    // render polls the worker) until the file content lands.
    let mut text = String::new();
    for _ in 0..200 {
        let frame = render_frame(&mut compositor, &editor, &dir, cols, rows);
        let frame_rows =
            support::render_snapshot::apply_ansi_to_screen(&mut screen, &frame, cols, rows);
        text = frame_rows.join("\n");
        if text.contains("alpha2 CHANGED") {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(
        text.contains("alpha2 CHANGED"),
        "preview content did not load; frame:\n{}",
        text
    );

    let row_of = |needle: &str| -> &str {
        text.lines()
            .find(|line| line.contains(needle))
            .unwrap_or_else(|| panic!("row containing {:?} not found in frame:\n{}", needle, text))
    };

    // Changed and added lines carry the git gutter marker in the preview.
    assert!(
        row_of("alpha2 CHANGED").contains('▍'),
        "modified line should have a gutter marker; frame:\n{}",
        text
    );
    assert!(
        row_of("alpha9 ADDED").contains('▍'),
        "added line should have a gutter marker; frame:\n{}",
        text
    );
    // Unchanged lines don't.
    assert!(
        !row_of("alpha3").contains('▍'),
        "unchanged line should not have a gutter marker; frame:\n{}",
        text
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn branch_compare_preview_marks_deleted_lines() {
    let dir = setup_repo("branch_compare_gutter_deleted");

    let base: String = (1..=8).map(|i| format!("beta{}\n", i)).collect();
    std::fs::write(dir.join("file.txt"), &base).unwrap();
    run_git(&dir, &["add", "."]);
    run_git(&dir, &["commit", "-m", "base"]);
    run_git(&dir, &["branch", "base"]);
    let modified = base.replace("beta4\n", "");
    std::fs::write(dir.join("file.txt"), &modified).unwrap();
    run_git(&dir, &["add", "."]);
    run_git(&dir, &["commit", "-m", "delete beta4"]);

    let files = vec![GitFileEntry {
        path: "file.txt".to_string(),
        status_char: 'M',
        staged: false,
        additions: 0,
        deletions: 1,
    }];
    let mut explorer = Explorer::new_branch_compare(dir.clone(), "base".to_string(), files);
    explorer.select_by_name("file.txt");
    explorer.set_preview_mode(true);

    let cols = 100;
    let rows = 24;
    let mut screen = vec![vec![' '; cols]; rows];
    let editor = Editor::new();
    let mut compositor = Compositor::new();
    compositor.open_explorer(explorer);

    let mut text = String::new();
    for _ in 0..200 {
        let frame = render_frame(&mut compositor, &editor, &dir, cols, rows);
        let frame_rows =
            support::render_snapshot::apply_ansi_to_screen(&mut screen, &frame, cols, rows);
        text = frame_rows.join("\n");
        if text.contains("beta5") {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    // The line before the deletion carries the deletion marker (▔), matching
    // the editor gutter's convention.
    let row = text
        .lines()
        .find(|line| line.contains("beta3"))
        .unwrap_or_else(|| panic!("row containing beta3 not found in frame:\n{}", text));
    assert!(
        row.contains('▔'),
        "line before a deletion should carry the deletion marker; frame:\n{}",
        text
    );

    let _ = std::fs::remove_dir_all(&dir);
}
