//! HTTP + WS 服务层（对应 REQ-009 / agent-observation-design.md）
//!
//! 模块拆分（长文件模块化）：状态/路由/鉴权中间件在 `mod`，认证端点 `auth`，
//! 图端点 `graph`，事件/WS `trace`，业务视图 `views`，审批 `approvals`，
//! 运行触发 `run`，静态托管 `static`。

pub mod approvals;
pub mod auth;
pub mod graph;
pub mod run;
pub mod static_files;
pub mod trace;
pub mod v2;
pub mod views;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::action::ActionRegistry;
use crate::agent::AgentEngine;
use crate::auth::{AuthUser, IdentityProvider, LocalIdentityProvider, Role, SessionStore};
use crate::planner::{ApprovalStore, RunStatus, lock_mutex};
use crate::runtime::RuntimeKernel;
use axum::extract::{Extension, Request};
use axum::http::{StatusCode, header::AUTHORIZATION};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde::Serialize;
use tokio::net::TcpListener;

// router 引用的子模块 handler
use approvals::list_approvals;
use approvals::resolve_approval;
use auth::auth_login;
use auth::auth_logout;
use auth::auth_me;
use graph::list_graph;
use run::run_start;
use static_files::serve_static;
use trace::get_trace;
use trace::list_trace;
use trace::timeline;
use trace::ws_handler;
use views::list_approval_audit_handler;
use views::list_evaluations;
use views::list_evolution;
use views::list_memory;
use views::list_mutations;
use views::list_tasks;
use views::offline_benchmark_mutation;
use views::review_mutation;
use views::revoke_learned;
use views::toggle_auto_approve;

struct RunRecord {
    status: RunStatus,
    /// 仅 Policy 拒绝才要求 Human Approval（WB2005）。
    human_approval: bool,
}

/// 共享应用状态。
pub struct AppState {
    /// Phase 10 V2 认知状态（Skill/Plugin/Session）。
    pub v2: v2::SharedV2,
    pub kernel: Arc<RuntimeKernel>,
    pub engine: Arc<dyn AgentEngine>,
    /// 动作注册表（V0.4）：对象自有动作的声明式来源。
    pub registry: ActionRegistry,
    /// 是否自动批准需审批动作（Human Approval）：默认 false → require_approval 动作挂起等待 HTTP 审批。
    auto_approve: bool,
    /// REQ-018 Evolution 第二阶段 AutoApprove 全局开关：默认 false → 行为等同 MVP（仍走 Human Approval）。
    auto_approve_evolution: bool,
    /// 挂起审批超时（None → 默认 APPROVAL_TIMEOUT 300s）；测试注入极短超时触发 Expired 回收路径。
    approval_timeout: Option<Duration>,
    /// 待审批请求存储（V0.4 遗留的 server 挂起式 HTTP 审批端点）；连接 Planner 挂起点与审批 API。
    store: Arc<ApprovalStore>,
    runs: Mutex<HashMap<String, RunRecord>>,
    pub static_dir: String,
    /// 身份提供方（V0.7 认证）：本地账号实现；OIDC 后续同 trait 插拔。
    idp: Arc<dyn IdentityProvider>,
    /// 会话存储（V0.7 认证）：不透明令牌签发/校验/撤销。
    sessions: SessionStore,
    /// 是否强制认证（V0.7）：生产 true；集成测试以 false 注入合成 Developer 直接放行。
    enforce_auth: bool,
    /// 登录失败计数（M3：防引导账号爆破）。按用户名：连续失败 ≥ 阈值 → 锁定窗口内拒绝 429。
    login_fails: Mutex<HashMap<String, LoginFail>>,
}

/// 登录失败记录（连续失败次数 + 首次失败时间，用于锁定窗口判定）。
struct LoginFail {
    count: u32,
    since: std::time::Instant,
}

/// 登录限流参数：连续失败 5 次锁定 5 分钟（窗口过后自动放行并重置计数）。
const MAX_LOGIN_FAILS: u32 = 5;
const LOGIN_LOCK_SECS: u64 = 300;

impl AppState {
    /// 检查该用户名是否处于登录锁定窗口。
    fn login_locked(&self, username: &str) -> bool {
        let g = lock_mutex(&self.login_fails);
        match g.get(username) {
            Some(f) => f.count >= MAX_LOGIN_FAILS && f.since.elapsed().as_secs() < LOGIN_LOCK_SECS,
            None => false,
        }
    }

    /// 记录一次登录失败（窗口过期自动重置）。
    fn record_login_failure(&self, username: &str) {
        let mut g = lock_mutex(&self.login_fails);
        match g.get_mut(username) {
            Some(f) => {
                if f.since.elapsed().as_secs() >= LOGIN_LOCK_SECS {
                    *f = LoginFail {
                        count: 1,
                        since: std::time::Instant::now(),
                    };
                } else {
                    f.count += 1;
                }
            }
            None => {
                g.insert(
                    username.to_string(),
                    LoginFail {
                        count: 1,
                        since: std::time::Instant::now(),
                    },
                );
            }
        }
    }

    /// 登录成功：清除失败计数。
    fn clear_login_failures(&self, username: &str) {
        lock_mutex(&self.login_fails).remove(username);
    }
}

/// 统一响应信封：`code` 为数值（0=成功；WBxxxx → 数字部分），`data` 为业务数据。
#[derive(Serialize)]

pub struct ApiResponse<T: Serialize> {
    pub code: i32,
    pub msg: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
}

impl<T: Serialize> ApiResponse<T> {
    pub fn ok(data: T) -> Json<ApiResponse<T>> {
        Json(ApiResponse {
            code: 0,
            msg: "success".into(),
            data: Some(data),
        })
    }
}

impl ApiResponse<()> {
    /// 错误响应：WB 错误码转数值（WB2004 → 2004）。
    pub fn error(code: i32, msg: impl Into<String>) -> Json<ApiResponse<()>> {
        Json(ApiResponse {
            code,
            msg: msg.into(),
            data: None,
        })
    }
}

/// WB 错误码字符串 → 数值（去掉 WB 前缀，未知 → 5000）。
fn wb_code(s: &str) -> i32 {
    s.trim_start_matches("WB").parse().unwrap_or(5000)
}

/// 当前微秒时间戳（run/approval 持久化时间戳）。
fn now_micros() -> String {
    let micros = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros();
    format!("{micros}")
}

// ── 请求参数 ──────────────────────────────────────────────────────────────

fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/v1/wb/agent/trace", get(list_trace))
        .route("/api/v1/wb/agent/trace/:id", get(get_trace))
        .route("/api/v1/wb/agent/run/:run_id/timeline", get(timeline))
        .route("/api/v1/wb/agent/memory", get(list_memory))
        .route("/api/v1/wb/agent/tasks", get(list_tasks))
        .route("/api/v1/wb/agent/evolution", get(list_evolution))
        .route("/api/v1/wb/agent/evaluation", get(list_evaluations))
        .route("/api/v1/wb/agent/mutation", get(list_mutations))
        .route(
            "/api/v1/wb/agent/mutation/:id/review",
            post(review_mutation),
        )
        .route(
            "/api/v1/wb/agent/mutation/:id/offline-benchmark",
            post(offline_benchmark_mutation),
        )
        .route(
            "/api/v1/wb/agent/evolution/learned/:id",
            delete(revoke_learned),
        )
        .route(
            "/api/v1/wb/agent/evolution/learned/:id/auto-approve",
            post(toggle_auto_approve),
        )
        .route(
            "/api/v1/wb/agent/evolution/audit",
            get(list_approval_audit_handler),
        )
        .route("/api/v1/wb/agent/run", post(run_start))
        .route(
            "/api/v1/wb/agent/run/:run_id/approvals",
            get(list_approvals),
        )
        .route(
            "/api/v1/wb/agent/run/:run_id/approval/:req_id",
            post(resolve_approval),
        )
        .route("/api/v1/wb/agent/:id/stream", get(ws_handler))
        // ── V0.7 认证端点 ──
        .route("/api/v1/wb/auth/login", post(auth_login))
        .route("/api/v1/wb/auth/logout", post(auth_logout))
        .route("/api/v1/wb/auth/me", get(auth_me))
        // ── V0.8 GraphRAG 基础（对象-关系图） ──
        .route("/api/v1/wb/graph", get(list_graph))
        // ── Phase 10 V2 认知面板（Skill / Plugin / Session） ──
        .route("/api/v1/v2/skills", get(v2::list_skills))
        .route(
            "/api/v1/v2/skills/import",
            axum::routing::post(v2::import_skill),
        )
        .route("/api/v1/v2/skills/:id/raw", get(v2::get_skill_raw))
        .route("/api/v1/v2/plugins", get(v2::list_plugins))
        .route("/api/v1/v2/workers", get(v2::list_workers))
        .route("/api/v1/v2/sessions", get(v2::list_sessions))
        .route("/api/v1/v2/sessions/:id", get(v2::get_session))
        .route(
            "/api/v1/v2/sessions/:id/create",
            axum::routing::post(v2::create_session),
        )
        // 静态文件托管（SPA 兜底 index.html），保留原始请求路径。
        .fallback(get(serve_static))
}

/// 启动 HTTP + WS 服务（绑定已就绪的 listener，返回绑定地址供测试使用）。
/// `auto_approve`：是否自动批准需审批动作（Human Approval）；
/// false → `require_approval` 动作挂起，由浏览器经 HTTP 审批端点裁决后恢复（V0.4 遗留）。
/// `enforce_auth`：是否强制认证（V0.7）；生产传 `true`，集成测试传 `false`（注入合成 Developer）。
#[allow(clippy::too_many_arguments)]
pub async fn serve(
    kernel: Arc<RuntimeKernel>,
    engine: Arc<dyn AgentEngine>,
    listener: TcpListener,
    static_dir: impl Into<String>,
    registry: ActionRegistry,
    auto_approve: bool,
    auto_approve_evolution: bool,
    enforce_auth: bool,
) -> std::io::Result<()> {
    serve_with_timeout(
        kernel,
        engine,
        listener,
        static_dir,
        registry,
        auto_approve,
        auto_approve_evolution,
        enforce_auth,
        None,
    )
    .await
}

/// `serve` 的可测试变体：允许注入挂起审批超时（测试触发 Expired 回收路径）。
/// `approval_timeout=None` 时使用默认 `APPROVAL_TIMEOUT`（300s）。
#[doc(hidden)]
#[allow(clippy::too_many_arguments)]
pub async fn serve_with_timeout(
    kernel: Arc<RuntimeKernel>,
    engine: Arc<dyn AgentEngine>,
    listener: TcpListener,
    static_dir: impl Into<String>,
    registry: ActionRegistry,
    auto_approve: bool,
    auto_approve_evolution: bool,
    enforce_auth: bool,
    approval_timeout: Option<Duration>,
) -> std::io::Result<()> {
    // 认证/会话提供方复用内核同一存储句柄（LibSQLStorage 内部 Arc，单连接共享库）。
    let storage = kernel.storage();
    let idp: Arc<dyn IdentityProvider> = Arc::new(LocalIdentityProvider::new(storage.clone()));
    let sessions = SessionStore::new(storage);
    let state = Arc::new(AppState {
        v2: Arc::new(v2::V2State::default()),
        kernel,
        engine,
        registry,
        auto_approve,
        auto_approve_evolution,
        approval_timeout,
        store: Arc::new(ApprovalStore::new()),
        runs: Mutex::new(HashMap::new()),
        static_dir: static_dir.into(),
        idp,
        sessions,
        enforce_auth,
        login_fails: Mutex::new(HashMap::new()),
    });
    let app = router()
        .layer(middleware::from_fn(auth_layer))
        .layer(middleware::from_fn(security_headers))
        .layer(Extension(state.clone()))
        .with_state(state.clone());
    // 重启恢复：重建 runs 内存映射；崩溃残留 Running→Interrupted、orphan Pending→Expired。
    recover(&state).await;
    // V10.1：预置 Worker（echo / git / codex / claude 壳）。
    bootstrap_v2_workers(&state);
    axum::serve(listener, app).await
}

/// 预置 Worker（V10.1）：echo / git / codex / claude 壳注册进 V2 Worker Registry。
/// Codex/Claude 命令可经环境变量覆盖（OPENAI_OS_CODEX_CMD / OPENAI_OS_CLAUDE_CMD）；
/// 未安装时注册仍成功（能力声明可见），执行阶段才失败。
fn bootstrap_v2_workers(state: &Arc<AppState>) {
    use crate::worker::adapters::{claude_worker, codex_worker};
    use crate::worker::manifest::{ExecutionProtocol, WorkerManifest};
    use crate::worker::process::ProcessWorker;

    let mut reg = state.v2.workers.write().unwrap();
    // echo CLI（text.echo）
    let mut echo_m = WorkerManifest::new("echo", "cli").with_capabilities(&["text.echo"]);
    echo_m.execution = ExecutionProtocol::Process;
    let _ = reg.register(Box::new(ProcessWorker::new(echo_m, "echo")));
    // git CLI（git.version / git.status）
    let mut git_m =
        WorkerManifest::new("git", "cli").with_capabilities(&["git.version", "git.status"]);
    git_m.execution = ExecutionProtocol::Process;
    let _ = reg.register(Box::new(ProcessWorker::new(git_m, "git")));
    // Codex / Claude 适配器壳（能力声明就位，命令可覆盖）
    let codex_cmd = std::env::var("OPENAI_OS_CODEX_CMD").unwrap_or_else(|_| "codex".into());
    let _ = reg.register(Box::new(codex_worker(Some(&codex_cmd))));
    let claude_cmd = std::env::var("OPENAI_OS_CLAUDE_CMD").unwrap_or_else(|_| "claude".into());
    let _ = reg.register(Box::new(claude_worker(Some(&claude_cmd))));
    // DeepSeek Harness 适配器壳（Phase 11D：Harness 是 Worker，不是内核）
    let deepseek_cmd =
        std::env::var("OPENAI_OS_DEEPSEEK_CMD").unwrap_or_else(|_| "deepseek".into());
    let _ = reg.register(Box::new(crate::worker::adapters::deepseek_worker(Some(
        &deepseek_cmd,
    ))));
}

/// 重启恢复（V0.7 持久化）：重建 runs 内存映射；崩溃残留 Running→Interrupted、orphan Pending→Expired
/// （简化分支：无"活 run"可恢复，故不做"run 存活→Pending 保留"死分支）。
async fn recover(state: &Arc<AppState>) {
    let runs = match state.kernel.storage().recover_persisted_state().await {
        Ok(r) => r,
        Err(_) => return,
    };
    let mut g = lock_mutex(&state.runs);
    for r in runs {
        g.insert(
            r.id.clone(),
            RunRecord {
                status: RunStatus::from_db_str(&r.status),
                human_approval: r.human_approval,
            },
        );
    }
}

/// 安全响应头（LOW：无 CORS 场景下补基础头，缓解 MIME 嗅探 / 点击劫持 / Referer 泄露）。
async fn security_headers(req: Request, next: Next) -> Response {
    let mut res = next.run(req).await;
    let h = res.headers_mut();
    h.insert(
        axum::http::header::HeaderName::from_static("x-content-type-options"),
        axum::http::HeaderValue::from_static("nosniff"),
    );
    h.insert(
        axum::http::header::HeaderName::from_static("x-frame-options"),
        axum::http::HeaderValue::from_static("DENY"),
    );
    h.insert(
        axum::http::header::HeaderName::from_static("referrer-policy"),
        axum::http::HeaderValue::from_static("no-referrer"),
    );
    res
}

/// 认证中间件（V0.7）：从 `Authorization: Bearer <token>` 解析会话，注入 `AuthUser` 扩展。
/// `AppState` 由外层 `Extension` 层注入（避免与 handler 的 `State` 重复，也规避多 axum 版本推断冲突）。
/// - 白名单（静态资源 + 登录）：直接放行。
/// - `enforce_auth=false`：测试/开发模式，注入合成 Developer 直接放行（仅测试用）。
/// - 其他：缺令牌 / 无效 / 过期 / 禁用用户 → 401。
async fn auth_layer(mut req: Request, next: Next) -> Response {
    let path = req.uri().path().to_string();
    if is_public(&path) {
        return next.run(req).await;
    }
    let st = match req.extensions().get::<Arc<AppState>>().cloned() {
        Some(s) => s,
        None => return unauthorized("服务未就绪"),
    };
    if !st.enforce_auth {
        // 测试/开发模式：注入 Developer 直接放行（不校验令牌，仅测试用，生产 enforce_auth=true）。
        req.extensions_mut().insert(AuthUser {
            id: "test".into(),
            username: "test".into(),
            role: Role::Developer,
        });
        return next.run(req).await;
    }
    let token = match bearer_token(&req).or_else(|| {
        // 浏览器 WebSocket 无法设置 Authorization 头 → WS 流路径允许 `?token=` 查询参数。
        if is_ws_stream_path(&path) {
            query_token(&req)
        } else {
            None
        }
    }) {
        Some(t) => t,
        None => return unauthorized("缺少认证令牌"),
    };
    match st.sessions.validate(&token).await {
        Ok(Some(user)) => {
            req.extensions_mut().insert(user);
            req.extensions_mut().insert(AuthToken(token));
            next.run(req).await
        }
        _ => unauthorized("无效或已过期的令牌"),
    }
}

/// 公共白名单：静态资源（非 `/api/` 前缀）与登录端点免鉴权。
fn is_public(path: &str) -> bool {
    path == "/api/v1/wb/auth/login" || !path.starts_with("/api/")
}

/// WS 实时流路径（`/api/v1/wb/agent/{id}/stream`）。
fn is_ws_stream_path(path: &str) -> bool {
    path.starts_with("/api/v1/wb/agent/") && path.ends_with("/stream")
}

/// 从不透明 `Authorization: Bearer <token>` 提取令牌。
fn bearer_token(req: &Request) -> Option<String> {
    let h = req.headers().get(AUTHORIZATION)?;
    let s = h.to_str().ok()?;
    s.strip_prefix("Bearer ").map(|t| t.trim().to_string())
}

/// 从查询参数提取令牌（仅 WS 流路径使用，REST 一律走 Bearer 头）。
fn query_token(req: &Request) -> Option<String> {
    let q = req.uri().query()?;
    for pair in q.split('&') {
        if let Some((k, v)) = pair.split_once('=')
            && k == "token"
            && !v.is_empty()
        {
            return Some(v.to_string());
        }
    }
    None
}

/// 401 响应（缺令牌 / 令牌无效）。
fn unauthorized(msg: &str) -> Response {
    (StatusCode::UNAUTHORIZED, ApiResponse::<()>::error(401, msg)).into_response()
}

/// 403 响应（已认证但角色无权）。
fn forbidden(msg: &str) -> Response {
    (StatusCode::FORBIDDEN, ApiResponse::<()>::error(403, msg)).into_response()
}

/// 中间件注入的会话令牌（供 logout 撤销）。
#[derive(Clone)]
pub(crate) struct AuthToken(pub String);
