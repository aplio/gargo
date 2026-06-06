//! Diff server for viewing git status and diffs in a browser with rich formatting.
//!
//! This module implements an HTTP server for a tig-like git status page.
//! It follows the async runtime pattern:
//! - Command enum for controlling the server
//! - Event enum for status updates
//! - Handle with mpsc channels for communication
//! - Worker that runs on separate thread with Tokio runtime

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, mpsc};
use std::thread;

use axum::{
    Router,
    extract::{Query, State},
    http::{HeaderValue, StatusCode, header},
    response::{Html, IntoResponse, Json, Response},
    routing::{get, post},
};
use tower_http::cors::CorsLayer;

use crate::command::diff_viewed::{PAGE_COMPARE, PAGE_STATUS, ViewedStore};
use crate::command::git_backend;
use crate::command::registry::{CommandContext, CommandEffect, CommandEntry, CommandRegistry};
use crate::diff_render::{
    DiffFile, DiffHighlights, FileStatus, LineHighlights, LineKind, content_hash_of,
    content_hash_of_bytes, parse_unified_diff, render_diff_styles, render_file_body_html,
    render_file_body_html_with_highlights,
};
use crate::input::action::{Action, AppAction, IntegrationAction};
use crate::split_render::{LineHl, build_split_rows, render_split_html, render_split_styles};
use crate::syntax::highlight::highlight_text;
use crate::syntax::language::{LanguageDef, LanguageRegistry};

/// Commands that can be sent to the diff server
#[derive(Debug, Clone)]
pub enum DiffServerCommand {
    Start {
        project_root: PathBuf,
        /// Optional override for gargo's data dir. Production callers pass
        /// `None` (uses `~/.local/share/gargo`); tests pass a temp dir so the
        /// viewed-state database stays isolated.
        data_dir: Option<PathBuf>,
    },
    Stop,
}

/// Events emitted by the diff server
#[derive(Debug, Clone)]
pub enum DiffServerEvent {
    Started { port: u16 },
    Stopped,
    Error(String),
}

/// Handle for communicating with the diff server worker thread
pub struct DiffServerHandle {
    pub command_tx: mpsc::Sender<DiffServerCommand>,
    pub event_rx: mpsc::Receiver<DiffServerEvent>,
    _worker_thread: Option<thread::JoinHandle<()>>,
}

impl DiffServerHandle {
    pub fn new() -> Result<Self, String> {
        let (command_tx, command_rx) = mpsc::channel();
        let (event_tx, event_rx) = mpsc::channel();

        let worker = DiffServerWorker {
            command_rx,
            event_tx,
            tokio_runtime: tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .map_err(|e| format!("Failed to build tokio runtime: {}", e))?,
            server_shutdown_tx: None,
        };

        let worker_thread = thread::Builder::new()
            .name("diff-server".to_string())
            .spawn(move || worker.run())
            .map_err(|e| format!("Failed to spawn worker thread: {}", e))?;

        Ok(Self {
            command_tx,
            event_rx,
            _worker_thread: Some(worker_thread),
        })
    }
}

/// Worker thread that manages the Tokio runtime and HTTP server
struct DiffServerWorker {
    command_rx: mpsc::Receiver<DiffServerCommand>,
    event_tx: mpsc::Sender<DiffServerEvent>,
    tokio_runtime: tokio::runtime::Runtime,
    server_shutdown_tx: Option<tokio::sync::oneshot::Sender<()>>,
}

impl DiffServerWorker {
    fn run(mut self) {
        loop {
            match self.command_rx.recv() {
                Ok(DiffServerCommand::Start {
                    project_root,
                    data_dir,
                }) => {
                    self.handle_start_server(project_root, data_dir);
                }
                Ok(DiffServerCommand::Stop) => self.handle_stop_server(),
                Err(_) => break, // Main thread exited
            }
        }
    }

    fn handle_start_server(&mut self, project_root: PathBuf, data_dir: Option<PathBuf>) {
        if self.server_shutdown_tx.is_some() {
            let _ = self
                .event_tx
                .send(DiffServerEvent::Error("Server already running".to_string()));
            return;
        }

        let listener = match self
            .tokio_runtime
            .block_on(tokio::net::TcpListener::bind("127.0.0.1:0"))
        {
            Ok(listener) => listener,
            Err(err) => {
                let _ = self.event_tx.send(DiffServerEvent::Error(format!(
                    "Failed to bind diff server on localhost: {}",
                    err
                )));
                return;
            }
        };
        let port = match listener.local_addr() {
            Ok(addr) => addr.port(),
            Err(err) => {
                let _ = self.event_tx.send(DiffServerEvent::Error(format!(
                    "Failed to read diff server local address: {}",
                    err
                )));
                return;
            }
        };

        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
        self.server_shutdown_tx = Some(shutdown_tx);

        let viewed = match data_dir {
            Some(dir) => ViewedStore::open_in_dir(&dir),
            None => ViewedStore::open(),
        };
        let server_state = Arc::new(DiffServerState {
            project_root: std::fs::canonicalize(&project_root).unwrap_or(project_root),
            viewed,
        });
        let event_tx = self.event_tx.clone();
        self.tokio_runtime.spawn(async move {
            run_server(listener, shutdown_rx, server_state).await;
        });

        let _ = event_tx.send(DiffServerEvent::Started { port });
    }

    fn handle_stop_server(&mut self) {
        if let Some(shutdown_tx) = self.server_shutdown_tx.take() {
            let _ = shutdown_tx.send(());
            let _ = self.event_tx.send(DiffServerEvent::Stopped);
        } else {
            let _ = self
                .event_tx
                .send(DiffServerEvent::Error("Server not running".to_string()));
        }
    }
}

pub(crate) struct DiffServerState {
    pub(crate) project_root: PathBuf,
    /// On-disk persistence for per-file "Viewed" checkboxes.
    pub(crate) viewed: ViewedStore,
}

impl DiffServerState {
    /// Stable key for this repo in the viewed-state database.
    fn repo_key(&self) -> String {
        self.project_root.to_string_lossy().to_string()
    }
}

/// HTML template with diff2html integration
const DIFF_HTML_TEMPLATE: &str = include_str!("../../assets/diff_server/diff.html");

const COMMIT_HTML_TEMPLATE: &str = include_str!("../../assets/diff_server/commit.html");

/// HTML template for the compare-branches page.
const COMPARE_HTML_TEMPLATE: &str = include_str!("../../assets/diff_server/compare.html");

/// Side-by-side ("split view") page template. Server-rendered: the body
/// is built in Rust and embedded inline, no XHR. Keyboard handling
/// (`j`/`k`/`n`/`p`/`gg`/`G`/`q`) lives in the shared `SHORTCUTS_JS`.
const SPLIT_HTML_TEMPLATE: &str = include_str!("../../assets/diff_server/split.html");

/// Origin of a split-view request. Determines which refs to read the old
/// and new file contents from, plus what the "Back" link points to.
#[derive(Debug, Clone)]
enum SplitSource {
    Status { section: String },
    Compare { base: String, compare: String },
    Commit { hash: String },
}

fn parse_split_source(params: &HashMap<String, String>) -> Result<SplitSource, String> {
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
fn split_refs(source: &SplitSource) -> (Option<String>, Option<String>) {
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
fn parse_commit_hash_value(hash: &str) -> Option<String> {
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
async fn read_full_file_at_ref(
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
async fn load_split_diff_file(
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

fn ref_label(r: Option<&str>) -> String {
    match r {
        None => "working tree".to_string(),
        Some("") => "index".to_string(),
        Some(name) => name.to_string(),
    }
}

/// Soft cap on combined old + new line count. The split view loads the
/// whole file into the DOM in a single pass; beyond this much content the
/// browser stalls long enough to feel broken.
const SPLIT_MAX_LINES: usize = 50_000;

/// Build per-line highlight maps (keyed by 1-based line number) from the
/// full text of one side using the language inferred from `path`.
fn build_line_highlights(lines: &[String], path: &str) -> Option<LineHl> {
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

fn split_back_url(
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

/// Run the HTTP server
async fn run_server(
    listener: tokio::net::TcpListener,
    shutdown_rx: tokio::sync::oneshot::Receiver<()>,
    state: Arc<DiffServerState>,
) {
    let app = Router::new()
        .route(
            "/assets/server-shared.css",
            get(crate::command::gargo_preview_server::handle_shared_css_asset),
        )
        .route(
            "/assets/server-shortcuts.js",
            get(crate::command::gargo_preview_server::handle_shortcuts_js_asset),
        )
        .route("/diff", get(handle_html_request))
        .route("/compare", get(handle_compare_html_request))
        .route("/split", get(handle_split_request))
        .route("/commit", get(handle_commit_html_request))
        .route("/api/status", get(handle_api_status_request))
        .route("/api/status/file", get(handle_api_status_file_request))
        .route("/api/status/viewed", post(handle_api_status_viewed_request))
        .route("/api/status/stage", post(handle_api_status_stage_request))
        .route(
            "/api/status/unstage",
            post(handle_api_status_unstage_request),
        )
        .route(
            "/api/status/commit-prepare",
            get(handle_api_commit_prepare_request),
        )
        .route("/api/status/commit", post(handle_api_commit_request))
        .route(
            "/api/status/context",
            get(handle_api_status_context_request),
        )
        .route("/api/branches", get(handle_api_branches_request))
        .route("/api/compare", get(handle_api_compare_request))
        .route("/api/compare/file", get(handle_api_compare_file_request))
        .route(
            "/api/compare/context",
            get(handle_api_compare_context_request),
        )
        .route(
            "/api/compare/viewed",
            post(handle_api_compare_viewed_request),
        )
        .with_state(state)
        .layer(CorsLayer::permissive());

    let _ = axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = shutdown_rx.await;
        })
        .await;
}

/// Serve the HTML page with diff2html
pub(crate) async fn handle_html_request(
    State(state): State<Arc<DiffServerState>>,
) -> impl IntoResponse {
    use crate::command::gargo_preview_server as gh;
    let root_path = state.project_root.display().to_string();
    let (ctx, repo_url, default_branch) = gh::resolve_page_context(&state.project_root).await;
    let rail = crate::command::app_shell::app_rail_html(&ctx, repo_url.as_deref(), "status");
    let ctx_script = repo_ctx_script(&ctx, repo_url.as_deref(), default_branch.as_deref());
    Html(
        DIFF_HTML_TEMPLATE
            .replace("{{ROOT_PATH}}", &html_escape(&root_path))
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
            .replace("{{DIFF_STYLES}}", render_diff_styles()),
    )
}

/// Serve the commit page: a focused view that lists the staged files and takes
/// a commit message + optional amend, then POSTs to `/api/status/commit`.
pub(crate) async fn handle_commit_html_request(
    State(state): State<Arc<DiffServerState>>,
) -> impl IntoResponse {
    use crate::command::gargo_preview_server as gh;
    let (ctx, repo_url, default_branch) = gh::resolve_page_context(&state.project_root).await;
    // Keep "Status" highlighted in the rail — the commit page is part of that flow.
    let rail = crate::command::app_shell::app_rail_html(&ctx, repo_url.as_deref(), "status");
    let ctx_script = repo_ctx_script(&ctx, repo_url.as_deref(), default_branch.as_deref());
    Html(
        COMMIT_HTML_TEMPLATE
            .replace("{{APP_RAIL}}", &rail)
            .replace("{{REPO_CTX_SCRIPT}}", &ctx_script)
            .replace(
                "{{SHARED_CSS}}",
                &crate::command::server_shared::shared_css_link(),
            )
            .replace(
                "{{SHORTCUTS_JS}}",
                &crate::command::server_shared::shortcuts_js_tag(),
            ),
    )
}

fn parse_bool_param(value: Option<&String>, default: bool) -> bool {
    match value.map(|v| v.as_str()) {
        Some("true") => true,
        Some("false") => false,
        _ => default,
    }
}

fn html_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

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
async fn scan_untracked_file(repo_root: &Path, rel_path: &str) -> (usize, bool, String) {
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

async fn git_output_from_command(
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

/// API endpoint that returns unstaged/staged diffs and untracked files.
pub(crate) async fn handle_api_status_request(
    State(state): State<Arc<DiffServerState>>,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    let show_untracked = parse_bool_param(params.get("show_untracked"), true);
    let repo_root = &state.project_root;

    let (unstaged_res, staged_res) = (
        status_diff_text(repo_root, false),
        status_diff_text(repo_root, true),
    );
    let viewed = load_viewed_map(&state, PAGE_STATUS, String::new(), String::new()).await;
    let unstaged_raw = match unstaged_res {
        Ok(output) => output,
        Err(error) => return bad_request(error),
    };
    let staged_raw = match staged_res {
        Ok(output) => output,
        Err(error) => return bad_request(error),
    };

    let unstaged_files: Vec<serde_json::Value> = parse_unified_diff(&unstaged_raw)
        .iter()
        .map(|f| file_metadata_json(f, diff_file_is_viewed(&viewed, "unstaged", f)))
        .collect();
    let staged_files: Vec<serde_json::Value> = parse_unified_diff(&staged_raw)
        .iter()
        .map(|f| file_metadata_json(f, diff_file_is_viewed(&viewed, "staged", f)))
        .collect();

    let untracked_files: Vec<serde_json::Value> = if show_untracked {
        let paths: Vec<String> = match git_backend::status_files(repo_root) {
            Some((changed, _)) => changed
                .into_iter()
                .filter(|entry| entry.status_char == '?')
                .map(|entry| entry.path)
                .collect(),
            None => return bad_request("failed to read git status"),
        };

        // Scan every untracked file concurrently — each scan is independent
        // file I/O, so a serial loop needlessly waited on one read at a time.
        // Results are slotted back by index to preserve `ls-files` order.
        let mut set = tokio::task::JoinSet::new();
        for (idx, path) in paths.iter().cloned().enumerate() {
            let root = repo_root.to_path_buf();
            set.spawn(async move { (idx, scan_untracked_file(&root, &path).await) });
        }
        let mut scans: Vec<(usize, bool, String)> = vec![(0, false, String::new()); paths.len()];
        while let Some(joined) = set.join_next().await {
            if let Ok((idx, scan)) = joined {
                scans[idx] = scan;
            }
        }

        paths
            .iter()
            .zip(scans)
            .map(|(path, (additions, binary, hash))| {
                // A whole untracked file shows up as an all-additions diff, so
                // its line count drives the client's huge-diff collapse decision.
                let is_viewed = viewed
                    .get(&("untracked".to_string(), path.to_string()))
                    .is_some_and(|stored| !hash.is_empty() && *stored == hash);
                serde_json::json!({
                    "path": path,
                    "old_path": serde_json::Value::Null,
                    "status": "untracked",
                    "binary": binary,
                    "additions": additions,
                    "deletions": 0,
                    "viewed": is_viewed,
                })
            })
            .collect()
    } else {
        Vec::new()
    };

    ok_json(serde_json::json!({
        "unstaged": unstaged_files,
        "staged": staged_files,
        "untracked": untracked_files,
    }))
}

pub(crate) async fn handle_api_status_file_request(
    State(state): State<Arc<DiffServerState>>,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    let section = match params.get("section").map(String::as_str) {
        Some(s) if matches!(s, "staged" | "unstaged" | "untracked") => s,
        _ => return bad_request("missing or invalid `section` query parameter"),
    };
    let path_raw = match params.get("path") {
        Some(v) => v,
        None => return bad_request("missing `path` query parameter"),
    };
    let path = match parse_diff_path(path_raw) {
        Some(p) => p,
        None => return bad_request(format!("invalid path: {}", path_raw)),
    };
    let file = match load_status_diff_file(&state.project_root, section, &path).await {
        Ok(file) => file,
        Err(e) => return bad_request(e),
    };
    match file {
        Some(file) => {
            let html = render_highlighted(&file);
            ok_json(serde_json::json!({
                "path": file.path,
                "status": file.status.as_str(),
                "additions": file.additions,
                "deletions": file.deletions,
                "binary": file.binary,
                "html": html,
            }))
        }
        None => ok_json(serde_json::json!({
            "path": path,
            "status": section,
            "additions": 0,
            "deletions": 0,
            "binary": false,
            "html": empty_diff_html(),
        })),
    }
}

/// Fetch N lines from a file at a given git ref (or working tree).
///
/// `git_ref = Some("")` reads from the index (`git show :path`),
/// `Some("HEAD")` from HEAD, etc.; `None` reads from the working tree.
/// Lines are returned with their original content (newlines stripped).
async fn read_file_range_at_ref(
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

fn parse_usize_param(params: &HashMap<String, String>, key: &str) -> Result<usize, String> {
    params
        .get(key)
        .ok_or_else(|| format!("missing `{}` query parameter", key))?
        .parse::<usize>()
        .map_err(|e| format!("invalid `{}`: {}", key, e))
}

pub(crate) async fn handle_api_status_context_request(
    State(state): State<Arc<DiffServerState>>,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    let git_ref: Option<&str> = match params.get("section").map(String::as_str) {
        Some("staged") => Some("HEAD"),
        Some("unstaged") | Some("untracked") | None => None,
        _ => return bad_request("invalid `section` query parameter"),
    };
    let path_raw = match params.get("path") {
        Some(v) => v,
        None => return bad_request("missing `path` query parameter"),
    };
    let path = match parse_diff_path(path_raw) {
        Some(p) => p,
        None => return bad_request(format!("invalid path: {}", path_raw)),
    };
    let start = match parse_usize_param(&params, "start") {
        Ok(v) => v,
        Err(e) => return bad_request(e),
    };
    let end = match parse_usize_param(&params, "end") {
        Ok(v) => v,
        Err(e) => return bad_request(e),
    };
    match read_file_range_at_ref(&state.project_root, git_ref, &path, start, end).await {
        Ok(lines) => ok_json(serde_json::json!({ "lines": lines })),
        Err(e) => bad_request(e),
    }
}

pub(crate) async fn handle_api_compare_context_request(
    State(state): State<Arc<DiffServerState>>,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    let path_raw = match params.get("path") {
        Some(v) => v,
        None => return bad_request("missing `path` query parameter"),
    };
    let path = match parse_diff_path(path_raw) {
        Some(p) => p,
        None => return bad_request(format!("invalid path: {}", path_raw)),
    };
    let git_ref = match params.get("ref") {
        Some(v) if !v.is_empty() => v.as_str(),
        _ => return bad_request("missing `ref` query parameter"),
    };
    let start = match parse_usize_param(&params, "start") {
        Ok(v) => v,
        Err(e) => return bad_request(e),
    };
    let end = match parse_usize_param(&params, "end") {
        Ok(v) => v,
        Err(e) => return bad_request(e),
    };
    match read_file_range_at_ref(&state.project_root, Some(git_ref), &path, start, end).await {
        Ok(lines) => ok_json(serde_json::json!({ "lines": lines })),
        Err(e) => bad_request(e),
    }
}

/// Run the per-file `git diff` for a status section and return the parsed
/// [`DiffFile`], if any. Shared by the file-HTML and set-viewed endpoints so
/// both hash and render over identical data.
async fn load_status_diff_file(
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

fn status_diff_text(repo_root: &Path, staged: bool) -> Result<String, String> {
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

#[derive(serde::Deserialize)]
pub(crate) struct StatusViewedRequest {
    section: String,
    path: String,
    viewed: bool,
}

/// POST endpoint: persist the "Viewed" checkbox for one status-page file.
///
/// When `viewed` is true the file's current content hash is computed and
/// stored, so the checkbox is later honored only while the content matches.
pub(crate) async fn handle_api_status_viewed_request(
    State(state): State<Arc<DiffServerState>>,
    Json(req): Json<StatusViewedRequest>,
) -> Response {
    let section = match req.section.as_str() {
        s @ ("staged" | "unstaged" | "untracked") => s,
        _ => return bad_request("missing or invalid `section`"),
    };
    let path = match parse_diff_path(&req.path) {
        Some(p) => p,
        None => return bad_request(format!("invalid path: {}", req.path)),
    };

    if !req.viewed {
        store_viewed(
            &state,
            PAGE_STATUS,
            String::new(),
            String::new(),
            section.to_string(),
            path,
            None,
        )
        .await;
        return ok_json(serde_json::json!({ "viewed": false }));
    }

    // Pin the viewed record to the file's current content.
    let hash = if section == "untracked" {
        let (_, _, h) = scan_untracked_file(&state.project_root, &path).await;
        h
    } else {
        match load_status_diff_file(&state.project_root, section, &path).await {
            Ok(Some(file)) => content_hash_of(&file),
            Ok(None) => String::new(),
            Err(e) => return bad_request(e),
        }
    };
    if hash.is_empty() {
        // No content to anchor the record to (e.g. the file vanished).
        return ok_json(serde_json::json!({ "viewed": false }));
    }
    store_viewed(
        &state,
        PAGE_STATUS,
        String::new(),
        String::new(),
        section.to_string(),
        path,
        Some(hash),
    )
    .await;
    ok_json(serde_json::json!({ "viewed": true }))
}

#[derive(serde::Deserialize)]
pub(crate) struct StagePathRequest {
    path: String,
}

/// Content hash anchoring a "Viewed" record for `path` as rendered in
/// `section`, or `None` when the file has no content in that section right now
/// (e.g. a fully-staged file no longer appears under `unstaged`).
async fn viewed_hash_for_section(
    state: &Arc<DiffServerState>,
    section: &str,
    path: &str,
) -> Option<String> {
    if section == "untracked" {
        let (_, _, h) = scan_untracked_file(&state.project_root, path).await;
        return (!h.is_empty()).then_some(h);
    }
    match load_status_diff_file(&state.project_root, section, path).await {
        Ok(Some(file)) => Some(content_hash_of(&file)),
        _ => None,
    }
}

/// The first of `from_sections` in which `path` is genuinely viewed *right now*
/// — a stored record whose hash still matches the file's content. Call this
/// before a stage/unstage so the checkbox carries over, while a record left
/// stale by an edit (content no longer matches) is not silently revived.
async fn viewed_source_section(
    state: &Arc<DiffServerState>,
    path: &str,
    from_sections: &[&'static str],
) -> Option<&'static str> {
    let viewed = load_viewed_map(state, PAGE_STATUS, String::new(), String::new()).await;
    for &section in from_sections {
        if let Some(stored) = viewed.get(&(section.to_string(), path.to_string()))
            && viewed_hash_for_section(state, section, path)
                .await
                .as_deref()
                == Some(stored.as_str())
        {
            return Some(section);
        }
    }
    None
}

/// Move a file's "Viewed" record from `from` to the first of `to_sections`
/// whose representation now has content, re-pinning it to a freshly computed
/// hash. The staged (`index..HEAD`) and unstaged (`worktree..index`) diffs hash
/// differently, so the record must be re-anchored rather than copied verbatim.
/// Call this after the stage/unstage git op has run.
async fn move_viewed_record(
    state: &Arc<DiffServerState>,
    path: &str,
    from: &str,
    to_sections: &[&'static str],
) {
    store_viewed(
        state,
        PAGE_STATUS,
        String::new(),
        String::new(),
        from.to_string(),
        path.to_string(),
        None,
    )
    .await;
    for &to in to_sections {
        if let Some(hash) = viewed_hash_for_section(state, to, path).await {
            store_viewed(
                state,
                PAGE_STATUS,
                String::new(),
                String::new(),
                to.to_string(),
                path.to_string(),
                Some(hash),
            )
            .await;
            return;
        }
    }
}

/// POST endpoint: stage one file (`git add -- <path>`). Works for modified,
/// deleted, and untracked paths alike — `git add` records each appropriately.
pub(crate) async fn handle_api_status_stage_request(
    State(state): State<Arc<DiffServerState>>,
    Json(req): Json<StagePathRequest>,
) -> Response {
    let path = match parse_diff_path(&req.path) {
        Some(p) => p,
        None => return bad_request(format!("invalid path: {}", req.path)),
    };
    // Capture whether the file is viewed before it hops sections, then carry the
    // checkbox over to the staged section once the move has happened.
    let viewed_from = viewed_source_section(&state, &path, &["unstaged", "untracked"]).await;
    match git_output_in_repo(&state.project_root, &["add", "--", &path]).await {
        Ok(_) => {
            if let Some(from) = viewed_from {
                move_viewed_record(&state, &path, from, &["staged"]).await;
            }
            ok_json(serde_json::json!({ "ok": true }))
        }
        Err(e) => bad_request(e),
    }
}

/// POST endpoint: unstage one file. `git reset -- <path>` restores the index
/// entry from HEAD (and works before the first commit, where it just removes
/// the path from the index).
pub(crate) async fn handle_api_status_unstage_request(
    State(state): State<Arc<DiffServerState>>,
    Json(req): Json<StagePathRequest>,
) -> Response {
    let path = match parse_diff_path(&req.path) {
        Some(p) => p,
        None => return bad_request(format!("invalid path: {}", req.path)),
    };
    // Carry the "Viewed" checkbox back to wherever the file lands after the
    // reset: a tracked file returns to `unstaged`, a freshly-added one to
    // `untracked`.
    let viewed_from = viewed_source_section(&state, &path, &["staged"]).await;
    match git_output_in_repo(&state.project_root, &["reset", "--quiet", "--", &path]).await {
        Ok(_) => {
            if let Some(from) = viewed_from {
                move_viewed_record(&state, &path, from, &["unstaged", "untracked"]).await;
            }
            ok_json(serde_json::json!({ "ok": true }))
        }
        Err(e) => bad_request(e),
    }
}

/// GET endpoint backing the commit page: the list of staged files, the current
/// branch, and HEAD's subject+body (so the amend toggle can prefill it).
pub(crate) async fn handle_api_commit_prepare_request(
    State(state): State<Arc<DiffServerState>>,
) -> Response {
    let repo_root = &state.project_root;
    let staged_raw = match status_diff_text(repo_root, true) {
        Ok(output) => output,
        Err(error) => return bad_request(error),
    };
    let staged: Vec<serde_json::Value> = parse_unified_diff(&staged_raw)
        .iter()
        .map(|f| file_metadata_json(f, false))
        .collect();

    let branch = git_backend::current_branch(repo_root).unwrap_or_default();

    // HEAD's full message for the amend toggle to prefill. Empty before the
    // first commit, in which case amend is not offered.
    let head_meta = git_backend::commit_meta(repo_root, "HEAD");
    let last_message = head_meta
        .as_ref()
        .map(|m| m.message.trim_end().to_string())
        .unwrap_or_default();
    let has_head = head_meta.is_some();

    ok_json(serde_json::json!({
        "staged": staged,
        "branch": branch,
        "last_message": last_message,
        "has_head": has_head,
    }))
}

#[derive(serde::Deserialize)]
pub(crate) struct CommitRequest {
    message: String,
    #[serde(default)]
    amend: bool,
}

/// POST endpoint: create a commit from the staged changes. With `amend` it
/// rewrites HEAD instead. The message is passed via stdin-free `-m`, and the
/// commit runs with `--cleanup=strip` so trailing whitespace is normalized.
pub(crate) async fn handle_api_commit_request(
    State(state): State<Arc<DiffServerState>>,
    Json(req): Json<CommitRequest>,
) -> Response {
    let message = req.message.trim().to_string();
    if message.is_empty() {
        return bad_request("commit message must not be empty");
    }

    let mut args: Vec<&str> = vec!["commit", "--cleanup=strip"];
    if req.amend {
        args.push("--amend");
    }
    // `-m` consumes the next argument literally, so the message can never be
    // parsed as a flag even if it begins with `-`.
    args.push("-m");
    args.push(&message);

    match git_output_in_repo(&state.project_root, &args).await {
        Ok(_) => ok_json(serde_json::json!({ "ok": true })),
        Err(e) => bad_request(e),
    }
}

pub(crate) fn file_metadata_json(file: &DiffFile, viewed: bool) -> serde_json::Value {
    serde_json::json!({
        "path": file.path,
        "old_path": file.old_path,
        "status": file.status.as_str(),
        "binary": file.binary,
        "additions": file.additions,
        "deletions": file.deletions,
        "viewed": viewed,
    })
}

/// `(section, path) -> stored content hash` for one page / branch context.
type ViewedMap = HashMap<(String, String), String>;

/// Load every viewed-file record for a page / branch context off the async
/// runtime, since a contended SQLite read can block briefly on `busy_timeout`.
async fn load_viewed_map(
    state: &Arc<DiffServerState>,
    page: &'static str,
    base_ref: String,
    compare_ref: String,
) -> ViewedMap {
    let state = state.clone();
    tokio::task::spawn_blocking(move || {
        state
            .viewed
            .viewed_map(&state.repo_key(), page, &base_ref, &compare_ref)
    })
    .await
    .unwrap_or_default()
}

/// Persist (or, with `hash == None`, clear) a file's viewed record off the
/// async runtime. Best-effort: failures leave the viewed state unpersisted.
#[allow(clippy::too_many_arguments)]
async fn store_viewed(
    state: &Arc<DiffServerState>,
    page: &'static str,
    base_ref: String,
    compare_ref: String,
    section: String,
    path: String,
    hash: Option<String>,
) {
    let state = state.clone();
    let _ = tokio::task::spawn_blocking(move || {
        let key = state.repo_key();
        match hash {
            Some(h) => state
                .viewed
                .set(&key, page, &base_ref, &compare_ref, &section, &path, &h),
            None => state
                .viewed
                .unset(&key, page, &base_ref, &compare_ref, &section, &path),
        }
    })
    .await;
}

/// Whether `file`'s current content matches its stored viewed record.
fn diff_file_is_viewed(viewed: &ViewedMap, section: &str, file: &DiffFile) -> bool {
    viewed
        .get(&(section.to_string(), file.path.clone()))
        .is_some_and(|stored| *stored == content_hash_of(file))
}

pub(crate) fn empty_diff_html() -> String {
    r#"<div class="gr-diff-body"><div class="gr-line gr-line-hunk"><span class="gr-ln"></span><span class="gr-lnr"></span><span class="gr-sign"></span><span class="gr-text">(no content changes)</span></div></div>"#
        .to_string()
}

/// Render `file` to HTML, applying tree-sitter syntax highlighting when
/// the file's extension maps to a known language. Falls back to plain
/// rendering for unknown languages, binary files, or rename-only entries.
pub(crate) fn render_highlighted(file: &DiffFile) -> String {
    if file.binary || file.hunks.is_empty() {
        return render_file_body_html(file);
    }
    let registry = LanguageRegistry::new();
    let Some(lang) = registry.detect_by_extension(&file.path) else {
        return render_file_body_html(file);
    };
    let highlights = compute_diff_highlights(file, lang);
    render_file_body_html_with_highlights(file, &highlights)
}

/// Reconstruct the new- and old-side line streams for `file`, run
/// `highlight_text` over each, and translate the per-row span maps back
/// into `(hunk_idx, line_idx) → LineHighlights`.
///
/// Single-line fragments don't parse cleanly under tree-sitter (`fn foo(`
/// alone isn't valid Rust), so we feed both sides as a single body each.
/// Context lines receive their spans from the new-side pass; the old-side
/// pass only attaches to actual Remove lines.
fn compute_diff_highlights(file: &DiffFile, lang: &LanguageDef) -> DiffHighlights {
    let mut result: DiffHighlights = HashMap::new();

    // New side: Context + Add.
    let mut new_text = String::new();
    let mut new_map: Vec<(usize, usize)> = Vec::new();
    for (hi, hunk) in file.hunks.iter().enumerate() {
        for (li, line) in hunk.lines.iter().enumerate() {
            if matches!(line.kind, LineKind::Context | LineKind::Add) {
                new_map.push((hi, li));
                new_text.push_str(&line.content);
                new_text.push('\n');
            }
        }
    }
    if !new_text.is_empty() {
        let spans_per_row = highlight_text(&new_text, lang);
        for (row, key) in new_map.iter().enumerate() {
            let Some(spans) = spans_per_row.get(&row) else {
                continue;
            };
            let content_len = file.hunks[key.0].lines[key.1].content.len();
            let entry = result.entry(*key).or_default();
            for s in spans {
                let start = s.start.min(content_len);
                let end = s.end.min(content_len);
                if start < end {
                    entry.spans.push((start, end, s.capture_name.clone()));
                }
            }
        }
    }

    // Old side: Context + Remove, but only attach to Remove lines.
    let mut old_text = String::new();
    let mut old_map: Vec<(usize, usize)> = Vec::new();
    for (hi, hunk) in file.hunks.iter().enumerate() {
        for (li, line) in hunk.lines.iter().enumerate() {
            if matches!(line.kind, LineKind::Context | LineKind::Remove) {
                old_map.push((hi, li));
                old_text.push_str(&line.content);
                old_text.push('\n');
            }
        }
    }
    if !old_text.is_empty() {
        let spans_per_row = highlight_text(&old_text, lang);
        for (row, key) in old_map.iter().enumerate() {
            let (hi, li) = *key;
            if file.hunks[hi].lines[li].kind != LineKind::Remove {
                continue;
            }
            let Some(spans) = spans_per_row.get(&row) else {
                continue;
            };
            let content_len = file.hunks[hi].lines[li].content.len();
            let entry = result.entry(*key).or_default();
            for s in spans {
                let start = s.start.min(content_len);
                let end = s.end.min(content_len);
                if start < end {
                    entry.spans.push((start, end, s.capture_name.clone()));
                }
            }
        }
    }

    result
}

/// Validate a relative path used as a git diff argument.
///
/// We always pass paths after `--` so flag injection is structurally blocked,
/// but we still reject control characters, path traversal, absolute paths,
/// and unreasonably long inputs to keep the API surface tight.
pub(crate) fn parse_diff_path(value: &str) -> Option<String> {
    if value.is_empty() || value.len() > 4096 {
        return None;
    }
    if value.starts_with('-') || value.starts_with('/') {
        return None;
    }
    if value.contains('\0') || value.contains('\n') || value.contains('\r') {
        return None;
    }
    for segment in value.split('/') {
        if segment == ".." {
            return None;
        }
    }
    Some(value.to_string())
}

/// Validate a git branch name to block flag injection and command injection.
///
/// Accepts the conservative subset `[A-Za-z0-9._/\-]`, rejects names that start
/// with `-` (so they can never be parsed as a git CLI flag), and caps the length
/// to bound the work git has to do on a malicious input.
fn parse_branch_name(value: &str) -> Option<String> {
    if value.is_empty() || value.len() > 256 {
        return None;
    }
    if value.starts_with('-') {
        return None;
    }
    let ok = value
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '/' | '-'));
    if !ok {
        return None;
    }
    Some(value.to_string())
}

pub(crate) fn bad_request(message: impl Into<String>) -> Response {
    let payload = serde_json::json!({ "error": message.into() });
    let mut response = (StatusCode::BAD_REQUEST, Json(payload)).into_response();
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("no-store, no-cache, must-revalidate"),
    );
    response
}

pub(crate) fn ok_json(payload: serde_json::Value) -> Response {
    let mut response = (StatusCode::OK, Json(payload)).into_response();
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("no-store, no-cache, must-revalidate"),
    );
    response
}

fn repo_ctx_script(
    ctx: &crate::command::gargo_preview_server::RepoUrlContext,
    github_base: Option<&str>,
    default_branch: Option<&str>,
) -> String {
    crate::command::server_shared::repo_ctx_script(
        &ctx.owner,
        &ctx.repo,
        &ctx.branch,
        github_base,
        default_branch,
    )
}

/// Serve the compare-branches HTML page.
pub(crate) async fn handle_compare_html_request(
    State(state): State<Arc<DiffServerState>>,
) -> impl IntoResponse {
    use crate::command::gargo_preview_server as gh;
    let root_path = state.project_root.display().to_string();
    let (ctx, repo_url, default_branch) = gh::resolve_page_context(&state.project_root).await;
    let rail = crate::command::app_shell::app_rail_html(&ctx, repo_url.as_deref(), "branches");
    let ctx_script = repo_ctx_script(&ctx, repo_url.as_deref(), default_branch.as_deref());
    Html(
        COMPARE_HTML_TEMPLATE
            .replace("{{ROOT_PATH}}", &html_escape(&root_path))
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
            .replace("{{DIFF_STYLES}}", render_diff_styles()),
    )
}

/// List local and remote branches in the repo along with the current HEAD.
///
/// `for-each-ref` lets us tell which side a ref came from via its full
/// `refname` (so callers can compare e.g. `origin/master` against a local
/// branch without ambiguity), and lets us skip the `*/HEAD` symbolic refs
/// that would otherwise duplicate a remote's default branch.
pub(crate) async fn handle_api_branches_request(
    State(state): State<Arc<DiffServerState>>,
) -> Response {
    // Listing refs + reading `origin/HEAD` is in-process gix work (no subprocess);
    // it still touches the ref store on disk, so run it on the blocking pool.
    let repo_root = state.project_root.clone();
    let result = tokio::task::spawn_blocking(move || {
        let list = crate::command::git_backend::list_branches(&repo_root)?;
        let origin_head = crate::command::git_backend::origin_head_short(&repo_root);
        Some((list, origin_head))
    })
    .await;

    let Ok(Some((list, origin_head))) = result else {
        return bad_request("not a git repository");
    };

    let default = resolve_default_from(origin_head, &list.branches);

    ok_json(serde_json::json!({
        "current": list.current,
        "default": default,
        "branches": list.branches,
        "remotes": list.remotes,
    }))
}

/// Best-effort detection of the repository's default branch from an already-fetched
/// `origin/HEAD` symbolic-ref output (pass `None` when the probe failed).
///
/// Tries `origin/HEAD` first (set by `git clone` or `git remote set-head`), then
/// falls back to the well-known `main` / `master` names if either exists
/// locally. Returns `None` only for repos without remote and without either
/// conventional name. Kept as a pure function so the `symbolic-ref` spawn can run
/// concurrently with `for-each-ref` in the caller.
fn resolve_default_from(origin_head: Option<String>, known: &[String]) -> Option<String> {
    if let Some(output) = origin_head {
        let trimmed = output.trim();
        if let Some(rest) = trimmed.strip_prefix("origin/")
            && !rest.is_empty()
            && known.iter().any(|b| b == rest)
        {
            return Some(rest.to_string());
        }
    }
    for candidate in ["main", "master"] {
        if known.iter().any(|b| b == candidate) {
            return Some(candidate.to_string());
        }
    }
    None
}

/// Compute `git diff base...compare` for the requested branches.
pub(crate) async fn handle_api_compare_request(
    State(state): State<Arc<DiffServerState>>,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    let (base, compare) = match parse_compare_branches(&params) {
        Ok(v) => v,
        Err(resp) => return resp,
    };

    // In-process gix `base...compare` diff (no `git diff` subprocess).
    let repo_root = state.project_root.clone();
    let (base_c, compare_c) = (base.clone(), compare.clone());
    let diff = tokio::task::spawn_blocking(move || {
        crate::command::git_backend::compare_diff_text(&repo_root, &base_c, &compare_c, None)
    })
    .await
    .ok()
    .flatten();
    let diff = match diff {
        Some(output) => output,
        None => return bad_request("invalid base/compare ref"),
    };

    // Viewed records are scoped to this exact base/compare pair, so switching
    // either branch naturally resets the checkboxes.
    let viewed = load_viewed_map(&state, PAGE_COMPARE, base.clone(), compare.clone()).await;
    let files: Vec<serde_json::Value> = parse_unified_diff(&diff)
        .iter()
        .map(|f| file_metadata_json(f, diff_file_is_viewed(&viewed, "", f)))
        .collect();

    ok_json(serde_json::json!({
        "base": base,
        "compare": compare,
        "files": files,
    }))
}

/// `base...compare` diff for a single file via in-process gix, parsed.
async fn load_compare_diff_file(
    repo_root: &Path,
    base: &str,
    compare: &str,
    path: &str,
) -> Result<Option<DiffFile>, String> {
    let (root, base, compare, path) = (
        repo_root.to_path_buf(),
        base.to_string(),
        compare.to_string(),
        path.to_string(),
    );
    let diff = tokio::task::spawn_blocking(move || {
        crate::command::git_backend::compare_diff_text(&root, &base, &compare, Some(&path))
    })
    .await
    .map_err(|e| e.to_string())?
    .ok_or_else(|| "invalid base/compare ref".to_string())?;
    Ok(parse_unified_diff(&diff).into_iter().next())
}

pub(crate) async fn handle_api_compare_file_request(
    State(state): State<Arc<DiffServerState>>,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    let (base, compare) = match parse_compare_branches(&params) {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let path_raw = match params.get("path") {
        Some(v) => v,
        None => return bad_request("missing `path` query parameter"),
    };
    let path = match parse_diff_path(path_raw) {
        Some(p) => p,
        None => return bad_request(format!("invalid path: {}", path_raw)),
    };

    let file = match load_compare_diff_file(&state.project_root, &base, &compare, &path).await {
        Ok(file) => file,
        Err(e) => return bad_request(e),
    };

    match file {
        Some(file) => {
            let html = render_highlighted(&file);
            ok_json(serde_json::json!({
                "path": file.path,
                "status": file.status.as_str(),
                "additions": file.additions,
                "deletions": file.deletions,
                "binary": file.binary,
                "html": html,
            }))
        }
        None => ok_json(serde_json::json!({
            "path": path,
            "status": "modified",
            "additions": 0,
            "deletions": 0,
            "binary": false,
            "html": empty_diff_html(),
        })),
    }
}

#[derive(serde::Deserialize)]
pub(crate) struct CompareViewedRequest {
    base: String,
    compare: String,
    path: String,
    viewed: bool,
}

/// POST endpoint: persist the "Viewed" checkbox for one compare-page file.
///
/// The record is scoped to the `base`/`compare` branch pair and pinned to the
/// file's current content hash.
pub(crate) async fn handle_api_compare_viewed_request(
    State(state): State<Arc<DiffServerState>>,
    Json(req): Json<CompareViewedRequest>,
) -> Response {
    let base = match parse_branch_name(&req.base) {
        Some(b) => b,
        None => return bad_request(format!("invalid branch name: {}", req.base)),
    };
    let compare = match parse_branch_name(&req.compare) {
        Some(c) => c,
        None => return bad_request(format!("invalid branch name: {}", req.compare)),
    };
    let path = match parse_diff_path(&req.path) {
        Some(p) => p,
        None => return bad_request(format!("invalid path: {}", req.path)),
    };

    if !req.viewed {
        store_viewed(
            &state,
            PAGE_COMPARE,
            base,
            compare,
            String::new(),
            path,
            None,
        )
        .await;
        return ok_json(serde_json::json!({ "viewed": false }));
    }

    let hash = match load_compare_diff_file(&state.project_root, &base, &compare, &path).await {
        Ok(Some(file)) => content_hash_of(&file),
        Ok(None) => String::new(),
        Err(e) => return bad_request(e),
    };
    if hash.is_empty() {
        return ok_json(serde_json::json!({ "viewed": false }));
    }
    store_viewed(
        &state,
        PAGE_COMPARE,
        base,
        compare,
        String::new(),
        path,
        Some(hash),
    )
    .await;
    ok_json(serde_json::json!({ "viewed": true }))
}

#[allow(clippy::result_large_err)]
fn parse_compare_branches(params: &HashMap<String, String>) -> Result<(String, String), Response> {
    let base_raw = params
        .get("base")
        .ok_or_else(|| bad_request("missing `base` query parameter"))?;
    let compare_raw = params
        .get("compare")
        .ok_or_else(|| bad_request("missing `compare` query parameter"))?;
    let base = parse_branch_name(base_raw)
        .ok_or_else(|| bad_request(format!("invalid branch name: {}", base_raw)))?;
    let compare = parse_branch_name(compare_raw)
        .ok_or_else(|| bad_request(format!("invalid branch name: {}", compare_raw)))?;
    Ok((base, compare))
}

/// Register diff server commands in the command palette
pub fn register(registry: &mut CommandRegistry) {
    registry.register(CommandEntry {
        id: "server.start_diff".into(),
        label: "Start Diff Server".into(),
        category: Some("Server".into()),
        action: Box::new(|_ctx: &CommandContext| {
            CommandEffect::Action(Action::App(AppAction::Integration(
                IntegrationAction::RunPluginCommand {
                    id: "server.start_diff".to_string(),
                },
            )))
        }),
    });

    registry.register(CommandEntry {
        id: "server.stop_diff".into(),
        label: "Stop Diff Server".into(),
        category: Some("Server".into()),
        action: Box::new(|_ctx: &CommandContext| {
            CommandEffect::Action(Action::App(AppAction::Integration(
                IntegrationAction::RunPluginCommand {
                    id: "server.stop_diff".to_string(),
                },
            )))
        }),
    });

    registry.register(CommandEntry {
        id: "server.open_compare".into(),
        label: "Open Compare Branches".into(),
        category: Some("Server".into()),
        action: Box::new(|_ctx: &CommandContext| {
            CommandEffect::Action(Action::App(AppAction::Integration(
                IntegrationAction::RunPluginCommand {
                    id: "server.open_compare".to_string(),
                },
            )))
        }),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_highlighted_emits_syntax_classes_for_rust_diff() {
        let diff = "\
diff --git a/lib.rs b/lib.rs
index 1..2 100644
--- a/lib.rs
+++ b/lib.rs
@@ -1,3 +1,3 @@
 fn keep() {}
-fn old() { let x = 1; }
+fn renamed() { let y = 2; }
";
        let file = parse_unified_diff(diff).into_iter().next().unwrap();
        let html = render_highlighted(&file);
        // Diff line wrappers still present.
        assert!(html.contains(r#"<div class="gr-line gr-line-add">"#));
        assert!(html.contains(r#"<div class="gr-line gr-line-remove">"#));
        assert!(html.contains(r#"<div class="gr-line gr-line-context">"#));
        // Tree-sitter Rust should classify "fn" and "let" as keywords on
        // both the added and removed lines.
        assert!(
            html.contains("gr-hl-keyword"),
            "expected gr-hl-keyword class, got:\n{}",
            html
        );
    }

    #[test]
    fn render_highlighted_falls_back_for_unknown_extension() {
        let diff = "\
diff --git a/notes.unknownext b/notes.unknownext
index 1..2 100644
--- a/notes.unknownext
+++ b/notes.unknownext
@@ -1,1 +1,1 @@
-old line
+new line
";
        let file = parse_unified_diff(diff).into_iter().next().unwrap();
        let html = render_highlighted(&file);
        assert!(!html.contains("gr-hl-"), "should not highlight: {}", html);
        // Plain diff body still renders normally.
        assert!(html.contains(r#"<div class="gr-line gr-line-add">"#));
    }

    #[test]
    fn render_highlighted_falls_back_for_binary() {
        let diff = "\
diff --git a/img.rs b/img.rs
index abc..def
Binary files a/img.rs and b/img.rs differ
";
        let file = parse_unified_diff(diff).into_iter().next().unwrap();
        let html = render_highlighted(&file);
        assert!(html.contains("(binary file changes not shown)"));
        assert!(!html.contains("gr-hl-"));
    }
}
