use std::sync::Arc;

use axum::extract::{OriginalUri, State};
use axum::response::{Html, IntoResponse, Response};

use super::*;
pub(crate) async fn serve_static(State(st): State<Arc<AppState>>, uri: OriginalUri) -> Response {
    if uri.path().starts_with("/api/") {
        let body = ApiResponse::<()>::error(404, "接口不存在");
        return (StatusCode::NOT_FOUND, body).into_response();
    }
    let path = uri.path().trim_start_matches('/');
    let rel = if path.is_empty() {
        "index.html".to_string()
    } else {
        path.to_string()
    };
    let full = format!("{}/{}", st.static_dir, rel);
    match tokio::fs::read(&full).await {
        Ok(bytes) => {
            // 缓存策略：index.html 完全不缓存（no-store——HTML 是入口，必须每次最新）；
            // 带 hash 的资源（assets/*）长期缓存（immutable——文件名即指纹）。
            let cache = if rel.starts_with("assets/") {
                "public, max-age=31536000, immutable"
            } else {
                "no-store, no-cache, must-revalidate"
            };
            (
                [
                    (axum::http::header::CONTENT_TYPE, content_type(&full)),
                    (axum::http::header::CACHE_CONTROL, cache.to_string()),
                ],
                bytes,
            )
                .into_response()
        }
        Err(_) => {
            let idx = format!("{}/index.html", st.static_dir);
            match tokio::fs::read(&idx).await {
                Ok(b) => (
                    [(
                        axum::http::header::CONTENT_TYPE,
                        "text/html; charset=utf-8".to_string(),
                    )],
                    b,
                )
                    .into_response(),
                Err(_) => Html(
                    "<h1>OpenAIP-OS Agent Observation UI</h1>\
                     <p>未检测到 ui/dist。请进入 ui/ 执行 <code>npm install &amp;&amp; npm run build</code> 后重启服务。</p>"
                        .to_string(),
                )
                .into_response(),
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
