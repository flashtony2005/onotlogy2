//! Planner 闭环编排（对应 REQ-011 / prd-agent-integration.md 4.3）
//!
//! OpenAIP 自有核心：**Goal → LLMGoalParser → Planner → PlanStep → Capability → Execute**。
//! Pi 仅作 Cognitive Engine（经 `AgentEngine::reason` 提供 NLU/推理），本模块拥有编排与决策。
//! 每步产出 cognitive `TraceEvent` 并绑定 `object_refs`（可解释性根基）；Policy 拒 → WB2005；
//! 认知引擎不可用 → WB2004。

use crate::action::{ActionEngine, ActionError, ActionRegistry};
use crate::agent::{AgentEngine, AgentError, Context, Decision, WorldSnapshot};
use crate::evolution::LearnedKnowledge;
use crate::ir::{ActionDef, CompareOp, PolicyDef, ScalarType, ValueExpr};
use crate::runtime::RuntimeKernel;
use crate::storage::StorageError;
use crate::tool_backend::{ActionHook, NullBackend, ToolBackend, ToolError, ToolOutcome};
use async_trait::async_trait;
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::io::{self, Write};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::MutexGuard;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::oneshot;

/// HTTP 审批挂起的最长等待时间；超时按拒绝处理（不授予权限）。
pub const APPROVAL_TIMEOUT: Duration = Duration::from_secs(300);

/// 获取互斥锁：poison 时恢复内层数据（不 panic），避免审批/缓存路径因 poison 整进程崩溃。
fn lock_mutex<'a, T>(m: &'a Mutex<T>) -> MutexGuard<'a, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// REQ-018 运行时配置：Evolution 第二阶段 AutoApprove 的全局开关。
/// **默认关闭**——不显式开启，行为等同 MVP（require_approval 动作仍走 Human Approval）。
#[derive(Debug, Clone, Copy, Default)]
pub struct RuntimeConfig {
    /// 允许基于已验证 LearnedKnowledge 对 require_approval 动作自动放行（替代一次 Human Approval）。
    pub auto_approve_from_evolution: bool,
}

/// REQ-018 自动放行审计器：在 `check_action` 判定自动放行时记录审计。
/// 返回 Err → 调用方必须回退 Human Approval（绝不静默放行）。
#[async_trait]
pub trait ApprovalAuditor: Send + Sync {
    async fn record(&self, learned_id: &str, action: &str) -> Result<(), String>;
}

/// REQ-018 `may_auto_approve`：5 道闸门全过 + 开关开启 + 存在已验证 LearnedKnowledge → 允许自动放行。
/// 任一闸门不满足返回 false（调用方回退 Human Approval）。纯函数，便于单测（TC-026b/c/d/f）。
pub fn may_auto_approve(
    config: &RuntimeConfig,
    learned: Option<&LearnedKnowledge>,
    success_runs: usize,
    failure_count: usize,
    policy: &PolicyDef,
) -> bool {
    /// 高置信阈值（建议 0.8，取自 VALIDATE_THRESHOLD 之上）。
    const AUTO_APPROVE_CONFIDENCE: f64 = 0.8;
    /// 多次成功阈值（去重 run 数，建议 3）。
    const AUTO_APPROVE_MIN_RUNS: usize = 3;

    // 1) 显式允许开关
    if !config.auto_approve_from_evolution {
        return false;
    }
    // 2) 已验证知识（存在且未撤销）
    let Some(lk) = learned else {
        return false;
    };
    if !lk.auto_approve_enabled {
        return false;
    }
    // 3) 高置信
    if lk.confidence < AUTO_APPROVE_CONFIDENCE {
        return false;
    }
    // 4) 多次成功（去重 run 数）
    if success_runs < AUTO_APPROVE_MIN_RUNS {
        return false;
    }
    // 5) 无失败（强判定：依赖 ActionFailed 埋点）
    if failure_count > 0 {
        return false;
    }
    // 6) 低风险（已有 require_approval + 非空 precondition；采用现有 PolicyDef 判定）
    if !(policy.require_approval && !policy.precondition.is_empty()) {
        return false;
    }
    true
}

/// Planner 运行错误，映射到 WB 错误码。
#[derive(Debug, thiserror::Error)]
pub enum PlannerError {
    #[error(transparent)]
    Storage(#[from] StorageError),
    /// 认知引擎不可用 → WB2004（TC-010-b）。
    #[error("认知引擎不可用（Pi 未就绪/崩溃/鉴权失败）: {0}")]
    EngineUnavailable(String),
    /// Policy 拒绝 → WB2005（TC-011-b），等待 Human Approval。
    #[error("Policy 拒绝（需 Human Approval）: {0}")]
    PolicyDenied(String),
    /// 动作执行失败（V0.4 Action Engine）。
    #[error("动作执行失败: {0}")]
    Action(#[from] ActionError),
}

/// 运行状态码（与 timeline.status 对齐）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunStatus {
    Running,
    Completed,
    Failed,
    /// 进程崩溃未正常结束（重启恢复时由 Running 标记，区别于 Failed）。
    Interrupted,
}

impl RunStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            RunStatus::Running => "Running",
            RunStatus::Completed => "Completed",
            RunStatus::Failed => "Failed",
            RunStatus::Interrupted => "Interrupted",
        }
    }

    /// 持久化字符串 → 状态（未知 → Interrupted，安全兜底，避免回退成 Running）。
    pub fn from_db_str(s: &str) -> RunStatus {
        match s {
            "Running" => RunStatus::Running,
            "Completed" => RunStatus::Completed,
            "Failed" => RunStatus::Failed,
            _ => RunStatus::Interrupted,
        }
    }
}

/// 一次 Agent 运行的结果。
#[derive(Debug, Clone)]
pub struct AgentRunResult {
    pub run_id: String,
    pub agent_id: String,
    pub status: RunStatus,
    pub goal: String,
    pub error: Option<String>,
}

/// 能力（Capability Registry）：对世界模型的**受控**操作；Pi 必须经此路由，禁止直写。
#[derive(Debug, Clone)]
pub enum Capability {
    /// 查询某类型对象。
    QueryObjects { object_type: String },
    /// 创建对象（最小 Action 闭环的核心写操作）。
    CreateObject {
        object_type: String,
        id: String,
        data: Value,
    },
    /// 建立关系边。
    LinkRelation {
        source: String,
        edge: String,
        target: String,
    },
    /// 发出领域事件。
    EmitEvent {
        event_type: String,
        refs: Vec<String>,
    },
    /// 删除对象（破坏性，MVP 默认需 Human Approval → Policy 拒绝，见 TC-011-b）。
    DeleteObject { id: String },
    /// 声明式动作执行（V0.4 完整 Action Engine）：经 ActionRegistry 路由到对象自有动作。
    Action {
        object_type: String,
        action: String,
        target_id: String,
        args: Value,
    },
}

/// 计划步：一个能力 + 绑定对象 + 描述。
#[derive(Debug, Clone)]
pub struct PlanStep {
    pub capability: Capability,
    pub object_refs: Vec<String>,
    pub description: String,
}

/// Policy 引擎：对 Action（PlanStep）做约束与审批。破坏性动作默认拒；声明式动作经
/// 自带 `PolicyDef`（前置条件 + 审批闸门）校验；拒 → WB2005，等待 Human Approval。
pub struct Policy;

/// Human Approval 审批器：决定 `require_approval` 的动作是否被放行。
/// 默认 `AutoApprover { allow: false }`（拒，等待人工）；服务/测试可注入自动批准。
///
/// `approve` 为 `async`：CLI 的 `ConsoleApprover` 同步读 stdin 立即返回；
/// 服务的 `ServerApprover` 在此挂起运行、等待 HTTP 端点裁决后恢复。
#[async_trait]
pub trait PolicyApprover: Send + Sync {
    /// 返回 true 表示该步已获人工/自动批准，可越过审批闸门执行。
    async fn approve(&self, step: &PlanStep) -> bool;
}

/// 自动审批器：无人工场景（测试 / 服务）使用；`allow=true` 直接放行。
#[derive(Debug, Clone, Copy)]
pub struct AutoApprover {
    pub allow: bool,
}
#[async_trait]
impl PolicyApprover for AutoApprover {
    async fn approve(&self, _step: &PlanStep) -> bool {
        self.allow
    }
}

/// 交互式审批器：经 stdin 询问 y/n（CLI `agent run` 默认使用）。
///
/// 同一动作（按 `action@target_id`）只询问一次（决策缓存），避免对多个同类目标
/// 重复打断用户。非 Action 步用 description 作 key。
pub struct ConsoleApprover {
    cache: std::sync::Mutex<HashMap<String, bool>>,
}
impl ConsoleApprover {
    pub fn new() -> Self {
        Self {
            cache: std::sync::Mutex::new(HashMap::new()),
        }
    }

    /// 取审批 key：声明式动作用 `action@target_id`，其余用 description。
    fn key_of(step: &PlanStep) -> String {
        match &step.capability {
            Capability::Action {
                action, target_id, ..
            } => format!("{action}@{target_id}"),
            _ => step.description.clone(),
        }
    }
}
impl Default for ConsoleApprover {
    fn default() -> Self {
        Self::new()
    }
}
#[async_trait]
impl PolicyApprover for ConsoleApprover {
    async fn approve(&self, step: &PlanStep) -> bool {
        let key = Self::key_of(step);
        if let Some(v) = lock_mutex(&self.cache).get(&key) {
            return *v;
        }
        let ans = prompt(&format!(
            "⚠️ 需 Human Approval：{}？[y/N] ",
            step.description
        ));
        lock_mutex(&self.cache).insert(key, ans);
        ans
    }
}

/// 向 stdout 打印提示并从 stdin 读一行，解析 y/yes（忽略大小写）为批准。
/// `run_goal` 在 async 上下文同步调用本函数，用 `block_in_place` 避免阻塞 event loop。
fn prompt(question: &str) -> bool {
    print!("{question}");
    let _ = io::stdout().flush();
    let mut buf = String::new();
    tokio::task::block_in_place(|| {
        let _ = io::stdin().read_line(&mut buf);
    });
    matches!(buf.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}

/// REQ-018 运行级审计器：把自动放行审计写入内核存储（绑定当前 run_id）。
/// 写库失败 → 返回 Err，使 `check_action` 回退 Human Approval（绝不静默放行）。
pub struct RunAuditor<'a> {
    kernel: &'a RuntimeKernel,
    run_id: String,
}
impl<'a> RunAuditor<'a> {
    pub fn new(kernel: &'a RuntimeKernel, run_id: &str) -> Self {
        Self {
            kernel,
            run_id: run_id.to_string(),
        }
    }
}
#[async_trait]
impl ApprovalAuditor for RunAuditor<'_> {
    async fn record(&self, learned_id: &str, action: &str) -> Result<(), String> {
        self.kernel
            .record_approval_audit(
                &self.run_id,
                action,
                learned_id,
                "auto_approve",
                "5 道闸门全过，替代一次 Human Approval",
            )
            .await
            .map_err(|e| e.to_string())
    }
}

/// 待审批请求状态（与 timeline / HTTP 响应对齐）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "PascalCase")]
pub enum ApprovalState {
    Pending,
    Approved,
    Rejected,
    /// 挂起超时（Planner 已按拒绝回退）：与人工拒绝（Rejected）区分，UI 不再显示幽灵待审批。
    Expired,
}

impl ApprovalState {
    /// 状态 → 持久化字符串。
    pub fn as_str(&self) -> &'static str {
        match self {
            ApprovalState::Pending => "Pending",
            ApprovalState::Approved => "Approved",
            ApprovalState::Rejected => "Rejected",
            ApprovalState::Expired => "Expired",
        }
    }

    /// 持久化字符串 → 状态（未知 → Pending，安全兜底）。
    pub fn from_db_str(s: &str) -> ApprovalState {
        match s {
            "Approved" => ApprovalState::Approved,
            "Rejected" => ApprovalState::Rejected,
            "Expired" => ApprovalState::Expired,
            _ => ApprovalState::Pending,
        }
    }
}

/// 一条待审批请求（挂起态，供浏览器/HTTP 审批端点裁决后恢复运行）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalRequest {
    pub req_id: String,
    pub run_id: String,
    pub agent_id: String,
    /// 动作标识（`action@target_id`），便于 UI 展示与去重。
    pub step_id: String,
    pub description: String,
    pub state: ApprovalState,
}

/// 审批请求存储：连接 Planner 挂起点与 HTTP 审批端点（V0.4 遗留的 server 挂起式 HTTP 审批）。
pub struct ApprovalStore {
    requests: Mutex<HashMap<String, ApprovalEntry>>,
}

struct ApprovalEntry {
    request: ApprovalRequest,
    /// 挂起 Planner 的唤醒信道；裁决后置 None。
    tx: Option<oneshot::Sender<bool>>,
}

impl ApprovalStore {
    pub fn new() -> Self {
        Self {
            requests: Mutex::new(HashMap::new()),
        }
    }

    /// 列出某运行的所有审批请求（含历史裁决），供 UI 渲染审批列表。
    pub fn list_for_run(&self, run_id: &str) -> Vec<ApprovalRequest> {
        self.requests
            .lock()
            .unwrap()
            .values()
            .filter(|e| e.request.run_id == run_id)
            .map(|e| e.request.clone())
            .collect()
    }

    /// 取单条审批请求（快照）。
    pub fn get(&self, req_id: &str) -> Option<ApprovalRequest> {
        self.requests
            .lock()
            .unwrap()
            .get(req_id)
            .map(|e| e.request.clone())
    }

    /// 内部：注册待审批并挂起（由 `ServerApprover` 调用，返回裁决信道接收端）。
    fn register(&self, req: ApprovalRequest) -> oneshot::Receiver<bool> {
        let (tx, rx) = oneshot::channel();
        lock_mutex(&self.requests).insert(
            req.req_id.clone(),
            ApprovalEntry {
                request: req,
                tx: Some(tx),
            },
        );
        rx
    }

    /// 超时回收：挂起 Planner 已按拒绝回退后调用，把残留请求翻为 Expired 并清掉 tx。
    /// 之后 `resolve` 对其返回 false（映射 404），避免「请求仍在 Pending + resolve 误判成功」
    /// 导致的 UI 幽灵待审批（Critical-1/2）；Expired 与人工 Rejected 可区分（Critical-3）。
    /// 已裁决（tx 为 None）或已 Expired 的请求幂等：直接返回。
    pub fn expire(&self, req_id: &str) {
        let mut g = lock_mutex(&self.requests);
        if let Some(e) = g.get_mut(req_id) {
            e.tx = None;
            if e.request.state == ApprovalState::Pending {
                e.request.state = ApprovalState::Expired;
            }
        }
    }

    /// 测试专用：直接插入一条请求（绕过挂起信道生命周期），便于单测 resolve/expire 语义。
    /// 非 cfg 限定，因集成测试位于独立 crate，无法访问 cfg(test) 项。
    #[doc(hidden)]
    pub fn requests_insert_for_test(
        &self,
        req_id: &str,
        state: ApprovalState,
        tx: Option<oneshot::Sender<bool>>,
    ) {
        use crate::planner::ApprovalRequest;
        lock_mutex(&self.requests).insert(
            req_id.to_string(),
            ApprovalEntry {
                request: ApprovalRequest {
                    req_id: req_id.to_string(),
                    run_id: "test-run".to_string(),
                    agent_id: "test-agent".to_string(),
                    step_id: "step".to_string(),
                    description: "test".to_string(),
                    state,
                },
                tx,
            },
        );
    }

    /// 裁决：发信号唤醒挂起的 Planner，更新状态；返回是否成功裁决。
    /// 请求不存在 / 已裁决（tx 已被取走）/ 已超时 Expired → 返回 false（HTTP 端点映射 404）。
    pub fn resolve(&self, req_id: &str, approve: bool) -> bool {
        let mut g = lock_mutex(&self.requests);
        let Some(e) = g.get_mut(req_id) else {
            return false;
        };
        // 已裁决（tx 已被取走）或已超时（Expired）→ 拒绝裁决
        let Some(tx) = e.tx.take() else {
            return false;
        };
        e.request.state = if approve {
            ApprovalState::Approved
        } else {
            ApprovalState::Rejected
        };
        let _ = tx.send(approve);
        true
    }
}

impl Default for ApprovalStore {
    fn default() -> Self {
        Self::new()
    }
}

/// 服务端挂起式审批器：遇 `require_approval` 动作时挂起运行，注册待审批请求，
/// 并发 `PendingApproval` 认知事件（含 `reqId`）；浏览器经 HTTP 端点裁决后恢复。
/// 对应 V0.4 遗留的 server 挂起式 HTTP 审批端点（让浏览器点批准）。
pub struct ServerApprover {
    kernel: Arc<RuntimeKernel>,
    store: Arc<ApprovalStore>,
    run_id: String,
    agent_id: String,
    seq: AtomicU64,
    /// 挂起超时（默认 APPROVAL_TIMEOUT）；测试可注入极短超时触发超时回收路径。
    timeout: Duration,
}

impl ServerApprover {
    pub fn new(
        kernel: Arc<RuntimeKernel>,
        store: Arc<ApprovalStore>,
        run_id: String,
        agent_id: String,
    ) -> Self {
        Self {
            kernel,
            store,
            run_id,
            agent_id,
            seq: AtomicU64::new(0),
            timeout: APPROVAL_TIMEOUT,
        }
    }

    /// 覆盖挂起超时（默认 APPROVAL_TIMEOUT）；测试注入极短超时触发 Expired 回收路径。
    pub fn with_timeout(mut self, d: Duration) -> Self {
        self.timeout = d;
        self
    }
}

#[async_trait]
impl PolicyApprover for ServerApprover {
    async fn approve(&self, step: &PlanStep) -> bool {
        let req_id = format!(
            "apr_{}_{}",
            self.run_id,
            self.seq.fetch_add(1, Ordering::SeqCst)
        );
        let step_id = ConsoleApprover::key_of(step);
        // 注册待审批并挂起（rx 在 resolve 时被唤醒）
        let rx = self.store.register(ApprovalRequest {
            req_id: req_id.clone(),
            run_id: self.run_id.clone(),
            agent_id: self.agent_id.clone(),
            step_id: step_id.clone(),
            description: step.description.clone(),
            state: ApprovalState::Pending,
        });
        // 持久化待审批（DB 真相源；内存 store 仅服务 oneshot 唤醒，重启后虽无法唤醒但不影响审计/列表）
        let _ = self
            .kernel
            .storage()
            .insert_approval(
                &req_id,
                &self.run_id,
                &self.agent_id,
                &step_id,
                &step.description,
            )
            .await;
        // 发 PendingApproval 认知事件（含 reqId，UI 据此渲染审批按钮）
        let _ = self
            .kernel
            .emit_cognitive(
                "PendingApproval",
                &self.run_id,
                &self.agent_id,
                &step.object_refs,
                Some(&format!("等待人工审批: {}", step.description)),
                Some(
                    json!({ "reqId": req_id, "stepId": step_id, "description": step.description }),
                ),
                Some(json!("Pending")),
                Some(0.0),
            )
            .await;
        // 挂起等待 HTTP 端点裁决（approve=true 放行 / false=WB2005 终止）。
        // 超时：回收残留请求（store.expire 翻 Expired + 清 tx），避免 UI 幽灵待审批。
        let (decision, outcome) = match tokio::time::timeout(self.timeout, rx).await {
            Ok(Ok(v)) => (v, if v { "Approved" } else { "Rejected" }),
            _ => {
                self.store.expire(&req_id);
                (false, "Expired")
            }
        };
        // 发 ApprovalDecision 事件（供 timeline / WS 实时反映结果）；
        // Expired 与人工 Rejected 通过 outcome 区分，便于 UI/审计溯源（Critical-3）。
        let _ = self
            .kernel
            .emit_cognitive(
                "ApprovalDecision",
                &self.run_id,
                &self.agent_id,
                &step.object_refs,
                Some(&format!(
                    "审批结果: {}（reqId={}）",
                    match outcome {
                        "Approved" => "批准",
                        "Rejected" => "拒绝",
                        _ => "超时未裁决",
                    },
                    req_id
                )),
                Some(json!({
                    "reqId": req_id,
                    "decision": outcome
                })),
                Some(json!(outcome)),
                Some(if decision { 0.9 } else { 0.0 }),
            )
            .await;
        decision
    }
}

impl Policy {
    /// 校验计划步：破坏性动作（删对象）默认拒，转 Human Approval；其余允许。
    pub fn check(step: &PlanStep) -> PolicyDecision {
        match &step.capability {
            Capability::DeleteObject { id } => PolicyDecision::Deny {
                reason: format!("删除对象 {id} 属破坏性操作，需 Human Approval"),
            },
            _ => PolicyDecision::Allow,
        }
    }

    /// 校验声明式动作：结合动作自带 `PolicyDef`（前置条件 + 审批闸门）。
    ///
    /// - `def` 为 None（动作未注册）→ 放行；
    /// - `precondition` 全部满足才放行（AND）；任一不满足 → Deny；
    /// - `require_approval=true` 默认 Deny（WB2005）：
    ///   - 若满足 REQ-018 自动放行 5 道闸门，经 `auditor` 记录审计后替代 Human Approval（Allow）；
    ///   - 否则仍须经 `approver` 放行（Human Approval）；审计失败/无知识 → 回退 Human（绝不静默放行）。
    #[allow(clippy::too_many_arguments)]
    pub async fn check_action(
        step: &PlanStep,
        def: Option<&ActionDef>,
        approver: &dyn PolicyApprover,
        target: Option<&Value>,
        args: &HashMap<String, Value>,
        config: &RuntimeConfig,
        learned: Option<&LearnedKnowledge>,
        failure_count: usize,
        auditor: Option<&dyn ApprovalAuditor>,
    ) -> PolicyDecision {
        let Some(def) = def else {
            return PolicyDecision::Allow;
        };
        // 前置条件：全部满足才放行
        for clause in &def.policy.precondition {
            let lhs = resolve_policy_field(&clause.field, args, target);
            let rhs = resolve_value(&clause.value, args, target, "");
            if !compare_values(&lhs, &rhs, clause.op) {
                return PolicyDecision::Deny {
                    reason: format!(
                        "动作 {} 不满足前置条件 {}={:?} {:?} {:?}",
                        def.name, clause.field, lhs, clause.op, rhs
                    ),
                };
            }
        }
        // 审批闸门：require_approval 默认 Deny，需 approver 放行（Human Approval）。
        // approver.approve 为 async：ConsoleApprover 同步返回；ServerApprover 挂起等待 HTTP 裁决。
        if def.policy.require_approval {
            // REQ-018 自动放行：5 道闸门全过 → 替代一次 Human Approval（须先写审计）。
            let success_runs = learned.map(|l| l.run_ids.len()).unwrap_or(0);
            if may_auto_approve(config, learned, success_runs, failure_count, &def.policy) {
                match auditor {
                    // 审计成功 → 自动放行（替代 Human Approval）
                    Some(a) => {
                        if let Some(lk) = learned {
                            match a.record(lk.id.as_str(), &def.name).await {
                                Ok(()) => return PolicyDecision::Allow,
                                // 审计写库失败 → 回退 Human Approval（绝不静默放行）
                                Err(_) => { /* fall through to approver */ }
                            }
                        }
                    }
                    // 纯单元测试无 auditor：直接放行（审计留待集成层）
                    None => return PolicyDecision::Allow,
                }
            }
            if !approver.approve(step).await {
                return PolicyDecision::Deny {
                    reason: format!("动作 {} 需 Human Approval（WB2005）", def.name),
                };
            }
        }
        PolicyDecision::Allow
    }
}

/// 把 Policy 字段引用解析为值：`self.<f>` → 目标字段；参数名 → 参数；否则 → 目标字段。
fn resolve_policy_field(
    field: &str,
    args: &HashMap<String, Value>,
    target: Option<&Value>,
) -> Value {
    let ve = if let Some(rest) = field.strip_prefix("self.") {
        ValueExpr::SelfField(rest.to_string())
    } else if args.contains_key(field) {
        ValueExpr::Param(field.to_string())
    } else {
        ValueExpr::SelfField(field.to_string())
    };
    resolve_value(&ve, args, target, "")
}

/// 解析值表达式（与 ActionEngine 同源逻辑，供 Policy 比较使用）。
fn resolve_value(
    v: &ValueExpr,
    args: &HashMap<String, Value>,
    target: Option<&Value>,
    target_id: &str,
) -> Value {
    match v {
        ValueExpr::Literal(l) => l.clone(),
        ValueExpr::Param(p) => args.get(p).cloned().unwrap_or(Value::Null),
        ValueExpr::SelfId => Value::String(target_id.to_string()),
        ValueExpr::SelfField(f) => target
            .and_then(|t| {
                t.get(f)
                    .cloned()
                    .or_else(|| t.get("data").and_then(|d| d.get(f)).cloned())
            })
            .unwrap_or(Value::Null),
    }
}

fn step_key(step: &PlanStep) -> Option<String> {
    match &step.capability {
        Capability::Action {
            object_type,
            action,
            target_id,
            ..
        } => Some(format!("{object_type}:{action}:{target_id}")),
        _ => None,
    }
}

/// 用户目标优先，其次解析出的 Intent；都认不出则不过滤（只靠前置条件）。
fn action_matches_intent(action: &str, goal: &str, intent: &str) -> bool {
    match intent_action_allowlist(goal, intent) {
        None => true,
        Some(allow) => allow
            .iter()
            .any(|a| a.eq_ignore_ascii_case(action)),
    }
}

fn intent_action_allowlist(goal: &str, intent: &str) -> Option<&'static [&'static str]> {
    classify_intent_text(goal).or_else(|| classify_intent_text(intent))
}

fn classify_intent_text(text: &str) -> Option<&'static [&'static str]> {
    let t = text.to_ascii_lowercase();
    if contains_any(&t, &["lint", "clippy", "质量", "静态检查"]) {
        return Some(&["lint"]);
    }
    if contains_any(&t, &["refund", "退款"]) {
        return Some(&["refund"]);
    }
    if contains_any(&t, &["close account", "closeaccount", "关闭账号", "关闭账户"]) {
        return Some(&["closeaccount"]);
    }
    if contains_any(
        &t,
        &[
            "retain",
            "retention",
            "churn",
            "挽留",
            "task",
            "任务",
        ],
    ) {
        return Some(&["retain", "start", "complete"]);
    }
    None
}

fn contains_any(hay: &str, needles: &[&str]) -> bool {
    needles.iter().any(|n| hay.contains(n))
}

/// 从目标/Intent 抽取动作参数：只填 ODL `params:` 声明过的字段。
/// 具名 `amount=500` 优先；否则 Int/Float 取文本中最后一个数字（避免把对象 id 当成金额）。
fn extract_action_args(
    act: &ActionDef,
    goal: &str,
    intent: &str,
    world_ids: &[String],
) -> HashMap<String, Value> {
    let hay = format!("{goal} {intent}");
    let for_nums = strip_object_ids(&hay, world_ids);
    let nums = scan_numbers(&for_nums);
    let mut out = HashMap::new();
    for p in &act.params {
        if let Some(v) = extract_named_param(&hay, &p.name, p.ty) {
            out.insert(p.name.clone(), v);
            continue;
        }
        match p.ty {
            ScalarType::Int | ScalarType::Float => {
                if let Some(&n) = nums.last() {
                    out.insert(p.name.clone(), number_as(p.ty, n));
                }
            }
            ScalarType::Bool => {
                if let Some(b) = extract_bool(&hay) {
                    out.insert(p.name.clone(), Value::Bool(b));
                }
            }
            ScalarType::String => {}
        }
    }
    out
}

fn extract_named_param(hay: &str, name: &str, ty: ScalarType) -> Option<Value> {
    let lower = hay.to_ascii_lowercase();
    let key = name.to_ascii_lowercase();
    let idx = lower.find(&key)?;
    let rest = hay[idx + name.len()..].trim_start();
    let rest = rest
        .strip_prefix(['=', ':', '：'])
        .map(str::trim_start)
        .unwrap_or(rest);
    match ty {
        ScalarType::Int | ScalarType::Float => {
            let n = scan_numbers(rest).into_iter().next()?;
            Some(number_as(ty, n))
        }
        ScalarType::Bool => extract_bool(rest).map(Value::Bool),
        ScalarType::String => {
            let tok = rest.split_whitespace().next()?;
            Some(Value::String(tok.trim_matches(|c| c == '"' || c == '\'').to_string()))
        }
    }
}

fn strip_object_ids(hay: &str, ids: &[String]) -> String {
    hay.split(|c: char| !c.is_alphanumeric() && c != '_' && c != '.' && c != '-')
        .filter(|t| !t.is_empty() && !ids.iter().any(|id| id == t))
        .collect::<Vec<_>>()
        .join(" ")
}

fn token_in_text(text: &str, id: &str) -> bool {
    text.split(|c: char| !c.is_alphanumeric() && c != '_' && c != '.' && c != '-')
        .any(|t| t == id)
}

/// 目标文本点名优先；否则用解析 refs（去掉 world 哨兵）；都没有则不限制（None）。
fn resolve_target_ids(
    goal: &str,
    parsed_refs: &[String],
    world_ids: &[String],
) -> Option<Vec<String>> {
    let mentioned: Vec<String> = world_ids
        .iter()
        .filter(|id| token_in_text(goal, id))
        .cloned()
        .collect();
    if !mentioned.is_empty() {
        return Some(mentioned);
    }
    let parsed: Vec<String> = parsed_refs
        .iter()
        .filter(|r| !r.is_empty() && r.as_str() != "world" && world_ids.iter().any(|id| id == *r))
        .cloned()
        .collect();
    if parsed.is_empty() {
        None
    } else {
        Some(parsed)
    }
}

fn world_object_ids(world: &WorldSnapshot) -> Vec<String> {
    world
        .objects
        .iter()
        .filter_map(|o| o.get("id").and_then(|v| v.as_str()).map(|s| s.to_string()))
        .collect()
}

fn object_in_scope(obj: &Value, scope: &[String]) -> bool {
    let id = obj.get("id").and_then(|v| v.as_str()).unwrap_or("");
    if scope.iter().any(|s| s == id) {
        return true;
    }
    if scope.iter().any(|s| id.starts_with(&format!("{s}_"))) {
        return true;
    }
    if let Some(cust) = obj
        .get("data")
        .and_then(|d| d.get("customer"))
        .and_then(|v| v.as_str())
    {
        return scope.iter().any(|s| s == cust);
    }
    false
}

fn scan_numbers(s: &str) -> Vec<f64> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for ch in s.chars() {
        if ch.is_ascii_digit() || ch == '.' || (ch == '-' && cur.is_empty()) {
            cur.push(ch);
        } else if !cur.is_empty() {
            if let Ok(n) = cur.parse::<f64>() {
                out.push(n);
            }
            cur.clear();
        }
    }
    if !cur.is_empty() {
        if let Ok(n) = cur.parse::<f64>() {
            out.push(n);
        }
    }
    out
}

fn number_as(ty: ScalarType, n: f64) -> Value {
    match ty {
        ScalarType::Int => Value::from(n as i64),
        _ => Value::from(n),
    }
}

fn extract_bool(s: &str) -> Option<bool> {
    let t = s.to_ascii_lowercase();
    if t.contains("true") || t.contains("yes") || t.contains("是") {
        Some(true)
    } else if t.contains("false") || t.contains("no") || t.contains("否") {
        Some(false)
    } else {
        None
    }
}

fn action_preconditions_hold(
    act: &ActionDef,
    target: Option<&Value>,
    args: &HashMap<String, Value>,
) -> bool {
    for clause in &act.policy.precondition {
        let lhs = resolve_policy_field(&clause.field, args, target);
        let rhs = resolve_value(&clause.value, args, target, "");
        if !compare_values(&lhs, &rhs, clause.op) {
            return false;
        }
    }
    true
}

/// 按运算符比较两个 JSON 值：Eq/Ne 走全相等；其余按数值（f64）比较，非数则 false。
fn compare_values(a: &Value, b: &Value, op: CompareOp) -> bool {
    use CompareOp::*;
    match op {
        Eq => a == b,
        Ne => a != b,
        _ => match (a.as_f64(), b.as_f64()) {
            (Some(x), Some(y)) => match op {
                Lt => x < y,
                Le => x <= y,
                Gt => x > y,
                Ge => x >= y,
                _ => false,
            },
            _ => false,
        },
    }
}

/// Policy 决策。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyDecision {
    Allow,
    Deny { reason: String },
}

/// LLM 目标解析器：经认知引擎把自然语言目标解析为结构化 `Goal`（NLU）。
pub struct LLMGoalParser;

impl LLMGoalParser {
    /// 解析目标：调用引擎 step=Goal，返回意图/置信度/作用对象。
    pub async fn parse(
        engine: &dyn AgentEngine,
        kernel: &RuntimeKernel,
        goal: &str,
        run_id: &str,
        agent_id: &str,
        world: &WorldSnapshot,
    ) -> Result<Goal, PlannerError> {
        let ctx = Context {
            goal: goal.to_string(),
            run_id: run_id.to_string(),
            agent_id: agent_id.to_string(),
            step: "Goal".to_string(),
            prior_output: Value::Null,
            world: world.clone(),
        };
        // Goal 事件由 run_goal 显式发射一次（避免与后续结构化 Goal 事件重复）。
        let dec = call_engine(kernel, engine, &ctx, false).await?;
        Ok(Goal {
            intent: dec.intent,
            confidence: dec.confidence,
            object_refs: dec.object_refs,
        })
    }
}

/// 解析后的目标。
#[derive(Debug, Clone)]
pub struct Goal {
    pub intent: String,
    pub confidence: f64,
    pub object_refs: Vec<String>,
}

/// Planner：OpenAIP 自有决策编排核心。
pub struct Planner<'a> {
    kernel: &'a RuntimeKernel,
    registry: ActionRegistry,
    approver: Arc<dyn PolicyApprover>,
    config: RuntimeConfig,
    backend: Arc<dyn ToolBackend>,
    hooks: Vec<Arc<dyn ActionHook>>,
}

impl<'a> Planner<'a> {
    /// 带动作注册表（V0.4 完整 Action Engine）：候选对象经声明式动作执行。
    pub fn with_actions(kernel: &'a RuntimeKernel, registry: ActionRegistry) -> Self {
        Self {
            kernel,
            registry,
            approver: Arc::new(AutoApprover { allow: false }),
            config: RuntimeConfig::default(),
            backend: Arc::new(NullBackend),
            hooks: Vec::new(),
        }
    }

    /// 注入出世界 ToolBackend（不支持的动作仍只跑 ActionEngine）。
    pub fn with_backend(mut self, backend: Arc<dyn ToolBackend>) -> Self {
        self.backend = backend;
        self
    }

    /// 追加一条流水线闸门（pre Deny 则本步不执行）。
    pub fn with_hook(mut self, hook: Arc<dyn ActionHook>) -> Self {
        self.hooks.push(hook);
        self
    }

    /// 注入 REQ-018 运行时配置（Evolution 第二阶段 AutoApprove 开关）。
    pub fn with_config(mut self, config: RuntimeConfig) -> Self {
        self.config = config;
        self
    }

    /// 注入 Policy 审批器（如服务侧自动批准 / CLI 交互式审批）。
    pub fn with_approver(mut self, approver: Arc<dyn PolicyApprover>) -> Self {
        self.approver = approver;
        self
    }

    /// 候选对象：有 actions 的对象；若 scope 有值则只保留范围内（含派生任务）。
    fn select_candidates(&self, world: &WorldSnapshot, scope: Option<&[String]>) -> Vec<String> {
        world
            .objects
            .iter()
            .filter(|o| {
                let t = o.get("type").and_then(|v| v.as_str()).unwrap_or("");
                self.registry.has_actions(t)
            })
            .filter(|o| scope.map(|s| object_in_scope(o, s)).unwrap_or(true))
            .filter_map(|o| o.get("id").and_then(|x| x.as_str()).map(|s| s.to_string()))
            .collect()
    }

    /// 计划构建：候选对象上、意图允许且当前快照满足前置条件的动作 → `Capability::Action`。
    fn build_plan(
        &self,
        world: &WorldSnapshot,
        candidates: &[String],
        goal: &str,
        intent: &str,
    ) -> Vec<PlanStep> {
        let mut steps = Vec::new();
        for c in candidates {
            let obj = world
                .objects
                .iter()
                .find(|o| o.get("id").and_then(|v| v.as_str()) == Some(c.as_str()));
            let otype = obj
                .and_then(|o| o.get("type").and_then(|v| v.as_str()))
                .unwrap_or("");
            for act in self.registry.actions_of(otype) {
                if !action_matches_intent(&act.name, goal, intent) {
                    continue;
                }
                let world_ids = world_object_ids(world);
                let args = extract_action_args(act, goal, intent, &world_ids);
                if !action_preconditions_hold(act, obj, &args) {
                    continue;
                }
                steps.push(PlanStep {
                    capability: Capability::Action {
                        object_type: otype.to_string(),
                        action: act.name.clone(),
                        target_id: c.clone(),
                        args: json!(args),
                    },
                    object_refs: vec![c.clone()],
                    description: format!("对 {c} 执行声明式动作 {}", act.name),
                });
            }
        }
        steps
    }

    async fn apply_steps(
        &self,
        steps: &[PlanStep],
        world: &WorldSnapshot,
        run_id: &str,
        agent_id: &str,
        executed_actions: &mut Vec<String>,
        executed_keys: &mut HashSet<String>,
    ) -> Result<(), PlannerError> {
        for step in steps {
            let decision = match &step.capability {
                Capability::Action {
                    object_type,
                    action,
                    target_id,
                    args,
                } => {
                    let def = self.registry.get(object_type, action);
                    let target = world.objects.iter().find(|o| {
                        o.get("id").and_then(|v| v.as_str()) == Some(target_id.as_str())
                    });
                    let args_map: HashMap<String, Value> = args
                        .as_object()
                        .map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
                        .unwrap_or_default();
                    let learned = self.kernel.evolution_state().await.ok().and_then(|st| {
                        st.learned.into_iter().find(|l| l.action == *action)
                    });
                    let failure_count = self.kernel.count_action_failed(action).await.unwrap_or(0);
                    let auditor = RunAuditor::new(self.kernel, run_id);
                    Policy::check_action(
                        step,
                        def,
                        self.approver.as_ref(),
                        target,
                        &args_map,
                        &self.config,
                        learned.as_ref(),
                        failure_count,
                        Some(&auditor),
                    )
                    .await
                }
                _ => Policy::check(step),
            };
            match decision {
                PolicyDecision::Deny { reason } => {
                    self.kernel
                        .emit_cognitive(
                            "PolicyCheck",
                            run_id,
                            agent_id,
                            &step.object_refs,
                            Some(&format!("DENY: {reason}")),
                            Some(json!({ "capability": step.description })),
                            Some(json!("Deny")),
                            Some(0.0),
                        )
                        .await?;
                    return Err(PlannerError::PolicyDenied(reason));
                }
                PolicyDecision::Allow => {
                    self.kernel
                        .emit_cognitive(
                            "PolicyCheck",
                            run_id,
                            agent_id,
                            &step.object_refs,
                            Some("ALLOW"),
                            Some(json!({ "capability": step.description })),
                            Some(json!("Allow")),
                            Some(0.95),
                        )
                        .await?;
                    self.execute_step(step, run_id, agent_id).await?;
                    if let Capability::Action { action, .. } = &step.capability {
                        executed_actions.push(action.clone());
                    }
                    if let Some(k) = step_key(step) {
                        executed_keys.insert(k);
                    }
                    self.kernel
                        .emit_cognitive(
                            "Action",
                            run_id,
                            agent_id,
                            &step.object_refs,
                            Some(&step.description),
                            Some(json!({ "capability": step.description })),
                            Some(json!("Success")),
                            Some(0.9),
                        )
                        .await?;
                }
            }
        }
        Ok(())
    }

    /// 运行一个目标，跑通完整认知闭环并返回运行结果。
    /// `run_id` 由调用方预生成（server 需先占坑状态映射）。
    pub async fn run_goal(
        &self,
        engine: &dyn AgentEngine,
        goal: &str,
        agent_id: &str,
        run_id: &str,
    ) -> Result<AgentRunResult, PlannerError> {
        let run_id = run_id.to_string();
        let world = self.kernel.snapshot_world().await?;

        // 1) Goal：NLU 解析意图（绑定作用对象）
        let goal_parsed =
            LLMGoalParser::parse(engine, self.kernel, goal, &run_id, agent_id, &world).await?;
        let goal_refs = non_empty(goal_parsed.object_refs.clone());
        self.kernel
            .emit_cognitive(
                "Goal",
                &run_id,
                agent_id,
                &goal_refs,
                Some(&format!("目标解析: {goal}")),
                Some(json!({ "goal": goal })),
                Some(json!({ "intent": goal_parsed.intent, "confidence": goal_parsed.confidence })),
                Some(goal_parsed.confidence),
            )
            .await?;

        // 2) Intent：引擎在 Goal 基础上细化意图
        let intent_dec = self
            .reason(
                engine,
                goal,
                &run_id,
                agent_id,
                "Intent",
                json!({ "intent": goal_parsed.intent }),
                &world,
            )
            .await?;
        let _ = intent_dec;

        // 3) OntologyQuery：从世界模型找出目标作用对象。
        //    候选 = 声明了 actions 的对象（避免把派生对象再当候选）。
        let root_scope = resolve_target_ids(
            goal,
            &goal_parsed.object_refs,
            &world_object_ids(&world),
        );
        let candidates = self.select_candidates(&world, root_scope.as_deref());
        let scope_refs = non_empty(candidates.clone());
        self.kernel
            .emit_cognitive(
                "OntologyQuery",
                &run_id,
                agent_id,
                &scope_refs,
                Some("查询世界模型中的客户对象"),
                Some(json!({ "goal": goal })),
                Some(json!({ "objects": candidates })),
                Some(0.9),
            )
            .await?;

        // 3.5) MemoryRecall：召回历史沉淀（V0.5 记忆闭环起点）。
        //      无记忆（首跑）→ 跳过；有记忆 → 发认知事件绑定候选对象，供本次计划复用。
        let memories = self.kernel.recall_all().await?;
        if !memories.is_empty() && !scope_refs.is_empty() {
            self.kernel
                .emit_cognitive(
                    "MemoryRecall",
                    &run_id,
                    agent_id,
                    &scope_refs,
                    Some("召回历史沉淀经验，复用至本次计划"),
                    Some(json!({ "memories": memories
                        .iter()
                        .map(|m| json!({ "label": m.label, "hits": m.hits }))
                        .collect::<Vec<_>>() })),
                    Some(json!({ "count": memories.len() })),
                    Some(0.85),
                )
                .await?;
        }

        // 4) Reasoning：引擎基于快照推理（为何存在流失风险）
        let reason_dec = self
            .reason(
                engine,
                goal,
                &run_id,
                agent_id,
                "Reasoning",
                json!({ "candidates": candidates }),
                &world,
            )
            .await?;

        // 5) Planning：引擎产出计划摘要；OpenAIP 基于此生成 PlanStep（Capability Registry）
        let plan_dec = self
            .reason(
                engine,
                goal,
                &run_id,
                agent_id,
                "Planning",
                json!({ "candidates": candidates, "reasoning": reason_dec.reasoning }),
                &world,
            )
            .await?;
        // 5') 计划构建：注册表驱动的声明式动作 → Capability::Action。
        //     世界因动作改变后重快照、重规划，直到无新适用动作（一轮目标推到稳态）。
        const MAX_PLAN_WAVES: usize = 8;
        let mut world = world;
        let mut scope_refs = scope_refs;
        let mut steps = self.build_plan(&world, &candidates, goal, &goal_parsed.intent);
        self.kernel
            .emit_cognitive(
                "Planning",
                &run_id,
                agent_id,
                &scope_refs,
                Some(&plan_dec.reasoning),
                Some(json!({ "candidates": candidates, "wave": 0 })),
                Some(json!({ "steps": steps.iter().map(|s| json!({ "description": s.description, "object_refs": s.object_refs })).collect::<Vec<_>>() })),
                Some(plan_dec.confidence),
            )
            .await?;

        let mut executed_actions: Vec<String> = Vec::new();
        let mut executed_keys: HashSet<String> = HashSet::new();
        for wave in 0..MAX_PLAN_WAVES {
            if wave > 0 {
                world = self.kernel.snapshot_world().await?;
                let next = self.select_candidates(&world, root_scope.as_deref());
                scope_refs = non_empty(next.clone());
                steps = self.build_plan(&world, &next, goal, &goal_parsed.intent);
                steps.retain(|s| {
                    step_key(s)
                        .map(|k| !executed_keys.contains(&k))
                        .unwrap_or(true)
                });
                if steps.is_empty() {
                    break;
                }
                self.kernel
                    .emit_cognitive(
                        "Planning",
                        &run_id,
                        agent_id,
                        &scope_refs,
                        Some(&format!("重规划 wave={wave}（世界已变，推进未完成任务）")),
                        Some(json!({ "wave": wave, "candidates": next })),
                        Some(json!({ "steps": steps.iter().map(|s| json!({ "description": s.description })).collect::<Vec<_>>() })),
                        Some(0.9),
                    )
                    .await?;
            }
            self.apply_steps(
                &steps,
                &world,
                &run_id,
                agent_id,
                &mut executed_actions,
                &mut executed_keys,
            )
            .await?;
        }

        // 7) MemoryUpdate：记录挽留经验模式（认知闭环收尾）
        self.kernel
            .emit_cognitive(
                "MemoryUpdate",
                &run_id,
                agent_id,
                &scope_refs,
                Some("记录挽留经验模式 #RetentionPattern"),
                Some(json!({ "pattern": "retention" })),
                Some(json!({ "experience": "RetentionPattern" })),
                Some(0.8),
            )
            .await?;

        // 7') Memory 沉淀（V0.5）：每个被执行动作按 `action.<name>` 沉淀为记忆（hits 累加）。
        //     下一轮 run 的 MemoryRecall 会召回，形成 Action→Memory→下次 Plan 的闭环。
        for name in &executed_actions {
            self.kernel
                .record_memory(
                    &format!("action.{name}"),
                    json!({ "experience": name, "kind": "action" }),
                )
                .await?;
        }

        // 7'') Evolution（BL-003 · REQ-017）：把本轮经验提炼为 Knowledge（Experience→Pattern→
        //     Proposal→Validation→LearnedKnowledge）。**不修改 Policy、不绕过审批**（Learn≠Permission）；
        //     run_id 作为证据注入可追溯链。幂等，可重复运行。
        //     学习层 best-effort：失败不阻断本次 Agent 运行，仅发 Reflection 事件留痕。
        if let Err(e) = self.kernel.run_evolution(&run_id).await {
            let _ = self
                .kernel
                .emit_cognitive(
                    "Reflection",
                    &run_id,
                    agent_id,
                    &scope_refs,
                    Some(&format!("Evolution 运行失败（不影响本次 run）: {e}")),
                    Some(json!({ "phase": "evolution" })),
                    Some(json!({ "error": e.to_string() })),
                    Some(0.0),
                )
                .await;
        }

        Ok(AgentRunResult {
            run_id,
            agent_id: agent_id.to_string(),
            status: RunStatus::Completed,
            goal: goal.to_string(),
            error: None,
        })
    }

    /// 调用引擎并直接发认知事件（Intent/Reasoning/Planning 等步）。
    #[allow(clippy::too_many_arguments)]
    async fn reason(
        &self,
        engine: &dyn AgentEngine,
        goal: &str,
        run_id: &str,
        agent_id: &str,
        step: &str,
        prior: Value,
        world: &WorldSnapshot,
    ) -> Result<Decision, PlannerError> {
        let ctx = Context {
            goal: goal.to_string(),
            run_id: run_id.to_string(),
            agent_id: agent_id.to_string(),
            step: step.to_string(),
            prior_output: prior,
            world: world.clone(),
        };
        call_engine(self.kernel, engine, &ctx, true).await
    }

    /// 执行一个计划步的能力（经 Capability Registry 路由到 Ontology 操作）。
    async fn execute_step(
        &self,
        step: &PlanStep,
        run_id: &str,
        agent_id: &str,
    ) -> Result<(), PlannerError> {
        match &step.capability {
            Capability::QueryObjects { object_type } => {
                self.kernel.query_objects(object_type).await?;
            }
            Capability::CreateObject {
                object_type,
                id,
                data,
            } => {
                // 幂等：先查再建；已存在则更新（upsert），避免同一库二次运行主键冲突。
                if self.kernel.get_object(id).await?.is_some() {
                    self.kernel.update_object(id, data.clone()).await?;
                } else {
                    self.kernel
                        .create_object(object_type, id, data.clone())
                        .await?;
                }
            }
            Capability::LinkRelation {
                source,
                edge,
                target,
            } => {
                // 幂等：同一边已存在则跳过，避免二次运行触发 WB2007。
                let exists = self
                    .kernel
                    .neighbors(source)
                    .await?
                    .iter()
                    .any(|r| r.relation == *edge && r.target == *target);
                if !exists {
                    self.kernel.link_relation(source, edge, target).await?;
                }
            }
            Capability::EmitEvent { event_type, refs } => {
                let r: Vec<&str> = refs.iter().map(|s| s.as_str()).collect();
                self.kernel.emit_domain_event(event_type, &r).await?;
            }
            Capability::DeleteObject { id } => {
                // 正常流程不会到达（Policy 已拒），保留为能力注册表一项。
                self.kernel.delete_object(id).await?;
            }
            Capability::Action {
                object_type,
                action,
                target_id,
                args,
            } => {
                // V0.4 完整 Action Engine：经注册表取声明式动作定义并解释执行。
                let def = self
                    .registry
                    .get(object_type, action)
                    .cloned()
                    .ok_or_else(|| {
                        PlannerError::Action(ActionError::TargetNotFound(format!(
                            "动作 {action}@{object_type} 未注册"
                        )))
                    })?;
                let mut args_map: HashMap<String, Value> = args
                    .as_object()
                    .map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
                    .unwrap_or_default();
                for hook in &self.hooks {
                    if let Err(reason) = hook.pre(object_type, action, target_id).await {
                        return Err(PlannerError::PolicyDenied(format!(
                            "闸门 {} 拒绝: {reason}",
                            hook.name()
                        )));
                    }
                }
                let mut outcome = ToolOutcome::default();
                if self.backend.supports(object_type, action) {
                    let target = self.kernel.get_object(target_id).await?.ok_or_else(|| {
                        PlannerError::Action(ActionError::TargetNotFound(target_id.clone()))
                    })?;
                    let _ = self
                        .kernel
                        .emit_cognitive(
                            "ToolBackend",
                            run_id,
                            agent_id,
                            std::slice::from_ref(target_id),
                            Some(&format!("后端 {} 执行 {action}@{object_type}", self.backend.name())),
                            Some(json!({ "backend": self.backend.name(), "action": action })),
                            None,
                            Some(0.9),
                        )
                        .await;
                    outcome = self
                        .backend
                        .execute(object_type, action, &target, &args_map)
                        .await
                        .map_err(|e: ToolError| PlannerError::Action(ActionError::Backend(e.to_string())))?;
                    args_map.extend(outcome.extra_args.clone());
                }
                if !outcome.skip_world {
                    let engine = ActionEngine::new(self.kernel);
                    match engine.invoke(&def, target_id, &args_map).await {
                        Ok(_) => {}
                        Err(e) => {
                            let _ = self
                                .kernel
                                .emit_cognitive(
                                    "ActionFailed",
                                    run_id,
                                    agent_id,
                                    std::slice::from_ref(target_id),
                                    Some(&format!("动作 {action}@{object_type} 执行失败：{e}")),
                                    Some(json!({ "action": action })),
                                    Some(json!({ "error": e.to_string() })),
                                    Some(0.0),
                                )
                                .await;
                            return Err(PlannerError::Action(e));
                        }
                    }
                }
                for hook in &self.hooks {
                    hook.post(object_type, action, target_id, &outcome).await;
                }
            }
        }
        Ok(())
    }
}

/// 调用认知引擎（统一处理 WB2004 映射）；`emit=true` 时发认知事件。
async fn call_engine(
    kernel: &RuntimeKernel,
    engine: &dyn AgentEngine,
    ctx: &Context,
    emit: bool,
) -> Result<Decision, PlannerError> {
    let dec = engine.reason(ctx).await.map_err(|e| match e {
        AgentError::Unavailable(m) | AgentError::Parse(m) => PlannerError::EngineUnavailable(m),
    })?;
    if emit {
        let refs = non_empty(dec.object_refs.clone());
        kernel
            .emit_cognitive(
                &ctx.step,
                &ctx.run_id,
                &ctx.agent_id,
                &refs,
                Some(&dec.reasoning),
                Some(json!({ "intent": dec.intent })),
                Some(dec.output.clone()),
                Some(dec.confidence),
            )
            .await?;
    }
    Ok(dec)
}

/// object_refs 非空约束（WB2006）的兜底：无具体对象时用 "world" 哨兵表示"针对整个世界"。
fn non_empty(refs: Vec<String>) -> Vec<String> {
    if refs.is_empty() {
        vec!["world".to_string()]
    } else {
        refs
    }
}

/// 生成运行 ID（run_{micros}_{seq}）。
fn new_run_id() -> String {
    static RUN_SEQ: AtomicU64 = AtomicU64::new(0);
    let micros = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros();
    format!("run_{}_{}", micros, RUN_SEQ.fetch_add(1, Ordering::SeqCst))
}

/// 预生成运行 ID（server 在触发运行前占坑状态映射）。
pub fn generate_run_id() -> String {
    new_run_id()
}

#[cfg(test)]
mod intent_filter_tests {
    use super::*;

    #[test]
    fn goal_text_wins_over_generic_intent() {
        assert!(action_matches_intent(
            "closeAccount",
            "close account",
            "Customer Retention"
        ));
        assert!(!action_matches_intent(
            "retain",
            "close account",
            "Customer Retention"
        ));
    }

    #[test]
    fn retention_goal_excludes_refund() {
        assert!(action_matches_intent("retain", "manage retention tasks", ""));
        assert!(action_matches_intent("start", "manage retention tasks", ""));
        assert!(action_matches_intent("complete", "manage retention tasks", ""));
        assert!(!action_matches_intent("refund", "manage retention tasks", ""));
    }

    #[test]
    fn refund_goal_only_allows_refund() {
        assert!(action_matches_intent("refund", "退款", "Customer Retention"));
        assert!(!action_matches_intent("retain", "退款", "Customer Retention"));
    }

    #[test]
    fn unknown_goal_does_not_filter() {
        assert!(action_matches_intent("refund", "hello world", "unrelated"));
        assert!(action_matches_intent("retain", "hello world", "unrelated"));
    }

    fn amount_param() -> ActionDef {
        ActionDef {
            name: "refund".into(),
            params: vec![crate::ir::ParamDef {
                name: "amount".into(),
                ty: ScalarType::Float,
            }],
            updates: vec![],
            creates: vec![],
            policy: crate::ir::PolicyDef::default(),
        }
    }

    #[test]
    fn extract_last_number_as_amount() {
        let args = extract_action_args(&amount_param(), "refund this customer 500", "", &[]);
        assert_eq!(args.get("amount").and_then(|v| v.as_f64()), Some(500.0));
    }

    #[test]
    fn extract_named_amount_wins() {
        let args = extract_action_args(
            &amount_param(),
            "refund customer 001 amount=80",
            "",
            &["001".into()],
        );
        assert_eq!(args.get("amount").and_then(|v| v.as_f64()), Some(80.0));
    }

    #[test]
    fn extract_skips_when_no_number() {
        let args = extract_action_args(&amount_param(), "manage retention tasks", "", &[]);
        assert!(args.is_empty());
    }

    #[test]
    fn extract_skips_object_id_as_amount() {
        let args = extract_action_args(
            &amount_param(),
            "refund 001 500",
            "",
            &["001".into()],
        );
        assert_eq!(args.get("amount").and_then(|v| v.as_f64()), Some(500.0));
    }

    #[test]
    fn resolve_prefers_goal_mentioned_ids() {
        let world = vec!["001".into(), "002".into()];
        let got = resolve_target_ids("refund 001 500", &["002".into()], &world);
        assert_eq!(got, Some(vec!["001".into()]));
    }

    #[test]
    fn resolve_falls_back_to_parsed_refs() {
        let world = vec!["001".into(), "002".into()];
        let got = resolve_target_ids(
            "manage retention tasks",
            &["001".into(), "world".into()],
            &world,
        );
        assert_eq!(got, Some(vec!["001".into()]));
    }

    #[test]
    fn resolve_unrestricted_when_nothing_named() {
        let world = vec!["001".into()];
        assert_eq!(
            resolve_target_ids("hello", &["world".into()], &world),
            None
        );
    }
}
