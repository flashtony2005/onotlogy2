//! Rule —— 行为即数据（Phase 7：Self-Evolving Agent 第一层）。
//!
//! 第一原则：**Self-Evolution must follow the same execution path as
//! ordinary world evolution.** 行为演化改变的是「Decision 如何产生」，
//! 不是「Decision 如何执行」。
//!
//! 分形：`Agent → Agent` 尺度——决策机制成为 Agent 自身可观察、可评价、
//! 提出修改并受控采用的对象（与 RuntimeDefinition 同构的 ABI）。

pub mod engine;
pub mod evolution;
pub mod expr;

pub use engine::RuleEngine;
pub use evolution::{
    AdoptionLogEntry, BehaviorChange, BehaviorProposal, ProposalEvidence, RuleError, RuleEvolver,
    RuleStats, RuleStatsStore, ValidationCheck, ValidationContext, ValidationStatus,
    propose_changes, validate, validate_into,
};
pub use expr::ConditionExpr;

/// 行为规则（可序列化 ABI——行为从代码逻辑变为可演化数据）。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BehaviorRule {
    /// 稳定 id。
    pub id: String,
    /// 能力名（如 "customer.retain"）。
    pub capability: String,
    /// 决策参数。
    pub args: serde_json::Value,
    /// 结构化决策条件（可演化，非闭包）。
    pub condition: ConditionExpr,
    /// false = 已停用（可被 Disable 提案）。
    pub enabled: bool,
    /// 选择顺序（可被 Promote 提案提升）。
    pub priority: i64,
    /// 每次 adopt 递增——并发演化一致性（stale 防护）。
    pub version: u64,
}

impl BehaviorRule {
    pub fn new(
        id: impl Into<String>,
        capability: impl Into<String>,
        args: serde_json::Value,
        condition: ConditionExpr,
    ) -> Self {
        Self {
            id: id.into(),
            capability: capability.into(),
            args,
            condition,
            enabled: true,
            priority: 0,
            version: 1,
        }
    }

    pub fn with_priority(mut self, p: i64) -> Self {
        self.priority = p;
        self
    }

    /// 规则快照指纹（evidence；主校验以 version 为准）。
    pub fn snapshot_hash(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        serde_json::to_string(self).unwrap_or_default().hash(&mut h);
        h.finish()
    }
}

/// RuleStore：行为规则集合（可序列化——行为 ABI 的载体）。
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RuleStore {
    pub rules: Vec<BehaviorRule>,
}

impl RuleStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_rule(mut self, r: BehaviorRule) -> Self {
        self.rules.push(r);
        self
    }

    pub fn rule(&self, id: &str) -> Option<&BehaviorRule> {
        self.rules.iter().find(|r| r.id == id)
    }

    pub fn enabled_count(&self) -> usize {
        self.rules.iter().filter(|r| r.enabled).count()
    }
}
