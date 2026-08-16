//! Context —— 时空作用域（Kernel Algebra ①：我在哪里存在）。
//!
//! P2 Lifecycle Isolation：Context 销毁终止子 Fiber，**不隐式补偿业务副作用**。
//! P3 Temporal Minimalism：只保证 `is_current(now)` 当前有效（as_of 属 Phase 4）。

use serde_json::Value;

use super::state::StateStore;
use super::{FiberId, KernelError};

/// 作用域层级（权限继承天然成立：World → Tenant → … → Action）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    World,
    Tenant,
    Session,
    Agent,
    Task,
    Action,
}

/// Context 状态机（与 Fiber 状态机完全分离）：
/// ACTIVE → DISPOSING → DISPOSED
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextState {
    Active,
    Disposing,
    Disposed,
}

/// 时空作用域。
#[derive(Debug, Clone)]
pub struct Context {
    pub id: ContextId,
    pub parent: Option<ContextId>,
    /// 同一 Entity 在不同时间段的 Context 靠它定位（D4 补充，as_of 时使用）。
    pub entity_id: String,
    pub scope: Scope,
    pub state: ContextState,
    pub valid_from: i64,
    pub valid_until: Option<i64>,
    pub state_store: StateStore,
    pub fibers: Vec<FiberId>,
}

impl Context {
    pub fn new(
        id: ContextId,
        parent: Option<ContextId>,
        scope: Scope,
        entity_id: String,
        now: i64,
    ) -> Self {
        Self {
            id,
            parent,
            entity_id,
            scope,
            state: ContextState::Active,
            valid_from: now,
            valid_until: None,
            state_store: StateStore::new(),
            fibers: Vec::new(),
        }
    }

    /// 当前有效（P3：Kernel 只保证这一件事）。
    pub fn is_current(&self, now: i64) -> bool {
        self.state != ContextState::Disposed
            && now >= self.valid_from
            && self.valid_until.map(|u| now < u).unwrap_or(true)
    }

    /// 时间窗过滤查询（软过滤；as_of 精确重建属 Phase 4）。
    pub fn valid_at(&self, t: i64) -> bool {
        t >= self.valid_from && self.valid_until.map(|u| t < u).unwrap_or(true)
    }

    /// 状态转换（合法边：Active→Disposing→Disposed）。
    pub fn transition(&mut self, to: ContextState) -> Result<(), KernelError> {
        let ok = match (self.state, to) {
            (ContextState::Active, ContextState::Disposing) => true,
            (ContextState::Disposing, ContextState::Disposed) => true,
            (a, b) if a == b => true, // 幂等
            _ => false,
        };
        if !ok {
            return Err(KernelError::InvalidTransition(
                format!("{:?}", self.state),
                format!("{:?}", to),
            ));
        }
        self.state = to;
        Ok(())
    }

    /// Context 作用域状态（KV）。
    pub fn set(&mut self, key: impl Into<String>, value: Value) {
        self.state_store.set(key, value);
    }
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.state_store.get(key)
    }
}

/// Context id（进程内自增；持久化后与存储 id 对齐）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ContextId(pub u64);

impl std::fmt::Display for ContextId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ctx_{}", self.0)
    }
}
