use std::sync::Arc;

/// 静态文件托管（SPA 兜底 index.html）。作为 Router::fallback，保留原始请求路径。
use crate::auth::AuthUser;
use crate::evolution::{
    EvolutionState, LearnedKnowledge, Pattern, Proposal, ProposalKind, ProposalStatus,
};
use crate::storage::ApprovalAudit;
use axum::extract::{Extension, State};
use axum::response::Response;
use serde::Serialize;
use serde_json::{Value, json};

use super::*;
/// Memory 视图 DTO（V0.5 记忆沉淀 / BL-008）：与 `wb_memory` 同源。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MemoryRecordDto {
    label: String,
    payload: Value,
    hits: i64,
    created_at: String,
    updated_at: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MemoryListDto {
    total: usize,
    list: Vec<MemoryRecordDto>,
}

/// 任务观察 DTO：本体对象 `RetentionTask` 的状态投影。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TaskRecordDto {
    id: String,
    title: String,
    customer: String,
    status: String,
    priority: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TaskListDto {
    total: usize,
    list: Vec<TaskRecordDto>,
}

// ── Evolution Engine（BL-003 · REQ-017）：Experience → Knowledge ────────────────────────

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PatternDto {
    label: String,
    action: String,
    support: i64,
    confidence: f64,
    run_ids: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProposalDto {
    id: String,
    kind: String,
    target: String,
    confidence: f64,
    status: String,
    pattern_label: String,
    run_ids: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LearnedDto {
    id: String,
    kind: String,
    action: String,
    confidence: f64,
    proposal_id: String,
    run_ids: Vec<String>,
    /// REQ-018 自动放行资格（可撤销）
    auto_approve_enabled: bool,
    created_at: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct EvolutionDto {
    patterns: Vec<PatternDto>,
    proposals: Vec<ProposalDto>,
    learned: Vec<LearnedDto>,
}

// ── 路由 ─────────────────────────────────────────────────────────────────
/// 构建路由（API 优先，静态托管兜底）。状态由 `auth_layer` 中间件经 `from_fn_with_state`
/// 统一注入（handler 与中间件共用 `Arc<AppState>`），故此处不调 `.with_state`。
pub(crate) async fn list_memory(
    State(st): State<Arc<AppState>>,
    Extension(_auth): Extension<AuthUser>,
) -> Response {
    let records = match st.kernel.recall_all().await {
        Ok(r) => r,
        Err(_) => return ApiResponse::error(5000, "存储错误").into_response(),
    };
    let list = records
        .into_iter()
        .map(|m| MemoryRecordDto {
            label: m.label,
            payload: m.payload,
            hits: m.hits,
            created_at: m.created_at,
            updated_at: m.updated_at,
        })
        .collect::<Vec<_>>();
    let total = list.len();
    ApiResponse::ok(MemoryListDto { total, list }).into_response()
}

fn task_field(data: &Value, key: &str) -> String {
    data.get(key)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

fn task_status_rank(status: &str) -> u8 {
    match status {
        "Pending" => 0,
        "InProgress" => 1,
        "Completed" => 2,
        _ => 3,
    }
}

/// `GET /api/v1/wb/agent/tasks`：RetentionTask 状态列表（观察层任务视图）。
pub(crate) async fn list_tasks(
    State(st): State<Arc<AppState>>,
    Extension(_auth): Extension<AuthUser>,
) -> Response {
    let records = match st.kernel.query_objects("RetentionTask").await {
        Ok(r) => r,
        Err(_) => return ApiResponse::error(5000, "存储错误").into_response(),
    };
    let mut list: Vec<TaskRecordDto> = records
        .into_iter()
        .map(|o| TaskRecordDto {
            id: o.id,
            title: task_field(&o.data, "title"),
            customer: task_field(&o.data, "customer"),
            status: task_field(&o.data, "status"),
            priority: task_field(&o.data, "priority"),
        })
        .collect();
    list.sort_by(|a, b| {
        task_status_rank(&a.status)
            .cmp(&task_status_rank(&b.status))
            .then_with(|| a.id.cmp(&b.id))
    });
    let total = list.len();
    ApiResponse::ok(TaskListDto { total, list }).into_response()
}

/// `GET /api/v1/wb/agent/evolution`：Evolution 聚合状态（BL-003 · REQ-017）。
/// 返回 patterns / proposals / learned（含可追溯证据 run_ids 与 proposal_id）。
pub(crate) async fn list_evolution(
    State(st): State<Arc<AppState>>,
    Extension(_auth): Extension<AuthUser>,
) -> Response {
    let state: EvolutionState = match st.kernel.evolution_state().await {
        Ok(s) => s,
        Err(_) => return ApiResponse::error(5000, "存储错误").into_response(),
    };
    let patterns = state
        .patterns
        .into_iter()
        .map(|p: Pattern| PatternDto {
            label: p.label,
            action: p.action,
            support: p.support,
            confidence: p.confidence,
            run_ids: p.run_ids,
        })
        .collect::<Vec<_>>();
    let proposals = state
        .proposals
        .into_iter()
        .map(|p: Proposal| ProposalDto {
            id: p.id,
            kind: match p.kind {
                ProposalKind::ActionPattern => "ActionPattern",
            }
            .to_string(),
            target: p.target,
            confidence: p.confidence,
            status: match p.status {
                ProposalStatus::Proposed => "Proposed",
                ProposalStatus::Validated => "Validated",
                ProposalStatus::Rejected => "Rejected",
            }
            .to_string(),
            pattern_label: p.pattern_label,
            run_ids: p.run_ids,
        })
        .collect::<Vec<_>>();
    let learned = state
        .learned
        .into_iter()
        .map(|l: LearnedKnowledge| LearnedDto {
            id: l.id,
            kind: match l.kind {
                ProposalKind::ActionPattern => "ActionPattern",
            }
            .to_string(),
            action: l.action,
            confidence: l.confidence,
            proposal_id: l.proposal_id,
            run_ids: l.run_ids,
            auto_approve_enabled: l.auto_approve_enabled,
            created_at: l.created_at,
        })
        .collect::<Vec<_>>();
    ApiResponse::ok(EvolutionDto {
        patterns,
        proposals,
        learned,
    })
    .into_response()
}

/// `DELETE /api/v1/wb/agent/evolution/learned/:id`：撤销一条 LearnedKnowledge（可撤销承诺）。
pub(crate) async fn revoke_learned(
    State(st): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    Path(id): Path<String>,
) -> Response {
    // 写 Evolution（撤销 LearnedKnowledge）仅 Developer（最小权限边界，D3）。
    if !user.can_write_evolution() {
        return forbidden("当前角色无权写 Evolution（需 Developer）");
    }
    match st.kernel.revoke_learned(&id).await {
        Ok(()) => ApiResponse::ok(json!({ "id": id, "revoked": true })).into_response(),
        Err(_) => ApiResponse::error(5000, "存储错误").into_response(),
    }
}

/// `POST /api/v1/wb/agent/evolution/learned/:id/auto-approve`：REV-018 可撤销——
/// 切换某条 LearnedKnowledge 的自动放行资格（`enabled=true|false`），幂等，仅翻转
/// `auto_approve_enabled` + 记 audit，不删除知识。撤销后该 action 下次命中即回退 Human Approval。
#[derive(serde::Deserialize)]
pub(crate) struct AutoApproveBody {
    #[serde(default)]
    enabled: bool,
}

pub(crate) async fn toggle_auto_approve(
    State(st): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    Path(id): Path<String>,
    Json(body): Json<AutoApproveBody>,
) -> Response {
    // 写 Evolution（切换自动放行资格）仅 Developer（最小权限边界，D3）。
    if !user.can_write_evolution() {
        return forbidden("当前角色无权写 Evolution（需 Developer）");
    }
    if st
        .kernel
        .set_learned_auto_approve(&id, body.enabled)
        .await
        .is_err()
    {
        return ApiResponse::error(5000, "存储错误").into_response();
    }
    let (decision, reason) = if body.enabled {
        ("auto_approve_enabled", "恢复自动放行资格")
    } else {
        (
            "revoke_disabled",
            "撤销自动放行资格（下次该 action 回退 Human Approval）",
        )
    };
    // 配置态切换也入审计（可追溯谁/何时撤销），run_id/action 以 id 占位（非 run 触发）。
    let _ = st
        .kernel
        .record_approval_audit(&id, &id, &id, decision, reason)
        .await;
    ApiResponse::ok(json!({ "id": id, "autoApproveEnabled": body.enabled })).into_response()
}

/// `wb_approval_audit` 审计 DTO（REQ-018）：每次自动放行/撤销可追溯至 LearnedKnowledge + run。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ApprovalAuditDto {
    id: String,
    run_id: String,
    action: String,
    learned_id: String,
    decision: String,
    reason: String,
    created_at: String,
}

impl From<ApprovalAudit> for ApprovalAuditDto {
    fn from(a: ApprovalAudit) -> Self {
        Self {
            id: a.id,
            run_id: a.run_id,
            action: a.action,
            learned_id: a.learned_id,
            decision: a.decision,
            reason: a.reason,
            created_at: a.created_at,
        }
    }
}

/// `GET /api/v1/wb/agent/evolution/audit`：列出全部自动放行审计（REQ-018 可审计）。
pub(crate) async fn list_approval_audit_handler(
    State(st): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
) -> Response {
    // 读 Audit 需 Operator 及以上。
    if !user.can_read_audit() {
        return forbidden("当前角色无权读取审计（需 Operator 及以上）");
    }
    match st.kernel.list_approval_audit(None).await {
        Ok(rows) => ApiResponse::ok(
            rows.into_iter()
                .map(ApprovalAuditDto::from)
                .collect::<Vec<_>>(),
        )
        .into_response(),
        Err(_) => ApiResponse::error(5000, "存储错误").into_response(),
    }
}
