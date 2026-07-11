//! E2E: the branch-compare sidebar preview toggles into a split
//! (side-by-side before/after) view with the `s` key, through the real
//! compositor key-routing and render path.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use gargo::command::git::GitFileEntry;
use gargo::command::registry::CommandRegistry;
use gargo::config::Config;
use gargo::core::editor::Editor;
use gargo::input::chord::KeyState;
use gargo::syntax::language::LanguageRegistry;
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
fn s_key_toggles_split_preview_in_branch_compare_sidebar() {
    let dir: PathBuf = std::env::temp_dir().join("gargo_e2e_branch_compare_split");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    run_git(&dir, &["init"]);
    run_git(&dir, &["config", "user.name", "gargo-test"]);
    run_git(&dir, &["config", "user.email", "gargo-test@example.com"]);

    let base: String = (1..=8)
        .map(|i| {
            if i == 2 {
                "OLD-line2\n".to_string()
            } else {
                format!("gamma{}\n", i)
            }
        })
        .collect();
    std::fs::write(dir.join("file.txt"), &base).unwrap();
    run_git(&dir, &["add", "."]);
    run_git(&dir, &["commit", "-m", "base"]);
    run_git(&dir, &["branch", "base"]);
    let modified = base.replace("OLD-line2\n", "NEW-line2\n");
    std::fs::write(dir.join("file.txt"), &modified).unwrap();
    run_git(&dir, &["add", "."]);
    run_git(&dir, &["commit", "-m", "change line 2"]);

    let files = vec![GitFileEntry {
        path: "file.txt".to_string(),
        status_char: 'M',
        staged: false,
        additions: 1,
        deletions: 1,
    }];
    let mut explorer = Explorer::new_branch_compare(dir.clone(), "base".to_string(), files);
    explorer.select_by_name("file.txt");
    explorer.set_preview_mode(true);

    let cols = 120;
    let rows = 24;
    let mut screen = vec![vec![' '; cols]; rows];
    let editor = Editor::new();
    let mut compositor = Compositor::new();
    compositor.open_explorer(explorer);

    // Wait for the inline preview to load first.
    let mut text = String::new();
    for _ in 0..200 {
        let frame = render_frame(&mut compositor, &editor, &dir, cols, rows);
        let frame_rows =
            support::render_snapshot::apply_ansi_to_screen(&mut screen, &frame, cols, rows);
        text = frame_rows.join("\n");
        if text.contains("NEW-line2") {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(
        text.contains("NEW-line2"),
        "inline preview did not load; frame:\n{}",
        text
    );
    assert!(
        !text.contains("OLD-line2"),
        "inline preview should not show the base version; frame:\n{}",
        text
    );

    // `s` toggles the split preview via the compositor's key routing.
    let registry = CommandRegistry::new();
    let lang_registry = LanguageRegistry::new();
    let config = Config::default();
    let key_state = KeyState::Normal;
    compositor.handle_key(
        KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE),
        &registry,
        &lang_registry,
        &config,
        &key_state,
    );

    let frame = render_frame(&mut compositor, &editor, &dir, cols, rows);
    let frame_rows =
        support::render_snapshot::apply_ansi_to_screen(&mut screen, &frame, cols, rows);
    let text = frame_rows.join("\n");

    assert!(
        text.contains("PREVIEW[split]"),
        "split title missing; frame:\n{}",
        text
    );
    let changed_row = text
        .lines()
        .find(|line| line.contains("OLD-line2"))
        .unwrap_or_else(|| panic!("base version not visible in split view; frame:\n{}", text));
    let old_pos = changed_row.find("OLD-line2").unwrap();
    let new_pos = changed_row
        .find("NEW-line2")
        .unwrap_or_else(|| panic!("both versions should share a row; row: {:?}", changed_row));
    assert!(
        old_pos < new_pos,
        "before should be left of after; row: {:?}",
        changed_row
    );

    // Toggling again restores the inline preview.
    compositor.handle_key(
        KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE),
        &registry,
        &lang_registry,
        &config,
        &key_state,
    );
    let frame = render_frame(&mut compositor, &editor, &dir, cols, rows);
    let frame_rows =
        support::render_snapshot::apply_ansi_to_screen(&mut screen, &frame, cols, rows);
    let text = frame_rows.join("\n");
    assert!(
        !text.contains("PREVIEW[split]") && !text.contains("OLD-line2"),
        "inline preview should be restored; frame:\n{}",
        text
    );

    let _ = std::fs::remove_dir_all(&dir);
}
