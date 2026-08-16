//! Scheduler —— Fiber 的「时间」实现（什么时候运行/等待/恢复/销毁）。
//!
//! Phase 1 最小实现：
//! - ready 队列视图（RUNNING 的 fiber）
//! - `poll_once`：驱动一个 RUNNING fiber 执行一次 poll 步骤（observe/reason/decide 由上层
//!   注入——P4 认知边界：LLM 推理是 poll 步骤，不是 Kernel 的一部分）
//! - WAITING 恢复已由 `Kernel::relate` 模式匹配自动完成（本模块只提供查询视图）
//!
//! 完整抢占式调度（时间片、优先级、暂停/恢复策略）属后续阶段。

use serde_json::Value;

use super::fiber::FiberState;
use super::{FiberId, Kernel, KernelError};

/// 一次 poll 步骤的产出：Fiber 下一步该做什么。
#[derive(Debug, Clone)]
pub enum PollStep {
    /// 提交一个 Effect（调用方随后 kernel.emit）。
    Emit(Value),
    /// 等待某类 Relation（调用方随后 kernel.wait_on + advance(Waiting)）。
    Wait,
    /// 完成（调用方随后 advance(Completed)）。
    Complete(Value),
    /// 失败（调用方随后 advance(Failed)，触发 P1 补偿）。
    Fail(String),
}

/// 调度器视图（借用 kernel）。
pub struct Scheduler<'k> {
    kernel: &'k Kernel,
}

impl<'k> Scheduler<'k> {
    pub fn new(kernel: &'k Kernel) -> Self {
        Self { kernel }
    }

    /// RUNNING 的 fiber（可运行集合）。
    pub fn running(&self) -> Vec<FiberId> {
        self.kernel
            .all_fibers()
            .filter(|f| f.state == FiberState::Running)
            .map(|f| f.id)
            .collect()
    }

    /// WAITING 的 fiber（等待恢复信号）。
    pub fn waiting(&self) -> Vec<FiberId> {
        self.kernel
            .all_fibers()
            .filter(|f| f.state == FiberState::Waiting)
            .map(|f| f.id)
            .collect()
    }

    /// 执行一次 poll 步骤（P4：推理/决策由调用方提供，Kernel 只驱动状态）。
    /// 返回 PollStep 后由调用方落地（emit/wait/complete/fail）。
    pub fn poll_once(
        &self,
        fiber: FiberId,
        poll: impl FnOnce(&Value) -> PollStep,
    ) -> Result<PollStep, KernelError> {
        let f = self
            .kernel
            .fiber(fiber)
            .ok_or(KernelError::FiberNotFound(fiber))?;
        if f.state != FiberState::Running {
            return Err(KernelError::FiberNotRunning("poll".into()));
        }
        Ok(poll(&f.input))
    }
}
