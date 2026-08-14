//! Capability —— 组合层能力声明（描述 Fiber 可以执行什么，本身不执行）。
//!
//! Capability 与 Effect 的关系：Capability 声明动作 + 可逆性 + 补偿 + 策略 + 关系投影；
//! 真正执行 = Fiber → Policy 校验 → emit(Effect) → relate(投影)。

use std::collections::HashMap;

use serde_json::Value;

use super::policy::Policy;
use crate::kernel::{Compensation, Reversibility};

/// 能力声明。
#[derive(Debug, Clone)]
pub struct Capability {
    /// 能力名（如 "user.delete"）。
    pub name: String,
    /// 底层动作（映射到 Effect.action）。
    pub action: String,
    pub reversibility: Reversibility,
    /// 声明式补偿（Fiber 失败时由 CompensationEngine 逆序执行）。
    pub inverse: Option<Compensation>,
    /// 提交前约束。
    pub policy: Policy,
    /// 提交成功后自动投影的 Relation：(predicate, target)。
    /// source 取 args["target"]（无则用 capability 名）。
    pub projection: Option<(String, String)>,
}

impl Capability {
    pub fn new(name: impl Into<String>, action: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            action: action.into(),
            reversibility: Reversibility::Idempotent,
            inverse: None,
            policy: Policy::default(),
            projection: None,
        }
    }

    pub fn with_reversibility(mut self, r: Reversibility) -> Self {
        self.reversibility = r;
        self
    }

    pub fn with_inverse(mut self, action: impl Into<String>) -> Self {
        self.inverse = Some(Compensation {
            action: action.into(),
            payload: Value::Null,
        });
        self
    }

    pub fn with_policy(mut self, p: Policy) -> Self {
        self.policy = p;
        self
    }

    pub fn with_projection(
        mut self,
        predicate: impl Into<String>,
        target: impl Into<String>,
    ) -> Self {
        self.projection = Some((predicate.into(), target.into()));
        self
    }
}

/// 能力注册表（按名称查找；Service 组合使用）。
#[derive(Debug, Clone, Default)]
pub struct CapabilityRegistry {
    map: HashMap<String, Capability>,
}

impl CapabilityRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, cap: Capability) {
        self.map.insert(cap.name.clone(), cap);
    }

    pub fn get(&self, name: &str) -> Option<&Capability> {
        self.map.get(name)
    }

    pub fn names(&self) -> impl Iterator<Item = &String> {
        self.map.keys()
    }
}
