//! Split (side-by-side) diff view: source parsing, loading, and handler.

use super::*;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use axum::{
    extract::{Query, State},
    response::{Html, IntoResponse, Response},
};

use crate::command::git_backend;
use crate::diff_render::{
    DiffFile, FileStatus, LineHighlights, parse_unified_diff, render_diff_styles,
};
use crate::split_render::{LineHl, build_split_rows, render_split_html, render_split_styles};
use crate::syntax::highlight::highlight_text;
use crate::syntax::language::LanguageRegistry;

/// Origin of a split-view request. Determines which refs to read the old
/// and new file contents from, plus what the "Back" link points to.
#[derive(Debug, Clone)]
pub(crate) enum SplitSource {
    Status { section: String },
    Compare { base: String, compare: String },
    Commit { hash: String },
}

pub(crate) fn parse_split_source(params: &HashMap<String, String>) -> Result<SplitSource, String> {
    let src = params
        .get("source")
        .map(String::as_str)
        .ok_or_else(|| "missing `source` query parameter".to_string())?;
    match src {
        "status" => {
            let section = params
                .get("section")
                .ok_or_else(|| "missing `section` query parameter".to_string())?;
            match section.as_str() {
                "staged" | "unstaged" | "untracked" => Ok(SplitSource::Status {
                    section: section.clone(),
                }),
                _ => Err(format!("invalid section: {section}")),
            }
        }
        "compare" => {
            let base_raw = params
                .get("base")
                .ok_or_else(|| "missing `base` query parameter".to_string())?;
            let compare_raw = params
                .get("compare")
                .ok_or_else(|| "missing `compare` query parameter".to_string())?;
            let base = parse_branch_name(base_raw)
                .ok_or_else(|| format!("invalid branch name: {base_raw}"))?;
            let compare = parse_branch_name(compare_raw)
                .ok_or_else(|| format!("invalid branch name: {compare_raw}"))?;
            Ok(SplitSource::Compare { base, compare })
        }
        "commit" => {
            let hash_raw = params
                .get("hash")
                .ok_or_else(|| "missing `hash` query parameter".to_string())?;
            let hash = parse_commit_hash_value(hash_raw)
                .ok_or_else(|| format!("invalid commit hash: {hash_raw}"))?;
            Ok(SplitSource::Commit { hash })
        }
        other => Err(format!("invalid source: {other}")),
    }
}

/// Map a split source to `(old_ref, new_ref)`. `None` means "working tree";
/// `Some("")` means the git index (`git show :path`).
pub(crate) fn split_refs(source: &SplitSource) -> (Option<String>, Option<String>) {
    match source {
        SplitSource::Status { section } => match section.as_str() {
            "staged" => (Some("HEAD".into()), Some(String::new())),
            "unstaged" => (Some(String::new()), None),
            "untracked" => (None, None),
            _ => (None, None),
        },
        SplitSource::Compare { base, compare } => (Some(base.clone()), Some(compare.clone())),
        SplitSource::Commit { hash } => (Some(format!("{hash}^")), Some(hash.clone())),
    }
}

/// 64-hex limit covers SHA-256; the gargo_server module has a duplicate.
/// Kept private here so `/split` doesn't depend on the github router module.
pub(crate) fn parse_commit_hash_value(hash: &str) -> Option<String> {
    if hash.is_empty() || hash.len() > 64 {
        return None;
    }
    if hash.chars().all(|c| c.is_ascii_hexdigit()) {
        Some(hash.to_string())
    } else {
        None
    }
}

/// Fetch full file contents at a ref (or working tree). Returns `None` when
/// the path does not exist on that side (added on new, deleted on old, etc.)
/// so the caller can render a one-sided split.
pub(crate) async fn read_full_file_at_ref(
    repo_root: &Path,
    git_ref: Option<&str>,
    rel_path: &str,
) -> Result<Option<Vec<String>>, String> {
    let exists = match git_ref {
        Some(r) => {
            git_backend::blob_at_revspec(repo_root, &format!("{}:{}", r, rel_path)).is_some()
        }
        None => {
            let full = repo_root.join(rel_path);
            tokio::fs::try_exists(&full).await.unwrap_or(false)
        }
    };
    if !exists {
        return Ok(None);
    }
    let lines = read_file_range_at_ref(repo_root, git_ref, rel_path, 1, usize::MAX).await?;
    Ok(Some(lines))
}

/// Load the parsed `DiffFile` for any split source. Reuses the existing
/// per-page loaders so status/compare paths share identical git invocations
/// with their non-split counterparts.
pub(crate) async fn load_split_diff_file(
    repo_root: &Path,
    source: &SplitSource,
    path: &str,
) -> Result<Option<DiffFile>, String> {
    match source {
        SplitSource::Status { section } => load_status_diff_file(repo_root, section, path).await,
        SplitSource::Compare { base, compare } => {
            load_compare_diff_file(repo_root, base, compare, path).await
        }
        SplitSource::Commit { hash } => {
            let diff = git_backend::commit_diff_text(repo_root, hash, Some(path))
                .ok_or_else(|| format!("failed to load commit diff for {hash}"))?;
            Ok(parse_unified_diff(&diff).into_iter().next())
        }
    }
}

pub(crate) fn ref_label(r: Option<&str>) -> String {
    match r {
        None => "working tree".to_string(),
        Some("") => "index".to_string(),
        Some(name) => name.to_string(),
    }
}

/// Soft cap on combined old + new line count. The split view loads the
/// whole file into the DOM in a single pass; beyond this much content the
/// browser stalls long enough to feel broken.
pub(crate) const SPLIT_MAX_LINES: usize = 50_000;

/// Build per-line highlight maps (keyed by 1-based line number) from the
/// full text of one side using the language inferred from `path`.
pub(crate) fn build_line_highlights(lines: &[String], path: &str) -> Option<LineHl> {
    let registry = LanguageRegistry::new();
    let lang = registry.detect_by_extension(path)?;
    let joined = lines.join("\n");
    let spans_per_row = highlight_text(&joined, lang);
    let mut out: LineHl = HashMap::new();
    for (row, spans) in spans_per_row {
        if spans.is_empty() {
            continue;
        }
        let content_len = lines.get(row).map(|s| s.len()).unwrap_or(0);
        let mut packed: Vec<(usize, usize, String)> = Vec::with_capacity(spans.len());
        for s in spans {
            let start = s.start.min(content_len);
            let end = s.end.min(content_len);
            if start < end {
                packed.push((start, end, s.capture_name));
            }
        }
        if !packed.is_empty() {
            out.insert(row + 1, LineHighlights { spans: packed });
        }
    }
    Some(out)
}

pub(crate) fn split_back_url(
    source: &SplitSource,
    url_ctx: &crate::command::gargo_preview_server::RepoUrlContext,
) -> String {
    // Inputs are pre-validated (parse_branch_name, parse_commit_hash_value,
    // owner/repo from git config), so no percent-encoding needed here.
    match source {
        SplitSource::Status { .. } => "/status".to_string(),
        SplitSource::Compare { base, compare } => {
            format!("/compare?base={base}&compare={compare}")
        }
        SplitSource::Commit { hash } => {
            if url_ctx.owner.is_empty() || url_ctx.repo.is_empty() {
                "/status".to_string()
            } else {
                format!("/{}/{}/commit/{}", url_ctx.owner, url_ctx.repo, hash)
            }
        }
    }
}

/// Serve the split-view HTML page for a single file.
pub(crate) async fn handle_split_request(
    State(state): State<Arc<DiffServerState>>,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    let source = match parse_split_source(&params) {
        Ok(s) => s,
        Err(e) => return bad_request(e),
    };
    let path_raw = match params.get("path") {
        Some(v) => v,
        None => return bad_request("missing `path` query parameter"),
    };
    let path = match parse_diff_path(path_raw) {
        Some(p) => p,
        None => return bad_request(format!("invalid path: {path_raw}")),
    };

    let (old_ref, new_ref) = split_refs(&source);
    let repo_root = &state.project_root;

    // Pull the parsed DiffFile (hunks + rename info). For untracked status,
    // the diff loader may return None when the file is brand-new; in that
    // case we render right-only from the working-tree contents only.
    let diff_file = match load_split_diff_file(repo_root, &source, &path).await {
        Ok(f) => f,
        Err(e) => return bad_request(e),
    };

    // For renames the old side reads from `old_path`, not `path`.
    let old_read_path: String = diff_file
        .as_ref()
        .and_then(|f| f.old_path.clone())
        .unwrap_or_else(|| path.clone());

    // Commit on the root: parent ref doesn't exist. Catch the read error and
    // fall back to right-only rather than 400ing the page.
    let old_lines: Option<Vec<String>> =
        read_full_file_at_ref(repo_root, old_ref.as_deref(), &old_read_path)
            .await
            .unwrap_or_default();
    let new_lines = match read_full_file_at_ref(repo_root, new_ref.as_deref(), &path).await {
        Ok(v) => v,
        Err(e) => return bad_request(e),
    };

    // Synthesize a minimal DiffFile when the per-source loader had nothing
    // to say (e.g. brand-new untracked file). build_split_rows still needs
    // an instance to look at `hunks`/`old_path`/`binary`.
    let diff_file = diff_file.unwrap_or_else(|| DiffFile {
        path: path.clone(),
        old_path: None,
        status: if old_lines.is_none() {
            FileStatus::Added
        } else if new_lines.is_none() {
            FileStatus::Deleted
        } else {
            FileStatus::Modified
        },
        binary: false,
        hunks: Vec::new(),
        additions: 0,
        deletions: 0,
    });

    let (ctx, repo_url, default_branch) =
        crate::command::gargo_preview_server::resolve_page_context(repo_root).await;
    let active_tab = match &source {
        SplitSource::Status { .. } => "status",
        SplitSource::Compare { .. } => "branches",
        SplitSource::Commit { .. } => "commits",
    };
    let rail = crate::command::app_shell::app_rail_html(&ctx, repo_url.as_deref(), active_tab);
    let ctx_script = repo_ctx_script(&ctx, repo_url.as_deref(), default_branch.as_deref());
    let back_url = split_back_url(&source, &ctx);

    let path_label = match &diff_file.old_path {
        Some(op) if op != &diff_file.path => {
            format!("{} → {}", html_escape(op), html_escape(&diff_file.path))
        }
        _ => html_escape(&path),
    };
    let path_label = format!(
        "{} {}",
        path_label,
        crate::command::app_shell::open_actions_html(
            &ctx,
            &path,
            repo_url.as_deref(),
            default_branch.as_deref(),
        )
    );
    let refs_label = format!(
        "{} → {}",
        html_escape(&ref_label(old_ref.as_deref())),
        html_escape(&ref_label(new_ref.as_deref())),
    );

    // Decide the page body. Binary, oversize, and one-sided pages get a
    // notice banner above the grid (or in place of it).
    let (notice_html, body_html) = if diff_file.binary {
        (
            r#"<div class="split-notice">Binary file — side-by-side view is not available.</div>"#
                .to_string(),
            String::new(),
        )
    } else {
        let old_len = old_lines.as_ref().map(|v| v.len()).unwrap_or(0);
        let new_len = new_lines.as_ref().map(|v| v.len()).unwrap_or(0);
        if old_len + new_len > SPLIT_MAX_LINES {
            (
                format!(
                    r#"<div class="split-notice">File too large for split view ({} + {} = {} lines, cap {}). Use the standard diff page.</div>"#,
                    old_len,
                    new_len,
                    old_len + new_len,
                    SPLIT_MAX_LINES
                ),
                String::new(),
            )
        } else {
            let rows = build_split_rows(old_lines.as_deref(), new_lines.as_deref(), &diff_file);
            let old_hl = old_lines
                .as_deref()
                .and_then(|l| build_line_highlights(l, &old_read_path));
            let new_hl = new_lines
                .as_deref()
                .and_then(|l| build_line_highlights(l, &path));
            let body = render_split_html(&rows, old_hl.as_ref(), new_hl.as_ref());
            let mut notice = String::new();
            if old_lines.is_none() {
                notice = r#"<div class="split-notice">Old version not present (added or untracked) — only the new side is shown.</div>"#.to_string();
            } else if new_lines.is_none() {
                notice = r#"<div class="split-notice">New version not present (deleted) — only the old side is shown.</div>"#.to_string();
            }
            (notice, body)
        }
    };

    let html = SPLIT_HTML_TEMPLATE
        .replace("{{PATH}}", &html_escape(&path))
        .replace("{{PATH_LABEL}}", &path_label)
        .replace("{{REFS_LABEL}}", &refs_label)
        .replace("{{APP_RAIL}}", &rail)
        .replace("{{REPO_CTX_SCRIPT}}", &ctx_script)
        .replace(
            "{{SHARED_CSS}}",
            &crate::command::server_shared::shared_css_link(),
        )
        .replace(
            "{{SHORTCUTS_JS}}",
            &crate::command::server_shared::shortcuts_js_tag(),
        )
        .replace("{{DIFF_STYLES}}", render_diff_styles())
        .replace("{{SPLIT_STYLES}}", render_split_styles())
        .replace("{{BACK_URL}}", &html_escape(&back_url))
        .replace("{{NOTICE}}", &notice_html)
        .replace("{{SPLIT_BODY}}", &body_html);

    Html(html).into_response()
}
