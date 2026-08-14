use std::sync::Arc;

use crate::auth::AuthUser;
use axum::Json;
use axum::extract::{Extension, State};
use axum::http::StatusCode;
use axum::response::Response;
use serde_json::json;

use super::*;
/// 登录请求体。
#[derive(serde::Deserialize)]
pub(crate) struct LoginBody {
    username: String,
    password: String,
}

/// `POST /api/v1/wb/auth/login`：本地账号校验 → 签发不透明会话令牌。公开端点。
/// M3：登录限流——连续失败 5 次锁定该用户名 5 分钟（防引导账号爆破）。
pub(crate) async fn auth_login(
    State(st): State<Arc<AppState>>,
    Json(body): Json<LoginBody>,
) -> Response {
    if st.login_locked(&body.username) {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            ApiResponse::<()>::error(
                429,
                format!("登录失败次数过多，请 {} 分钟后再试", LOGIN_LOCK_SECS / 60),
            ),
        )
            .into_response();
    }
    let user = match st.idp.authenticate(&body.username, &body.password).await {
        Ok(Some(u)) => u,
        Ok(None) => {
            st.record_login_failure(&body.username);
            return unauthorized("用户名或口令错误");
        }
        Err(_) => return ApiResponse::<()>::error(5000, "认证服务错误").into_response(),
    };
    st.clear_login_failures(&body.username);
    let token = match st.sessions.issue(&user).await {
        Ok(t) => t,
        Err(_) => return ApiResponse::<()>::error(5000, "会话签发失败").into_response(),
    };
    ApiResponse::ok(json!({
        "token": token,
        "user": { "username": user.username, "role": user.role }
    }))
    .into_response()
}

/// `POST /api/v1/wb/auth/logout`：撤销当前会话令牌。需鉴权。
pub(crate) async fn auth_logout(
    State(st): State<Arc<AppState>>,
    Extension(token): Extension<AuthToken>,
) -> Response {
    if st.sessions.revoke(&token.0).await.is_err() {
        return ApiResponse::<()>::error(5000, "登出失败").into_response();
    }
    ApiResponse::ok(json!({ "loggedOut": true })).into_response()
}

/// `GET /api/v1/wb/auth/me`：返回当前认证用户（供前端初始化角色）。需鉴权。
pub(crate) async fn auth_me(Extension(user): Extension<AuthUser>) -> Response {
    ApiResponse::ok(json!({
        "username": user.username,
        "role": user.role
    }))
    .into_response()
}
