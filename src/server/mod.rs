//! HTTP + WS 服务层（对应 REQ-009 / agent-observation-design.md）
//!
//! 模块拆分（长文件模块化）：状态/路由/鉴权中间件在 `mod`，认证端点 `auth`，
//! 图端点 `graph`，事件/WS `trace`，业务视图 `views`，审批 `approvals`，
//! 运行触发 `run`，静态托管 `static`。

pub mod approvals;
pub mod auth;
pub mod automation;
pub mod business_loop;
pub mod domain_packs;
pub mod facade;
pub mod graph;
pub mod ontology;
mod ops;
mod plugins;
mod rate_limit;
pub mod run;
pub mod static_files;
pub mod trace;
pub mod users;
pub mod v2;
pub mod views;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use crate::auth::{AuthUser, IdentityProvider, LocalIdentityProvider, Role, SessionStore};
use crate::cognitive::approval::{ApprovalDecision, ApprovalRequest};
use crate::compose::Service;
use crate::session::run::RunStatus;
use crate::storage::LibSQLStorage;
use axum::extract::{Extension, Request};
use axum::http::{StatusCode, header::AUTHORIZATION};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Serialize;
use tokio::net::TcpListener;

use rate_limit::{Channel, RateLimiter};

// router 引用的子模块 handler
use approvals::list_approvals;
use approvals::resolve_approval;
use auth::auth_login;
use auth::auth_logout;
use auth::auth_me;
use auth::auth_rotate_key;
use automation::{
    cancel_job, get_job, get_pack_status, list_jobs, run_operation, run_pack, scan_pack,
};
use business_loop::{discover, list_goal_gaps, recommendations, run_next};
use facade::{
    facade_audit_query, facade_fabric_cancel, facade_fabric_submit, facade_project_create,
    facade_project_deps, facade_project_event, facade_project_list, facade_project_seed,
    facade_world_query,
};
use graph::list_graph;
use ontology::{
    apply_knowledge_proposal, create_knowledge_proposal, current_knowledge,
    diagnose_current_knowledge, initialize_knowledge, list_knowledge_proposals,
    list_knowledge_versions, reason_current_knowledge, review_knowledge_proposal,
    rollback_knowledge,
};
use ops::{healthz, metrics, readyz};
use plugins::{get_plugin_manifest, invoke_plugin_capability, list_plugins};
use run::run_start;
use static_files::serve_static;
use trace::get_trace;
use trace::list_trace;
use trace::timeline;
use trace::ws_handler;
use users::{create_user, list_users, set_user_disabled, set_user_role, update_user_password};
use views::list_audit;
use views::list_evaluations;
use views::list_mutations;
use views::loop_health;
use views::offline_benchmark_mutation;
use views::review_mutation;

/// 获取互斥锁：poison 时恢复内层数据（不 panic），避免审批/runs 映射路径因 poison 整进程崩溃。
pub(crate) fn lock_mutex<'a, T>(m: &'a Mutex<T>) -> MutexGuard<'a, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// HTTP 审批挂起的最长等待时间；超时按 Expire 裁决（不授予权限）。
pub(crate) const APPROVAL_TIMEOUT: Duration = Duration::from_secs(300);

struct RunRecord {
    status: RunStatus,
    /// 仅 Policy 拒绝才要求 Human Approval（WB2005）。
    human_approval: bool,
}

/// M1c 挂起审批条目：请求展示信息 + 裁决通道（run 的 resolver 阻塞等待）。
/// 裁决后从注册表移除；超时看门狗经同通道发 Expire。
/// 挂起审批的续跑目标（T1：compose Fiber 由 resolver 唤醒；fabric 由裁决时同步续跑）。
#[derive(Debug, Clone)]
pub enum ResumeTarget {
    /// compose 认知闭环：resolver 阻塞等 tx 裁决。
    ComposeFiber(crate::kernel::FiberId),
    /// fabric 提交：裁决时同步调 facade.fabric_resolve（无阻塞 resolver）。
    FabricRun { session_id: String, run_id: String },
}

pub struct PendingApproval {
    pub run_id: String,
    pub request: ApprovalRequest,
    pub tx: std::sync::mpsc::Sender<ApprovalDecision>,
    pub created_at: std::time::Instant,
    /// T1：裁决后如何续跑（compose=fiber 唤醒；fabric=同步 resume）。
    pub resume: ResumeTarget,
}

impl PendingApproval {
    /// 从持久化 task.request 重建 fabric 审批条目。
    pub(crate) fn from_fabric_request(
        request: &crate::fabric::AgentMessage,
    ) -> Option<(String, Self)> {
        let run_id = request.correlation_id.clone()?;
        let capability = request.capability.clone().unwrap_or_default();
        let (tx, _rx) = std::sync::mpsc::channel();
        Some((
            run_id.clone(),
            Self {
                run_id: run_id.clone(),
                request: ApprovalRequest {
                    id: run_id.clone(),
                    run_id: run_id.clone(),
                    context: crate::kernel::ContextId(0),
                    fiber: crate::kernel::FiberId(0),
                    capability,
                    action: request.intent.clone().unwrap_or_default(),
                    args: request.payload.clone(),
                    created_at: request.timestamp,
                },
                tx,
                created_at: std::time::Instant::now(),
                resume: ResumeTarget::FabricRun {
                    session_id: request.session_id.clone(),
                    run_id,
                },
            },
        ))
    }
}

/// 共享应用状态。
pub struct AppState {
    /// Phase 10 V2 认知状态（Skill/Plugin/Session）。
    pub v2: v2::SharedV2,
    pub storage: Arc<LibSQLStorage>,
    /// 可发现的业务包目录；macOS 本地版由启动壳指向 Resources/domain-packs。
    pub domain_packs_dir: PathBuf,
    /// 宿主级插件目录：HTTP、CLI、自动化 Job 共享同一能力解析边界。
    pub plugin_catalog: crate::plugin::catalog::SharedPluginCatalog,
    /// Phase 2.5：watcher + 持久化任务队列 + 业务包调度器。
    pub automation: Arc<crate::automation::AutomationEngine>,
    /// M1c V2 组合层：run 路径的服务集合（ODL → compose::Service）。
    pub v2_services: Arc<tokio::sync::RwLock<Vec<Service>>>,
    /// M1c V2 世界效应声明（Phase 5：Effect 真实改变 World）。
    pub v2_effect_defs: Arc<tokio::sync::RwLock<Vec<crate::compose::defs::RuntimeDefinition>>>,
    /// M1c V2 认知引擎（run 路径；PiEngine 生产，测试注入 FakeEngine）。
    pub v2_engine: Arc<dyn crate::cognitive::CognitiveEngine>,
    /// 是否自动批准需审批动作（Human Approval）：默认 false → require_approval 动作挂起等待 HTTP 审批。
    auto_approve: bool,
    /// REQ-018 Evolution 第二阶段 AutoApprove 全局开关：默认 false → 行为等同 MVP（仍走 Human Approval）。
    auto_approve_evolution: bool,
    /// 挂起审批超时（None → 默认 APPROVAL_TIMEOUT 300s）；测试注入极短超时触发 Expired 回收路径。
    approval_timeout: Option<Duration>,
    /// M1c 挂起审批注册表（run 路径 Waiting 时注册；HTTP 端点据此列表/裁决）。
    pub pending: Arc<Mutex<HashMap<String, PendingApproval>>>,
    /// 事件发布（WS 实时流；run 终局 flush 时逐条发布，订阅者与历史补齐同源去重）。
    pub event_tx: tokio::sync::broadcast::Sender<crate::trace::TraceEvent>,
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
    /// Phase 7：Public Semantic Facade（world/audit/fabric 三语义域；Server Transport）。
    pub facade: Arc<crate::facade::Facade>,
    /// S2a：执行/审批/WS 独立限流；限流器自身故障时 fail-closed。
    pub(crate) rate_limiter: Arc<RateLimiter>,
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

// ── 请求参数 ──────────────────────────────────────────────────────────────

fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
        .route("/metrics", get(metrics))
        .route("/api/v1/wb/agent/trace", get(list_trace))
        .route("/api/v1/wb/agent/trace/:id", get(get_trace))
        .route("/api/v1/wb/agent/run/:run_id/timeline", get(timeline))
        .route("/api/v1/wb/agent/evaluation", get(list_evaluations))
        .route("/api/v1/wb/agent/audit", get(list_audit))
        .route("/api/v1/wb/agent/mutation", get(list_mutations))
        .route("/api/v1/wb/agent/health", get(loop_health))
        .route(
            "/api/v1/wb/agent/mutation/:id/review",
            post(review_mutation),
        )
        .route(
            "/api/v1/wb/agent/mutation/:id/offline-benchmark",
            post(offline_benchmark_mutation),
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
        .route("/api/v1/wb/auth/rotate-key", post(auth_rotate_key))
        .route("/api/v1/wb/auth/me", get(auth_me))
        // ── V0.9 用户管理（仅 Developer） ──
        .route("/api/v1/wb/users", get(list_users).post(create_user))
        .route(
            "/api/v1/wb/users/:id/password",
            axum::routing::patch(update_user_password),
        )
        .route(
            "/api/v1/wb/users/:id/disabled",
            axum::routing::patch(set_user_disabled),
        )
        .route(
            "/api/v1/wb/users/:id/role",
            axum::routing::patch(set_user_role),
        )
        // ── V0.8 GraphRAG 基础（对象-关系图） ──
        .route("/api/v1/wb/graph", get(list_graph))
        // 通用插件能力 API：传输层不绑定具体业务插件。
        .route("/api/v1/plugins", get(list_plugins))
        .route(
            "/api/v1/plugins/:plugin_id/manifest",
            get(get_plugin_manifest),
        )
        .route(
            "/api/v1/plugins/:plugin_id/capabilities/:capability/invoke",
            post(invoke_plugin_capability),
        )
        // ── Phase 2.5 自动运行引擎 ──
        .route("/api/v1/automation/jobs", get(list_jobs))
        .route("/api/v1/automation/jobs/:id", get(get_job))
        .route("/api/v1/automation/jobs/:id/cancel", post(cancel_job))
        .route(
            "/api/v1/domain-packs/:id/automation-status",
            get(get_pack_status),
        )
        .route("/api/v1/domain-packs/:id/run", post(run_pack))
        .route(
            "/api/v1/domain-packs/:id/operations/:operation/run",
            post(run_operation),
        )
        .route("/api/v1/domain-packs/:id/scan", post(scan_pack))
        // Phase 7：Facade HTTP Transport（单一语义源：与 CLI/Plugin 同一 OpenAipFacade）
        // ── 知识本体：DL 推理 + 不可变版本与受控演化 ──
        .route("/api/v1/knowledge/ontology/current", get(current_knowledge))
        .route(
            "/api/v1/knowledge/ontology/versions",
            get(list_knowledge_versions),
        )
        .route(
            "/api/v1/knowledge/ontology/init",
            post(initialize_knowledge),
        )
        .route("/api/v1/knowledge/reason", post(reason_current_knowledge))
        .route(
            "/api/v1/knowledge/diagnosis",
            get(diagnose_current_knowledge),
        )
        .route(
            "/api/v1/knowledge/proposals",
            get(list_knowledge_proposals).post(create_knowledge_proposal),
        )
        .route(
            "/api/v1/knowledge/proposals/:id/review",
            post(review_knowledge_proposal),
        )
        .route(
            "/api/v1/knowledge/proposals/:id/apply",
            post(apply_knowledge_proposal),
        )
        .route("/api/v1/knowledge/rollback", post(rollback_knowledge))
        .route("/api/v1/facade/world/query", post(facade_world_query))
        .route("/api/v1/facade/audit/query", post(facade_audit_query))
        .route("/api/v1/facade/fabric/submit", post(facade_fabric_submit))
        .route(
            "/api/v1/facade/fabric/:id/cancel",
            post(facade_fabric_cancel),
        )
        .route("/api/v1/facade/project/create", post(facade_project_create))
        .route("/api/v1/facade/project/list", post(facade_project_list))
        .route("/api/v1/facade/project/seed", post(facade_project_seed))
        .route("/api/v1/facade/project/event", post(facade_project_event))
        .route("/api/v1/facade/project/deps", post(facade_project_deps))
        // ── Business Loop v1：状态 → 机会 → 决策 → 结果 → 状态变化 ──
        .route("/api/v1/business/goals", get(list_goal_gaps))
        .route("/api/v1/business/recommendations", get(recommendations))
        .route(
            "/api/v1/business/opportunity-paths",
            get(business_loop::opportunity_paths),
        )
        .route(
            "/api/v1/business/portfolio",
            get(business_loop::business_portfolio),
        )
        .route("/api/v1/business/discover-opportunities", post(discover))
        .route("/api/v1/business/run-next", post(run_next))
        .route(
            "/api/v1/business/opportunities/:id/decide",
            post(business_loop::decide),
        )
        .route(
            "/api/v1/business/opportunities/:id/execute",
            post(business_loop::execute),
        )
        .route(
            "/api/v1/business/decisions/:id/approval",
            post(business_loop::approve_decision),
        )
        .route(
            "/api/v1/business/follow-ups/:id/start",
            post(business_loop::start_follow_up_object),
        )
        .route(
            "/api/v1/business/follow-ups/:id/result",
            post(business_loop::settle_follow_up_object),
        )
        // ── Phase 2.3 业务包管理 ──
        .route("/api/v1/domain-packs", get(domain_packs::list_domain_packs))
        .route(
            "/api/v1/domain-packs/:id/install",
            post(domain_packs::install_domain_pack),
        )
        .route(
            "/api/v1/domain-packs/:id/runtime",
            get(domain_packs::get_domain_pack_runtime)
                .put(domain_packs::update_domain_pack_runtime),
        )
        .route(
            "/api/v1/domain-packs/:id/enable",
            post(domain_packs::enable_domain_pack),
        )
        .route(
            "/api/v1/domain-packs/:id/disable",
            post(domain_packs::disable_domain_pack),
        )
        .route(
            "/api/v1/domain-packs/:id/operations",
            get(domain_packs::get_domain_pack_operations),
        )
        .route(
            "/api/v1/domain-packs/:id/strategies",
            get(domain_packs::get_domain_pack_strategies),
        )
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
/// false → `require_approval` 动作挂起，由浏览器经 HTTP 审批端点显式裁决后恢复。
/// `enforce_auth`：是否强制认证（V0.7）；生产传 `true`，集成测试传 `false`（注入合成 Developer）。
#[allow(clippy::too_many_arguments)]
pub async fn serve(
    storage: Arc<LibSQLStorage>,
    services: Vec<Service>,
    effect_defs: Vec<crate::compose::defs::RuntimeDefinition>,
    engine: Arc<dyn crate::cognitive::CognitiveEngine>,
    listener: TcpListener,
    static_dir: impl Into<String>,
    auto_approve: bool,
    auto_approve_evolution: bool,
    enforce_auth: bool,
) -> std::io::Result<()> {
    serve_with_timeout(
        storage,
        services,
        effect_defs,
        engine,
        listener,
        static_dir,
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
    storage: Arc<LibSQLStorage>,
    services: Vec<Service>,
    effect_defs: Vec<crate::compose::defs::RuntimeDefinition>,
    engine: Arc<dyn crate::cognitive::CognitiveEngine>,
    listener: TcpListener,
    static_dir: impl Into<String>,
    auto_approve: bool,
    auto_approve_evolution: bool,
    enforce_auth: bool,
    approval_timeout: Option<Duration>,
) -> std::io::Result<()> {
    serve_with_facade(
        storage,
        services,
        effect_defs,
        engine,
        listener,
        static_dir,
        auto_approve,
        auto_approve_evolution,
        enforce_auth,
        approval_timeout,
        None,
    )
    .await
}

/// 带 Facade 注入的服务启动（T1 测试用：注入自定义 facade 装配）；None = 默认装配。
#[allow(clippy::too_many_arguments)]
async fn configured_lottery_source_roots(
    storage: &LibSQLStorage,
    domain_packs_dir: &std::path::Path,
) -> Result<Vec<PathBuf>, crate::storage::StorageError> {
    let mut roots = std::env::var_os("ONTOLOGY_LOTTERY_SOURCE_ROOT")
        .map(|value| std::env::split_paths(&value).collect::<Vec<_>>())
        .unwrap_or_default();
    roots.push(domain_packs_dir.to_path_buf());

    for object in storage.list_all_objects().await? {
        if !object.id.starts_with("domain-pack-runtime:") {
            continue;
        }
        let Some(source_path) = object
            .data
            .get("source_path")
            .and_then(|value| value.as_str())
        else {
            continue;
        };
        let Some(parent) = std::path::Path::new(source_path).parent() else {
            continue;
        };
        roots.push(parent.to_path_buf());
    }

    Ok(roots)
}

#[allow(clippy::too_many_arguments)]
pub async fn serve_with_facade(
    storage: Arc<LibSQLStorage>,
    services: Vec<Service>,
    effect_defs: Vec<crate::compose::defs::RuntimeDefinition>,
    engine: Arc<dyn crate::cognitive::CognitiveEngine>,
    listener: TcpListener,
    static_dir: impl Into<String>,
    auto_approve: bool,
    auto_approve_evolution: bool,
    enforce_auth: bool,
    approval_timeout: Option<Duration>,
    facade_override: Option<std::sync::Arc<crate::facade::Facade>>,
) -> std::io::Result<()> {
    // 认证/会话提供方复用同一存储句柄（LibSQLStorage 内部 Arc，单连接共享库）。
    let idp: Arc<dyn IdentityProvider> =
        Arc::new(LocalIdentityProvider::new(storage.as_ref().clone()));
    let sessions = SessionStore::new(storage.as_ref().clone());
    // storage 随后被 move 进 AppState——Facade 先行 clone（同一底层连接）。
    let storage_f = storage.clone();
    let handle = tokio::runtime::Handle::current();
    // Facade::new 会触发 agent_connected 事件（同步 sink block_on）→ 阻塞上下文
    // 装配（multi_thread runtime 下 block_in_place 合法；current_thread 测试环境
    // 用空 Facade——world/audit 查询不受影响，fabric 路由 fail-closed）。
    let facade = match facade_override {
        Some(f) => f,
        None => match handle.runtime_flavor() {
            tokio::runtime::RuntimeFlavor::MultiThread => {
                tokio::task::block_in_place(|| build_facade(&storage_f, handle))
            }
            _ => std::sync::Arc::new(
                crate::facade::Facade::new(
                    storage_f,
                    handle,
                    crate::fabric::router::FabricRouter::new(),
                    vec![],
                    &[],
                )
                .expect("Facade 基础装配（空 router/adapters 恒成功）"),
            ),
        },
    };
    let domain_packs_dir = std::env::var_os("ONTOLOGY_DOMAIN_PACKS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("domain-packs"));
    let lottery_source_roots = configured_lottery_source_roots(&storage, &domain_packs_dir)
        .await
        .map_err(|error| std::io::Error::other(format!("彩票插件数据源目录装配失败：{error}")))?;
    let plugin_catalog = std::sync::Arc::new(
        crate::plugin::catalog::PluginCatalog::with_builtins(
            storage.as_ref().clone(),
            lottery_source_roots,
        )
        .map_err(|error| std::io::Error::other(format!("插件目录装配失败：{error}")))?,
    );
    let v2_state = v2::V2State::with_builtins()
        .map_err(|error| std::io::Error::other(format!("内置 Skill 装配失败：{error}")))?;
    let automation = Arc::new(crate::automation::AutomationEngine::new(
        storage.clone(),
        plugin_catalog.clone(),
        domain_packs_dir.clone(),
        facade.event_tx(),
    ));
    // Approved ontology evolution is durable: the persisted runtime IR supersedes
    // process-start definitions on every boot.
    let (services, effect_defs) = match storage.current_knowledge_version().await {
        Ok(Some(version)) => match version.runtime_ir {
            Some(runtime_ir) => (
                crate::compose::defs::ir_to_services(&runtime_ir),
                crate::compose::defs::ir_to_runtime(&runtime_ir),
            ),
            None => (services, effect_defs),
        },
        Ok(None) => (services, effect_defs),
        Err(error) => {
            return Err(std::io::Error::other(format!(
                "知识本体启动恢复失败：{error}"
            )));
        }
    };
    let state = Arc::new(AppState {
        v2: Arc::new(v2_state),
        storage,
        domain_packs_dir,
        plugin_catalog,
        automation,
        v2_services: Arc::new(tokio::sync::RwLock::new(services)),
        v2_effect_defs: Arc::new(tokio::sync::RwLock::new(effect_defs)),
        v2_engine: engine,
        auto_approve,
        auto_approve_evolution,
        approval_timeout,
        pending: Arc::new(Mutex::new(HashMap::new())),
        // C1：复用 Facade 自建的广播频道（Bridge sink 已 with_broadcast），
        // 保证 WS /stream 收到 fabric.* 事件，而非另建一个收不全的频道。
        event_tx: facade.event_tx(),
        runs: Mutex::new(HashMap::new()),
        static_dir: static_dir.into(),
        idp,
        sessions,
        enforce_auth,
        login_fails: Mutex::new(HashMap::new()),
        facade,
        rate_limiter: Arc::new(RateLimiter::new()),
    });
    let app = router()
        .layer(middleware::from_fn(auth_layer))
        .layer(middleware::from_fn(security_headers))
        .layer(Extension(state.clone()))
        .with_state(state.clone());
    // 重启恢复：重建 runs 内存映射；崩溃残留 Running→Interrupted、orphan Pending→Expired。
    recover(&state).await;
    recover_fabric_pending(&state).await;
    // V10.1：预置 Worker（echo / git / codex / claude 壳）。
    bootstrap_v2_workers(&state);
    state
        .automation
        .start()
        .await
        .map_err(|error| std::io::Error::other(error.to_string()))?;

    // S2：优雅关闭——收到 SIGINT / SIGTERM 后先回收孤儿后台 job 再退出，
    // 避免 spawn_blocking 后台进程泄漏为孤儿（facade.shutdown 透传 bridge）。
    // 先停止接收新连接并等待在途 HTTP 请求排空，再关闭 Fabric 适配器。
    // 这样 SIGTERM 不会在请求仍处于事务边界内时抢先杀掉执行器；已落库的
    // pending/running 状态由下一次启动的 recover 路径继续处理。
    let server_result = axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await;
    state.automation.shutdown().await;
    state.facade.shutdown();
    server_result
}

/// S2：优雅关闭信号——等待 SIGINT（Ctrl-C）或 Unix SIGTERM，触发后由调用方回收
/// 孤儿后台 job 再退出。`tokio::signal` 需 signal feature（已在 Cargo.toml 启用）。
async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        use tokio::signal::unix::{SignalKind, signal};
        if let Ok(mut sig) = signal(SignalKind::terminate()) {
            let _ = sig.recv().await;
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}

/// 预置 Worker（V10.1）：echo / git / codex / claude 壳注册进 V2 Worker Registry。
/// Codex/Claude 命令可经环境变量覆盖（OPENAI_OS_CODEX_CMD / OPENAI_OS_CLAUDE_CMD）；
/// 未安装时注册仍成功（能力声明可见），执行阶段才失败。
/// Phase 7：装配 Facade（Server Transport 的语义后端）。
/// 意图映射固定；适配器动态发现（connect 成功的 Agent 把 discover 能力注册进
/// Router——Codex/DSH 是 Capability Provider）；领域规则：fabric.task_completed
/// → fabric.status（提交即世界，UI World 视图可见）。
fn build_facade(
    storage: &std::sync::Arc<crate::storage::LibSQLStorage>,
    handle: tokio::runtime::Handle,
) -> std::sync::Arc<crate::facade::Facade> {
    use crate::fabric::adapter::AgentAdapter;
    use crate::fabric::adapters::{codex_adapter, dsh_adapter, shell_adapter};
    use crate::fabric::router::FabricRouter;

    let mut router = FabricRouter::new();
    router.register_intent("modify_code", "code.write");
    router.register_intent("agent_task", "agent.task");
    router.register_intent("run_shell", "shell.execute");
    router.register_intent("run_tests", "code.test");
    router.mark_high_risk("shell.execute");

    let mut adapters: Vec<std::sync::Arc<dyn AgentAdapter>> = Vec::new();
    let dsh_command = std::env::var("OAIP_DSH_COMMAND").ok();
    for a in [
        codex_adapter(None),
        dsh_adapter(dsh_command.as_deref()),
        shell_adapter(None),
    ] {
        if a.connect().is_ok() {
            let id = a.id().to_string();
            for c in a.discover() {
                router.register_agent(c.name, id.clone(), 1);
            }
            adapters.push(std::sync::Arc::new(a));
        } else {
            eprintln!("[facade] 适配器离线（命令不可用）：{}", a.id());
        }
    }
    std::sync::Arc::new(
        crate::facade::Facade::new(
            storage.clone(),
            handle,
            router,
            adapters,
            &[("fabric.task_completed", "fabric.status")],
        )
        .expect("Facade 装配失败（适配器已预过滤，不应发生）"),
    )
}

fn bootstrap_v2_workers(state: &Arc<AppState>) {
    use crate::worker::adapters::{claude_worker, codex_worker};
    use crate::worker::process::ProcessWorker;
    use crate::worker_core::manifest::{ExecutionProtocol, WorkerManifest};

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
    // DSH（DeepSeek Harness）Worker（Phase 11D：Harness 是 Worker，不是内核）
    let dsh_cmd = std::env::var("OPENAI_OS_DSH_CMD").unwrap_or_else(|_| "dsh".into());
    let _ = reg.register(Box::new(crate::worker::adapters::dsh_worker(Some(
        &dsh_cmd,
    ))));
    // Pi Agent Worker（Worker 形态，放开工具；Thinker 形态在 oaip agent run 的 PiAgentEngine）
    let pi_cmd = std::env::var("OPENAI_OS_PI_CMD").unwrap_or_else(|_| "pi".into());
    let _ = reg.register(Box::new(crate::worker::adapters::pi_worker(Some(&pi_cmd))));
}

/// 重启恢复（V0.7 持久化）：重建 runs 内存映射；崩溃残留 Running→Interrupted、orphan Pending→Expired
/// （简化分支：无"活 run"可恢复，故不做"run 存活→Pending 保留"死分支）。
async fn recover(state: &Arc<AppState>) {
    let runs = match state.storage.recover_persisted_state().await {
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

/// 恢复尚未终结的 fabric 审批：Event Log 是唯一持久化事实源，会话与审批表只是运行时投影。
async fn recover_fabric_pending(state: &Arc<AppState>) {
    // list_events 单页最多 1000 条；用 after_id 分页，避免事件量增长后旧挂起审批
    // 被启动恢复的窗口截断。
    let mut events = Vec::new();
    let mut after_id = None;
    loop {
        let page = match state
            .storage
            .list_events(crate::storage::TraceQuery {
                after_id,
                limit: 1000,
                newest_first: false,
                ..Default::default()
            })
            .await
        {
            Ok(page) => page,
            Err(err) => {
                eprintln!("[server] fabric 审批恢复读取事件失败: {err}");
                return;
            }
        };
        if page.is_empty() {
            break;
        }
        after_id = page.last().map(|event| event.id.clone());
        let complete_page = page.len() == 1000;
        events.extend(page);
        if !complete_page {
            break;
        }
    }

    let mut requests = HashMap::new();
    let mut awaiting = std::collections::HashSet::new();
    let mut terminal = std::collections::HashSet::new();
    for event in events {
        let Some(input) = event.input else {
            continue;
        };
        match event.event_type.as_str() {
            "fabric.task_requested" => {
                if let Some(request) = input.get("request").and_then(|v| {
                    serde_json::from_value::<crate::fabric::AgentMessage>(v.clone()).ok()
                }) && let Some(run_id) = request.correlation_id.clone()
                {
                    requests.insert(run_id, request);
                }
            }
            "fabric.approval_requested" => {
                if let Some(run_id) = input.get("run_id").and_then(|v| v.as_str()) {
                    awaiting.insert(run_id.to_string());
                }
            }
            "fabric.approval_rejected" => {
                if let Some(run_id) = input.get("run_id").and_then(|v| v.as_str()) {
                    terminal.insert(run_id.to_string());
                }
            }
            "fabric.task_completed" | "fabric.task_failed" | "fabric.task_cancelled" => {
                if let Some(run_id) = input.get("correlation_id").and_then(|v| v.as_str()) {
                    terminal.insert(run_id.to_string());
                }
            }
            _ => {}
        }
    }

    for run_id in awaiting.difference(&terminal) {
        let Some(request) = requests.get(run_id) else {
            eprintln!("[server] fabric 审批缺少可恢复 task.request: {run_id}");
            continue;
        };
        if let Err(err) = state.facade.restore_fabric_request(request.clone()) {
            eprintln!("[server] fabric 会话恢复失败 {run_id}: {err}");
            continue;
        }
        if let Some((key, pending)) = PendingApproval::from_fabric_request(request) {
            lock_mutex(&state.pending).insert(key, pending);
        }
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
    } else {
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
            }
            _ => return unauthorized("无效或已过期的令牌"),
        }
    }

    // WS 在 handler 中取得连接租约（需要在 upgrade 成功后持有 guard）；
    // HTTP 执行/审批请求在认证后、进入业务 handler 前限流。
    if let Some(channel) = rate_channel(&path, req.method()) {
        let Some(user) = req.extensions().get::<AuthUser>() else {
            return rate_limited(channel);
        };
        if !st.rate_limiter.allow(channel, &user.id) {
            return rate_limited(channel);
        }
    }
    next.run(req).await
}

fn rate_channel(path: &str, method: &axum::http::Method) -> Option<Channel> {
    if *method == axum::http::Method::POST
        && (path == "/api/v1/wb/agent/run"
            || path == "/api/v1/facade/fabric/submit"
            || path.ends_with("/cancel")
            || (path.starts_with("/api/v1/plugins/") && path.ends_with("/invoke")))
    {
        return Some(Channel::Execution);
    }
    if (path.ends_with("/approvals") && *method == axum::http::Method::GET)
        || (path.contains("/approval/") && *method == axum::http::Method::POST)
    {
        return Some(Channel::Approval);
    }
    None
}

fn rate_limited(channel: Channel) -> Response {
    let msg = match channel {
        Channel::Execution => "执行请求过于频繁，请稍后重试",
        Channel::Approval => "审批请求过于频繁，请稍后重试",
        Channel::WebSocket => "WebSocket 请求过于频繁，请稍后重试",
    };
    (
        StatusCode::TOO_MANY_REQUESTS,
        ApiResponse::<()>::error(429, msg),
    )
        .into_response()
}

/// 公共白名单：静态资源（非 `/api/` 前缀）与登录端点免鉴权。
fn is_public(path: &str) -> bool {
    matches!(
        path,
        "/healthz" | "/readyz" | "/metrics" | "/api/v1/wb/auth/login"
    ) || !path.starts_with("/api/")
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
