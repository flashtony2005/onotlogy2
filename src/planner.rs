//! Planner 闭环编排（对应 REQ-011 / prd-agent-integration.md 4.3）
//!
//! OpenAIP 自有核心：**Goal → LLMGoalParser → Planner → PlanStep → Capability → Execute**。
//! Pi 仅作 Cognitive Engine（经 `AgentEngine::reason` 提供 NLU/推理），本模块拥有编排与决策。
//! 每步产出 cognitive `TraceEvent` 并绑定 `object_refs`（可解释性根基）；Policy 拒 → WB2005；
//! 认知引擎不可用 → WB2004。

use crate::agent::{AgentEngine, AgentError, Context, Decision, WorldSnapshot};
use crate::runtime::RuntimeKernel;
use crate::storage::StorageError;
use serde_json::{Value, json};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

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
}

/// 计划步：一个能力 + 绑定对象 + 描述。
#[derive(Debug, Clone)]
pub struct PlanStep {
    pub capability: Capability,
    pub object_refs: Vec<String>,
    pub description: String,
}

/// Policy 引擎：对 Action（PlanStep）做约束与审批。MVP 仅做基础规则；拒 → WB2005。
pub struct Policy;

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
}

impl<'a> Planner<'a> {
    pub fn new(kernel: &'a RuntimeKernel) -> Self {
        Self { kernel }
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

        // 3) OntologyQuery：从世界模型找出目标作用对象（MVP：全部客户）
        let candidates: Vec<String> = world
            .objects
            .iter()
            .filter_map(|o| o.get("id").and_then(|x| x.as_str()).map(|s| s.to_string()))
            .collect();
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
        let steps = build_retention_plan(&candidates);
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
        for step in &steps {
            match Policy::check(step) {
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
                self.kernel
                    .create_object(object_type, id, data.clone())
                    .await?;
            }
            Capability::LinkRelation {
                source,
                edge,
                target,
            } => {
                self.kernel.link_relation(source, edge, target).await?;
            }
            Capability::EmitEvent { event_type, refs } => {
                let r: Vec<&str> = refs.iter().map(|s| s.as_str()).collect();
                self.kernel.emit_domain_event(event_type, &r).await?;
            }
            Capability::DeleteObject { id } => {
                // 正常流程不会到达（Policy 已拒），保留为能力注册表一项。
                self.kernel.delete_object(id).await?;
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
