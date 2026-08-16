//! AgentLoop —— 认知闭环（D1：Agent 长期 Fiber；每次任务 = Task Fiber 的 poll 循环）。
//!
//! 每轮 poll：Observe → Decide → Execute（Policy→审批→Effect→投影）→ Observe Result
//! 完成判定（D5）：expected_relations 全部满足（Agent 不信任自己的执行）。
//! Waiting（审批挂起）：返回 LoopOutcome::Waiting，relate(Approval) 恢复后 `resume_until_done` 续跑。

use serde_json::{Value, json};

use super::engine::{AgentView, CognitiveEngine};
use super::memory::Memory;
use super::observe::observe;
use crate::compose::Service;
use crate::compose::task::{StepOutcome, Task, TaskExecutor};
use crate::kernel::fiber::{FiberId, FiberState};
use crate::kernel::{ContextId, Kernel, KernelError};
use crate::trace::{TraceEvent, TraceKind};

/// 一轮循环的产出。
#[derive(Debug, Clone, PartialEq)]
pub enum LoopOutcome {
    /// 继续（内部循环用；run_until_done 不对外返回）。
    Continue,
    /// 目标达成（expected_relations 满足或无更多决策）。
    Completed,
    /// 审批挂起（relate 恢复后 resume_until_done 续跑）。
    Waiting,
    /// 业务拒绝（Effect 未提交；已走空补偿）。
    Denied(String),
    /// 运行失败（无决策且目标未达成；或执行失败已补偿）。
    Failed(String),
}

impl LoopOutcome {
    /// G6：结局 → (run 状态, 是否需人工裁决)。
    /// Waiting = 运行中但挂起待批；Denied = 人工拒绝致失败；
    /// Failed = 运行失败（无人工）；Continue 不应作为最终结局，映射 Running。
    pub fn to_run_status(&self) -> (crate::session::run::RunStatus, bool) {
        use crate::session::run::RunStatus;
        match self {
            LoopOutcome::Completed => (RunStatus::Completed, false),
            LoopOutcome::Waiting => (RunStatus::Running, true),
            LoopOutcome::Denied(_) => (RunStatus::Failed, true),
            LoopOutcome::Failed(_) => (RunStatus::Failed, false),
            LoopOutcome::Continue => (RunStatus::Running, false),
        }
    }
}

/// Agent：长期认知实体（记忆 + 引擎 + 作用域）。
pub struct Agent {
    pub id: String,
    /// Agent 所在 Context（世界作用域；任务在其下派生）。
    pub context: ContextId,
    pub memory: Memory,
}

impl Agent {
    pub fn new(id: impl Into<String>, context: ContextId) -> Self {
        Self {
            id: id.into(),
            context,
            memory: Memory::new(),
        }
    }
}

/// 认知闭环驱动器（借用 kernel/agent/services/engine）。
pub struct AgentLoop<'k> {
    kernel: &'k mut Kernel,
    agent: &'k mut Agent,
    services: &'k [Service],
    engine: &'k dyn CognitiveEngine,
    /// Phase 5：世界效应声明（注入后 Effect 真实改变 World，D5 重投影闭环）。
    effect_defs: Option<Vec<crate::ontology::RuntimeDefinition>>,
    /// Phase 6：演化引擎（认知状态；None = 不采集不 Gate）。
    evolution: Option<&'k mut crate::evolution2::EvolutionEngine>,
    /// Phase 7：规则级经验分账（rule 粒度；与 action 级并存——双尺度）。
    rule_stats: Option<&'k mut crate::rule::RuleStatsStore>,
    /// M1b G1：认知 Trace 事件缓冲（观察层；调用方持有并负责落库——
    /// 与 V1 run_goal 的 wb_trace_event 同源，UI buildTimeline/deriveState 依赖）。
    trace: Option<&'k mut Vec<TraceEvent>>,
    /// M1b G2：挂起审批请求缓冲（交互层；调用方持有，CLI/HTTP 适配器据此裁决）。
    approvals: Option<&'k mut Vec<super::approval::ApprovalRequest>>,
    /// Trace/审批事件的 run_id 绑定（with_trace / with_approvals 时指定）。
    run_id: Option<String>,
    /// 已提交动作数（MemoryUpdate 的 actions 统计）。
    action_count: usize,
}

impl<'k> AgentLoop<'k> {
    pub fn new(
        kernel: &'k mut Kernel,
        agent: &'k mut Agent,
        services: &'k [Service],
        engine: &'k dyn CognitiveEngine,
    ) -> Self {
        Self {
            kernel,
            agent,
            services,
            engine,
            effect_defs: None,
            evolution: None,
            rule_stats: None,
            trace: None,
            approvals: None,
            run_id: None,
            action_count: 0,
        }
    }

    /// 注入世界效应声明（RelationEffectExecutor 生效：Effect 真实改变 World）。
    pub fn with_effect_defs(mut self, defs: Vec<crate::ontology::RuntimeDefinition>) -> Self {
        self.effect_defs = Some(defs);
        self
    }

    /// 注入演化引擎（Phase 6：outcome 采集 + Gate 判定）。
    pub fn with_evolution(mut self, ev: &'k mut crate::evolution2::EvolutionEngine) -> Self {
        self.evolution = Some(ev);
        self
    }

    /// 注入规则级经验分账（Phase 7：rule_stats，双尺度）。
    pub fn with_rule_stats(mut self, stats: &'k mut crate::rule::RuleStatsStore) -> Self {
        self.rule_stats = Some(stats);
        self
    }

    /// 开启认知 Trace 事件流（G1）：事件写入调用方持有的 sink，
    /// resume 续跑时传入同一 sink 即可保持单次 run 的事件连续性。
    pub fn with_trace(mut self, run_id: impl Into<String>, sink: &'k mut Vec<TraceEvent>) -> Self {
        self.trace = Some(sink);
        self.run_id = Some(run_id.into());
        self
    }

    /// 开启挂起审批注册（G2）：Waiting 时把类型化 ApprovalRequest 推入调用方 sink，
    /// CLI/HTTP 适配器据此裁决（resolve/expire）。run_id 与 with_trace 共用同一字段。
    pub fn with_approvals(
        mut self,
        run_id: impl Into<String>,
        sink: &'k mut Vec<super::approval::ApprovalRequest>,
    ) -> Self {
        self.approvals = Some(sink);
        self.run_id = Some(run_id.into());
        self
    }

    /// 推一条认知事件（观察层；空世界 object_refs 兜底 "world"，与 V1 同源）。
    fn emit_trace(
        &mut self,
        event_type: &str,
        refs: &[String],
        reasoning: Option<&str>,
        input: Option<Value>,
        output: Option<Value>,
        confidence: Option<f64>,
    ) {
        let Some(sink) = self.trace.as_deref_mut() else {
            return;
        };
        sink.push(TraceEvent {
            id: crate::trace::new_trace_id(),
            kind: TraceKind::Cognitive,
            event_type: event_type.to_string(),
            object_refs: if refs.is_empty() {
                vec!["world".to_string()]
            } else {
                refs.to_vec()
            },
            run_id: self.run_id.clone(),
            agent_id: Some(self.agent.id.clone()),
            reasoning: reasoning.map(|s| s.to_string()),
            input,
            output,
            confidence,
            timestamp: crate::trace::now_ts(),
        });
    }

    /// 单次事件（扫描 sink 去重：resume 续跑不重复 Goal/OntologyQuery/MemoryUpdate）。
    fn emit_trace_once(
        &mut self,
        event_type: &str,
        refs: &[String],
        reasoning: Option<&str>,
        input: Option<Value>,
        output: Option<Value>,
        confidence: Option<f64>,
    ) {
        let already = self
            .trace
            .as_deref()
            .is_some_and(|sink| sink.iter().any(|e| e.event_type == event_type));
        if !already {
            self.emit_trace(event_type, refs, reasoning, input, output, confidence);
        }
    }

    /// 视图绑定对象（观察到的关系端点去重；可解释性根基）。
    fn refs_of(view: &AgentView) -> Vec<String> {
        let mut refs: Vec<String> = view
            .related
            .iter()
            .flat_map(|(s, _p, t)| [s.clone(), t.clone()])
            .collect();
        refs.sort();
        refs.dedup();
        refs
    }

    fn task_executor(&mut self) -> TaskExecutor<'_> {
        let tx = TaskExecutor::new(self.kernel);
        match &self.effect_defs {
            Some(defs) => tx
                .with_effect_defs(defs.clone())
                .with_relation_compensation(),
            None => tx,
        }
    }

    fn find_capability(&self, name: &str) -> Option<&'k crate::compose::Capability> {
        self.services.iter().find_map(|s| s.capability(name))
    }

    /// 一轮：Observe → Decide → Execute。
    pub fn run_once(&mut self, fiber: FiberId, task: &Task) -> Result<LoopOutcome, KernelError> {
        // 1) 观察（D5：重新确认世界状态）
        let caps: Vec<String> = self
            .services
            .iter()
            .flat_map(|s| s.capability_names().cloned().collect::<Vec<_>>())
            .collect();
        let view: AgentView = observe(
            self.kernel,
            self.agent.context,
            &task.goal,
            &self.agent.memory,
            &caps,
            self.evolution.as_deref(),
        );
        let refs = Self::refs_of(&view);

        // G1：首轮认知事件（Goal / OntologyQuery；resume 不重复）
        self.emit_trace_once(
            "Goal",
            &refs,
            Some(&format!("目标解析: {}", task.goal)),
            Some(json!({ "goal": task.goal })),
            None,
            None,
        );
        self.emit_trace_once(
            "OntologyQuery",
            &refs,
            Some("查询世界模型中的相关对象"),
            Some(json!({ "goal": task.goal })),
            Some(json!({ "objects": refs })),
            Some(0.9),
        );

        // 2) 决策（认知引擎）
        let decision = match self.engine.decide(&view) {
            Ok(Some(d)) => d,
            Ok(None) => {
                // 无更多动作 → D5 完成判定
                let done = self.task_executor().check_complete(fiber, task)?;
                return Ok(if done {
                    self.emit_trace_once(
                        "MemoryUpdate",
                        &refs,
                        Some(&format!(
                            "记录本轮经验模式（执行 {} 个动作）",
                            self.action_count
                        )),
                        Some(
                            json!({ "pattern": "action-execution", "actions": self.action_count }),
                        ),
                        Some(json!({ "experience": "ExecutedActions" })),
                        Some(0.8),
                    );
                    LoopOutcome::Completed
                } else {
                    // 目标未达成且无决策 → 失败（无 Effect，不补偿）
                    self.kernel.advance(fiber, FiberState::Failed)?;
                    LoopOutcome::Failed("无决策且期望关系未达成".into())
                });
            }
            Err(e) => {
                // 认知引擎错误 → 失败
                self.kernel.advance(fiber, FiberState::Failed)?;
                self.emit_trace(
                    "ActionFailed",
                    &refs,
                    Some("认知引擎决策失败"),
                    None,
                    Some(json!({ "error": e.to_string() })),
                    Some(0.0),
                );
                return Ok(LoopOutcome::Failed(e.to_string()));
            }
        };

        // G1：决策事件（Reasoning / Intent / Planning）
        self.emit_trace(
            "Reasoning",
            &refs,
            Some(&format!("基于观察决策执行能力 {}", decision.capability)),
            Some(json!({ "goal": task.goal, "candidates": refs })),
            Some(json!({ "capability": decision.capability })),
            Some(decision.confidence),
        );
        self.emit_trace(
            "Intent",
            &refs,
            None,
            None,
            Some(json!({ "intent": decision.capability })),
            Some(decision.confidence),
        );
        self.emit_trace(
            "Planning",
            &refs,
            Some(&format!("计划执行能力 {}", decision.capability)),
            None,
            Some(json!({ "capability": decision.capability, "args": decision.args })),
            Some(decision.confidence),
        );

        // 3) 执行（能力查找 → TaskExecutor step：Policy→审批→Effect→投影）
        let Some(cap) = self.find_capability(&decision.capability) else {
            self.kernel.advance(fiber, FiberState::Failed)?;
            self.emit_trace(
                "ActionFailed",
                &refs,
                Some("未知能力"),
                Some(json!({ "capability": decision.capability })),
                Some(json!({ "error": format!("未知能力: {}", decision.capability) })),
                Some(0.0),
            );
            // 埋点（D1：outcome 分账；按能力名归因）
            if let Some(ev) = &mut self.evolution {
                ev.record(&decision.capability, crate::evolution2::OutcomeKind::Failed);
            }
            return Ok(LoopOutcome::Failed(format!(
                "未知能力: {}",
                decision.capability
            )));
        };
        // Phase 6：预计算 EvolutionGate（只影响 RequireApproval 分支）
        let gate_pass = self
            .evolution
            .as_ref()
            .map(|ev| ev.gate(&cap.action, cap.reversibility))
            .unwrap_or(false);
        // G2：步骤参数副本（step 会 move args；审批请求注册需要）
        let step_args = decision.args.clone();
        let outcome = self
            .task_executor()
            .step_with_gate(fiber, cap, decision.args, gate_pass)?;
        match outcome {
            StepOutcome::Committed => {
                self.action_count += 1;
                // G1：策略判定 + 执行事件
                self.emit_trace(
                    "PolicyCheck",
                    &refs,
                    Some("ALLOW"),
                    Some(json!({ "capability": cap.name })),
                    Some(json!("Allow")),
                    Some(0.95),
                );
                self.emit_trace(
                    "Action",
                    &refs,
                    Some(&cap.name),
                    Some(json!({ "capability": cap.name })),
                    Some(json!("Success")),
                    Some(0.9),
                );
                // 沉淀记忆（D3：Effect 成功后认知状态更新）
                self.agent
                    .memory
                    .record(cap.action.clone(), json!({ "capability": cap.name }));
                // 埋点（D1：Success 分账；D4：规则级分账并存）
                if let Some(ev) = &mut self.evolution {
                    ev.record(&cap.action, crate::evolution2::OutcomeKind::Success);
                }
                if let (Some(stats), Some(rid)) = (&mut self.rule_stats, &decision.rule) {
                    stats.record(rid.clone(), crate::evolution2::OutcomeKind::Success);
                }
                Ok(LoopOutcome::Continue)
            }
            StepOutcome::Waiting => {
                // G1：审批挂起事件（UI humanApproval=Required 判定源）
                self.emit_trace(
                    "PolicyCheck",
                    &refs,
                    Some("REQUIRE_APPROVAL"),
                    Some(json!({ "capability": cap.name })),
                    Some(json!("RequireApproval")),
                    Some(0.5),
                );
                self.emit_trace(
                    "PendingApproval",
                    &refs,
                    Some("等待人工审批"),
                    Some(json!({ "capability": cap.name, "args": step_args })),
                    Some(json!({ "pending": true })),
                    Some(0.5),
                );
                // G2：类型化审批请求注册（CLI/HTTP 适配器据此裁决）
                if let (Some(sink), Some(context)) = (
                    self.approvals.as_deref_mut(),
                    self.kernel.fiber(fiber).map(|f| f.context),
                ) {
                    sink.push(super::approval::ApprovalRequest {
                        id: crate::trace::new_trace_id(),
                        run_id: self.run_id.clone().unwrap_or_default(),
                        context,
                        fiber,
                        capability: cap.name.clone(),
                        action: cap.action.clone(),
                        args: step_args.clone(),
                        created_at: self.kernel.now(),
                    });
                }
                Ok(LoopOutcome::Waiting)
            }
            StepOutcome::Denied(reason) => {
                // G1：拒绝事件（deriveState → Failed 判定源；与 V1 同源零置信）
                self.emit_trace(
                    "PolicyCheck",
                    &refs,
                    Some(&format!("DENY: {reason}")),
                    Some(json!({ "capability": cap.name })),
                    Some(json!("Deny")),
                    Some(0.0),
                );
                // 已 FAILED（空补偿链）→ 明确走补偿收尾
                let compensated = self.task_executor().compensate(fiber).is_ok()
                    && self
                        .kernel
                        .fiber(fiber)
                        .map(|f| f.state == FiberState::Compensated)
                        .unwrap_or(false);
                // 埋点（D1：Denied / Compensated 分账）
                if let Some(ev) = &mut self.evolution {
                    ev.record(&cap.action, crate::evolution2::OutcomeKind::Denied);
                    if compensated {
                        ev.record(&cap.action, crate::evolution2::OutcomeKind::Compensated);
                    }
                }
                if let (Some(stats), Some(rid)) = (&mut self.rule_stats, &decision.rule) {
                    stats.record(rid.clone(), crate::evolution2::OutcomeKind::Denied);
                    if compensated {
                        stats.record(rid.clone(), crate::evolution2::OutcomeKind::Compensated);
                    }
                }
                Ok(LoopOutcome::Denied(reason))
            }
        }
    }

    /// 完整任务循环：spawn Task Fiber → 循环至 Completed/Waiting/Denied/Failed。
    pub fn run_until_done(
        &mut self,
        ctx: ContextId,
        task: &Task,
        input: Value,
    ) -> Result<LoopOutcome, KernelError> {
        let fiber = self.task_executor().spawn(ctx, task, input)?;
        self.resume_until_done(fiber, task)
    }

    /// 续跑（审批恢复后调用：relate(Approval) → resume → 再入循环）。
    pub fn resume_until_done(
        &mut self,
        fiber: FiberId,
        task: &Task,
    ) -> Result<LoopOutcome, KernelError> {
        loop {
            match self.run_once(fiber, task)? {
                LoopOutcome::Continue => continue,
                out => return Ok(out),
            }
        }
    }
}
