//! WorkerPolicy —— Worker 层策略与审批（fail-closed：未配置 = DENY，不是 TRY）。

use std::collections::HashMap;

use serde_json::Value;

/// Worker 级策略判定。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkerVerdict {
    Allow,
    RequireApproval,
    Deny(String),
}

/// 规则种类（独立标记——deny 不依赖表达式求值，杜绝意外放行）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RuleKind {
    Allow,
    RequireApproval,
    Deny,
}

/// Worker Policy（capability → 规则；未配置 → Deny，Fail Closed）。
#[derive(Debug, Clone, Default)]
pub struct WorkerPolicy {
    rules: HashMap<String, RuleKind>,
}

impl WorkerPolicy {
    pub fn new() -> Self {
        Self::default()
    }

    /// allow：免审批放行。
    pub fn allow(mut self, capability: &str) -> Self {
        self.rules.insert(capability.to_string(), RuleKind::Allow);
        self
    }

    /// require_approval：挂起等待人工审批。
    pub fn require_approval(mut self, capability: &str) -> Self {
        self.rules
            .insert(capability.to_string(), RuleKind::RequireApproval);
        self
    }

    /// deny：显式拒绝。
    pub fn deny(mut self, capability: &str) -> Self {
        self.rules.insert(capability.to_string(), RuleKind::Deny);
        self
    }

    /// 判定（fail-closed：未配置 = Deny）。
    pub fn check(&self, capability: &str, _args: &Value) -> WorkerVerdict {
        match self.rules.get(capability) {
            None => WorkerVerdict::Deny(format!("未配置策略（fail-closed）: {capability}")),
            Some(RuleKind::Allow) => WorkerVerdict::Allow,
            Some(RuleKind::RequireApproval) => WorkerVerdict::RequireApproval,
            Some(RuleKind::Deny) => WorkerVerdict::Deny(format!("策略拒绝: {capability}")),
        }
    }
}
