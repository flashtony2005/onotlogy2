//! Production operations endpoints: liveness, readiness, and Prometheus text metrics.

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde_json::json;

use super::AppState;
use crate::storage::AuditChainHealth;

/// Process liveness probe. It deliberately does not touch storage or auth.
pub(crate) async fn healthz() -> Response {
    (
        StatusCode::OK,
        Json(json!({"status": "ok", "service": "oaip"})),
    )
        .into_response()
}

/// Service readiness probe. A broken audit chain makes the instance unready.
pub(crate) async fn readyz(State(state): State<Arc<AppState>>) -> Response {
    match state.storage.audit_chain_health() {
        AuditChainHealth::Valid => (
            StatusCode::OK,
            Json(json!({"status": "ready", "service": "oaip", "auditChain": "valid"})),
        )
            .into_response(),
        AuditChainHealth::Unknown | AuditChainHealth::Invalid => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "status": "not_ready",
                "service": "oaip",
                "auditChain": "invalid",
            })),
        )
            .into_response(),
    }
}

/// Low-dependency Prometheus text endpoint for the local-first deployment.
pub(crate) async fn metrics(State(state): State<Arc<AppState>>) -> Response {
    let ready = matches!(state.storage.audit_chain_health(), AuditChainHealth::Valid);
    // run/trace 总量只读 SQL 聚合计数，避免在每次抓取时额外反序列化业务正文；
    // 审计链状态来自启动/显式深校验与追加热路径的缓存，不在抓取请求中全链扫描。
    let runs = state.storage.count_runs().await;
    let events = state.storage.count_trace_events().await;
    let pending = super::lock_mutex(&state.pending).len();

    let mut body = String::from(
        "# HELP oaip_ready Whether the service passes the readiness checks.\n# TYPE oaip_ready gauge\n",
    );
    body.push_str(&format!("oaip_ready {}\n", i32::from(ready)));
    body.push_str("# HELP oaip_runs_total Persisted run records.\n# TYPE oaip_runs_total gauge\n");
    body.push_str(&format!(
        "oaip_runs_total {}\n",
        runs.as_ref().copied().unwrap_or(0)
    ));
    body.push_str(
        "# HELP oaip_trace_events_total Persisted append-only trace events.\n# TYPE oaip_trace_events_total gauge\n",
    );
    body.push_str(&format!(
        "oaip_trace_events_total {}\n",
        events.as_ref().copied().unwrap_or(0)
    ));
    body.push_str(
        "# HELP oaip_pending_approvals Current in-memory pending approvals.\n# TYPE oaip_pending_approvals gauge\n",
    );
    body.push_str(&format!("oaip_pending_approvals {pending}\n"));
    body.push_str(
        "# HELP oaip_storage_errors Storage reads that failed while collecting metrics.\n# TYPE oaip_storage_errors gauge\n",
    );
    body.push_str(&format!(
        "oaip_storage_errors {}\n",
        i32::from(runs.is_err()) + i32::from(events.is_err())
    ));

    let mut response = body.into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/plain; version=0.0.4; charset=utf-8"),
    );
    response
}
