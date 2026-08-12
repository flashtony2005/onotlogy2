//! Planner 闭环编排（对应 REQ-011 / prd-agent-integration.md 4.3）
//!
//! OpenAIP 自有核心：**Goal → LLMGoalParser → Planner → PlanStep → Capability → Execute**。
//! Pi 仅作 Cognitive Engine（经 `AgentEngine::reason` 提供 NLU/推理），本模块拥有编排与决策。
//! 每步产出 cognitive `TraceEvent` 并绑定 `object_refs`（可解释性根基）；Policy 拒 → WB2005；
//! 认知引擎不可用 → WB2004。

use crate::action::{ActionEngine, ActionError, ActionRegistry};
use crate::agent::{AgentEngine, AgentError, Context, Decision, WorldSnapshot};
use crate::ir::{ActionDef, CompareOp, ValueExpr};
use crate::runtime::RuntimeKernel;
use crate::storage::StorageError;
use async_trait::async_trait;
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{self, Write};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::oneshot;

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
}

impl RunStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            RunStatus::Running => "Running",
            RunStatus::Completed => "Completed",
            RunStatus::Failed => "Failed",
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
        if let Some(v) = self.cache.lock().unwrap().get(&key) {
            return *v;
        }
        let ans = prompt(&format!(
            "⚠️ 需 Human Approval：{}？[y/N] ",
            step.description
        ));
        self.cache.lock().unwrap().insert(key, ans);
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

/// 待审批请求状态（与 timeline / HTTP 响应对齐）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "PascalCase")]
pub enum ApprovalState {
    Pending,
    Approved,
    Rejected,
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
        self.requests.lock().unwrap().insert(
            req.req_id.clone(),
            ApprovalEntry {
                request: req,
                tx: Some(tx),
            },
        );
        rx
    }

    /// 裁决：发信号唤醒挂起的 Planner，更新状态；返回是否存在该请求（已裁决/不存在 → false）。
    /// 裁决：发信号唤醒挂起的 Planner，更新状态；返回是否成功裁决。
    /// 请求不存在或已裁决（tx 已被取走）→ 返回 false（HTTP 端点映射 404）。
    pub fn resolve(&self, req_id: &str, approve: bool) -> bool {
        let mut g = self.requests.lock().unwrap();
        let Some(e) = g.get_mut(req_id) else {
            return false;
        };
        // 已裁决（tx 已被取走）→ 拒绝二次裁决
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
        }
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
        // 挂起等待 HTTP 端点裁决（approve=true 放行 / false=WB2005 终止）
        let decision = rx.await.unwrap_or(false);
        // 发 ApprovalDecision 事件（供 timeline / WS 实时反映结果）
        let _ = self
            .kernel
            .emit_cognitive(
                "ApprovalDecision",
                &self.run_id,
                &self.agent_id,
                &step.object_refs,
                Some(&format!(
                    "审批结果: {}（reqId={}）",
                    if decision { "批准" } else { "拒绝" },
                    req_id
                )),
                Some(json!({
                    "reqId": req_id,
                    "decision": if decision { "Approved" } else { "Rejected" }
                })),
                Some(json!(if decision { "Approved" } else { "Rejected" })),
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
    /// - `require_approval=true` 默认 Deny（WB2005），须经 `approver` 放行。
    pub async fn check_action(
        step: &PlanStep,
        def: Option<&ActionDef>,
        approver: &dyn PolicyApprover,
        target: Option<&Value>,
        args: &HashMap<String, Value>,
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
        if def.policy.require_approval && !approver.approve(step).await {
            return PolicyDecision::Deny {
                reason: format!("动作 {} 需 Human Approval（WB2005）", def.name),
            };
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
            .and_then(|t| t.get(f))
            .cloned()
            .unwrap_or(Value::Null),
    }
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
    /// 动作注册表（V0.4）：对象自有动作的声明式来源；空 → legacy 兜底。
    registry: ActionRegistry,
    /// Policy 审批器（Human Approval 闸门）；默认 `AutoApprover { allow: false }`（拒，等待人工）。
    approver: Arc<dyn PolicyApprover>,
}

impl<'a> Planner<'a> {
    /// 无动作注册表（legacy 兜底：硬编码挽留计划）。
    pub fn new(kernel: &'a RuntimeKernel) -> Self {
        Self {
            kernel,
            registry: ActionRegistry::empty(),
            approver: Arc::new(AutoApprover { allow: false }),
        }
    }

    /// 带动作注册表（V0.4 完整 Action Engine）：候选对象经声明式动作执行。
    pub fn with_actions(kernel: &'a RuntimeKernel, registry: ActionRegistry) -> Self {
        Self {
            kernel,
            registry,
            approver: Arc::new(AutoApprover { allow: false }),
        }
    }

    /// 注入 Policy 审批器（如服务侧自动批准 / CLI 交互式审批）。
    pub fn with_approver(mut self, approver: Arc<dyn PolicyApprover>) -> Self {
        self.approver = approver;
        self
    }

    /// 候选对象选择：注册表非空 → 声明了 actions 的对象；否则 legacy 仅 Customer。
    /// 均排除派生对象（如 RetentionTask），二次运行保持幂等。
    fn select_candidates(&self, world: &WorldSnapshot) -> Vec<String> {
        world
            .objects
            .iter()
            .filter(|o| {
                let t = o.get("type").and_then(|v| v.as_str()).unwrap_or("");
                if self.registry.is_empty() {
                    t == "Customer"
                } else {
                    self.registry.has_actions(t)
                }
            })
            .filter_map(|o| o.get("id").and_then(|x| x.as_str()).map(|s| s.to_string()))
            .collect()
    }

    /// 计划构建：候选对象声明了动作 → `Capability::Action`（经 ActionEngine 执行）；
    /// 否则 legacy 挽留计划（无 ODL 时保持 V0.3 行为）。
    fn build_plan(&self, world: &WorldSnapshot, candidates: &[String]) -> Vec<PlanStep> {
        if self.registry.is_empty() {
            return build_retention_plan(candidates);
        }
        let mut steps = Vec::new();
        for c in candidates {
            let otype = world
                .objects
                .iter()
                .find(|o| o.get("id").and_then(|v| v.as_str()) == Some(c.as_str()))
                .and_then(|o| o.get("type").and_then(|v| v.as_str()))
                .unwrap_or("");
            for act in self.registry.actions_of(otype) {
                steps.push(PlanStep {
                    capability: Capability::Action {
                        object_type: otype.to_string(),
                        action: act.name.clone(),
                        target_id: c.clone(),
                        args: json!({}),
                    },
                    object_refs: vec![c.clone()],
                    description: format!("对 {c} 执行声明式动作 {}", act.name),
                });
            }
        }
        steps
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
        //    V0.4：若注册表非空，候选 = 声明了 actions 的对象（避免把派生对象再当候选）；
        //    legacy 兜底：仅 Customer（保持 V0.3 行为幂等）。
        let candidates = self.select_candidates(&world);
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
        // 5') 计划构建：注册表驱动的声明式动作 → Capability::Action；否则 legacy 挽留计划。
        let steps = self.build_plan(&world, &candidates);
        self.kernel
            .emit_cognitive(
                "Planning",
                &run_id,
                agent_id,
                &scope_refs,
                Some(&plan_dec.reasoning),
                Some(json!({ "candidates": candidates })),
                Some(json!({ "steps": steps.iter().map(|s| json!({ "description": s.description, "object_refs": s.object_refs })).collect::<Vec<_>>() })),
                Some(plan_dec.confidence),
            )
            .await?;

        // 6) PolicyCheck + Action：逐步经 Policy 校验后执行（最小闭环）
        let mut executed_actions: Vec<String> = Vec::new();
        for step in &steps {
            // 声明式动作结合其自带 PolicyDef（前置条件 + 审批闸门）校验；
            // 其余能力走通用 Policy（破坏性动作默认拒）。
            let decision = match &step.capability {
                Capability::Action {
                    object_type,
                    action,
                    target_id,
                    args,
                } => {
                    let def = self.registry.get(object_type, action);
                    let target = world
                        .objects
                        .iter()
                        .find(|o| o.get("id").and_then(|v| v.as_str()) == Some(target_id.as_str()));
                    let args_map: HashMap<String, Value> = args
                        .as_object()
                        .map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
                        .unwrap_or_default();
                    Policy::check_action(step, def, self.approver.as_ref(), target, &args_map).await
                }
                _ => Policy::check(step),
            };
            match decision {
                PolicyDecision::Deny { reason } => {
                    self.kernel
                        .emit_cognitive(
                            "PolicyCheck",
                            &run_id,
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
                            &run_id,
                            agent_id,
                            &step.object_refs,
                            Some("ALLOW"),
                            Some(json!({ "capability": step.description })),
                            Some(json!("Allow")),
                            Some(0.95),
                        )
                        .await?;
                    self.execute_step(step).await?;
                    if let Capability::Action { action, .. } = &step.capability {
                        executed_actions.push(action.clone());
                    }
                    self.kernel
                        .emit_cognitive(
                            "Action",
                            &run_id,
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
        self.kernel.run_evolution(&run_id).await?;

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
    async fn execute_step(&self, step: &PlanStep) -> Result<(), PlannerError> {
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
                let args_map: HashMap<String, Value> = args
                    .as_object()
                    .map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
                    .unwrap_or_default();
                let engine = ActionEngine::new(self.kernel);
                engine.invoke(&def, target_id, &args_map).await?;
            }
        }
        Ok(())
    }
}

/// 由候选客户对象生成挽留计划步（OpenAIP 自有规划：每客户创建挽留任务并建边）。
fn build_retention_plan(candidates: &[String]) -> Vec<PlanStep> {
    let mut steps = Vec::new();
    for c in candidates {
        let task_id = format!("task_{c}");
        steps.push(PlanStep {
            capability: Capability::CreateObject {
                object_type: "RetentionTask".to_string(),
                id: task_id.clone(),
                data: json!({ "title": "客户挽留计划", "customer": c }),
            },
            object_refs: vec![c.clone(), task_id.clone()],
            description: format!("为 {c} 创建挽留任务 {task_id}"),
        });
        steps.push(PlanStep {
            capability: Capability::LinkRelation {
                source: c.clone(),
                edge: "needs_retention".to_string(),
                target: task_id,
            },
            object_refs: vec![c.clone()],
            description: format!("建立 {c} -needs_retention-> 挽留任务"),
        });
    }
    steps
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
