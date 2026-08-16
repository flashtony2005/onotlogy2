//! 业务视图端点（Phase 11 存活面）：Evaluation / Mutation / Loop Health。
//!
//! M1d：V1 的 Memory/Task/Evolution 视图与审计/撤销端点随 V1 门面删除
//! （审计事实在 Kernel 事件轨，V2 查询入口归 M2）。

use std::sync::Arc;

use crate::auth::AuthUser;
use crate::evaluation::{EvaluationResult, EventRef};
use crate::mutation::{MutationProposal, MutationStatus};
use crate::skill::apply::fork_candidate;
use axum::Json;
use axum::extract::{Extension, Path, Query, State};
use axum::response::Response;
use serde::Serialize;
use serde_json::{Value, json};

use super::*;

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
        .storage
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
    let list = match st.storage.list_mutations(status).await {
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
    let mut p = match st.storage.get_mutation(&id).await {
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
    if st.storage.save_mutation(&p).await.is_err() {
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
        &st.storage,
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

/// `GET /api/v1/wb/agent/health`：Loop Health（V2 多速率环元评估，Operator 及以上）。
pub(crate) async fn loop_health(
    State(st): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
) -> Response {
    if !user.can_read_evaluation() {
        return forbidden("当前角色无权读取 Loop Health（需 Operator 及以上）").into_response();
    }
    match crate::health::collect(&st.storage).await {
        Ok(h) => ApiResponse::ok(h).into_response(),
        Err(_) => ApiResponse::error(5000, "存储错误").into_response(),
    }
}

/// 评测查询参数（runId 过滤）。
#[derive(serde::Deserialize)]
pub(crate) struct EvalQuery {
    #[serde(rename = "runId", default)]
    run_id: Option<String>,
}

/// 证据项 DTO。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct EventRefDto {
    seq: u64,
    event_type: String,
}
