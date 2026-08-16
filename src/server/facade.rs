//! Facade HTTP Transport（Phase 7 落地）—— Server 是 OpenAipFacade 的一等 Transport。
//!
//! 端点（单一语义源：与 CLI/Plugin 同一 `OpenAipFacade`，无第二套语义）：
//! - `POST /api/v1/facade/world/query`  —— world.query（Event Log → Projection → Kernel）
//! - `POST /api/v1/facade/audit/query`  —— audit.query（审计语义域）
//! - `POST /api/v1/facade/fabric/submit`—— fabric.submit（Router → Adapter → 事件 → 世界）
//!
//! 执行约定：Facade 是同步 API（内部 block_on storage）→ handler 内
//! `tokio::task::block_in_place`（multi_thread runtime 下合法）。

use std::sync::Arc;

use crate::auth::AuthUser;
use crate::facade::{AuditQuery, FabricRequest, OpenAipFacade, ProjectCreate, WorldQuery};
use axum::Json;
use axum::extract::{Extension, State};
use axum::response::Response;

use super::*;

/// world.query：世界视图（读权限 Operator 及以上，与审计同档）。
pub(crate) async fn facade_world_query(
    State(st): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    Json(body): Json<WorldQuery>,
) -> Response {
    if !user.can_read_audit() {
        return forbidden("当前角色无权读取世界（需 Operator 及以上）").into_response();
    }
    match tokio::task::block_in_place(|| st.facade.world_query(body)) {
        Ok(v) => ApiResponse::ok(v).into_response(),
        Err(e) => ApiResponse::error(5000, format!("世界查询失败: {e}")).into_response(),
    }
}

/// audit.query：审计语义域（approval.* / fabric.* / evolution.gate_approved / policy.denied）。
pub(crate) async fn facade_audit_query(
    State(st): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    Json(body): Json<AuditQuery>,
) -> Response {
    if !user.can_read_audit() {
        return forbidden("当前角色无权读取审计（需 Operator 及以上）").into_response();
    }
    match tokio::task::block_in_place(|| st.facade.audit_query(body)) {
        Ok(v) => ApiResponse::ok(v).into_response(),
        Err(e) => ApiResponse::error(5000, format!("审计查询失败: {e}")).into_response(),
    }
}

/// fabric.submit：提交任务（触发权限，与 run 同档）。
pub(crate) async fn facade_fabric_submit(
    State(st): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    Json(body): Json<FabricRequest>,
) -> Response {
    if !user.can_trigger_run() {
        return forbidden("当前角色无权提交任务（需 Operator 及以上）").into_response();
    }
    match tokio::task::block_in_place(|| st.facade.fabric_submit(body)) {
        Ok(v) => ApiResponse::ok(v).into_response(),
        Err(e) => {
            ApiResponse::error(wb_code("WB2005"), format!("fabric 提交失败: {e}")).into_response()
        }
    }
}

/// project.create：注册项目（项目世界的顶层组织单元）。
pub(crate) async fn facade_project_create(
    State(st): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    Json(body): Json<ProjectCreate>,
) -> Response {
    if !user.can_trigger_run() {
        return forbidden("当前角色无权创建项目（需 Operator 及以上）").into_response();
    }
    match tokio::task::block_in_place(|| st.facade.project_create(body)) {
        Ok(v) => ApiResponse::ok(v).into_response(),
        Err(e) => ApiResponse::error(5000, format!("项目创建失败: {e}")).into_response(),
    }
}

/// project.list：列出全部项目。
pub(crate) async fn facade_project_list(
    State(st): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
) -> Response {
    if !user.can_read_audit() {
        return forbidden("当前角色无权读取项目（需 Operator 及以上）").into_response();
    }
    match tokio::task::block_in_place(|| st.facade.project_list()) {
        Ok(v) => ApiResponse::ok(v).into_response(),
        Err(e) => ApiResponse::error(5000, format!("项目列表失败: {e}")).into_response(),
    }
}

/// project.seed：把 OpenAIP-OS 工程本体写成项目世界事件（模块/文件/测试）。
/// root 取当前工作目录（server 在工程根运行时的形态；CLI 亦同）。
pub(crate) async fn facade_project_seed(
    State(st): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
) -> Response {
    if !user.can_trigger_run() {
        return forbidden("当前角色无权 seed 项目（需 Operator 及以上）").into_response();
    }
    let cwd = std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| ".".to_string());
    let n = tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current()
            .block_on(crate::project_seed::seed_openaip_os(&st.storage, &cwd))
    });
    match n {
        Ok(n) => ApiResponse::ok(serde_json::json!({ "events": n })).into_response(),
        Err(e) => ApiResponse::error(5000, format!("seed 失败: {e}")).into_response(),
    }
}
