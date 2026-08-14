//! Fiber —— 世界中的生命体（Kernel Algebra ②：谁正在经历时间）。
//!
//! Task 是语义对象，Fiber 是运行实例：同一 Task 可派生无限 Fiber。
//! Fiber 持有 EffectLedger（补偿是自己的责任，Runtime 不猜测怎么回滚）。

use serde_json::Value;

use super::effect::EffectLedger;
use super::relation::Relation;
use super::{ContextId, KernelError};

/// Fiber 状态机（最终锁定版，见 runtime-kernel-v2.md §4）：
///
/// ```text
/// CREATED → STARTING → READY → RUNNING ⇄ WAITING / PAUSED
///                                    │
///                        ┌───────────┴───────────┐
///                        ↓                       ↓
///                    COMPLETED                FAILED
///                                                ↓
///                                          COMPENSATING
///                                          /          \
///                                         ↓            ↓
///                                   COMPENSATED    ESCALATED（人工介入）
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FiberState {
    Created,
    Starting,
    Ready,
    Running,
    Waiting,
    Paused,
    Completed,
    Failed,
    Compensating,
    Compensated,
    Escalated,
}

impl FiberState {
    pub fn as_str(&self) -> &'static str {
        match self {
            FiberState::Created => "Created",
            FiberState::Starting => "Starting",
            FiberState::Ready => "Ready",
            FiberState::Running => "Running",
            FiberState::Waiting => "Waiting",
            FiberState::Paused => "Paused",
            FiberState::Completed => "Completed",
            FiberState::Failed => "Failed",
            FiberState::Compensating => "Compensating",
            FiberState::Compensated => "Compensated",
            FiberState::Escalated => "Escalated",
        }
    }
}

/// 等待模式：WAITING 的 Fiber 声明它等待哪类 Relation（恢复信号统一经 relate 进入）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WaitPattern {
    /// None = 任意 source。
    pub source: Option<String>,
    pub predicate: String,
    /// None = 任意 target。
    pub target: Option<String>,
}

impl WaitPattern {
    /// 精确 predicate 匹配 + source/target 通配或精确匹配。
    pub fn matches(&self, r: &Relation) -> bool {
        r.predicate == self.predicate
            && self.source.as_ref().map(|s| s == &r.source).unwrap_or(true)
            && self.target.as_ref().map(|t| t == &r.target).unwrap_or(true)
    }
}

/// 运行实例。
#[derive(Debug, Clone)]
pub struct Fiber {
    pub id: FiberId,
    pub context: ContextId,
    pub parent: Option<FiberId>,
    pub state: FiberState,
    /// 语义任务描述（Task Definition 引用；Task 本身属上层）。
    pub task: String,
    pub input: Value,
    pub output: Option<Value>,
    /// 已提交 Effect 账本（P1 补偿依据，按提交顺序）。
    pub ledger: EffectLedger,
    pub relations: Vec<super::RelationId>,
    /// WAITING 时等待的恢复信号（relate 匹配则自动 resume）。
    pub wait_for: Option<WaitPattern>,
    pub created_at: i64,
}

impl Fiber {
    pub fn new(
        id: FiberId,
        context: ContextId,
        parent: Option<FiberId>,
        task: &str,
        input: Value,
        now: i64,
    ) -> Self {
        Self {
            id,
            context,
            parent,
            state: FiberState::Created,
            task: task.to_string(),
            input,
            output: None,
            ledger: EffectLedger::new(),
            relations: Vec::new(),
            wait_for: None,
            created_at: now,
        }
    }

    /// 状态转换（只允许合法边；非法转换返回 KernelError，不 panic）。
    pub fn transition(&mut self, to: FiberState) -> Result<(), KernelError> {
        let from = self.state;
        let ok = match (from, to) {
            (FiberState::Created, FiberState::Starting) => true,
            (FiberState::Starting, FiberState::Ready) => true,
            (FiberState::Ready, FiberState::Running) => true,
            (FiberState::Running, FiberState::Waiting) => true,
            (FiberState::Running, FiberState::Paused) => true,
            (FiberState::Running, FiberState::Completed) => true,
            (FiberState::Running, FiberState::Failed) => true,
            (FiberState::Waiting, FiberState::Running) => true,
            (FiberState::Paused, FiberState::Running) => true,
            (FiberState::Failed, FiberState::Compensating) => true,
            (FiberState::Compensating, FiberState::Compensated) => true,
            (FiberState::Compensating, FiberState::Escalated) => true,
            (a, b) if a == b => true, // 幂等
            _ => false,
        };
        if !ok {
            return Err(KernelError::InvalidTransition(
                from.as_str().to_string(),
                to.as_str().to_string(),
            ));
        }
        self.state = to;
        Ok(())
    }
}

/// Fiber id。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FiberId(pub u64);

impl std::fmt::Display for FiberId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "fiber_{}", self.0)
    }
}
