use std::fs;
use std::path::Path;
use std::process::Command;

use gargo::command::in_editor_diff::{IN_EDITOR_DIFF_TITLE, build_in_editor_diff_view};
use tempfile::tempdir;

fn run_git(repo: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .expect("run git command");
    assert!(
        output.status.success(),
        "git command failed: git {}\nstdout={}\nstderr={}",
        args.join(" "),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn build_in_editor_diff_view_renders_sections_and_jump_targets() {
    let repo_dir = tempdir().expect("create temp repo");
    let repo = repo_dir.path();

    run_git(repo, &["init"]);
    run_git(repo, &["config", "user.name", "gargo-test"]);
    run_git(repo, &["config", "user.email", "gargo-test@example.com"]);

    let tracked_file = repo.join("sample.txt");
    fs::write(&tracked_file, "line1\n").expect("write initial file");
    run_git(repo, &["add", "sample.txt"]);
    run_git(repo, &["commit", "-m", "init"]);

    fs::write(&tracked_file, "line1\nline2\n").expect("modify tracked file");
    fs::write(repo.join("new-untracked.txt"), "hello\n").expect("write untracked file");

    let view = build_in_editor_diff_view(repo).expect("build in-editor diff view");
    assert!(view.text.contains(IN_EDITOR_DIFF_TITLE));
    assert!(view.text.contains("## Changed (unstaged)"));
    assert!(view.text.contains("## Staged"));
    assert!(view.text.contains("## Untracked"));
    assert!(view.text.contains("+line2"));
    assert!(view.text.contains("new-untracked.txt"));

    let target_line_idx = view
        .text
        .lines()
        .position(|line| line == "+line2")
        .expect("added line should exist");
    let target = view
        .line_targets
        .get(&target_line_idx)
        .expect("target should exist for added line");
    assert!(target.path.ends_with("sample.txt"));
    assert_eq!(target.line, 1);
    assert_eq!(target.char_col, 0);
}

#[test]
fn build_in_editor_diff_view_maps_removed_lines_to_file_targets() {
    let repo_dir = tempdir().expect("create temp repo");
    let repo = repo_dir.path();

    run_git(repo, &["init"]);
    run_git(repo, &["config", "user.name", "gargo-test"]);
    run_git(repo, &["config", "user.email", "gargo-test@example.com"]);

    let tracked_file = repo.join("sample.txt");
    fs::write(&tracked_file, "line1\nline2\n").expect("write initial file");
    run_git(repo, &["add", "sample.txt"]);
    run_git(repo, &["commit", "-m", "init"]);

    fs::write(&tracked_file, "line1\n").expect("delete second line");

    let view = build_in_editor_diff_view(repo).expect("build in-editor diff view");
    let removed_line_idx = view
        .text
        .lines()
        .position(|line| line == "-line2")
        .expect("removed line should exist");
    let target = view
        .line_targets
        .get(&removed_line_idx)
        .expect("target should exist for removed line");
    assert!(target.path.ends_with("sample.txt"));
    assert_eq!(target.line, 1);
}
