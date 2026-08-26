//! Business Loop HTTP Transport。

use crate::auth::AuthUser;
use crate::business_loop::{
    BusinessLoopError, decide_business_approval, decide_opportunity, discover_opportunities,
    enqueue_business_optimization_job, execute_opportunity, goal_gaps, list_follow_ups,
    list_learning_insights, list_path_insights, optimize_toward_goals, plan_business_portfolio,
    plan_opportunity_paths, recommend_next_actions, reopen_interrupted_execution,
    run_next_best_action, settle_follow_up, start_follow_up,
};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use serde::Deserialize;
use serde_json::Value;
use std::sync::Arc;

use super::{ApiResponse, AppState, forbidden};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProjectScope {
    pub project_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GoalQuery {
    pub project_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DecideBody {
    pub action: Option<String>,
    pub reason: Option<String>,
    pub params: Option<Value>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ApprovalDecisionBody {
    pub approved: bool,
    pub reason: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FollowUpResultBody {
    pub success: Option<bool>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReconcileBody {
    pub project_id: String,
    pub lease_id: String,
    pub acknowledged_no_side_effect: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BusinessOptimizationBody {
    pub project_id: String,
    pub max_cycles: Option<u32>,
}

fn error_response(error: BusinessLoopError) -> Response {
    match error {
        BusinessLoopError::NotFound(message) => {
            ApiResponse::error(404, format!("不存在：{message}")).into_response()
        }
        BusinessLoopError::Invalid(message) => {
            (StatusCode::BAD_REQUEST, ApiResponse::error(400, message)).into_response()
        }
        BusinessLoopError::Data(error) => (
            StatusCode::BAD_REQUEST,
            ApiResponse::error(400, error.to_string()),
        )
            .into_response(),
        BusinessLoopError::Capability(error) => (
            StatusCode::BAD_GATEWAY,
            ApiResponse::error(5001, format!("外部能力执行失败：{error}")),
        )
            .into_response(),
        BusinessLoopError::Knowledge(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            ApiResponse::error(5000, format!("知识推理错误：{error}")),
        )
            .into_response(),
        BusinessLoopError::Odl(_) | BusinessLoopError::Storage(_) => {
            ApiResponse::error(5000, "业务闭环存储错误").into_response()
        }
    }
}

fn write_denied(user: &AuthUser) -> Option<Response> {
    if user.can_trigger_run() {
        None
    } else {
        Some(forbidden("当前角色无权执行业务闭环（需 Operator 及以上）"))
    }
}

pub(crate) async fn list_goal_gaps(
    State(st): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    Query(query): Query<GoalQuery>,
) -> Response {
    if !user.can_read_evaluation() {
        return forbidden("当前角色无权读取业务目标（需 Operator 及以上）").into_response();
    }
    let result = match query.project_id {
        Some(project_id) => goal_gaps(&st.storage, &project_id).await,
        None => {
            let mut gaps = Vec::new();
            match st.storage.list_projects().await {
                Ok(projects) => {
                    for project in projects {
                        match goal_gaps(&st.storage, &project.id).await {
                            Ok(mut project_gaps) => gaps.append(&mut project_gaps),
                            Err(BusinessLoopError::NotFound(_)) => {}
                            Err(error) => return error_response(error),
                        }
                    }
                }
                Err(error) => return error_response(error.into()),
            }
            Ok(gaps)
        }
    };
    match result {
        Ok(gaps) => ApiResponse::ok(gaps).into_response(),
        Err(error) => error_response(error),
    }
}

pub(crate) async fn discover(
    State(st): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    Json(body): Json<ProjectScope>,
) -> Response {
    if let Some(response) = write_denied(&user) {
        return response;
    }
    match discover_opportunities(&st.storage, &body.project_id).await {
        Ok(opportunities) => ApiResponse::ok(Value::Array(opportunities)).into_response(),
        Err(error) => error_response(error),
    }
}

pub(crate) async fn recommendations(
    State(st): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    Query(query): Query<ProjectScope>,
) -> Response {
    if !user.can_read_evaluation() {
        return forbidden("当前角色无权读取业务推荐（需 Operator 及以上）").into_response();
    }
    match recommend_next_actions(&st.storage, &query.project_id).await {
        Ok(recommendations) => ApiResponse::ok(recommendations).into_response(),
        Err(error) => error_response(error),
    }
}

pub(crate) async fn opportunity_paths(
    State(st): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    Query(query): Query<ProjectScope>,
) -> Response {
    if !user.can_read_evaluation() {
        return forbidden("当前角色无权读取业务路径（需 Operator 及以上）").into_response();
    }
    match plan_opportunity_paths(&st.storage, &query.project_id).await {
        Ok(paths) => ApiResponse::ok(paths).into_response(),
        Err(error) => error_response(error),
    }
}

pub(crate) async fn business_portfolio(
    State(st): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    Query(query): Query<ProjectScope>,
) -> Response {
    if !user.can_read_evaluation() {
        return forbidden("当前角色无权读取业务组合（需 Operator 及以上）").into_response();
    }
    match plan_business_portfolio(&st.storage, &query.project_id).await {
        Ok(portfolio) => ApiResponse::ok(portfolio).into_response(),
        Err(error) => error_response(error),
    }
}

pub(crate) async fn follow_ups(
    State(st): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    Query(query): Query<ProjectScope>,
) -> Response {
    if !user.can_read_evaluation() {
        return forbidden("当前角色无权读取业务验证队列（需 Operator 及以上）").into_response();
    }
    match list_follow_ups(&st.storage, &query.project_id).await {
        Ok(items) => ApiResponse::ok(items).into_response(),
        Err(error) => error_response(error),
    }
}

pub(crate) async fn learning_insights(
    State(st): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    Query(query): Query<ProjectScope>,
) -> Response {
    if !user.can_read_evaluation() {
        return forbidden("当前角色无权读取业务学习证据（需 Operator 及以上）").into_response();
    }
    match list_learning_insights(&st.storage, &query.project_id).await {
        Ok(items) => ApiResponse::ok(items).into_response(),
        Err(error) => error_response(error),
    }
}

pub(crate) async fn path_insights(
    State(st): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    Query(query): Query<ProjectScope>,
) -> Response {
    if !user.can_read_evaluation() {
        return forbidden("当前角色无权读取路径学习证据（需 Operator 及以上）").into_response();
    }
    match list_path_insights(&st.storage, &query.project_id).await {
        Ok(items) => ApiResponse::ok(items).into_response(),
        Err(error) => error_response(error),
    }
}

pub(crate) async fn optimization_runs(
    State(st): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    Query(query): Query<ProjectScope>,
) -> Response {
    if !user.can_read_evaluation() {
        return forbidden("当前角色无权读取业务优化历史（需 Operator 及以上）").into_response();
    }
    match st
        .storage
        .list_business_optimization_runs(&query.project_id, 50)
        .await
    {
        Ok(runs) => ApiResponse::ok(runs.into_iter().map(|run| run.payload).collect::<Vec<_>>())
            .into_response(),
        Err(error) => error_response(error.into()),
    }
}

pub(crate) async fn execution_leases(
    State(st): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    Query(query): Query<ProjectScope>,
) -> Response {
    if !user.can_read_evaluation() {
        return forbidden("当前角色无权读取执行恢复队列（需 Operator 及以上）").into_response();
    }
    match st
        .storage
        .list_expired_execution_leases(&query.project_id, 200)
        .await
    {
        Ok(leases) => ApiResponse::ok(leases).into_response(),
        Err(error) => error_response(error.into()),
    }
}

pub(crate) async fn run_next(
    State(st): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    Json(body): Json<ProjectScope>,
) -> Response {
    if let Some(response) = write_denied(&user) {
        return response;
    }
    let workers = st.v2.workers.read().await;
    match run_next_best_action(
        &st.storage,
        &st.plugin_catalog,
        &workers,
        &body.project_id,
        &user.username,
    )
    .await
    {
        Ok(result) => ApiResponse::ok(result).into_response(),
        Err(error) => error_response(error),
    }
}

pub(crate) async fn reconcile_opportunity(
    State(st): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    Path(id): Path<String>,
    Json(body): Json<ReconcileBody>,
) -> Response {
    if let Some(response) = write_denied(&user) {
        return response;
    }
    match reopen_interrupted_execution(
        &st.storage,
        &body.project_id,
        &id,
        &body.lease_id,
        body.acknowledged_no_side_effect,
    )
    .await
    {
        Ok(result) => ApiResponse::ok(result).into_response(),
        Err(error) => error_response(error),
    }
}

pub(crate) async fn decide(
    State(st): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    Path(id): Path<String>,
    Json(body): Json<DecideBody>,
) -> Response {
    if let Some(response) = write_denied(&user) {
        return response;
    }
    // project 与 opportunity 一同由存储记录约束；ID 全局可读，但写入只落在
    // 该 opportunity 所属项目，因此这里从机会对象反查项目归属。
    let project_ids = match st.storage.list_projects().await {
        Ok(projects) => projects
            .into_iter()
            .map(|project| project.id)
            .collect::<Vec<_>>(),
        Err(error) => return error_response(error.into()),
    };
    for project_id in project_ids {
        match decide_opportunity(
            &st.storage,
            &project_id,
            &id,
            &user.username,
            crate::business_loop::DecisionOptions {
                action: body.action.as_deref(),
                reason: body.reason.as_deref(),
                alternatives: None,
                params: body.params.clone(),
            },
        )
        .await
        {
            Ok(decision) => return ApiResponse::ok(decision).into_response(),
            Err(BusinessLoopError::NotFound(_)) => continue,
            Err(error) => return error_response(error),
        }
    }
    error_response(BusinessLoopError::NotFound(id))
}

pub(crate) async fn optimize(
    State(st): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    Json(body): Json<BusinessOptimizationBody>,
) -> Response {
    if let Some(response) = write_denied(&user) {
        return response;
    }
    let workers = st.v2.workers.read().await;
    match optimize_toward_goals(
        &st.storage,
        &st.plugin_catalog,
        &workers,
        &body.project_id,
        &user.username,
        body.max_cycles.unwrap_or(5),
    )
    .await
    {
        Ok(runs) => ApiResponse::ok(runs).into_response(),
        Err(error) => error_response(error),
    }
}

pub(crate) async fn start_optimization_job(
    State(st): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    Json(body): Json<BusinessOptimizationBody>,
) -> Response {
    if let Some(response) = write_denied(&user) {
        return response;
    }
    match enqueue_business_optimization_job(
        &st.storage,
        &body.project_id,
        &user.username,
        body.max_cycles.unwrap_or(5),
    )
    .await
    {
        Ok((job, _created)) => ApiResponse::ok(job).into_response(),
        Err(error) => error_response(error),
    }
}

pub(crate) async fn execute(
    State(st): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    Path(id): Path<String>,
) -> Response {
    if let Some(response) = write_denied(&user) {
        return response;
    }
    let project_ids = match st.storage.list_projects().await {
        Ok(projects) => projects
            .into_iter()
            .map(|project| project.id)
            .collect::<Vec<_>>(),
        Err(error) => return error_response(error.into()),
    };
    for project_id in project_ids {
        let workers = st.v2.workers.read().await;
        match execute_opportunity(&st.storage, &st.plugin_catalog, &workers, &project_id, &id).await
        {
            Ok(result) => return ApiResponse::ok(result).into_response(),
            Err(BusinessLoopError::NotFound(_)) => continue,
            Err(error) => return error_response(error),
        }
    }
    error_response(BusinessLoopError::NotFound(id))
}

pub(crate) async fn start_follow_up_object(
    State(st): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    Path(id): Path<String>,
) -> Response {
    if let Some(response) = write_denied(&user) {
        return response;
    }
    let project_ids = match st.storage.list_projects().await {
        Ok(projects) => projects
            .into_iter()
            .map(|project| project.id)
            .collect::<Vec<_>>(),
        Err(error) => return error_response(error.into()),
    };
    for project_id in project_ids {
        match start_follow_up(&st.storage, &project_id, &id).await {
            Ok(result) => return ApiResponse::ok(result).into_response(),
            Err(BusinessLoopError::NotFound(_)) => continue,
            Err(error) => return error_response(error),
        }
    }
    error_response(BusinessLoopError::NotFound(id))
}

pub(crate) async fn settle_follow_up_object(
    State(st): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    Path(id): Path<String>,
    Json(body): Json<FollowUpResultBody>,
) -> Response {
    if let Some(response) = write_denied(&user) {
        return response;
    }
    let project_ids = match st.storage.list_projects().await {
        Ok(projects) => projects
            .into_iter()
            .map(|project| project.id)
            .collect::<Vec<_>>(),
        Err(error) => return error_response(error.into()),
    };
    for project_id in project_ids {
        match settle_follow_up(&st.storage, &project_id, &id, body.success.unwrap_or(true)).await {
            Ok(result) => return ApiResponse::ok(result).into_response(),
            Err(BusinessLoopError::NotFound(_)) => continue,
            Err(error) => return error_response(error),
        }
    }
    error_response(BusinessLoopError::NotFound(id))
}

pub(crate) async fn approve_decision(
    State(st): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    Path(id): Path<String>,
    Json(body): Json<ApprovalDecisionBody>,
) -> Response {
    if let Some(response) = write_denied(&user) {
        return response;
    }
    match decide_business_approval(
        &st.storage,
        &id,
        body.approved,
        &user.username,
        body.reason.as_deref(),
    )
    .await
    {
        Ok(result) => ApiResponse::ok(result).into_response(),
        Err(error) => error_response(error),
    }
}
