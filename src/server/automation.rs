//! 自动运行引擎 HTTP 观察与手动触发端点。

use std::sync::Arc;

use axum::extract::{Extension, Path, State};
use axum::response::{IntoResponse, Response};
use serde::Serialize;

use crate::auth::AuthUser;
use crate::automation::{AutomationPackStatus, DomainPackOperation};
use crate::storage::AutomationJob;

use super::{ApiResponse, AppState, forbidden};

#[derive(Debug, Serialize)]
pub(crate) struct AutomationStatus {
    pub running: bool,
    pub jobs: Vec<AutomationJob>,
}

pub(crate) async fn list_jobs(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
) -> Response {
    if !user.can_read_audit() {
        return forbidden("当前角色无权查看自动化任务（需 Operator 及以上）");
    }
    match state.storage.list_automation_jobs(None, 100).await {
        Ok(jobs) => ApiResponse::ok(AutomationStatus {
            running: state.automation.is_running(),
            jobs,
        })
        .into_response(),
        Err(error) => {
            ApiResponse::<()>::error(5000, format!("读取自动化任务失败：{error}")).into_response()
        }
    }
}

pub(crate) async fn get_pack_status(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    Path(id): Path<String>,
) -> Response {
    if !user.can_read_audit() {
        return forbidden("当前角色无权查看自动化状态（需 Operator 及以上）");
    }
    match state.automation.pack_status(&id).await {
        Ok(status) => ApiResponse::<AutomationPackStatus>::ok(status).into_response(),
        Err(error) => {
            ApiResponse::<()>::error(5000, format!("读取自动化状态失败：{error}")).into_response()
        }
    }
}

pub(crate) async fn get_job(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    Path(id): Path<String>,
) -> Response {
    if !user.can_read_audit() {
        return forbidden("当前角色无权查看自动化任务（需 Operator 及以上）");
    }
    match state.storage.get_automation_job(&id).await {
        Ok(Some(job)) => ApiResponse::ok(job).into_response(),
        Ok(None) => ApiResponse::<()>::error(404, "自动化任务不存在").into_response(),
        Err(error) => {
            ApiResponse::<()>::error(5000, format!("读取自动化任务失败：{error}")).into_response()
        }
    }
}

pub(crate) async fn cancel_job(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    Path(id): Path<String>,
) -> Response {
    if !user.can_trigger_run() {
        return forbidden("当前角色无权取消自动化任务（需 Operator 及以上）");
    }
    match state.storage.cancel_automation_job(&id).await {
        Ok(()) => match state.storage.get_automation_job(&id).await {
            Ok(Some(job)) => ApiResponse::ok(job).into_response(),
            Ok(None) => ApiResponse::<()>::error(404, "自动化任务不存在").into_response(),
            Err(error) => ApiResponse::<()>::error(5000, format!("读取自动化任务失败：{error}"))
                .into_response(),
        },
        Err(crate::storage::StorageError::NotFound(_)) => {
            ApiResponse::<()>::error(404, "任务不存在或已结束").into_response()
        }
        Err(error) => {
            ApiResponse::<()>::error(5000, format!("取消自动化任务失败：{error}")).into_response()
        }
    }
}

pub(crate) async fn run_pack(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    Path(id): Path<String>,
) -> Response {
    if !user.can_trigger_run() {
        return forbidden("当前角色无权触发业务包（需 Operator 及以上）");
    }
    match state.automation.run_pack(&id).await {
        Ok(job) => ApiResponse::ok(job).into_response(),
        Err(error) => ApiResponse::<()>::error(4000, error.to_string()).into_response(),
    }
}

pub(crate) async fn run_operation(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    Path((id, operation)): Path<(String, String)>,
) -> Response {
    if !user.can_trigger_run() {
        return forbidden("当前角色无权触发业务包（需 Operator 及以上）");
    }
    let Some(operation) = DomainPackOperation::parse(&operation) else {
        return ApiResponse::<()>::error(4000, "不支持的业务包操作").into_response();
    };
    match state.automation.run_operation(&id, operation).await {
        Ok(job) => ApiResponse::ok(job).into_response(),
        Err(error) => ApiResponse::<()>::error(4000, error.to_string()).into_response(),
    }
}

pub(crate) async fn scan_pack(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    Path(id): Path<String>,
) -> Response {
    if !user.can_trigger_run() {
        return forbidden("当前角色无权扫描业务包（需 Operator 及以上）");
    }
    match state
        .automation
        .scan_pack_by(&id, Some(&user.username))
        .await
    {
        Ok(jobs) => ApiResponse::ok(jobs).into_response(),
        Err(error) => ApiResponse::<()>::error(4000, error.to_string()).into_response(),
    }
}
