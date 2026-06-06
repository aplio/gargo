//! HTML page handlers that fill the externalized templates.

use super::*;
use std::sync::Arc;

use axum::{
    extract::State,
    response::{Html, IntoResponse},
};

use crate::diff_render::render_diff_styles;

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
