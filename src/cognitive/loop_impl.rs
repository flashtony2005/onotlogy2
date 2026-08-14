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
        );

        // 2) 决策（认知引擎）
        let decision = match self.engine.decide(&view) {
            Ok(Some(d)) => d,
            Ok(None) => {
                // 无更多动作 → D5 完成判定
                let done = TaskExecutor::new(self.kernel).check_complete(fiber, task)?;
                return Ok(if done {
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
                return Ok(LoopOutcome::Failed(e.to_string()));
            }
        };

        // 3) 执行（能力查找 → TaskExecutor step：Policy→审批→Effect→投影）
        let Some(cap) = self.find_capability(&decision.capability) else {
            self.kernel.advance(fiber, FiberState::Failed)?;
            return Ok(LoopOutcome::Failed(format!(
                "未知能力: {}",
                decision.capability
            )));
        };
        match TaskExecutor::new(self.kernel).step(fiber, cap, decision.args)? {
            StepOutcome::Committed => {
                // 沉淀记忆（D3：Effect 成功后认知状态更新）
                self.agent
                    .memory
                    .record(cap.action.clone(), json!({ "capability": cap.name }));
                Ok(LoopOutcome::Continue)
            }
            StepOutcome::Waiting => Ok(LoopOutcome::Waiting),
            StepOutcome::Denied(reason) => {
                // 已 FAILED（空补偿链）→ 明确走补偿收尾
                let _ = TaskExecutor::new(self.kernel).compensate(fiber);
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
        let fiber = TaskExecutor::new(self.kernel).spawn(ctx, task, input)?;
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
