use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

use gix::bstr::ByteSlice;
use gix::diff::Rewrites;
use gix::dir::entry::Status;
use gix::filter::plumbing::driver::apply::Delay;
use gix::objs::tree::EntryKind;
use gix::sec::trust::DefaultForLevel;
use gix::status::{
    UntrackedFiles,
    index_worktree::Item,
    plumbing::index_as_worktree::{Change, EntryStatus},
};
use gix::{Commit, ObjectId, Repository, ThreadSafeRepository};
use imara_diff::{Algorithm, InternedInput, Interner};

use crate::command::git::{GitFileStatus, GitLineStatus};

const ALGORITHM: Algorithm = Algorithm::Histogram;
const MAX_DIFF_LINES: usize = 64 * u16::MAX as usize;
const MAX_DIFF_BYTES: usize = MAX_DIFF_LINES * 128;

pub fn status_map(project_root: &Path) -> HashMap<String, GitFileStatus> {
    let mut map = HashMap::new();

    let repo = match open_repo(project_root) {
        Ok(repo) => repo.to_thread_local(),
        Err(_) => return map,
    };

    let work_dir = match repo.workdir() {
        Some(dir) => dir.to_path_buf(),
        None => return map,
    };

    let status_platform = match repo.status(gix::progress::Discard) {
        Ok(status) => status
            .untracked_files(UntrackedFiles::Files)
            .index_worktree_rewrites(Some(Rewrites {
                copies: None,
                percentage: Some(0.5),
                limit: 1000,
                ..Default::default()
            })),
        Err(_) => return map,
    };

    let status_iter = match status_platform.into_index_worktree_iter(Vec::new()) {
        Ok(iter) => iter,
        Err(_) => return map,
    };

    for item in status_iter.flatten() {
        match item {
            Item::Modification {
                rela_path, status, ..
            } => {
                let Ok(path) = rela_path.to_path() else {
                    continue;
                };
                let rel = path.to_string_lossy().to_string();
                upsert_status(&mut map, rel, map_entry_status(status));
            }
            Item::DirectoryContents { entry, .. } if entry.status == Status::Untracked => {
                let Ok(path) = entry.rela_path.to_path() else {
                    continue;
                };
                let rel = path.to_string_lossy().to_string();
                upsert_status(&mut map, rel, GitFileStatus::Untracked);
            }
            Item::Rewrite {
                source,
                dirwalk_entry,
                ..
            } => {
                let Ok(from_path) = source.rela_path().to_path() else {
                    continue;
                };
                let Ok(to_path) = dirwalk_entry.rela_path.to_path() else {
                    continue;
                };

                let from_rel = from_path.to_string_lossy().to_string();
                let to_rel = to_path.to_string_lossy().to_string();

                upsert_status(&mut map, from_rel, GitFileStatus::Deleted);
                upsert_status(&mut map, to_rel, GitFileStatus::Added);
            }
            _ => {}
        }
    }

    if map.is_empty() {
        // Keep behavior resilient when gix status iterator cannot emit paths.
        let _ = work_dir;
    }

    map
}

pub fn diff_line_status_for_content(path: &Path, content: &str) -> HashMap<usize, GitLineStatus> {
    let content_line_count = line_count(content);
    if !within_diff_limits(content, content_line_count) {
        return HashMap::new();
    }

    let Some(base) = diff_base(path) else {
        return full_added_map(content);
    };

    let base_line_count = line_count(&base);
    if !within_diff_limits(&base, base_line_count) {
        return HashMap::new();
    }

    let mut input = InternedInput {
        before: Vec::with_capacity(base_line_count),
        after: Vec::with_capacity(content_line_count),
        interner: Interner::new(base_line_count + content_line_count),
    };
    input.update_before(base.split_inclusive('\n'));
    input.update_after(content.split_inclusive('\n'));

    let mut diff = imara_diff::Diff::default();
    diff.compute_with(
        ALGORITHM,
        &input.before,
        &input.after,
        input.interner.num_tokens(),
    );

    let mut map = HashMap::new();
    for hunk in diff.hunks() {
        let before = hunk.before.clone();
        let after = hunk.after.clone();

        if before.is_empty() && !after.is_empty() {
            for line in after.start..after.end {
                map.insert(line as usize, GitLineStatus::Added);
            }
            continue;
        }

        if after.is_empty() && !before.is_empty() {
            let line = after.start.saturating_sub(1) as usize;
            map.insert(line, GitLineStatus::Deleted);
            continue;
        }

        for line in after.start..after.end {
            map.insert(line as usize, GitLineStatus::Modified);
        }
    }

    map
}

pub fn diff_line_status_for_file(path: &Path) -> HashMap<usize, GitLineStatus> {
    let content = match std::fs::read_to_string(path) {
        Ok(content) => content,
        Err(_) => return HashMap::new(),
    };
    diff_line_status_for_content(path, &content)
}

fn diff_base(path: &Path) -> Option<String> {
    let file = gix::path::realpath(path).ok()?;
    let repo_dir = file.parent()?;

    let repo = open_repo(repo_dir).ok()?.to_thread_local();
    let head = repo.head_commit().ok()?;
    let oid = find_file_in_commit(&repo, &head, &file).ok()?;

    let file_object = repo.find_object(oid).ok()?;
    let data = file_object.detach().data;

    let bytes = if let Some(work_dir) = repo.workdir() {
        let rel_path = file.strip_prefix(work_dir).ok()?;
        let rel_path = gix::path::try_into_bstr(rel_path).ok()?;
        let (mut pipeline, _) = repo.filter_pipeline(None).ok()?;
        let mut worktree_outcome = pipeline
            .convert_to_worktree(&data, rel_path.as_ref(), Delay::Forbid)
            .ok()?;
        let mut buf = Vec::with_capacity(data.len());
        worktree_outcome.read_to_end(&mut buf).ok()?;
        buf
    } else {
        data
    };

    Some(String::from_utf8_lossy(&bytes).to_string())
}

fn open_repo(path: &Path) -> Result<ThreadSafeRepository, Box<gix::discover::Error>> {
    let mut open_opts_map = gix::sec::trust::Mapping::<gix::open::Options>::default();

    let config = gix::open::permissions::Config {
        system: true,
        git: true,
        user: true,
        env: true,
        includes: true,
        git_binary: cfg!(windows),
    };

    open_opts_map.reduced = open_opts_map.reduced.permissions(gix::open::Permissions {
        config,
        ..gix::open::Permissions::default_for_level(gix::sec::Trust::Reduced)
    });
    open_opts_map.full = open_opts_map.full.permissions(gix::open::Permissions {
        config,
        ..gix::open::Permissions::default_for_level(gix::sec::Trust::Full)
    });

    let discover_opts = gix::discover::upwards::Options {
        dot_git_only: true,
        ..Default::default()
    };

    ThreadSafeRepository::discover_with_environment_overrides_opts(
        path,
        discover_opts,
        open_opts_map,
    )
    .map_err(Box::new)
}

fn find_file_in_commit(
    repo: &Repository,
    commit: &Commit<'_>,
    file: &Path,
) -> Result<ObjectId, String> {
    let repo_dir = repo
        .workdir()
        .ok_or_else(|| "repo has no worktree".to_string())?;
    let rel_path = file
        .strip_prefix(repo_dir)
        .map_err(|_| "file is outside worktree".to_string())?;

    let tree = commit.tree().map_err(|e| e.to_string())?;
    let entry = tree
        .lookup_entry_by_path(rel_path)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "file is untracked".to_string())?;

    match entry.mode().kind() {
        EntryKind::Blob | EntryKind::BlobExecutable => Ok(entry.object_id()),
        _ => Err("entry is not a regular file".to_string()),
    }
}

fn map_entry_status<TSubmodule, TConflict>(
    status: EntryStatus<TSubmodule, TConflict>,
) -> GitFileStatus {
    match status {
        EntryStatus::Conflict { .. } => GitFileStatus::Conflict,
        EntryStatus::Change(Change::Removed) => GitFileStatus::Deleted,
        EntryStatus::IntentToAdd => GitFileStatus::Untracked,
        EntryStatus::Change(_) => GitFileStatus::Modified,
        EntryStatus::NeedsUpdate(_) => GitFileStatus::Modified,
    }
}

fn upsert_status(map: &mut HashMap<String, GitFileStatus>, path: String, status: GitFileStatus) {
    let entry = map.entry(path).or_insert(status);
    if priority(status) > priority(*entry) {
        *entry = status;
    }
}

fn priority(status: GitFileStatus) -> u8 {
    match status {
        GitFileStatus::Conflict => 4,
        GitFileStatus::Deleted => 3,
        GitFileStatus::Modified => 2,
        GitFileStatus::Added => 1,
        GitFileStatus::Untracked => 0,
    }
}

fn line_count(content: &str) -> usize {
    if content.is_empty() {
        0
    } else {
        content.split_inclusive('\n').count()
    }
}

fn within_diff_limits(content: &str, line_count: usize) -> bool {
    line_count <= MAX_DIFF_LINES && content.len() <= MAX_DIFF_BYTES
}

fn full_added_map(content: &str) -> HashMap<usize, GitLineStatus> {
    let mut out = HashMap::new();
    for (idx, line) in content.split_inclusive('\n').enumerate() {
        if !line.is_empty() {
            out.insert(idx, GitLineStatus::Added);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_added_marks_each_non_empty_line() {
        let map = full_added_map("a\nb\n");
        assert_eq!(map.get(&0), Some(&GitLineStatus::Added));
        assert_eq!(map.get(&1), Some(&GitLineStatus::Added));
    }

    #[test]
    fn diff_limits_enforced() {
        let huge = "x".repeat(MAX_DIFF_BYTES + 1);
        assert!(!within_diff_limits(&huge, 1));
    }
}
