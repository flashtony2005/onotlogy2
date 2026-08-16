//! Run 生命周期语义（M1b · G6）—— run 状态枚举、ID 生成、Fiber 状态映射。
//!
//! V1 时期 `RunStatus` 与 `generate_run_id` 住在 planner 里；planner 退役（M1d）后
//! 它们必须留在更稳定的家。run 是 Session 的生命周期概念（Session Event Log 的
//! run_id 轴），故落在 session 层——只依赖 std，不依赖任何上层。
//!
//! Fiber 是 Kernel 的执行载体（L0），run 是观察/审计轴（Session 层）：
//! `fiber_to_run_status` 是两者之间的唯一映射点，语义：
//! - Waiting / Paused 归 Running（审批挂起 = 运行中；human_approval 由 LoopOutcome 标记）；
//! - 补偿三态（Compensating/Compensated/Escalated）与 Cancelled 归 Failed（run 已失败）；
//! - Interrupted 是崩溃恢复中间态，必须与 Failed 区分（v0_8 语义）。

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::kernel::FiberState;

/// run 状态（持久化语义；UI deriveState 与表状态同源）。
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

/// Fiber 执行态 → run 状态（全映射，无遗漏；见模块注释）。
pub fn fiber_to_run_status(state: FiberState) -> RunStatus {
    match state {
        FiberState::Created
        | FiberState::Starting
        | FiberState::Ready
        | FiberState::Running
        | FiberState::Waiting
        | FiberState::Paused => RunStatus::Running,
        FiberState::Completed => RunStatus::Completed,
        FiberState::Interrupted => RunStatus::Interrupted,
        FiberState::Failed
        | FiberState::Compensating
        | FiberState::Compensated
        | FiberState::Escalated
        | FiberState::Cancelled => RunStatus::Failed,
    }
}

/// 当前 unix 微秒（业务时间戳；run id 组成）。
fn now_micros() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros()
        .to_string()
}

/// 生成运行 ID（server 在触发运行前占坑状态映射；全局单调唯一）。
pub fn generate_run_id() -> String {
    static RUN_SEQ: AtomicU64 = AtomicU64::new(0);
    format!(
        "run_{}_{}",
        now_micros(),
        RUN_SEQ.fetch_add(1, Ordering::SeqCst)
    )
}
