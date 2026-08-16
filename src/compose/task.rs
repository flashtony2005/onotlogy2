//! Task —— 组合层（对未来 Effect 序列的声明；Task ≠ 执行）。
//!
//! Task = goal + allowed capabilities（+ 期望关系属 Phase 3 规划扩展）。
//! 执行 Task = spawn Fiber，由 TaskExecutor 驱动：
//! Fiber poll（决策，Phase 3 接 Planner/LLM）→ Policy 校验 → 审批挂起/Effect 提交/Relation 投影。

use serde_json::Value;

use super::capability::Capability;
use super::policy::PolicyVerdict;
use crate::kernel::fiber::{FiberId, FiberState, WaitPattern};
use crate::kernel::relation::Relation;
use crate::kernel::{CompensationEngine, ContextId, Effect, Kernel, KernelError};
use crate::world_exec::{CompensationExecutor as _, EffectExecutor as _};

/// 期望关系（D5 目标函数）：Task 完成时世界应达成的状态。
/// Agent 不信任自己的执行——重新观察世界确认目标达成。
#[derive(Debug, Clone)]
pub struct ExpectedRelation {
    /// None = 任意 source。
    pub source: Option<String>,
    pub predicate: String,
    /// None = 任意 target。
    pub target: Option<String>,
}

impl ExpectedRelation {
    pub fn matches(&self, r: &crate::kernel::Relation) -> bool {
        r.predicate == self.predicate
            && self.source.as_ref().map(|s| s == &r.source).unwrap_or(true)
            && self.target.as_ref().map(|t| t == &r.target).unwrap_or(true)
    }
}

/// 任务声明（语义对象；每个执行实例 = 一个 Fiber）。
#[derive(Debug, Clone)]
pub struct Task {
    pub id: String,
    pub goal: String,
    /// 该任务允许使用的能力（Task 边界约束）。
    pub allowed_capabilities: Vec<String>,
    /// 完成判定依据（全部满足 → COMPLETED）。
    pub expected_relations: Vec<ExpectedRelation>,
}

impl Task {
    pub fn new(id: impl Into<String>, goal: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            goal: goal.into(),
            allowed_capabilities: Vec::new(),
            expected_relations: Vec::new(),
        }
    }

    pub fn allowing(mut self, caps: &[&str]) -> Self {
        self.allowed_capabilities = caps.iter().map(|s| s.to_string()).collect();
        self
    }

    /// 声明完成时世界应达成的关系（(predicate, target)；source 任意）。
    pub fn expecting(mut self, rels: &[(&str, &str)]) -> Self {
        self.expected_relations = rels
            .iter()
            .map(|(p, t)| ExpectedRelation {
                source: None,
                predicate: p.to_string(),
                target: Some(t.to_string()),
            })
            .collect();
        self
    }
}

/// 单步执行结果。
#[derive(Debug, Clone, PartialEq)]
pub enum StepOutcome {
    /// 已提交 Effect（+ 可选 Relation 投影）。
    Committed,
    /// 挂起等待审批（Fiber 已 WAITING；relate(Approval) 后自动恢复）。
    Waiting,
    /// 业务拒绝（Effect 未提交；Fiber 已 FAILED，补偿链为空）。
    Denied(String),
}

/// Task 执行器：驱动 Fiber 沿「Policy → 审批 → Effect → 执行 → 投影」链路推进。
pub struct TaskExecutor<'k> {
    kernel: &'k mut Kernel,
    /// 世界效应声明（Phase 5：注入后 Effect 真实改变 World；None = 旧 projection 模拟）。
    /// 执行器在方法内临时构造（借用 kernel），避免双重 &mut。
    effect_defs: Option<Vec<crate::compose::defs::RuntimeDefinition>>,
    /// D4 分离：true 时补偿走 RelationCompensationExecutor；false 回退 CompensationEngine。
    use_relation_compensation: bool,
}

impl<'k> TaskExecutor<'k> {
    pub fn new(kernel: &'k mut Kernel) -> Self {
        Self {
            kernel,
            effect_defs: None,
            use_relation_compensation: false,
        }
    }

    /// 注入世界效应声明（RelationEffectExecutor 生效：Effect 真实改变 World）。
    pub fn with_effect_defs(mut self, defs: Vec<crate::compose::defs::RuntimeDefinition>) -> Self {
        self.effect_defs = Some(defs);
        self
    }

    /// D4 分离：补偿走 RelationCompensationExecutor（撤销真实关系）。
    pub fn with_relation_compensation(mut self) -> Self {
        self.use_relation_compensation = true;
        self
    }

    /// 在指定 Context 派生 Task Fiber（Task 声明 → 运行实例）。
    pub fn spawn(
        &mut self,
        ctx: ContextId,
        task: &Task,
        input: Value,
    ) -> Result<FiberId, KernelError> {
        let f = self.kernel.spawn(ctx, None, &task.id, input)?;
        self.kernel.advance(f, FiberState::Starting)?;
        self.kernel.advance(f, FiberState::Ready)?;
        self.kernel.advance(f, FiberState::Running)?;
        Ok(f)
    }

    /// 执行一个决策步：能力 + 参数。
    ///
    /// 链路：allowed 校验 → Policy 校验（Deny / RequireApproval / Allow）
    /// → emit(Effect) → relate(投影)。Fiber 必须 RUNNING。
    /// 常规 step（无 EvolutionGate：审批一律人工）。
    pub fn step(
        &mut self,
        fiber: FiberId,
        capability: &Capability,
        args: Value,
    ) -> Result<StepOutcome, KernelError> {
        self.step_inner(fiber, capability, args, false, None)
    }

    /// Phase 6：带 EvolutionGate 判定的 step（gate_pass 由调用方预计算）。
    /// 只影响 RequireApproval 分支：通过 → 受控替代人工审批（审计进事实层）；
    /// 否则回退人工审批（WAIT——不能替代审批 ≠ 权限拒绝，I3）。
    pub fn step_with_gate(
        &mut self,
        fiber: FiberId,
        capability: &Capability,
        args: Value,
        gate_pass: bool,
    ) -> Result<StepOutcome, KernelError> {
        self.step_inner(fiber, capability, args, gate_pass, None)
    }

    /// Phase 8 · G1：幂等 step——同 (context, key) 只产生一次业务效果；
    /// 重放仅记录 `effect.idempotent_replay` 审计事件（S8）。
    pub fn step_idempotent(
        &mut self,
        fiber: FiberId,
        capability: &Capability,
        args: Value,
        key: &str,
    ) -> Result<StepOutcome, KernelError> {
        self.step_inner(fiber, capability, args, false, Some(key))
    }

    fn step_inner(
        &mut self,
        fiber: FiberId,
        capability: &Capability,
        args: Value,
        gate_pass: bool,
        idempotency_key: Option<&str>,
    ) -> Result<StepOutcome, KernelError> {
        let f = self
            .kernel
            .fiber(fiber)
            .ok_or(KernelError::FiberNotFound(fiber))?;
        if f.state != FiberState::Running {
            return Err(KernelError::FiberNotRunning("step".into()));
        }

        // Phase 8 · G1：幂等重放（同 key 已执行 → 不产生新业务效果，仅审计）
        if let Some(key) = idempotency_key {
            let ctx = self
                .kernel
                .fiber(fiber)
                .map(|f| f.context)
                .ok_or(KernelError::FiberNotFound(fiber))?;
            if self.kernel.idempotency_hit(ctx, key).is_some() {
                self.kernel.record_idempotent_replay(fiber, key);
                return Ok(StepOutcome::Committed);
            }
        }

        // 0) 必填参数校验（schema；缺参 = 业务拒绝）
        for p in &capability.required_params {
            if args.get(p).is_none() {
                let reason = format!("缺少必填参数: {p}");
                self.kernel
                    .record_policy_denied(fiber, &capability.name, &reason);
                self.kernel.advance(fiber, FiberState::Failed)?;
                return Ok(StepOutcome::Denied(reason));
            }
        }

        // 1) Policy 校验
        match capability.policy.check(&args) {
            PolicyVerdict::Deny(reason) => {
                self.kernel
                    .record_policy_denied(fiber, &capability.name, &reason);
                self.kernel.advance(fiber, FiberState::Failed)?;
                return Ok(StepOutcome::Denied(reason));
            }
            PolicyVerdict::RequireApproval => {
                // 已审批（relate 恢复时置位）→ 放行；否则挂起等待 Approval 关系
                let approved = self
                    .kernel
                    .fiber(fiber)
                    .map(|f| f.approved)
                    .unwrap_or(false);
                if !approved {
                    if gate_pass {
                        // Phase 6：EvolutionGate 受控替代人工审批（7 条件 AND 已由
                        // 调用方预计算）——审计进事实层，可观察/可撤销（D5）。
                        self.kernel.record_evolution_gate(
                            fiber,
                            &capability.name,
                            &capability.action,
                        );
                    } else {
                        // Gate 失败 = 不能替代审批 → 回退人工（WAIT），不是 DENY（I3）
                        self.kernel.wait_on(
                            fiber,
                            WaitPattern {
                                source: None,
                                predicate: "Approval".into(),
                                target: None,
                            },
                        )?;
                        self.kernel.advance(fiber, FiberState::Waiting)?;
                        // Phase 8 · G2：审批挂起审计（Suspension Point 事件）
                        self.kernel
                            .record_approval_requested(fiber, &capability.name);
                        return Ok(StepOutcome::Waiting);
                    }
                }
            }
            PolicyVerdict::Allow => {}
        }

        // 2) 提交 Effect（能力声明的动作 + 可逆性 + 补偿）
        let mut effect = Effect::new(
            capability.action.clone(),
            args.clone(),
            capability.reversibility,
            capability.inverse.clone(),
            self.kernel.now(),
        );
        effect.fiber = fiber; // executor 以 fiber 定位作用域（emit 也会重设）

        // 3) 真实世界变化（Phase 5：Effect ≠ Projection）
        //    有执行器 → executor 先执行（写 World，执行成功才 emit 记录）；
        //    无执行器 → 回退 capability.projection（旧模拟，兼容既有契约）。
        if let Some(defs) = &self.effect_defs {
            let mut ex = crate::world_exec::RelationEffectExecutor::new(self.kernel, defs.clone());
            let result = ex
                .execute(&effect)
                .map_err(|e| KernelError::Closed(format!("Effect 执行失败: {e}")))?;
            // D4：撤销清单写入 inverse payload——补偿时知道撤销哪些关系
            if let Some(inv) = &mut effect.inverse {
                inv.payload = serde_json::json!({ "relations": result.relations });
            }
            let eid = self.kernel.emit(fiber, effect)?;
            let ctx = self
                .kernel
                .fiber(fiber)
                .map(|f| f.context)
                .ok_or(KernelError::FiberNotFound(fiber))?;
            if let Some(key) = idempotency_key {
                self.kernel.record_idempotency(ctx, key, eid);
            }
            for (source, pred, target) in result.relations {
                self.kernel
                    .relate(Relation::new(ctx, source, pred, target, self.kernel.now()))?;
            }
        } else {
            let eid = self.kernel.emit(fiber, effect)?;
            if let Some(key) = idempotency_key {
                let ctx = self
                    .kernel
                    .fiber(fiber)
                    .map(|f| f.context)
                    .ok_or(KernelError::FiberNotFound(fiber))?;
                self.kernel.record_idempotency(ctx, key, eid);
            }
            if let Some((pred, target)) = &capability.projection {
                let source = args
                    .get("target")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&capability.name)
                    .to_string();
                let ctx = self
                    .kernel
                    .fiber(fiber)
                    .map(|f| f.context)
                    .ok_or(KernelError::FiberNotFound(fiber))?;
                self.kernel
                    .relate(Relation::new(ctx, source, pred, target, self.kernel.now()))?;
            }
        }

        Ok(StepOutcome::Committed)
    }

    /// D5 完成判定：expected_relations 全部满足 → Fiber COMPLETED；否则维持 RUNNING。
    pub fn check_complete(&mut self, fiber: FiberId, task: &Task) -> Result<bool, KernelError> {
        let ctx = self
            .kernel
            .fiber(fiber)
            .map(|f| f.context)
            .ok_or(KernelError::FiberNotFound(fiber))?;
        let all = task.expected_relations.iter().all(|e| {
            self.kernel
                .relations()
                .any(|r| r.context == ctx && r.is_current(self.kernel.now()) && e.matches(r))
        });
        if all {
            self.kernel.advance(fiber, FiberState::Completed)?;
        }
        Ok(all)
    }

    /// Fiber 失败 → 逆序补偿。
    /// D4 分离（use_relation_compensation）→ RelationCompensationExecutor 撤销真实关系；
    /// 否则回退 CompensationEngine（P1 Compensator 路径）。
    pub fn compensate(&mut self, fiber: FiberId) -> Result<(), KernelError> {
        if !self.use_relation_compensation {
            return CompensationEngine::new(self.kernel).compensate(fiber);
        }
        // 逆序遍历已提交可补偿 Effect 的 inverse（payload 携带撤销清单）
        let inverses: Vec<(String, Value)> = {
            let f = self
                .kernel
                .fiber(fiber)
                .ok_or(KernelError::FiberNotFound(fiber))?;
            f.ledger
                .reverse_order()
                .filter_map(|id| self.kernel.effect(*id))
                .filter(|e| e.is_compensatable())
                .filter_map(|e| e.inverse.clone())
                .map(|c| (c.action, c.payload))
                .collect()
        };
        self.kernel.advance(fiber, FiberState::Compensating)?;
        let mut ex = crate::world_exec::RelationCompensationExecutor::new(self.kernel);
        for (action, payload) in inverses {
            let comp = crate::kernel::Compensation { action, payload };
            ex.compensate(&comp)
                .map_err(|e| KernelError::Closed(format!("补偿失败（升级人工）: {e}")))?;
        }
        self.kernel.advance(fiber, FiberState::Compensated)
    }

    /// 便捷：把 RUNNING fiber 标记失败（业务动作执行失败时调用）。
    pub fn fail(&mut self, fiber: FiberId, reason: &str) -> Result<(), KernelError> {
        let _ = reason;
        self.kernel.advance(fiber, FiberState::Failed)
    }
}
