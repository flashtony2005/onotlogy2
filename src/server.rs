//! Trace API 服务（对应 REQ-009 / api-agent-trace.md）
//!
//! - REST：`GET /trace`、`GET /trace/{id}`、`GET /run/{runId}/timeline?view=`、`POST /run`
//! - Human Approval（V0.4 遗留）：`GET /run/{runId}/approvals`（待审批列表）、
//!   `POST /run/{runId}/approval/{reqId}`（裁决 `{"decision":"approve"|"reject"}`，
//!   `require_approval` 动作挂起等待浏览器点批准/拒绝后恢复或 WB2005 终止）
//! - WebSocket：`/api/v1/wb/agent/{id}/stream`（历史 + 增量 + `?after` 断线重连补齐）
//! - 静态托管：三层观察 UI 构建产物 `ui/dist`（不存在时返回提示）
//!
//! 所有事件同源 `TraceEvent`，WS 与 timeline 共用 `TraceEventDto`（camelCase 单一真相源）。

use crate::action::ActionRegistry;
use crate::agent::AgentEngine;
use crate::auth::{
    AuthUser, IdentityProvider, LocalIdentityProvider, Role, SessionStore, clamp_view,
};
use crate::evolution::{
    EvolutionState, LearnedKnowledge, Pattern, Proposal, ProposalKind, ProposalStatus,
};
use crate::planner::{
    ApprovalRequest, ApprovalState, ApprovalStore, AutoApprover, Planner, PlannerError,
    PolicyApprover, RunStatus, RuntimeConfig, ServerApprover, generate_run_id,
};
use crate::runtime::RuntimeKernel;
use crate::storage::{ApprovalAudit, ApprovalRecord, TraceQuery};
use crate::trace::{RunTimeline, TraceEventDto, View, build_timeline};
use axum::extract::ws::{Message, WebSocket};
use axum::extract::{Extension, OriginalUri, Path, Query, Request, State, WebSocketUpgrade};
use axum::http::{StatusCode, header::AUTHORIZATION};
use axum::middleware::{self, Next};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::net::TcpListener;

/// 运行记录（内存态，供 timeline 状态覆盖与 WB2004/2005 快速反馈）。
struct RunRecord {
    status: RunStatus,
    /// 仅 Policy 拒绝才要求 Human Approval（WB2005）。
    human_approval: bool,
}

/// 共享应用状态。
pub struct AppState {
    pub kernel: Arc<RuntimeKernel>,
    pub engine: Arc<dyn AgentEngine>,
    /// 动作注册表（V0.4）：对象自有动作的声明式来源；空 → legacy 兜底。
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
#[derive(serde::Deserialize)]
struct TraceListParams {
    #[serde(rename = "runId", default)]
    run_id: Option<String>,
    #[serde(rename = "agentId", default)]
    agent_id: Option<String>,
    #[serde(rename = "objectRef", default)]
    object_ref: Option<String>,
    #[serde(rename = "eventType", default)]
    event_type: Option<String>,
    #[serde(rename = "afterId", default)]
    after_id: Option<String>,
    #[serde(rename = "pageNum", default)]
    page_num: Option<usize>,
    #[serde(rename = "pageSize", default)]
    page_size: Option<usize>,
}

#[derive(serde::Deserialize)]
struct WsParams {
    #[serde(default)]
    after: Option<String>,
}

#[derive(serde::Deserialize)]
struct ViewParam {
    #[serde(default)]
    view: Option<String>,
}

#[derive(serde::Deserialize)]
#[allow(dead_code)]
struct RunBody {
    #[serde(rename = "agentId", default)]
    agent_id: Option<String>,
    goal: String,
    #[serde(rename = "objectRefs", default)]
    object_refs: Option<Vec<String>>,
    #[serde(default)]
    context: Option<Value>,
}

// ── 响应 DTO ─────────────────────────────────────────────────────────────
#[derive(Serialize)]
struct TraceListDto {
    total: usize,
    list: Vec<TraceEventDto>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RunStartDto {
    run_id: String,
    status: String,
    stream_url: String,
}

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
fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/v1/wb/agent/trace", get(list_trace))
        .route("/api/v1/wb/agent/trace/:id", get(get_trace))
        .route("/api/v1/wb/agent/run/:run_id/timeline", get(timeline))
        .route("/api/v1/wb/agent/memory", get(list_memory))
        .route("/api/v1/wb/agent/evolution", get(list_evolution))
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
    });
    let app = router()
        .layer(middleware::from_fn(auth_layer))
        .layer(Extension(state.clone()))
        .with_state(state.clone());
    // 重启恢复：重建 runs 内存映射；崩溃残留 Running→Interrupted、orphan Pending→Expired。
    recover(&state).await;
    axum::serve(listener, app).await
}

/// 重启恢复（V0.7 持久化）：重建 runs 内存映射；崩溃残留 Running→Interrupted、orphan Pending→Expired
/// （简化分支：无"活 run"可恢复，故不做"run 存活→Pending 保留"死分支）。
async fn recover(state: &Arc<AppState>) {
    let runs = match state.kernel.storage().recover_persisted_state().await {
        Ok(r) => r,
        Err(_) => return,
    };
    let mut g = state.runs.lock().unwrap();
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
    let token = match bearer_token(&req) {
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

/// 从不透明 `Authorization: Bearer <token>` 提取令牌。
fn bearer_token(req: &Request) -> Option<String> {
    let h = req.headers().get(AUTHORIZATION)?;
    let s = h.to_str().ok()?;
    s.strip_prefix("Bearer ").map(|t| t.trim().to_string())
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
struct AuthToken(pub String);

/// 登录请求体。
#[derive(serde::Deserialize)]
struct LoginBody {
    username: String,
    password: String,
}

/// `POST /api/v1/wb/auth/login`：本地账号校验 → 签发不透明会话令牌。公开端点。
async fn auth_login(State(st): State<Arc<AppState>>, Json(body): Json<LoginBody>) -> Response {
    // 登录限流/锁定：本地单用户场景可不启用；多用户暴露前应在本 handler 加简单计数器
    // （按 IP/用户名，连续失败 N 次锁定 M 分钟），防引导账号爆破（见 design-auth.md §3.6 补充）。
    let user = match st.idp.authenticate(&body.username, &body.password).await {
        Ok(Some(u)) => u,
        Ok(None) => return unauthorized("用户名或口令错误"),
        Err(_) => return ApiResponse::<()>::error(5000, "认证服务错误").into_response(),
    };
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
async fn auth_logout(
    State(st): State<Arc<AppState>>,
    Extension(token): Extension<AuthToken>,
) -> Response {
    if st.sessions.revoke(&token.0).await.is_err() {
        return ApiResponse::<()>::error(5000, "登出失败").into_response();
    }
    ApiResponse::ok(json!({ "loggedOut": true })).into_response()
}

/// `GET /api/v1/wb/auth/me`：返回当前认证用户（供前端初始化角色）。需鉴权。
async fn auth_me(Extension(user): Extension<AuthUser>) -> Response {
    ApiResponse::ok(json!({
        "username": user.username,
        "role": user.role
    }))
    .into_response()
}

// ──  handlers  ───────────────────────────────────────────────────────────
/// `GET /api/v1/wb/agent/trace`：分页查询 Trace 事件（run/agent/object/type 过滤）。
async fn list_trace(
    State(st): State<Arc<AppState>>,
    Extension(_auth): Extension<AuthUser>,
    Query(q): Query<TraceListParams>,
) -> Json<ApiResponse<TraceListDto>> {
    let tq = TraceQuery {
        run_id: q.run_id,
        agent_id: q.agent_id,
        object_ref: q.object_ref,
        event_type: q.event_type,
        after_id: q.after_id,
        limit: 1000,
    };
    let events = st.kernel.list_events(tq).await.unwrap_or_default();
    let total = events.len();
    let page = q.page_num.unwrap_or(1).max(1);
    let size = q.page_size.unwrap_or(50).clamp(1, 500);
    let start = (page - 1) * size;
    let end = (start + size).min(total);
    let list: Vec<TraceEventDto> = events[start..end]
        .iter()
        .map(|e| TraceEventDto::from(e.clone()))
        .collect();
    ApiResponse::ok(TraceListDto { total, list })
}

/// `GET /api/v1/wb/agent/trace/:id`：单条 Trace（缺失 → WB2002）。
async fn get_trace(
    State(st): State<Arc<AppState>>,
    Extension(_auth): Extension<AuthUser>,
    Path(id): Path<String>,
) -> Response {
    match st.kernel.get_trace(&id).await {
        Ok(Some(e)) => ApiResponse::ok(TraceEventDto::from(e)).into_response(),
        Ok(None) => ApiResponse::error(wb_code("WB2002"), "Trace 不存在").into_response(),
        Err(_) => ApiResponse::error(5000, "存储错误").into_response(),
    }
}

/// `GET /api/v1/wb/agent/run/:run_id/timeline?view=`：运行时间线（三层视图）。
async fn timeline(
    State(st): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    Path(run_id): Path<String>,
    Query(q): Query<ViewParam>,
) -> Json<ApiResponse<RunTimeline>> {
    // 服务端视图钳制：前端 `?view=` 仅作建议，被钳制到角色允许上限（不信任前端自提权限）。
    let requested = View::from_param(&q.view.unwrap_or_else(|| "operator".into()));
    let view = clamp_view(requested, user.role);
    let events = st
        .kernel
        .list_events(TraceQuery::for_run(&run_id))
        .await
        .unwrap_or_default();
    let agent_id = events
        .first()
        .and_then(|e| e.agent_id.clone())
        .unwrap_or_default();
    let mut tl = build_timeline(events, run_id.clone(), agent_id, view);
    // 用运行记录覆盖状态：内存优先，重启后内存为空则回退 DB（捕捉 WB2004/2005 失败）。
    if let Some(rec) = st.runs.lock().unwrap().get(&run_id) {
        tl.status = rec.status.as_str().to_string();
        if rec.human_approval {
            tl.human_approval = "Required".to_string();
        }
    } else if let Ok(Some(rec)) = st.kernel.storage().get_run(&run_id).await {
        tl.status = rec.status.clone();
        if rec.human_approval {
            tl.human_approval = "Required".to_string();
        }
    }
    ApiResponse::ok(tl)
}

/// `GET /api/v1/wb/agent/memory`：记忆存储全量（V0.5 Memory 视图数据源，按 hits 降序）。
async fn list_memory(
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

/// `GET /api/v1/wb/agent/evolution`：Evolution 聚合状态（BL-003 · REQ-017）。
/// 返回 patterns / proposals / learned（含可追溯证据 run_ids 与 proposal_id）。
async fn list_evolution(
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
async fn revoke_learned(
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
struct AutoApproveBody {
    #[serde(default)]
    enabled: bool,
}

async fn toggle_auto_approve(
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
async fn list_approval_audit_handler(
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

/// `GET /api/v1/wb/agent/run/:run_id/approvals`：列出该运行的全部审批请求（含历史裁决）。
async fn list_approvals(
    State(st): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    Path(run_id): Path<String>,
) -> Response {
    // 读审批列表需 Operator 及以上。
    if !user.can_read_audit() {
        return forbidden("当前角色无权读取审批（需 Operator 及以上）");
    }
    // 合并：DB 持久记录（跨重启）+ 内存 store（live 唤醒状态），按 req_id 去重，内存优先。
    let mut by_id: HashMap<String, ApprovalRequest> = HashMap::new();
    if let Ok(rows) = st.kernel.storage().list_approvals_for_run(&run_id).await {
        for r in rows {
            by_id.insert(r.id.clone(), approval_record_to_request(&r));
        }
    }
    for req in st.store.list_for_run(&run_id) {
        by_id.insert(req.req_id.clone(), req);
    }
    let mut list: Vec<ApprovalRequest> = by_id.into_values().collect();
    list.sort_by(|a, b| a.req_id.cmp(&b.req_id));
    ApiResponse::ok(list).into_response()
}

#[derive(serde::Deserialize)]
struct ApprovalDecisionBody {
    /// `approve` | `yes` | `true` → 批准；其余 → 拒绝（WB2005 终止）。
    decision: String,
}

/// `POST /api/v1/wb/agent/run/:run_id/approval/:req_id`：裁决一条待审批请求，唤醒挂起的运行。
async fn resolve_approval(
    State(st): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    Path((run_id, req_id)): Path<(String, String)>,
    Json(body): Json<ApprovalDecisionBody>,
) -> Response {
    // 裁决审批需 Operator 及以上。
    if !user.can_resolve_approval() {
        return forbidden("当前角色无权裁决审批（需 Operator 及以上）");
    }
    let Some(req) = st.store.get(&req_id) else {
        return ApiResponse::error(404, "审批请求不存在或已裁决").into_response();
    };
    if req.run_id != run_id {
        return ApiResponse::error(404, "审批请求不属于该运行").into_response();
    }
    let approve = matches!(
        body.decision.to_ascii_lowercase().as_str(),
        "approve" | "yes" | "true"
    );
    if st.store.resolve(&req_id, approve) {
        // 持久化裁决（DB 真相源；resolved_by 取当前操作人 id）。
        let decision = if approve { "Approved" } else { "Rejected" };
        let _ = st
            .kernel
            .storage()
            .update_approval_state(&req_id, decision, Some(&user.id), Some(&now_micros()))
            .await;
        ApiResponse::ok(json!({
            "reqId": req_id,
            "decision": if approve { "Approved" } else { "Rejected" }
        }))
        .into_response()
    } else {
        ApiResponse::error(404, "审批请求不存在或已裁决").into_response()
    }
}

/// 将持久审批记录映射为 API `ApprovalRequest`（state 由字符串还原）。
fn approval_record_to_request(r: &ApprovalRecord) -> ApprovalRequest {
    ApprovalRequest {
        req_id: r.id.clone(),
        run_id: r.run_id.clone(),
        agent_id: r.agent_id.clone(),
        step_id: r.step_id.clone(),
        description: r.description.clone(),
        state: ApprovalState::from_db_str(&r.state),
    }
}

/// `POST /api/v1/wb/agent/run`：触发 Agent 运行（异步，立即返回 runId + streamUrl）。
async fn run_start(
    State(st): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    Json(body): Json<RunBody>,
) -> Response {
    // 触发运行需 Operator 及以上（Executive 只读）。
    if !user.can_trigger_run() {
        return forbidden("当前角色无权触发运行（需 Operator 及以上）");
    }
    let agent_id = match body.agent_id {
        Some(a) if !a.is_empty() => a,
        _ => return ApiResponse::error(wb_code("WB1001"), "agentId 必填").into_response(),
    };
    if body.goal.trim().is_empty() {
        return ApiResponse::error(wb_code("WB1001"), "goal 必填").into_response();
    }
    // 快速健康检查 → WB2004（避免悬挂连接，pi-doc / prd 4.5）
    if !st.engine.health().await {
        return ApiResponse::error(wb_code("WB2004"), "认知引擎不可用（Pi 未就绪）")
            .into_response();
    }

    let run_id = generate_run_id();
    st.runs.lock().unwrap().insert(
        run_id.clone(),
        RunRecord {
            status: RunStatus::Running,
            human_approval: false,
        },
    );
    // 持久化 run（DB 真相源，跨重启可恢复状态）。
    let _ = st
        .kernel
        .storage()
        .insert_run(&run_id, &agent_id, &body.goal)
        .await;

    let kernel = st.kernel.clone();
    let engine = st.engine.clone();
    let st2 = st.clone();
    let store = st.store.clone();
    let auto_approve = st.auto_approve;
    let goal = body.goal.clone();
    let ag = agent_id.clone();
    let rid = run_id.clone();
    tokio::spawn(async move {
        // Human Approval 闸门：
        // - auto_approve → 直接放行（AutoApprover）；
        // - 否则 `require_approval` 动作挂起，注册待审批请求并等待浏览器经 HTTP 端点裁决
        //   （ServerApprover），批准则恢复执行，拒绝则 WB2005 终止。
        let approver: Arc<dyn PolicyApprover> = if auto_approve {
            Arc::new(AutoApprover { allow: true })
        } else {
            let mut sa = ServerApprover::new(kernel.clone(), store, rid.clone(), ag.clone());
            if let Some(t) = st.approval_timeout {
                sa = sa.with_timeout(t);
            }
            Arc::new(sa)
        };
        let res = Planner::with_actions(&kernel, st.registry.clone())
            .with_approver(approver)
            .with_config(RuntimeConfig {
                auto_approve_from_evolution: st.auto_approve_evolution,
            })
            .run_goal(engine.as_ref(), &goal, &ag, &rid)
            .await;
        let rec = match res {
            Ok(_) => RunRecord {
                status: RunStatus::Completed,
                human_approval: false,
            },
            Err(PlannerError::PolicyDenied(_)) => RunRecord {
                status: RunStatus::Failed,
                human_approval: true,
            },
            Err(_) => RunRecord {
                status: RunStatus::Failed,
                human_approval: false,
            },
        };
        // 持久化终态（与内存映射并行）。
        let _ = st2
            .kernel
            .storage()
            .update_run_status(&rid, rec.status.as_str(), Some(&now_micros()))
            .await;
        st2.runs.lock().unwrap().insert(rid, rec);
    });

    let dto = RunStartDto {
        run_id: run_id.clone(),
        status: "Running".into(),
        stream_url: format!("/api/v1/wb/agent/{run_id}/stream"),
    };
    ApiResponse::ok(dto).into_response()
}

/// WebSocket 握手：`/api/v1/wb/agent/:id/stream`。
async fn ws_handler(
    ws: WebSocketUpgrade,
    State(st): State<Arc<AppState>>,
    Extension(_auth): Extension<AuthUser>,
    Path(id): Path<String>,
    Query(params): Query<WsParams>,
) -> Response {
    ws.on_upgrade(move |socket| handle_ws(socket, st, id, params.after))
}

/// WS 主体：先补历史（含 `?after` 重连补齐），再增量推送匹配事件。
async fn handle_ws(socket: WebSocket, st: Arc<AppState>, id: String, after: Option<String>) {
    let (mut sender, mut receiver) = socket.split();

    // 历史补齐（run/agent 任一匹配）
    let history = collect_history(&st, &id, after.as_deref()).await;
    for dto in history {
        if sender
            .send(Message::Text(serde_json::to_string(&dto).unwrap()))
            .await
            .is_err()
        {
            return;
        }
    }

    // 增量：订阅 EventBus，转发匹配事件；同时监听客户端关闭。
    let mut rx = st.kernel.event_bus().subscribe();
    loop {
        tokio::select! {
            ev = rx.recv() => {
                match ev {
                    Ok(e) => {
                        if e.run_id.as_deref() == Some(id.as_str())
                            || e.agent_id.as_deref() == Some(id.as_str())
                        {
                            let dto = TraceEventDto::from(e);
                            if sender
                                .send(Message::Text(serde_json::to_string(&dto).unwrap()))
                                .await
                                .is_err()
                            {
                                return;
                            }
                        }
                    }
                    Err(_) => return, // 广播通道关闭
                }
            }
            client = receiver.next() => {
                match client {
                    Some(Ok(axum::extract::ws::Message::Close(_))) | None => return,
                    Some(Ok(_)) => continue,
                    Some(Err(_)) => return,
                }
            }
        }
    }
}

/// 收集历史事件（run 或 agent 匹配），去重排序，并按 after 游标截断（断线重连补齐）。
async fn collect_history(st: &AppState, id: &str, after: Option<&str>) -> Vec<TraceEventDto> {
    let mut run = st
        .kernel
        .list_events(TraceQuery::for_run(id))
        .await
        .unwrap_or_default();
    let mut agent = st
        .kernel
        .list_events(TraceQuery::for_agent(id))
        .await
        .unwrap_or_default();
    run.append(&mut agent);

    let mut seen: HashMap<String, crate::runtime::TraceEvent> = HashMap::new();
    for e in run {
        seen.insert(e.id.clone(), e);
    }
    let mut merged: Vec<_> = seen.into_values().collect();
    merged.sort_by(|a, b| {
        (a.timestamp.clone(), a.id.clone()).cmp(&(b.timestamp.clone(), b.id.clone()))
    });

    let cut = after.and_then(|aid| merged.iter().position(|e| e.id == aid).map(|p| p + 1));
    if let Some(start) = cut {
        merged = merged.split_off(start);
    }
    merged.into_iter().map(TraceEventDto::from).collect()
}

/// 静态文件托管（SPA 兜底 index.html）。作为 Router::fallback，保留原始请求路径。
async fn serve_static(State(st): State<Arc<AppState>>, uri: OriginalUri) -> Response {
    if uri.path().starts_with("/api/") {
        let body = ApiResponse::<()>::error(404, "接口不存在");
        return (StatusCode::NOT_FOUND, body).into_response();
    }
    let path = uri.path().trim_start_matches('/');
    let rel = if path.is_empty() {
        "index.html".to_string()
    } else {
        path.to_string()
    };
    let full = format!("{}/{}", st.static_dir, rel);
    match tokio::fs::read(&full).await {
        Ok(bytes) => (
            [(axum::http::header::CONTENT_TYPE, content_type(&full))],
            bytes,
        )
            .into_response(),
        Err(_) => {
            let idx = format!("{}/index.html", st.static_dir);
            match tokio::fs::read(&idx).await {
                Ok(b) => (
                    [(
                        axum::http::header::CONTENT_TYPE,
                        "text/html; charset=utf-8".to_string(),
                    )],
                    b,
                )
                    .into_response(),
                Err(_) => Html(
                    "<h1>OpenAIP-OS Agent Observation UI</h1>\
                     <p>未检测到 ui/dist。请进入 ui/ 执行 <code>npm install &amp;&amp; npm run build</code> 后重启服务。</p>"
                        .to_string(),
                )
                .into_response(),
            }
        }
    }
}

/// 按扩展名推断 Content-Type。
fn content_type(path: &str) -> String {
    let p = path.to_ascii_lowercase();
    if p.ends_with(".html") {
        "text/html; charset=utf-8"
    } else if p.ends_with(".js") {
        "application/javascript; charset=utf-8"
    } else if p.ends_with(".css") {
        "text/css; charset=utf-8"
    } else if p.ends_with(".json") {
        "application/json; charset=utf-8"
    } else if p.ends_with(".svg") {
        "image/svg+xml"
    } else if p.ends_with(".png") {
        "image/png"
    } else if p.ends_with(".ico") {
        "image/x-icon"
    } else if p.ends_with(".wasm") {
        "application/wasm"
    } else {
        "application/octet-stream"
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_start_dto_is_camel_case() {
        let dto = RunStartDto {
            run_id: "run_1".into(),
            status: "Running".into(),
            stream_url: "/api/v1/wb/agent/run_1/stream".into(),
        };
        let json = serde_json::to_value(dto).unwrap();
        assert_eq!(json["runId"], "run_1");
        assert_eq!(json["streamUrl"], "/api/v1/wb/agent/run_1/stream");
        assert!(json.get("run_id").is_none());
        assert!(json.get("stream_url").is_none());
    }
}
