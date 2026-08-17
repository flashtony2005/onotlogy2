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
use crate::project_seed::{project_add_object, seed_dependencies};
use axum::Json;
use axum::extract::{Extension, Path, State};
use axum::response::Response;
use serde::Deserialize;

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
        Ok(v) => {
            // T1：挂起态（status=pending）→ 注册审批（复用 /run/:run_id/approval 端点）
            if v.result.payload.get("status").and_then(|s| s.as_str()) == Some("pending")
                && let Some(run_id) = v.result.correlation_id.clone()
            {
                let capability = v
                    .result
                    .payload
                    .get("capability")
                    .and_then(|c| c.as_str())
                    .unwrap_or("")
                    .to_string();
                let (tx, _rx) = std::sync::mpsc::channel();
                lock_mutex(&st.pending).insert(
                    run_id.clone(),
                    super::PendingApproval {
                        run_id: run_id.clone(),
                        request: crate::cognitive::approval::ApprovalRequest {
                            id: run_id.clone(),
                            run_id: run_id.clone(),
                            context: crate::kernel::ContextId(0),
                            fiber: crate::kernel::FiberId(0),
                            capability: capability.clone(),
                            action: v.result.intent.clone().unwrap_or_default(),
                            args: v.result.payload.clone(),
                            created_at: crate::trace::now_ts().parse::<i64>().unwrap_or(0),
                        },
                        tx,
                        created_at: std::time::Instant::now(),
                        resume: super::ResumeTarget::FabricRun {
                            session_id: v.result.session_id.clone(),
                            run_id: run_id.clone(),
                        },
                    },
                );
            }
            ApiResponse::ok(v).into_response()
        }
        Err(e) => {
            ApiResponse::error(wb_code("WB2005"), format!("fabric 提交失败: {e}")).into_response()
        }
    }
}

/// fabric.cancel：取消运行中的 fabric 任务（按 correlation_id kill 子进程 + 取消标记）。
/// 触发权限与 submit 同档（Operator 及以上）。
pub(crate) async fn facade_fabric_cancel(
    State(st): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    Path(id): Path<String>,
) -> Response {
    if !user.can_trigger_run() {
        return forbidden("当前角色无权取消任务（需 Operator 及以上）").into_response();
    }
    match tokio::task::block_in_place(|| st.facade.fabric_cancel(&id)) {
        Ok(hits) if hits > 0 => ApiResponse::ok(serde_json::json!({
            "correlationId": id,
            "status": "cancelled"
        }))
        .into_response(),
        // P1#6 返回语义失真修复：未命中（correlation 未在运行）据实反映，不再谎报 cancelled。
        Ok(_) => ApiResponse::ok(serde_json::json!({
            "correlationId": id,
            "status": "not_found"
        }))
        .into_response(),
        Err(e) => {
            ApiResponse::error(wb_code("WB2005"), format!("fabric 取消失败: {e}")).into_response()
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

/// project.event 请求体（与 CLI `oaip facade project event` 同语义；HTTP 用 JSON 对象而非字符串）。
#[derive(Deserialize)]
pub(crate) struct ProjectEventReq {
    object_type: String,
    id: String,
    project: String,
    /// 对象载荷（JSON 对象，直接落库）。
    input: serde_json::Value,
    /// 可选：root --edge--> 此对象（如 contains）。
    #[serde(default)]
    edge: Option<String>,
}

/// project.event：给项目世界加对象（CreateObject(project) + 可选 root --edge--> 关系）。
/// 与 CLI `cmd_project_event` 同源（都调 `project_add_object`），Server 是另一 Transport。
pub(crate) async fn facade_project_event(
    State(st): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    Json(body): Json<ProjectEventReq>,
) -> Response {
    if !user.can_trigger_run() {
        return forbidden("当前角色无权添加项目对象（需 Operator 及以上）").into_response();
    }
    let res = tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current().block_on(project_add_object(
            &st.storage,
            &body.object_type,
            &body.id,
            &body.project,
            &body.input,
            body.edge.as_deref(),
        ))
    });
    match res {
        Ok(()) => ApiResponse::ok(serde_json::json!({ "ok": true })).into_response(),
        Err(e) => ApiResponse::error(5000, format!("添加项目对象失败: {e}")).into_response(),
    }
}

/// project.deps：依赖图 seed（crates 依赖 → depends_on 关系入项目世界）。
/// 与 CLI `cmd_project_deps` 同源（都调 `seed_dependencies`）；先确保工程结构在（幂等）。
pub(crate) async fn facade_project_deps(
    State(st): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
) -> Response {
    if !user.can_trigger_run() {
        return forbidden("当前角色无权 seed 依赖图（需 Operator 及以上）").into_response();
    }
    let cwd = std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| ".".to_string());
    let n = tokio::task::block_in_place(|| {
        let h = tokio::runtime::Handle::current();
        // 确保项目与工程结构在（幂等：已存在则跳过）
        let _ = h.block_on(crate::project_seed::seed_openaip_os(&st.storage, &cwd));
        h.block_on(seed_dependencies(&st.storage, &cwd))
    });
    match n {
        Ok(n) => ApiResponse::ok(serde_json::json!({ "relations": n })).into_response(),
        Err(e) => ApiResponse::error(5000, format!("依赖图 seed 失败: {e}")).into_response(),
    }
}
