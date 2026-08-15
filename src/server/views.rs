use std::sync::Arc;

/// 静态文件托管（SPA 兜底 index.html）。作为 Router::fallback，保留原始请求路径。
use crate::auth::AuthUser;
use crate::evaluation::{EvaluationResult, EventRef};
use crate::evolution::{
    EvolutionState, LearnedKnowledge, Pattern, Proposal, ProposalKind, ProposalStatus,
};
use crate::mutation::{MutationProposal, MutationStatus};
use crate::skill::apply::fork_candidate;
use crate::storage::ApprovalAudit;
use axum::extract::{Extension, Path, Query, State};
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

/// `GET /api/v1/wb/agent/evaluation`：Phase 11 评测结果（11A 证据系统，Operator 及以上）。
/// 返回每维度 Score{value, confidence, evidence} + unknowns + aggregate（派生表，可经 Reducer 重建）。
#[derive(serde::Deserialize)]
pub(crate) struct EvalQuery {
    #[serde(rename = "runId", default)]
    run_id: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct EventRefDto {
    seq: u64,
    event_type: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ScoreDto {
    value: Option<f64>,
    confidence: f64,
    evidence: Vec<EventRefDto>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct EvaluationDto {
    run_id: String,
    task_id: String,
    correctness: ScoreDto,
    completeness: ScoreDto,
    efficiency: ScoreDto,
    reliability: ScoreDto,
    policy_compliance: ScoreDto,
    output_quality: ScoreDto,
    aggregate: f64,
    unknowns: Vec<String>,
    failure_modes: Vec<String>,
    evaluator_version: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct EvaluationListDto {
    total: usize,
    list: Vec<EvaluationDto>,
}

pub(crate) async fn list_evaluations(
    State(st): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    Query(q): Query<EvalQuery>,
) -> Response {
    if !user.can_read_evaluation() {
        return forbidden("当前角色无权读取评测（需 Operator 及以上）").into_response();
    }
    let list = match st
        .kernel
        .storage()
        .list_evaluations(q.run_id.as_deref(), None, 500)
        .await
    {
        Ok(l) => l,
        Err(_) => return ApiResponse::error(5000, "存储错误").into_response(),
    };
    let total = list.len();
    let dtos: Vec<EvaluationDto> = list.into_iter().map(eval_dto).collect();
    ApiResponse::ok(EvaluationListDto { total, list: dtos }).into_response()
}

fn eval_dto(e: EvaluationResult) -> EvaluationDto {
    let score = |s: &crate::evaluation::Score| ScoreDto {
        value: s.value,
        confidence: s.confidence,
        evidence: s
            .evidence
            .iter()
            .map(|r: &EventRef| EventRefDto {
                seq: r.seq,
                event_type: r.event_type.clone(),
            })
            .collect(),
    };
    let unknowns: Vec<String> = e.unknowns().into_iter().map(String::from).collect();
    EvaluationDto {
        run_id: e.run_id,
        task_id: e.task_id,
        correctness: score(&e.correctness),
        completeness: score(&e.completeness),
        efficiency: score(&e.efficiency),
        reliability: score(&e.reliability),
        policy_compliance: score(&e.policy_compliance),
        output_quality: score(&e.output_quality),
        aggregate: e.aggregate,
        unknowns,
        failure_modes: e.failure_modes,
        evaluator_version: e.evaluator_version,
    }
}

/// `GET /api/v1/wb/agent/mutation`：Mutation 提案列表（Phase 11B 诊断环，Operator 及以上可读）。
#[derive(serde::Deserialize)]
pub(crate) struct MutationQuery {
    #[serde(default)]
    status: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MutationDto {
    id: String,
    target_asset: String,
    source_version: String,
    candidate_version: String,
    root_cause: String,
    evidence: Vec<String>,
    delta: Value,
    status: String,
    promotion_reason: Option<String>,
}

impl From<&MutationProposal> for MutationDto {
    fn from(p: &MutationProposal) -> Self {
        Self {
            id: p.id.clone(),
            target_asset: p.target_asset.clone(),
            source_version: p.source_version.clone(),
            candidate_version: p.candidate_version.clone(),
            root_cause: p.root_cause.clone(),
            evidence: p.evidence.clone(),
            delta: p.delta.clone(),
            status: p.status.as_str().to_string(),
            promotion_reason: p.promotion_reason.clone(),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MutationListDto {
    total: usize,
    list: Vec<MutationDto>,
}

pub(crate) async fn list_mutations(
    State(st): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    Query(q): Query<MutationQuery>,
) -> Response {
    if !user.can_read_evaluation() {
        return forbidden("当前角色无权读取 Mutation 提案（需 Operator 及以上）").into_response();
    }
    let status = match q.status.as_deref() {
        None | Some("") => None,
        Some(s) => match s.parse::<MutationStatus>() {
            Ok(st) => Some(st),
            Err(_) => {
                return ApiResponse::error(
                    4001,
                    "status 必须是 PROPOSED/VALIDATED/EXPERIMENTAL/ACTIVE/REJECTED/ARCHIVED",
                )
                .into_response();
            }
        },
    };
    let list = match st.kernel.storage().list_mutations(status).await {
        Ok(l) => l,
        Err(_) => return ApiResponse::error(5000, "存储错误").into_response(),
    };
    let total = list.len();
    let dtos = list.iter().map(MutationDto::from).collect::<Vec<_>>();
    ApiResponse::ok(MutationListDto { total, list: dtos }).into_response()
}

/// `POST /api/v1/wb/agent/mutation/{id}/review`：人工评审（Proposal ≠ Mutation）。
/// 仅 Developer；approve → VALIDATED 并尝试 Apply（fork 候选进 Skill Registry，原资产保留）；
/// reject → REJECTED（失败保留，不删除）。
#[derive(serde::Deserialize)]
pub(crate) struct ReviewBody {
    decision: String,
    reason: Option<String>,
}

pub(crate) async fn review_mutation(
    State(st): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    Path(id): Path<String>,
    Json(body): Json<ReviewBody>,
) -> Response {
    if !user.can_write_evolution() {
        return forbidden("当前角色无权评审 Mutation（需 Developer）");
    }
    let mut p = match st.kernel.storage().get_mutation(&id).await {
        Ok(Some(p)) => p,
        Ok(None) => {
            return ApiResponse::error(404, format!("Mutation 提案不存在：{id}")).into_response();
        }
        Err(_) => return ApiResponse::error(5000, "存储错误").into_response(),
    };
    let mut apply_info: Option<Value> = None;
    match body.decision.as_str() {
        "approve" => {
            if let Err(e) = p.review_approve(&user.username) {
                return ApiResponse::error(4001, format!("评审失败：{e}")).into_response();
            }
            // Apply：fork 候选进 Skill Registry（原资产保留；失败不影响评审状态，提示人工细化 Delta）
            if let Ok(mut reg) = st.v2.skills.write() {
                match fork_candidate(&mut reg, &p.target_asset, &p.candidate_version, &p.delta) {
                    Ok(cand) => {
                        apply_info =
                            Some(json!({ "candidateId": cand.id, "status": "EXPERIMENTAL" }));
                        p.promotion_reason = Some(format!(
                            "human approved by {}; applied candidate {}",
                            user.username, cand.id
                        ));
                    }
                    Err(e) => {
                        p.promotion_reason = Some(format!(
                            "human approved by {}; apply failed: {e}（请人工细化 Delta）",
                            user.username
                        ));
                    }
                }
            }
        }
        "reject" => {
            if let Err(e) =
                p.review_reject(&user.username, body.reason.as_deref().unwrap_or("未说明"))
            {
                return ApiResponse::error(4001, format!("评审失败：{e}")).into_response();
            }
        }
        other => {
            return ApiResponse::error(
                4001,
                format!("decision 必须是 approve|reject，收到 {other}"),
            )
            .into_response();
        }
    }
    if st.kernel.storage().save_mutation(&p).await.is_err() {
        return ApiResponse::error(5000, "存储错误").into_response();
    }
    let mut resp = serde_json::to_value(MutationDto::from(&p)).unwrap_or(json!({}));
    if let Some(info) = apply_info {
        resp["apply"] = info;
    }
    ApiResponse::ok(resp).into_response()
}

/// `POST /api/v1/wb/agent/mutation/{id}/offline-benchmark`：离线 Benchmark（Phase 11C）。
/// 仅 Developer。对 EXPERIMENTAL 候选在历史任务集上做覆盖率判定：
/// pass → EXPERIMENTAL；fail → REJECTED（持久）。结果性 Δ 需候选真实执行后测得。
#[derive(serde::Deserialize)]
pub(crate) struct OfflineBenchmarkBody {
    #[serde(rename = "taskIds", default)]
    task_ids: Option<Vec<String>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct OfflineVerdictDto {
    baseline_runs: usize,
    baseline_mean: f64,
    baseline_failure_rate: f64,
    targeted_runs: usize,
    coverage: f64,
    pass: bool,
    reason: String,
}

impl From<&crate::mutation::OfflineVerdict> for OfflineVerdictDto {
    fn from(v: &crate::mutation::OfflineVerdict) -> Self {
        Self {
            baseline_runs: v.baseline_runs,
            baseline_mean: v.baseline_mean,
            baseline_failure_rate: v.baseline_failure_rate,
            targeted_runs: v.targeted_runs,
            coverage: v.coverage,
            pass: v.pass,
            reason: v.reason.clone(),
        }
    }
}

pub(crate) async fn offline_benchmark_mutation(
    State(st): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    Path(id): Path<String>,
    body: Option<Json<OfflineBenchmarkBody>>,
) -> Response {
    if !user.can_write_evolution() {
        return forbidden("当前角色无权执行离线 Benchmark（需 Developer）").into_response();
    }
    let task_ids = body.map(|b| b.0).and_then(|b| b.task_ids);
    match crate::mutation::run_offline_benchmark(
        &st.kernel.storage(),
        &id,
        task_ids.as_deref(),
        &crate::mutation::OfflineConfig::default(),
    )
    .await
    {
        Ok((p, v)) => {
            let mut resp = serde_json::to_value(MutationDto::from(&p)).unwrap_or(json!({}));
            resp["verdict"] =
                serde_json::to_value(OfflineVerdictDto::from(&v)).unwrap_or(json!({}));
            ApiResponse::ok(resp).into_response()
        }
        Err(crate::storage::StorageError::NotFound(_)) => {
            ApiResponse::error(404, format!("Mutation 提案不存在：{id}")).into_response()
        }
        Err(crate::storage::StorageError::Constraint {
            code: "WB3002",
            message,
        }) => ApiResponse::error(4001, message).into_response(),
        Err(_) => ApiResponse::error(5000, "存储错误").into_response(),
    }
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
