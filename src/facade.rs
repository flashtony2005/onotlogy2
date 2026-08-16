//! OpenAIP Public Semantic Facade（Phase 7）—— 世界/审计/fabric 三语义域公共 API。
//!
//! 契约：docs/03_架构设计/facade-v1.md
//!
//! 核心边界：**Kernel 是内部世界；Facade 是语义 API；Plugin / Adapter 是外部连接层**。
//! - 内部不暴露原则：kernel/event_log/storage/router/worker 都是内部实现，
//!   对外只有 world.query / audit.query / fabric.submit。
//! - 单一语义源：CLI / Server / Plugin 全部最终调用 `OpenAipFacade`。
//! - 双向模型：DSH Plugin = OpenAIP 入口（本 Facade 的消费者）；
//!   dsh_adapter = OpenAIP 出口（fabric 的一等 Agent）。
//!
//! 执行约定：Facade 是同步 API，内部以 `Handle::block_on` 驱动 storage——
//! 阻塞上下文（server=spawn_blocking、CLI=block_in_place、插件=宿主线程），
//! 与 StorageWorldSink / StorageAuditSink 同约定。
//!
//! WorldService 采用 per-query 全量重建投影（P5.4 策略，正确性优先）；
//! P3（跨 run 常驻 Kernel + watermark）属后续里程碑。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::compose::task::WorldEventSink;
use crate::fabric::adapter::AgentAdapter;
use crate::fabric::router::FabricRouter;
use crate::fabric::session::SessionManager;
use crate::fabric::{AgentMessage, Bridge};
use crate::kernel::{Kernel, KernelError, Scope};
use crate::projection::{Projection, ProjectionError};
use crate::storage::{LibSQLStorage, TraceQuery};
use crate::trace::TraceEvent;

/// Facade 错误（三个语义域各自包装；内部错误不泄漏实现细节）。
#[derive(Debug, thiserror::Error)]
pub enum FacadeError {
    #[error("世界查询失败: {0}")]
    World(String),
    #[error("审计查询失败: {0}")]
    Audit(String),
    #[error("Fabric 提交失败: {0}")]
    Fabric(String),
    #[error("Facade 初始化失败: {0}")]
    Init(String),
}

impl From<KernelError> for FacadeError {
    fn from(e: KernelError) -> Self {
        FacadeError::World(e.to_string())
    }
}

impl From<ProjectionError> for FacadeError {
    fn from(e: ProjectionError) -> Self {
        FacadeError::World(e.to_string())
    }
}

// ── 语义域类型 ────────────────────────────────────────────────────────

/// world.query 请求（按实体/谓词过滤；默认全量）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorldQuery {
    pub object_ref: Option<String>,
    pub predicate: Option<String>,
    pub limit: usize,
}

/// 世界关系（Kernel 当前有效投影的只读视图）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorldRelation {
    pub source: String,
    pub predicate: String,
    pub target: String,
    pub valid_from: i64,
    pub valid_until: Option<i64>,
}

/// world.query 结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorldView {
    pub relations: Vec<WorldRelation>,
    pub total: usize,
    /// 视图时刻（Kernel 秒轴）。
    pub at: i64,
}

/// audit.query 请求（event_type 前缀匹配："approval." 匹配全部 approval.*）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AuditQuery {
    pub run_id: Option<String>,
    pub agent_id: Option<String>,
    pub event_type: Option<String>,
    pub limit: usize,
}

/// 审计事件（Event Log 的审计语义域）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    pub id: String,
    pub event_type: String,
    pub refs: Vec<String>,
    pub payload: Value,
    pub timestamp: String,
}

/// audit.query 结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditView {
    pub events: Vec<AuditEvent>,
    pub total: usize,
}

/// fabric.submit 请求（session 不存在自动创建，participants=[sender]）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FabricRequest {
    pub session_id: String,
    pub sender: String,
    pub intent: String,
    pub payload: Value,
}

/// fabric.submit 结果（task.result；事件已入 Event Log，可 audit.query 复核）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FabricSubmission {
    pub result: AgentMessage,
}

// ── Public Semantic API ───────────────────────────────────────────────

/// OpenAIP Public Semantic Facade（唯一语义入口；Transport 不持有语义）。
pub trait OpenAipFacade {
    fn world_query(&self, request: WorldQuery) -> Result<WorldView, FacadeError>;
    fn audit_query(&self, request: AuditQuery) -> Result<AuditView, FacadeError>;
    fn fabric_submit(&self, request: FabricRequest) -> Result<FabricSubmission, FacadeError>;
}

/// Facade 聚合实现（World + Audit + Fabric 三个内部服务）。
pub struct Facade {
    world: WorldService,
    audit: AuditService,
    fabric: FabricService,
}

impl Facade {
    /// 构造 Facade。`fabric_router` 与 `adapters` 装配 FabricService；
    /// `domain_rules` 注册世界投影的领域规则（如 fabric.task_completed → fabric.status）。
    pub fn new(
        storage: Arc<LibSQLStorage>,
        handle: tokio::runtime::Handle,
        fabric_router: FabricRouter,
        adapters: Vec<Arc<dyn AgentAdapter>>,
        domain_rules: &[(&str, &str)],
    ) -> Result<Self, FacadeError> {
        let sink: Option<Arc<dyn WorldEventSink>> = Some(Arc::new(
            crate::cognitive::runner::StorageWorldSink::new(storage.clone(), handle.clone()),
        ));
        let mut bridge = Bridge::new(fabric_router, sink);
        for a in adapters {
            bridge
                .connect_agent(a)
                .map_err(|e| FacadeError::Init(e.to_string()))?;
        }
        Ok(Self {
            world: WorldService::new(storage.clone(), handle.clone(), domain_rules),
            audit: AuditService::new(storage, handle),
            fabric: FabricService::new(bridge),
        })
    }
}

impl OpenAipFacade for Facade {
    fn world_query(&self, request: WorldQuery) -> Result<WorldView, FacadeError> {
        self.world.query(request)
    }

    fn audit_query(&self, request: AuditQuery) -> Result<AuditView, FacadeError> {
        self.audit.query(request)
    }

    fn fabric_submit(&self, request: FabricRequest) -> Result<FabricSubmission, FacadeError> {
        self.fabric.submit(request)
    }
}

// ── WorldService ──────────────────────────────────────────────────────

/// 世界视图服务：per-query 全量重建投影（P5.4 策略）→ 只读查询。
/// 每次查询从 Event Log 重建 Kernel——世界是事件的纯函数（P1），
/// 任何来源（fabric 提交/命令/插件）写入的事件都会出现在下一次视图里。
pub struct WorldService {
    storage: Arc<LibSQLStorage>,
    handle: tokio::runtime::Handle,
    domain_rules: HashMap<String, String>,
}

impl WorldService {
    pub fn new(
        storage: Arc<LibSQLStorage>,
        handle: tokio::runtime::Handle,
        domain_rules: &[(&str, &str)],
    ) -> Self {
        Self {
            storage,
            handle,
            domain_rules: domain_rules
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        }
    }

    pub fn query(&self, req: WorldQuery) -> Result<WorldView, FacadeError> {
        let mut kernel = Kernel::new();
        let ctx = kernel.create_context(None, Scope::World, "world")?;
        let mut p = Projection::new(ctx);
        for (ty, pred) in &self.domain_rules {
            p = p.with_domain_rule(ty.clone(), pred.clone());
        }
        self.handle
            .block_on(p.project_from_storage(&mut kernel, &self.storage))?;
        let now = kernel.now();
        let limit = if req.limit == 0 {
            usize::MAX
        } else {
            req.limit
        };
        let relations: Vec<WorldRelation> = kernel
            .relations()
            .filter(|r| r.context == ctx && r.is_current(now))
            .filter(|r| {
                req.object_ref
                    .as_ref()
                    .map(|o| r.source == *o || r.target == *o)
                    .unwrap_or(true)
            })
            .filter(|r| {
                req.predicate
                    .as_ref()
                    .map(|p| r.predicate == *p)
                    .unwrap_or(true)
            })
            .map(|r| WorldRelation {
                source: r.source.clone(),
                predicate: r.predicate.clone(),
                target: r.target.clone(),
                valid_from: r.valid_from,
                valid_until: r.valid_until,
            })
            .take(limit)
            .collect();
        let total = relations.len();
        Ok(WorldView {
            relations,
            total,
            at: now,
        })
    }
}

// ── AuditService ──────────────────────────────────────────────────────

/// 审计语义域（Event Log 的前缀集合：审批/演化闸门/通信黑匣子/策略拒绝）。
pub const AUDIT_PREFIXES: &[&str] = &[
    "approval.",
    "evolution.gate_approved",
    "fabric.",
    "policy.denied",
];

/// 审计服务：直读 Event Log 的审计语义域（黑匣子单一扇出源）。
pub struct AuditService {
    storage: Arc<LibSQLStorage>,
    handle: tokio::runtime::Handle,
}

impl AuditService {
    pub fn new(storage: Arc<LibSQLStorage>, handle: tokio::runtime::Handle) -> Self {
        Self { storage, handle }
    }

    pub fn query(&self, req: AuditQuery) -> Result<AuditView, FacadeError> {
        let limit = if req.limit == 0 { 500 } else { req.limit };
        let events: Vec<TraceEvent> = self
            .handle
            .block_on(self.storage.list_events(TraceQuery {
                run_id: req.run_id.clone(),
                agent_id: req.agent_id.clone(),
                limit,
                ..Default::default()
            }))
            .map_err(|e| FacadeError::Audit(e.to_string()))?;
        let filtered: Vec<AuditEvent> = events
            .into_iter()
            .filter(|e| {
                let in_domain = AUDIT_PREFIXES.iter().any(|p| e.event_type.starts_with(p));
                let by_type = req
                    .event_type
                    .as_ref()
                    .map(|t| e.event_type.starts_with(t))
                    .unwrap_or(true);
                in_domain && by_type
            })
            .map(|e| AuditEvent {
                id: e.id,
                event_type: e.event_type,
                refs: e.object_refs,
                payload: e.input.unwrap_or(Value::Null),
                timestamp: e.timestamp,
            })
            .collect();
        let total = filtered.len();
        Ok(AuditView {
            events: filtered,
            total,
        })
    }
}

// ── FabricService ─────────────────────────────────────────────────────

/// Fabric 提交服务：Bridge.submit（Router → Adapter → 事件落库）+ Session 管理。
/// `submit` 全 &self：session 经 Mutex；Bridge 提交只读自身。
pub struct FabricService {
    bridge: Bridge,
    sessions: Mutex<SessionManager>,
}

impl FabricService {
    pub fn new(bridge: Bridge) -> Self {
        Self {
            bridge,
            sessions: Mutex::new(SessionManager::new()),
        }
    }

    pub fn submit(&self, req: FabricRequest) -> Result<FabricSubmission, FacadeError> {
        let mut sessions = self
            .sessions
            .lock()
            .map_err(|_| FacadeError::Fabric("会话表锁中毒".into()))?;
        if sessions.get(&req.session_id).is_none() {
            // 契约：session 不存在自动创建（participants=[sender]；id=请求方 session_id）
            sessions.create_with_id(req.session_id.clone(), vec![req.sender.clone()]);
        }
        let result = self
            .bridge
            .submit(
                &mut sessions,
                &req.session_id,
                &req.sender,
                &req.intent,
                req.payload,
            )
            .map_err(|e| FacadeError::Fabric(e.to_string()))?;
        Ok(FabricSubmission { result })
    }
}
