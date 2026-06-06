//! Git subprocess helpers and file/diff loading for the status page.

use std::path::Path;


use crate::command::git_backend;
use crate::diff_render::{
    DiffFile, FileStatus,
    content_hash_of_bytes, parse_unified_diff,
};

/// Scan an untracked file: count its lines, detect whether it is binary, and
/// fingerprint its content for the "Viewed" checkbox.
///
/// Returns `(line_count, is_binary, content_hash)`. The read is capped at
/// `MAX_SCAN_BYTES` to bound memory: a file larger than the cap is certainly
/// huge, and the newline count within the scanned prefix is already well past
/// the collapse threshold for text. A file is treated as binary when it
/// contains a NUL byte, in which case it reports a zero line count. The hash
/// folds in the file's full length so growth past the cap is still detected;
/// it is empty only when the file cannot be read.
pub(crate) async fn scan_untracked_file(repo_root: &Path, rel_path: &str) -> (usize, bool, String) {
    use tokio::io::AsyncReadExt;

    const MAX_SCAN_BYTES: u64 = 2 * 1024 * 1024;

    let full = repo_root.join(rel_path);
    let total_len = tokio::fs::metadata(&full)
        .await
        .map(|m| m.len())
        .unwrap_or(0);
    let file = match tokio::fs::File::open(&full).await {
        Ok(f) => f,
        Err(_) => return (0, false, String::new()),
    };
    let mut buf = Vec::new();
    if file
        .take(MAX_SCAN_BYTES)
        .read_to_end(&mut buf)
        .await
        .is_err()
    {
        return (0, false, String::new());
    }
    let hash = content_hash_of_bytes(&buf, total_len);
    if buf.contains(&0) {
        return (0, true, hash);
    }
    if buf.is_empty() {
        return (0, false, hash);
    }
    let mut lines = buf.iter().filter(|&&b| b == b'\n').count();
    // A final line without a trailing newline still counts as a line.
    if buf.last() != Some(&b'\n') {
        lines += 1;
    }
    (lines, false, hash)
}


pub(crate) async fn git_output_in_repo(repo_root: &Path, args: &[&str]) -> Result<String, String> {
    let mut cmd = tokio::process::Command::new("git");
    cmd.args(["-c", "core.quotepath=off"]);
    cmd.args(["-c", "core.optionalLocks=false"]);
    cmd.args(args);
    cmd.current_dir(repo_root);
    git_output_from_command(cmd, &[], &format!("git {}", args.join(" "))).await
}


pub(crate) async fn git_output_from_command(
    mut cmd: tokio::process::Command,
    accepted_exit_codes: &[i32],
    display_cmd: &str,
) -> Result<String, String> {
    match cmd.output().await {
        Ok(output) if output.status.success() => Ok(String::from_utf8_lossy(&output.stdout).into()),
        Ok(output) => {
            let code = output.status.code().unwrap_or(-1);
            if accepted_exit_codes.contains(&code) {
                return Ok(String::from_utf8_lossy(&output.stdout).into());
            }

            let error = String::from_utf8_lossy(&output.stderr).trim().to_string();
            if error.is_empty() {
                Err(format!(
                    "Git command failed: {} (exit code {})",
                    display_cmd, code
                ))
            } else {
                Err(error)
            }
        }
        Err(e) => Err(format!("Failed to execute git: {}", e)),
    }
}


/// Fetch N lines from a file at a given git ref (or working tree).
///
/// `git_ref = Some("")` reads from the index (`git show :path`),
/// `Some("HEAD")` from HEAD, etc.; `None` reads from the working tree.
/// Lines are returned with their original content (newlines stripped).
pub(crate) async fn read_file_range_at_ref(
    repo_root: &Path,
    git_ref: Option<&str>,
    rel_path: &str,
    start: usize,
    end: usize,
) -> Result<Vec<String>, String> {
    let content = match git_ref {
        Some(r) => {
            // In-process gix blob read (no `git show` subprocess). Revspec
            // `<ref>:<path>` resolves HEAD / a branch / `:` (the index).
            let spec = format!("{}:{}", r, rel_path);
            crate::command::git_backend::blob_at_revspec(repo_root, &spec)
                .ok_or_else(|| format!("path not found at ref: {spec}"))?
        }
        None => {
            let path = repo_root.join(rel_path);
            tokio::fs::read_to_string(&path)
                .await
                .map_err(|e| format!("read {}: {}", path.display(), e))?
        }
    };
    let lines: Vec<&str> = content.split_terminator('\n').collect();
    let total = lines.len();
    let start_idx = start.saturating_sub(1).min(total);
    let end_idx = end.min(total);
    if start_idx >= end_idx {
        return Ok(Vec::new());
    }
    Ok(lines[start_idx..end_idx]
        .iter()
        .map(|s| s.to_string())
        .collect())
}


/// Run the per-file `git diff` for a status section and return the parsed
/// [`DiffFile`], if any. Shared by the file-HTML and set-viewed endpoints so
/// both hash and render over identical data.
pub(crate) async fn load_status_diff_file(
    repo_root: &Path,
    section: &str,
    path: &str,
) -> Result<Option<DiffFile>, String> {
    let diff_text = match section {
        "staged" => git_backend::file_diff_text(repo_root, path, true)
            .ok_or_else(|| format!("failed to load staged diff for {path}"))?,
        "unstaged" | "untracked" => git_backend::file_diff_text(repo_root, path, false)
            .ok_or_else(|| format!("failed to load unstaged diff for {path}"))?,
        _ => return Err(format!("invalid section: {section}")),
    };
    let mut files = parse_unified_diff(&diff_text);
    if section == "untracked" {
        for f in &mut files {
            f.status = FileStatus::Untracked;
        }
    }
    Ok(files.into_iter().next())
}


pub(crate) fn status_diff_text(repo_root: &Path, staged: bool) -> Result<String, String> {
    let (changed_entries, staged_entries) = git_backend::status_files(repo_root)
        .ok_or_else(|| "failed to read git status".to_string())?;
    let entries = if staged {
        staged_entries
    } else {
        changed_entries
            .into_iter()
            .filter(|entry| entry.status_char != '?')
            .collect()
    };
    let mut out = String::new();
    for entry in entries {
        if let Some(diff) = git_backend::file_diff_text(repo_root, &entry.path, staged) {
            out.push_str(&diff);
        }
    }
    Ok(out)
}

