//! Effect —— Kernel 层的副作用代数与审计账本。
//!
//! 本模块只描述“发生了什么”以及如何补偿：Effect 保存动作、输入、可逆性、
//! 反向动作和生命周期状态；EffectLedger 保存 Fiber 内的提交顺序。
//! Kernel 不执行具体 World mutation，真实世界变化由 `crate::world_exec` 的
//! `EffectExecutor` 负责，错误恢复由 `CompensationEngine` / `CompensationExecutor`
//! 负责。这样 Effect 记录与世界执行策略保持独立。
//!
//! P1 Failure Compensation：Fiber 失败自动逆序补偿；补偿失败升级人工（ESCALATED）。
//! 统一叫 Compensation 而非 rollback——现实世界很多操作无法真正回滚。

use serde_json::Value;

use super::FiberId;

/// 可逆性分类（衔接 Policy：`irreversible ⇒ 可要求审批`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reversibility {
    /// 可直接撤销（DB 事务语义）。
    Reversible,
    /// 需反向业务操作（Create Order → Cancel Order）。
    Compensatable,
    /// 重放安全（Send API request）。
    Idempotent,
    /// 不可逆（Send Email）——失败不补偿，仅标记。
    Irreversible,
}

/// Effect 状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectState {
    /// 已提交（emit 即提交；Phase 1 前由上层执行副作用）。
    Committed,
    /// 已补偿。
    Compensated,
    /// 补偿失败（升级人工的依据）。
    CompensationFailed,
}

/// 补偿定义：指向一个反向动作（Phase 1 由补偿执行器生成新的 Effect）。
#[derive(Debug, Clone)]
pub struct Compensation {
    pub action: String,
    pub payload: Value,
}

/// 一次可审计副作用。
#[derive(Debug, Clone)]
pub struct Effect {
    pub id: EffectId,
    pub fiber: FiberId,
    pub action: String,
    pub input: Value,
    pub output: Option<Value>,
    pub reversibility: Reversibility,
    pub inverse: Option<Compensation>,
    pub status: EffectState,
    pub timestamp: i64,
}

impl Effect {
    pub fn new(
        action: impl Into<String>,
        input: Value,
        reversibility: Reversibility,
        inverse: Option<Compensation>,
        now: i64,
    ) -> Self {
        // id/fiber 由 Kernel::emit 分配
        Self {
            id: EffectId(0),
            fiber: FiberId(0),
            action: action.into(),
            input,
            output: None,
            reversibility,
            inverse,
            status: EffectState::Committed,
            timestamp: now,
        }
    }

    /// 该 Effect 是否进入补偿序列（Irreversible 不补偿，仅标记）。
    pub fn is_compensatable(&self) -> bool {
        matches!(
            self.reversibility,
            Reversibility::Reversible | Reversibility::Compensatable
        )
    }
}

/// Effect 账本：按提交顺序记录，供补偿逆序遍历（P1）。
#[derive(Debug, Clone, Default)]
pub struct EffectLedger {
    committed: Vec<EffectId>,
}

impl EffectLedger {
    pub fn new() -> Self {
        Self::default()
    }

    /// 记录一次提交（emit 时由 Kernel 调用）。
    pub fn record(&mut self, id: EffectId) {
        self.committed.push(id);
    }

    /// 已提交顺序（正序）。
    pub fn committed(&self) -> &[EffectId] {
        &self.committed
    }

    /// 逆序（补偿顺序：C⁻¹ → B⁻¹ → A⁻¹）。
    pub fn reverse_order(&self) -> impl Iterator<Item = &EffectId> {
        self.committed.iter().rev()
    }
}

/// Effect id。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EffectId(pub u64);

impl std::fmt::Display for EffectId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "fx_{}", self.0)
    }
}
