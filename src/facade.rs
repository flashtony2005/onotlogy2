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

use crate::abi::ir::OntologyIR;
use crate::compose::task::WorldEventSink;
use crate::fabric::adapter::AgentAdapter;
use crate::fabric::router::FabricRouter;
use crate::fabric::session::{Session, SessionManager};
use crate::fabric::{AgentMessage, Bridge, MessageKind};
use crate::kernel::{Kernel, KernelError, Scope};
use crate::knowledge::{
    Ontology as KnowledgeOntology, OntologyProposal as KnowledgeOntologyProposal,
    Reasoner as KnowledgeReasoner,
};
use crate::knowledge_bridge::storage_world_snapshot_to_knowledge;
use crate::projection::{Projection, ProjectionError};
use crate::storage::{LibSQLStorage, OntologyEvolutionRecord, OntologyVersionRecord, TraceQuery};
use crate::trace::TraceEvent;
use crate::worker::process::ExecutionContext;

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
    #[error("知识推理失败: {0}")]
    Knowledge(String),
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
impl From<crate::knowledge_bridge::KnowledgeBridgeError> for FacadeError {
    fn from(error: crate::knowledge_bridge::KnowledgeBridgeError) -> Self {
        FacadeError::Knowledge(error.to_string())
    }
}
impl From<crate::storage::StorageError> for FacadeError {
    fn from(error: crate::storage::StorageError) -> Self {
        FacadeError::Knowledge(error.to_string())
    }
}

// ── 语义域类型 ────────────────────────────────────────────────────────

/// world.query 请求（按实体/谓词过滤；默认全量）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorldQuery {
    /// Phase 8：项目范围（None = 全局世界；Some = 该项目世界）。
    pub project_id: Option<String>,
    pub object_ref: Option<String>,
    pub predicate: Option<String>,
    #[serde(default)]
    pub limit: usize,
}

/// 世界关系（Kernel 当前有效投影的只读视图）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorldRelation {
    pub source: String,
    pub predicate: String,
    pub target: String,
    pub valid_from: i64,
    pub valid_until: Option<i64>,
}

/// world.query 结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorldView {
    pub relations: Vec<WorldRelation>,
    pub total: usize,
    /// 视图时刻（Kernel 秒轴）。
    pub at: i64,
}

/// audit.query 请求（event_type 前缀匹配："approval." 匹配全部 approval.*）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditQuery {
    /// Phase 8：项目范围（None = 全部；审计按项目过滤）。
    pub project_id: Option<String>,
    pub run_id: Option<String>,
    pub agent_id: Option<String>,
    pub event_type: Option<String>,
    #[serde(default)]
    pub limit: usize,
}

/// 审计事件（Event Log 的审计语义域）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditEvent {
    pub id: String,
    pub event_type: String,
    pub refs: Vec<String>,
    pub payload: Value,
    pub timestamp: String,
}

/// audit.query 结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditView {
    pub events: Vec<AuditEvent>,
    pub total: usize,
}

/// fabric.submit 请求（session 不存在自动创建，participants=[sender]）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FabricRequest {
    pub session_id: String,
    pub sender: String,
    pub intent: String,
    /// 任务参数；执行上下文不再混入此对象。
    pub payload: Value,
    /// 正式执行上下文。agent_task / modify_code 等文件型能力必须提供 isolated workspace。
    #[serde(default)]
    pub execution_context: Option<ExecutionContext>,
    /// Phase 8：项目归属（None = 全局；任务事件进项目世界）。
    #[serde(default)]
    pub project_id: Option<String>,
}

/// fabric.submit 结果（task.result；事件已入 Event Log，可 audit.query 复核）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FabricSubmission {
    pub result: AgentMessage,
}

/// project.create 请求（项目 = 世界/事件/运营的顶层组织单元）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectCreate {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// 项目本体定义（ODL 文本；决定项目世界语义）。
    #[serde(default)]
    pub ontology: String,
}

/// 项目视图。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectView {
    pub id: String,
    pub name: String,
    pub description: String,
    pub ontology: String,
    pub status: String,
    pub created_at: String,
}

// ── Public Semantic API ───────────────────────────────────────────────

/// OpenAIP Public Semantic Facade（唯一语义入口；Transport 不持有语义）。
pub trait OpenAipFacade {
    fn world_query(&self, request: WorldQuery) -> Result<WorldView, FacadeError>;
    fn audit_query(&self, request: AuditQuery) -> Result<AuditView, FacadeError>;
    fn fabric_submit(&self, request: FabricRequest) -> Result<FabricSubmission, FacadeError>;
    /// Phase 8：创建项目（项目世界的注册）。
    fn project_create(&self, request: ProjectCreate) -> Result<ProjectView, FacadeError>;
    /// Phase 8：列出项目。
    fn project_list(&self) -> Result<Vec<ProjectView>, FacadeError>;
}

/// Facade 聚合实现（World + Audit + Fabric 三个内部服务）。
pub struct Facade {
    world: WorldService,
    audit: AuditService,
    fabric: FabricService,
    storage: Arc<LibSQLStorage>,
    /// 事件实时广播频道（T2b-1/C1）：Bridge 的 StorageWorldSink 落库后扇出到此，
    /// WS `/stream` 据此实时推送 fabric.* 等事件。Facade 自建频道并暴露 getter，
    /// 调用方（server AppState）复用同一频道，避免 Bridge sink 与认知 Runner sink
    /// 各建各的、WS 收不全。
    event_tx: tokio::sync::broadcast::Sender<TraceEvent>,
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
        // C1：Facade 自建事件广播频道，Bridge 的 StorageWorldSink 落库后扇出到此——
        // 否则 fabric.* 事件只落库不推 WS（T2b-1 只接上了认知 Runner 路径）。
        // S4（broadcast lag 声明）：broadcast 缓冲容量 1024，故 WS 实时推送是
        // best-effort——慢客户端或瞬时生产风暴可能丢弃缓冲外的最近事件（仅实时缺口）。
        // 但 WS 断线重连走 after_id 游标从存储（权威）补齐，重连不丢数据；实时缺口
        // 不影响最终一致性。服务重启后新订阅者同样从历史回放起步。
        let (event_tx, _rx) = tokio::sync::broadcast::channel(1024);
        let sink: Option<Arc<dyn WorldEventSink>> = Some(Arc::new(
            crate::cognitive::runner::StorageWorldSink::new(storage.clone(), handle.clone())
                .with_broadcast(event_tx.clone()),
        ));
        let mut bridge = Bridge::new(fabric_router, sink);
        for a in adapters {
            bridge
                .connect_agent(a)
                .map_err(|e| FacadeError::Init(e.to_string()))?;
        }
        Ok(Self {
            world: WorldService::new(storage.clone(), handle.clone(), domain_rules),
            audit: AuditService::new(storage.clone(), handle.clone()),
            fabric: FabricService::new(bridge, handle),
            storage,
            event_tx,
        })
    }

    /// Build a DL reasoner from a caller-supplied TBox plus the current world.
    ///
    /// The schema is supplied explicitly because compiled definitions remain in
    /// the compiler/composition boundary; this method adds only the live ABox.
    pub fn knowledge_reasoner(
        &self,
        schema: &KnowledgeOntology,
    ) -> Result<KnowledgeReasoner, FacadeError> {
        self.world.storage_knowledge_reasoner(schema)
    }

    /// Initialize the first immutable DL ontology version.
    pub fn initialize_knowledge_version(
        &self,
        id: impl Into<String>,
        ontology: &KnowledgeOntology,
        runtime_ir: Option<&OntologyIR>,
    ) -> Result<OntologyVersionRecord, FacadeError> {
        self.world
            .initialize_knowledge_version(id, ontology, runtime_ir)
    }

    /// Create a semantically validated, pending ontology evolution proposal.
    pub fn propose_knowledge_change(
        &self,
        source_version_id: &str,
        proposal: &KnowledgeOntologyProposal,
        candidate_runtime_ir: Option<&OntologyIR>,
    ) -> Result<OntologyEvolutionRecord, FacadeError> {
        self.world
            .propose_knowledge_change(source_version_id, proposal, candidate_runtime_ir)
    }

    /// Record a named human approve/reject decision for an ontology proposal.
    pub fn decide_knowledge_proposal(
        &self,
        id: &str,
        approve: bool,
        reviewer: &str,
        reason: Option<&str>,
    ) -> Result<OntologyEvolutionRecord, FacadeError> {
        self.world
            .decide_knowledge_proposal(id, approve, reviewer, reason)
    }

    /// Atomically apply an approved ontology proposal as a new immutable version.
    pub fn apply_knowledge_proposal(
        &self,
        id: &str,
        target_version_id: impl Into<String>,
    ) -> Result<(OntologyEvolutionRecord, OntologyVersionRecord), FacadeError> {
        self.world.apply_knowledge_proposal(id, target_version_id)
    }

    /// Create a pending rollback proposal targeting an immutable prior version.
    pub fn propose_knowledge_rollback(
        &self,
        proposal_id: impl Into<String>,
        target_version_id: &str,
    ) -> Result<OntologyEvolutionRecord, FacadeError> {
        self.world
            .propose_knowledge_rollback(proposal_id, target_version_id)
    }

    pub fn current_knowledge_version(&self) -> Result<Option<OntologyVersionRecord>, FacadeError> {
        self.world.current_knowledge_version()
    }

    pub fn list_knowledge_versions(&self) -> Result<Vec<OntologyVersionRecord>, FacadeError> {
        self.world.list_knowledge_versions()
    }

    pub fn list_knowledge_proposals(&self) -> Result<Vec<OntologyEvolutionRecord>, FacadeError> {
        self.world.list_knowledge_proposals()
    }

    /// T1：裁决 fabric 挂起——approve 续跑执行 / reject 发审计事件。
    /// （facade 是唯一语义入口：CLI/HTTP/Plugin 裁决都走这里）
    pub fn fabric_resolve(
        &self,
        session_id: &str,
        run_id: &str,
        approve: bool,
    ) -> Result<AgentMessage, FacadeError> {
        self.fabric.resolve(session_id, run_id, approve)
    }

    /// 重启恢复单条 fabric task.request；仅由 server 启动恢复调用。
    pub(crate) fn restore_fabric_request(&self, request: AgentMessage) -> Result<(), FacadeError> {
        self.fabric.restore_request(request)
    }

    /// fabric.cancel()：按 correlation_id 取消运行，返回命中并 kill 的 adapter 数
    ///（0 = 该 correlation 未在运行；>0 = 已取消）。
    pub fn fabric_cancel(&self, correlation_id: &str) -> Result<usize, FacadeError> {
        self.fabric.cancel(correlation_id)
    }

    /// fabric.stream()：取会话消息快照（同步；实时推送见 WS 端点）。
    pub fn fabric_stream(&self, session_id: &str) -> Result<Vec<AgentMessage>, FacadeError> {
        self.fabric.stream(session_id)
    }

    /// capability.discover()：已接入能力提供者（adapter）id 列表。
    pub fn capability_discover(&self) -> Vec<String> {
        self.fabric.discover()
    }

    /// session.get()：按 id 取会话副本。
    pub fn session_get(&self, session_id: &str) -> Result<Session, FacadeError> {
        self.fabric.get(session_id)
    }

    /// 事件实时广播频道（T2b-1/C1）：server 的 AppState.event_tx 复用同一频道，
    /// 使 WS `/stream` 能收到 fabric.* 事件（Bridge sink 已 with_broadcast）。
    pub fn event_tx(&self) -> tokio::sync::broadcast::Sender<TraceEvent> {
        self.event_tx.clone()
    }

    /// S2：服务关闭时回收孤儿后台 job（透传 fabric.shutdown）。
    /// 由 server/mod.rs 的 `with_graceful_shutdown` 在 SIGINT/SIGTERM 时调用——
    /// 避免 spawn_blocking 后台进程泄漏为孤儿。
    pub fn shutdown(&self) {
        self.fabric.shutdown();
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

    fn project_create(&self, request: ProjectCreate) -> Result<ProjectView, FacadeError> {
        let rec = self
            .world
            .handle
            .block_on(self.storage.create_project(
                &request.id,
                &request.name,
                &request.description,
                &request.ontology,
            ))
            .map_err(|e| FacadeError::Fabric(format!("项目创建失败: {e}")))?;
        Ok(rec.into())
    }

    fn project_list(&self) -> Result<Vec<ProjectView>, FacadeError> {
        let recs = self
            .world
            .handle
            .block_on(self.storage.list_projects())
            .map_err(|e| FacadeError::Fabric(format!("项目列表失败: {e}")))?;
        Ok(recs.into_iter().map(Into::into).collect())
    }
}

impl From<crate::storage::ProjectRecord> for ProjectView {
    fn from(rec: crate::storage::ProjectRecord) -> Self {
        ProjectView {
            id: rec.id,
            name: rec.name,
            description: rec.description,
            ontology: rec.ontology,
            status: rec.status,
            created_at: rec.created_at,
        }
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
        // Phase 8：项目世界 = 独立 Context（ctx 名 = 项目 id）；None = 全局 world ctx
        let ctx_name = req
            .project_id
            .clone()
            .unwrap_or_else(|| "world".to_string());
        let ctx = kernel.create_context(None, Scope::World, &ctx_name)?;
        let mut p = Projection::new(ctx);
        for (ty, pred) in &self.domain_rules {
            p = p.with_domain_rule(ty.clone(), pred.clone());
        }
        self.handle.block_on(p.project_from_storage_for(
            &mut kernel,
            &self.storage,
            req.project_id.clone(),
        ))?;
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

    pub(crate) fn storage_knowledge_reasoner(
        &self,
        schema: &KnowledgeOntology,
    ) -> Result<KnowledgeReasoner, FacadeError> {
        let knowledge_base = self
            .handle
            .block_on(storage_world_snapshot_to_knowledge(schema, &self.storage))?;
        KnowledgeReasoner::new(&knowledge_base)
            .map_err(|error| FacadeError::Knowledge(error.to_string()))
    }

    pub fn initialize_knowledge_version(
        &self,
        id: impl Into<String>,
        ontology: &KnowledgeOntology,
        runtime_ir: Option<&OntologyIR>,
    ) -> Result<OntologyVersionRecord, FacadeError> {
        self.handle
            .block_on(
                self.storage
                    .initialize_knowledge_version(id, ontology, runtime_ir),
            )
            .map_err(FacadeError::from)
    }

    pub fn propose_knowledge_change(
        &self,
        source_version_id: &str,
        proposal: &KnowledgeOntologyProposal,
        candidate_runtime_ir: Option<&OntologyIR>,
    ) -> Result<OntologyEvolutionRecord, FacadeError> {
        self.handle
            .block_on(self.storage.propose_knowledge_change(
                source_version_id,
                proposal,
                candidate_runtime_ir,
            ))
            .map_err(FacadeError::from)
    }

    pub fn decide_knowledge_proposal(
        &self,
        id: &str,
        approve: bool,
        reviewer: &str,
        reason: Option<&str>,
    ) -> Result<OntologyEvolutionRecord, FacadeError> {
        self.handle
            .block_on(
                self.storage
                    .decide_knowledge_proposal(id, approve, reviewer, reason),
            )
            .map_err(FacadeError::from)
    }

    pub fn apply_knowledge_proposal(
        &self,
        id: &str,
        target_version_id: impl Into<String>,
    ) -> Result<(OntologyEvolutionRecord, OntologyVersionRecord), FacadeError> {
        self.handle
            .block_on(self.storage.apply_knowledge_proposal(id, target_version_id))
            .map_err(FacadeError::from)
    }

    pub fn propose_knowledge_rollback(
        &self,
        proposal_id: impl Into<String>,
        target_version_id: &str,
    ) -> Result<OntologyEvolutionRecord, FacadeError> {
        self.handle
            .block_on(
                self.storage
                    .propose_knowledge_rollback(proposal_id, target_version_id),
            )
            .map_err(FacadeError::from)
    }

    pub fn current_knowledge_version(&self) -> Result<Option<OntologyVersionRecord>, FacadeError> {
        self.handle
            .block_on(self.storage.current_knowledge_version())
            .map_err(FacadeError::from)
    }

    pub fn list_knowledge_versions(&self) -> Result<Vec<OntologyVersionRecord>, FacadeError> {
        self.handle
            .block_on(self.storage.list_knowledge_versions())
            .map_err(FacadeError::from)
    }

    pub fn list_knowledge_proposals(&self) -> Result<Vec<OntologyEvolutionRecord>, FacadeError> {
        self.handle
            .block_on(self.storage.list_knowledge_proposals())
            .map_err(FacadeError::from)
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
                project_id: req.project_id.clone(),
                run_id: req.run_id.clone(),
                agent_id: req.agent_id.clone(),
                limit,
                // C2/S3：审计语义域默认倒序（最新在前）——「查最新审计」拿到最旧 500 条
                // 是反直觉且会让 CLI 异步轮询永远漏掉最新终局事件。WS 断线补齐靠
                // after_id 游标（仍 ASC），不受影响。
                newest_first: true,
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
/// `submit` 全 &self：session 经 Arc<Mutex>；Bridge 经 Arc 可跨 spawn_blocking 共享。
/// 执行（子进程 stream）在后台 spawn_blocking，提交立即返 running（T2b-2 异步契约）。
pub struct FabricService {
    bridge: Arc<Bridge>,
    sessions: Arc<Mutex<SessionManager>>,
    handle: tokio::runtime::Handle,
}

impl FabricService {
    pub fn new(bridge: Bridge, handle: tokio::runtime::Handle) -> Self {
        Self {
            bridge: Arc::new(bridge),
            sessions: Arc::new(Mutex::new(SessionManager::new())),
            handle,
        }
    }

    /// 重启恢复：把持久化的 task.request 重新放回内存会话，供审批裁决后 resume。
    pub fn restore_request(&self, request: AgentMessage) -> Result<(), FacadeError> {
        if request.kind != MessageKind::TaskRequest {
            return Err(FacadeError::Fabric("恢复请求不是 task.request".into()));
        }
        let mut sessions = self
            .sessions
            .lock()
            .map_err(|_| FacadeError::Fabric("会话表锁中毒".into()))?;
        if sessions.get(&request.session_id).is_none() {
            sessions.create_with_id(request.session_id.clone(), vec![request.sender.clone()]);
        }
        let session = sessions
            .get_mut(&request.session_id)
            .ok_or_else(|| FacadeError::Fabric(format!("会话不存在: {}", request.session_id)))?;
        if !session.messages().iter().any(|m| {
            m.kind == MessageKind::TaskRequest && m.correlation_id == request.correlation_id
        }) {
            session
                .append(request)
                .map_err(|e| FacadeError::Fabric(e.to_string()))?;
        }
        Ok(())
    }

    pub fn submit(&self, req: FabricRequest) -> Result<FabricSubmission, FacadeError> {
        // 契约：session 不存在自动创建（participants=[sender]；id=请求方 session_id）
        {
            let mut sessions = self
                .sessions
                .lock()
                .map_err(|_| FacadeError::Fabric("会话表锁中毒".into()))?;
            if sessions.get(&req.session_id).is_none() {
                sessions.create_with_id(req.session_id.clone(), vec![req.sender.clone()]);
            }
        }
        // 同步：路由 + 事件 + 会话记录 → 立即返 running（或 pending）
        let (msg, job) = self
            .bridge
            .submit(
                self.sessions.clone(),
                &req.session_id,
                &req.sender,
                &req.intent,
                req.payload,
                req.execution_context.clone(),
                req.project_id.clone(),
            )
            .map_err(|e| FacadeError::Fabric(e.to_string()))?;
        // T2b-2 异步契约：子进程在后台执行（spawn_blocking），不阻塞提交返回
        if let Some(job) = job {
            let b = self.bridge.clone();
            let s = self.sessions.clone();
            // drop JoinHandle → 任务 detach 后在后台继续跑（fire-and-forget）
            drop(self.handle.spawn_blocking(move || {
                if let Err(err) = b.run_in_background(s, job) {
                    eprintln!("[fabric] 后台任务失败: {err}");
                }
            }));
        }
        Ok(FabricSubmission { result: msg })
    }

    /// T1：裁决 fabric 挂起——approve 续跑执行（resume → adapter.stream → 终局事件）；
    /// reject 只发 fabric.approval_rejected 审计事件，不执行。
    pub fn resolve(
        &self,
        session_id: &str,
        run_id: &str,
        approve: bool,
    ) -> Result<AgentMessage, FacadeError> {
        if approve {
            self.bridge
                .resume(self.sessions.clone(), session_id, run_id)
                .map_err(|e| FacadeError::Fabric(e.to_string()))
        } else {
            // 拒绝：审计事件（fabric.approval_rejected；run_id 回指挂起）
            self.bridge
                .reject_event(self.sessions.clone(), session_id, run_id)
                .map_err(|e| FacadeError::Fabric(e.to_string()))?;
            Err(crate::fabric::FabricError::Denied(format!("审批拒绝: {run_id}")).into())
        }
    }

    /// fabric.cancel()：按 correlation_id 取消运行，返回命中并 kill 的 adapter 数。
    /// 只走 bridge.cancel（kill 子进程 + 发取消审计事件），**不持 sessions 锁**——
    /// submit 执行期间持锁阻塞子进程，cancel 必须能并发抢占 kill，否则死锁
    /// （cancel 等锁、submit 等子进程结束才放锁）。会话终态由 bridge.finish 在
    /// 终态为 killed 时同源标记（与 fabric.task_cancelled 审计事件一致）。
    pub fn cancel(&self, correlation_id: &str) -> Result<usize, FacadeError> {
        self.bridge
            .cancel(correlation_id)
            .map_err(|e| FacadeError::Fabric(e.to_string()))
    }

    /// fabric.stream()（契约未来扩展 → 本次落地）：返回会话累积的消息快照（通信真相源）。
    /// v0.1 为同步快照——真实实时推送由 `/api/v1/wb/agent/:id/stream` WS 承担。
    pub fn stream(&self, session_id: &str) -> Result<Vec<AgentMessage>, FacadeError> {
        let sessions = self
            .sessions
            .lock()
            .map_err(|_| FacadeError::Fabric("会话表锁中毒".into()))?;
        match sessions.get(session_id) {
            Some(s) => Ok(s.messages().to_vec()),
            None => Err(FacadeError::Fabric(format!("会话不存在: {session_id}"))),
        }
    }

    /// capability.discover()（契约未来扩展 → 本次落地）：返回已接入的能力提供者（adapter）id。
    pub fn discover(&self) -> Vec<String> {
        self.bridge.discover()
    }

    /// S2：服务关闭时回收孤儿后台 job（透传 bridge.shutdown）。
    pub fn shutdown(&self) {
        self.bridge.shutdown();
    }

    /// session.get()（契约未来扩展 → 本次落地）：按 id 取会话副本（participants/state/messages）。
    pub fn get(&self, session_id: &str) -> Result<Session, FacadeError> {
        let sessions = self
            .sessions
            .lock()
            .map_err(|_| FacadeError::Fabric("会话表锁中毒".into()))?;
        match sessions.get(session_id) {
            Some(s) => Ok(s.clone()),
            None => Err(FacadeError::Fabric(format!("会话不存在: {session_id}"))),
        }
    }
}

impl From<crate::fabric::FabricError> for FacadeError {
    fn from(e: crate::fabric::FabricError) -> Self {
        FacadeError::Fabric(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::{FabricService, WorldService};
    use crate::fabric::{Bridge, router::FabricRouter};
    use crate::knowledge::{Axiom, ClassExpression, OntologyChange, OntologyProposal};
    use std::sync::Arc;

    /// 无适配器 / 无 sink 的空 FabricService：验证四个契约方法编译通过且错误路径正确。
    /// FabricService 现持有 tokio Handle（T2b-2 异步契约）→ 测试须在 runtime 内构造。
    fn empty_service() -> super::FabricService {
        FabricService::new(
            Bridge::new(FabricRouter::new(), None),
            tokio::runtime::Handle::current(),
        )
    }

    #[tokio::test(flavor = "current_thread")]
    async fn capability_discover_empty_without_adapters() {
        assert!(empty_service().discover().is_empty());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn fabric_cancel_is_best_effort_ok() {
        // 无 session / 无 adapter：取消是 best-effort，不应报错（返回命中数 0）。
        assert!(matches!(empty_service().cancel("corr_missing"), Ok(0)));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn fabric_stream_get_unknown_session_errors() {
        let svc = empty_service();
        assert!(svc.stream("nope").is_err());
        assert!(svc.get("nope").is_err());
    }

    fn governance_class(name: &str) -> ClassExpression {
        ClassExpression::class(format!("http://example.com/owl#{name}"))
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn facade_governs_and_applies_persistent_knowledge_evolution() {
        let storage = Arc::new(
            crate::storage::LibSQLStorage::open(":memory:")
                .await
                .unwrap(),
        );
        let world = WorldService::new(storage.clone(), tokio::runtime::Handle::current(), &[]);
        tokio::task::block_in_place(|| {
            let root = crate::knowledge::Ontology::new().with_axiom(Axiom::subclass(
                governance_class("Dog"),
                governance_class("Animal"),
            ));
            let root_version = world
                .initialize_knowledge_version("root", &root, None)
                .unwrap();
            assert_eq!(1, root_version.revision);

            let proposal = OntologyProposal::new("add-puppy").with_change(OntologyChange::Add(
                Axiom::subclass(governance_class("Puppy"), governance_class("Dog")),
            ));
            let pending = world
                .propose_knowledge_change("root", &proposal, None)
                .unwrap();
            assert!(!pending.review.is_approved());

            let approved = world
                .decide_knowledge_proposal("add-puppy", true, "alice", None)
                .unwrap();
            assert_eq!("alice", approved.review.reviewer().unwrap());
            let (_, target) = world.apply_knowledge_proposal("add-puppy", "v2").unwrap();
            assert_eq!(Some("root".to_owned()), target.parent_id);

            let rollback = world
                .propose_knowledge_rollback("rollback", "root")
                .unwrap();
            assert_eq!("v2", rollback.source_version_id);
            world
                .decide_knowledge_proposal("rollback", true, "bob", Some("restore"))
                .unwrap();
            let (_, restored) = world.apply_knowledge_proposal("rollback", "v3").unwrap();
            assert_eq!(root, restored.ontology);
            assert_eq!(3, world.list_knowledge_versions().unwrap().len());
        });
    }
}
