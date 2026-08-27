use std::path::{Component, Path};
use std::sync::Arc;

use axum::extract::{OriginalUri, State};
use axum::response::{Html, IntoResponse, Response};

use super::*;
pub(crate) async fn serve_static(State(st): State<Arc<AppState>>, uri: OriginalUri) -> Response {
    if uri.path().starts_with("/api/") {
        let body = ApiResponse::<()>::error(404, "接口不存在");
        return (StatusCode::NOT_FOUND, body).into_response();
    }

    let root = Path::new(&st.static_dir);
    let Ok(root) = tokio::fs::canonicalize(root).await else {
        return Html("<h1>OpenAIP-OS Agent Observation UI</h1>\
                     <p>未检测到 ui/dist。请进入 ui/ 执行 <code>npm install &amp;&amp; npm run build</code> 后重启服务。</p>".to_string()).into_response();
    };
    let path = uri.path().trim_start_matches('/');
    let rel = if path.is_empty() { "index.html" } else { path };
    // Reject traversal before joining. canonicalize below also prevents symlink escape.
    let components: Vec<_> = Path::new(rel).components().collect();
    if components
        .iter()
        .any(|c| !matches!(c, Component::Normal(_)))
    {
        return (
            StatusCode::NOT_FOUND,
            ApiResponse::<()>::error(404, "资源不存在"),
        )
            .into_response();
    }
    let candidate = root.join(rel);
    match tokio::fs::canonicalize(&candidate).await {
        Ok(full)
            if full.starts_with(&root)
                && tokio::fs::metadata(&full)
                    .await
                    .map(|m| m.is_file())
                    .unwrap_or(false) =>
        {
            match tokio::fs::read(&full).await {
                Ok(bytes) => {
                    let cache = if rel.starts_with("assets/") {
                        "public, max-age=31536000, immutable"
                    } else {
                        "no-store, no-cache, must-revalidate"
                    };
                    (
                        [
                            (
                                axum::http::header::CONTENT_TYPE,
                                content_type(full.to_string_lossy().as_ref()),
                            ),
                            (axum::http::header::CACHE_CONTROL, cache.to_string()),
                        ],
                        bytes,
                    )
                        .into_response()
                }
                Err(_) => (
                    StatusCode::NOT_FOUND,
                    ApiResponse::<()>::error(404, "资源不存在"),
                )
                    .into_response(),
            }
        }
        _ => {
            // SPA fallback is only for safe, non-traversal paths and never for API routes.
            let idx = root.join("index.html");
            match tokio::fs::canonicalize(&idx).await {
                Ok(index) if index.starts_with(&root) => match tokio::fs::read(&index).await {
                    Ok(b) => ([
                        (axum::http::header::CONTENT_TYPE, "text/html; charset=utf-8".to_string()),
                    ], b).into_response(),
                    Err(_) => Html("<h1>OpenAIP-OS Agent Observation UI</h1>\
                         <p>未检测到 ui/dist。请进入 ui/ 执行 <code>npm install &amp;&amp; npm run build</code> 后重启服务。</p>".to_string()).into_response(),
                },
                _ => Html("<h1>OpenAIP-OS Agent Observation UI</h1>\
                     <p>未检测到 ui/dist。请进入 ui/ 执行 <code>npm install &amp;&amp; npm run build</code> 后重启服务。</p>".to_string()).into_response(),
            }
        }
    }
}

/// 按扩展名推断 Content-Type。
fn content_type(path: &str) -> String {
    let p = path.to_ascii_lowercase();
    if p.ends_with(".html") {
        "text/html; charset=utf-8"
    } else if p.ends_with(".js") {
        "application/javascript; charset=utf-8"
    } else if p.ends_with(".css") {
        "text/css; charset=utf-8"
    } else if p.ends_with(".json") {
        "application/json; charset=utf-8"
    } else if p.ends_with(".svg") {
        "image/svg+xml"
    } else if p.ends_with(".png") {
        "image/png"
    } else if p.ends_with(".ico") {
        "image/x-icon"
    } else if p.ends_with(".wasm") {
        "application/wasm"
    } else {
        "application/octet-stream"
    }
    .to_string()
}
