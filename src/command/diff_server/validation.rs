//! Request validation + small response/HTML helpers.


use axum::{
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Json, Response},
};


pub(crate) fn parse_bool_param(value: Option<&String>, default: bool) -> bool {
    match value.map(|v| v.as_str()) {
        Some("true") => true,
        Some("false") => false,
        _ => default,
    }
}


pub(crate) fn html_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
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
pub(crate) fn parse_branch_name(value: &str) -> Option<String> {
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


pub(crate) fn repo_ctx_script(
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

