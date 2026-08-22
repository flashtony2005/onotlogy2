//! 本地用户管理 HTTP API（V0.9）。
//!
//! 所有接口只允许 Developer 调用；密码仅在请求体中短暂出现，响应绝不返回 password_hash。

use std::sync::Arc;

use axum::extract::{Extension, Path, State};
use axum::response::{IntoResponse, Response};
use axum::{Json, http::StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::auth::{AuthUser, LocalIdentityProvider};
use crate::storage::UserRecord;

use super::{ApiResponse, AppState, forbidden};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct UserDto {
    id: String,
    username: String,
    role: String,
    display_name: String,
    disabled: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CreateUserBody {
    username: String,
    password: String,
    role: String,
    #[serde(default)]
    display_name: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct PasswordBody {
    password: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct DisabledBody {
    disabled: bool,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RoleBody {
    role: String,
}

fn to_dto(user: UserRecord) -> UserDto {
    UserDto {
        id: user.id,
        username: user.username,
        role: user.role,
        display_name: user.display_name,
        disabled: user.disabled,
    }
}

fn bad_request(message: impl Into<String>) -> Response {
    (
        StatusCode::BAD_REQUEST,
        ApiResponse::<()>::error(400, message),
    )
        .into_response()
}

fn internal_error(message: impl Into<String>) -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        ApiResponse::<()>::error(5000, message),
    )
        .into_response()
}

fn validate_role(role: &str) -> Option<&'static str> {
    match role.trim().to_ascii_lowercase().as_str() {
        "executive" => Some("executive"),
        "operator" => Some("operator"),
        "developer" => Some("developer"),
        _ => None,
    }
}

#[allow(clippy::result_large_err)]
fn validate_password(password: &str) -> Result<(), Response> {
    if password.chars().count() < 8 {
        return Err(bad_request("密码至少需要 8 个字符"));
    }
    Ok(())
}

#[allow(clippy::result_large_err)]
fn check_admin(user: &AuthUser) -> Result<(), Response> {
    if user.can_manage_users() {
        Ok(())
    } else {
        Err(forbidden("当前角色无权管理用户（需 Developer）"))
    }
}

/// GET /api/v1/wb/users
pub(crate) async fn list_users(
    State(st): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
) -> Response {
    if let Err(response) = check_admin(&user) {
        return response;
    }
    match st.storage.list_users().await {
        Ok(users) => {
            ApiResponse::ok(users.into_iter().map(to_dto).collect::<Vec<_>>()).into_response()
        }
        Err(error) => internal_error(format!("读取用户列表失败：{error}")),
    }
}

/// POST /api/v1/wb/users
pub(crate) async fn create_user(
    State(st): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    Json(body): Json<CreateUserBody>,
) -> Response {
    if let Err(response) = check_admin(&user) {
        return response;
    }
    let username = body.username.trim();
    if username.is_empty() {
        return bad_request("用户名不能为空");
    }
    if let Err(response) = validate_password(&body.password) {
        return response;
    }
    let Some(role) = validate_role(&body.role) else {
        return bad_request("角色必须是 executive、operator 或 developer");
    };
    let password_hash = match LocalIdentityProvider::hash_password(&body.password) {
        Ok(hash) => hash,
        Err(error) => return internal_error(format!("密码哈希失败：{error}")),
    };
    let record = UserRecord {
        id: format!("user.{username}"),
        username: username.to_string(),
        password_hash,
        role: role.to_string(),
        display_name: body.display_name.trim().to_string(),
        disabled: false,
    };
    match st.storage.create_user(&record).await {
        Ok(()) => ApiResponse::ok(to_dto(record)).into_response(),
        Err(error) => bad_request(format!("创建用户失败：{error}")),
    }
}

/// PATCH /api/v1/wb/users/:id/password
pub(crate) async fn update_user_password(
    State(st): State<Arc<AppState>>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<String>,
    Json(body): Json<PasswordBody>,
) -> Response {
    if let Err(response) = check_admin(&actor) {
        return response;
    }
    if let Err(response) = validate_password(&body.password) {
        return response;
    }
    let Some(target) = (match st.storage.get_user_by_id(&id).await {
        Ok(user) => user,
        Err(error) => return internal_error(format!("读取用户失败：{error}")),
    }) else {
        return bad_request("用户不存在");
    };
    let password_hash = match LocalIdentityProvider::hash_password(&body.password) {
        Ok(hash) => hash,
        Err(error) => return internal_error(format!("密码哈希失败：{error}")),
    };
    match st
        .storage
        .update_user_password(&target.username, &password_hash)
        .await
    {
        Ok(()) => ApiResponse::ok(json!({ "updated": true })).into_response(),
        Err(error) => internal_error(format!("重设密码失败：{error}")),
    }
}

/// PATCH /api/v1/wb/users/:id/disabled
pub(crate) async fn set_user_disabled(
    State(st): State<Arc<AppState>>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<String>,
    Json(body): Json<DisabledBody>,
) -> Response {
    if let Err(response) = check_admin(&actor) {
        return response;
    }
    if actor.id == id {
        return bad_request("不能禁用当前登录账号");
    }
    match st.storage.set_user_disabled(&id, body.disabled).await {
        Ok(()) => ApiResponse::ok(json!({ "disabled": body.disabled })).into_response(),
        Err(error) => bad_request(format!("更新用户状态失败：{error}")),
    }
}

/// PATCH /api/v1/wb/users/:id/role
pub(crate) async fn set_user_role(
    State(st): State<Arc<AppState>>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<String>,
    Json(body): Json<RoleBody>,
) -> Response {
    if let Err(response) = check_admin(&actor) {
        return response;
    }
    if actor.id == id {
        return bad_request("不能修改当前登录账号的角色");
    }
    let Some(role) = validate_role(&body.role) else {
        return bad_request("角色必须是 executive、operator 或 developer");
    };
    match st.storage.update_user_role(&id, role).await {
        Ok(()) => ApiResponse::ok(json!({ "role": role })).into_response(),
        Err(error) => bad_request(format!("更新用户角色失败：{error}")),
    }
}
