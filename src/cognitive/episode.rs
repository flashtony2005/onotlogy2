//! Episode —— Execution Memory（Phase 8 · G5）。
//!
//! Memory 不直接成为 Runtime 的核心依赖，而是 **Runtime Event 的上层消费者**：
//! `Event Journal → Episode（goal/plan/action/result/failure/compensation/approval 聚合）`。
//!
//! Episode 是 Learning 的输入（S10）：同种失败多次 → Pattern → Improvement Proposal
//! （只提案，不自动修改生产系统）。

use crate::kernel::fiber::FiberState;
use crate::kernel::{FiberId, Kernel};

/// Episode 结局。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EpisodeOutcome {
    Completed,
    Failed,
    Compensated,
    Escalated,
    Cancelled,
    /// 尚未终结（运行/等待/中断中）。
    Incomplete,
}

impl EpisodeOutcome {
    fn of(state: FiberState) -> Self {
        match state {
            FiberState::Completed => EpisodeOutcome::Completed,
            FiberState::Failed => EpisodeOutcome::Failed,
            FiberState::Compensated => EpisodeOutcome::Compensated,
            FiberState::Escalated => EpisodeOutcome::Escalated,
            FiberState::Cancelled => EpisodeOutcome::Cancelled,
            _ => EpisodeOutcome::Incomplete,
        }
    }
}

/// 一条执行片段（Execution Memory 单元）。
#[derive(Debug, Clone)]
pub struct Episode {
    pub fiber: Option<FiberId>,
    /// 目标（fiber.task）。
    pub goal: String,
    /// 已提交动作（effect.committed → action 语义名）。
    pub actions: Vec<String>,
    /// 补偿动作（compensation.completed → action）。
    pub compensations: Vec<String>,
    /// 审批次数（approval.granted + approval.rejected）。
    pub approvals: u64,
    pub outcome: EpisodeOutcome,
}

impl Episode {
    /// 该片段是否失败信号（Learning 关注：失败/补偿/升级）。
    pub fn is_failure_signal(&self) -> bool {
        matches!(
            self.outcome,
            EpisodeOutcome::Failed | EpisodeOutcome::Compensated | EpisodeOutcome::Escalated
        )
    }
}

/// 从 Kernel 事实层聚合 Episode（只读；不修改任何状态）。
pub fn episodes_from_events(kernel: &Kernel) -> Vec<Episode> {
    kernel
        .all_fibers()
        .map(|f| {
            let fiber_id = f.id;
            let mut actions: Vec<String> = Vec::new();
            let mut compensations: Vec<String> = Vec::new();
            let mut approvals: u64 = 0;
            for ev in kernel.events() {
                if ev.fiber != Some(fiber_id) {
                    continue;
                }
                match ev.ty.as_str() {
                    "effect.committed" => {
                        if let Some(effect) = ev
                            .payload
                            .get("effect")
                            .and_then(|v| v.as_str())
                            .and_then(|s| {
                                s.strip_prefix("fx_")
                                    .and_then(|n| n.parse::<u64>().ok())
                                    .map(crate::kernel::EffectId)
                            })
                            .and_then(|id| kernel.effect(id))
                        {
                            actions.push(effect.action.clone());
                        }
                    }
                    "compensation.completed" => {
                        if let Some(action) = ev.payload.get("action").and_then(|v| v.as_str()) {
                            compensations.push(action.to_string());
                        }
                    }
                    "approval.granted" | "approval.rejected" => approvals += 1,
                    _ => {}
                }
            }
            Episode {
                fiber: Some(fiber_id),
                goal: f.task.clone(),
                actions,
                compensations,
                approvals,
                outcome: EpisodeOutcome::of(f.state),
            }
        })
        .collect()
}
