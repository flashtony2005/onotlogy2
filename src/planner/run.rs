use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::action::{ActionEngine, ActionError, ActionRegistry};
use crate::agent::{AgentEngine, AgentError, Context, Decision, WorldSnapshot};
use crate::runtime::RuntimeKernel;
use crate::tool_backend::{ActionHook, NullBackend, ToolBackend, ToolOutcome};
use serde_json::{Value, json};

use super::approval::{PolicyApprover, RunAuditor};
use super::intent::action_matches_intent;
use super::intent::action_preconditions_hold;
use super::intent::extract_action_args;
use super::intent::object_in_scope;
use super::intent::resolve_target_ids;
use super::intent::world_object_ids;
use super::policy::step_key;
use super::util::non_empty;
use super::*;

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

/// 规划产物（Thinker 第一步的输出；执行由调用方选择 ActionEngine 或 Worker ABI）。
#[derive(Debug, Clone)]
pub struct PlannedRun {
    pub world: WorldSnapshot,
    /// capability 计划（PlanStep.capability.name = Worker 词汇）。
    pub steps: Vec<PlanStep>,
    pub scope_refs: Vec<String>,
    pub root_scope: Option<Vec<String>>,
    pub goal_parsed: Goal,
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
        executed_actions: &mut Vec<(String, String)>,
        executed_keys: &mut HashSet<String>,
    ) -> Result<(), PlannerError> {
        for step in steps {
            let decision =
                match &step.capability {
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
                        // H3：学习知识按 `(object_type, action)` 限定键控，杜绝跨对象类型串扰
                        // （类型 A 上积累的置信度/成功数/失败数不得放行类型 B 的同名动作）。
                        let qualified = format!("{object_type}.{action}");
                        let learned =
                            self.kernel.evolution_state().await.ok().and_then(|st| {
                                st.learned.into_iter().find(|l| l.action == qualified)
                            });
                        let failure_count = self
                            .kernel
                            .count_action_failed(&qualified)
                            .await
                            .unwrap_or(0);
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
                    if let Capability::Action {
                        object_type,
                        action,
                        ..
                    } = &step.capability
                    {
                        executed_actions.push((object_type.clone(), action.clone()));
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

    /// 规划（Thinker 第一步）：Goal → Intent → OntologyQuery → Reasoning → Planning →
    /// 注册表驱动的 PlanStep（capability 计划）。**只规划不执行**——执行由调用方决定
    /// （ActionEngine 或 Worker ABI）。V2 桥：`plan()` 的产物可喂给 WorkerOrchestrator。
    pub async fn plan(
        &self,
        engine: &dyn AgentEngine,
        goal: &str,
        agent_id: &str,
        run_id: &str,
    ) -> Result<PlannedRun, PlannerError> {
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
        let root_scope =
            resolve_target_ids(goal, &goal_parsed.object_refs, &world_object_ids(&world));
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
        let steps = self.build_plan(&world, &candidates, goal, &goal_parsed.intent);
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

        Ok(PlannedRun {
            world,
            steps,
            scope_refs,
            root_scope,
            goal_parsed,
        })
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
        // 规划（V2：与执行分离——plan() 产物可经 ActionEngine 或 Worker ABI 执行）
        let planned = self.plan(engine, goal, agent_id, &run_id).await?;
        let PlannedRun {
            mut world,
            mut steps,
            mut scope_refs,
            root_scope,
            goal_parsed,
        } = planned;

        // 世界因动作改变后重快照、重规划，直到无新适用动作（一轮目标推到稳态）。
        const MAX_PLAN_WAVES: usize = 8;
        let mut executed_actions: Vec<(String, String)> = Vec::new();
        let mut executed_keys: HashSet<String> = HashSet::new();
        // 设计决策（无 Saga 补偿）：中途失败/被拒不回滚已执行步骤的副作用。
        // 依据：① 本地优先单写者场景，失败后人类可直接经 UI/CLI 修正世界；
        //       ② 所有副作用均有 TraceEvent 留痕可审计；③ 动作幂等（upsert/去重），
        //       重跑不会放大错误。引入补偿（Saga/undo）会显著增加复杂度，与
        //       AGENTS.md「选择最简单的实现」冲突，故显式不做。
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

        // 7) MemoryUpdate：记录本轮经验模式（认知闭环收尾；文案通用化，不绑定「挽留」领域）
        let update_note = format!("记录本轮经验模式（执行 {} 个动作）", executed_actions.len());
        self.kernel
            .emit_cognitive(
                "MemoryUpdate",
                &run_id,
                agent_id,
                &scope_refs,
                Some(&update_note),
                Some(json!({ "pattern": "action-execution", "actions": executed_actions.len() })),
                Some(json!({ "experience": "ExecutedActions" })),
                Some(0.8),
            )
            .await?;

        // 7') Memory 沉淀（V0.5）：每个被执行动作按 `action.<object_type>.<name>` 沉淀为记忆
        //     （hits 累加；对象类型并入标签，Evolution 据此按 (类型, 动作) 隔离知识，防跨类型串扰）。
        //     下一轮 run 的 MemoryRecall 会召回，形成 Action→Memory→下次 Plan 的闭环。
        for (otype, name) in &executed_actions {
            self.kernel
                .record_memory(
                    &format!("action.{otype}.{name}"),
                    json!({ "experience": format!("{otype}.{name}"), "kind": "action" }),
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
                            Some(&format!(
                                "后端 {} 执行 {action}@{object_type}",
                                self.backend.name()
                            )),
                            Some(json!({ "backend": self.backend.name(), "action": action })),
                            None,
                            Some(0.9),
                        )
                        .await;
                    outcome = match self
                        .backend
                        .execute(object_type, action, &target, &args_map)
                        .await
                    {
                        Ok(o) => o,
                        Err(e) => {
                            // H2：后端失败同样埋 ActionFailed（限定键），「无失败」闸门不可见不得漏记。
                            let _ = self
                                .kernel
                                .emit_cognitive(
                                    "ActionFailed",
                                    run_id,
                                    agent_id,
                                    std::slice::from_ref(target_id),
                                    Some(&format!("后端 {action}@{object_type} 执行失败：{e}")),
                                    Some(json!({ "action": format!("{object_type}.{action}") })),
                                    Some(json!({ "error": e.to_string() })),
                                    Some(0.0),
                                )
                                .await;
                            return Err(PlannerError::Action(ActionError::Backend(e.to_string())));
                        }
                    };
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
                                    // H3：失败计数按 (object_type, action) 限定，跨类型不串扰。
                                    Some(json!({ "action": format!("{object_type}.{action}") })),
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
