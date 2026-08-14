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

/// Task 执行器：驱动 Fiber 沿「Policy → 审批 → Effect → 投影」链路推进。
pub struct TaskExecutor<'k> {
    kernel: &'k mut Kernel,
}

impl<'k> TaskExecutor<'k> {
    pub fn new(kernel: &'k mut Kernel) -> Self {
        Self { kernel }
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
    pub fn step(
        &mut self,
        fiber: FiberId,
        capability: &Capability,
        args: Value,
    ) -> Result<StepOutcome, KernelError> {
        let f = self
            .kernel
            .fiber(fiber)
            .ok_or(KernelError::FiberNotFound(fiber))?;
        if f.state != FiberState::Running {
            return Err(KernelError::FiberNotRunning("step".into()));
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
                    self.kernel.wait_on(
                        fiber,
                        WaitPattern {
                            source: None,
                            predicate: "Approval".into(),
                            target: None,
                        },
                    )?;
                    self.kernel.advance(fiber, FiberState::Waiting)?;
                    return Ok(StepOutcome::Waiting);
                }
            }
            PolicyVerdict::Allow => {}
        }

        // 2) 提交 Effect（能力声明的动作 + 可逆性 + 补偿）
        let effect = Effect::new(
            capability.action.clone(),
            args.clone(),
            capability.reversibility,
            capability.inverse.clone(),
            self.kernel.now(),
        );
        self.kernel.emit(fiber, effect)?;

        // 3) Relation 投影（副作用的事实记录）
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

    /// Fiber 失败 → P1 自动逆序补偿（能力声明的 inverse 生效）。
    pub fn compensate(&mut self, fiber: FiberId) -> Result<(), KernelError> {
        CompensationEngine::new(self.kernel).compensate(fiber)
    }

    /// 便捷：把 RUNNING fiber 标记失败（业务动作执行失败时调用）。
    pub fn fail(&mut self, fiber: FiberId, reason: &str) -> Result<(), KernelError> {
        let _ = reason;
        self.kernel.advance(fiber, FiberState::Failed)
    }
}
