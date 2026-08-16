//! ABI-4：Router —— Bridge 的真正核心。
//!
//! 不是 `receiver = codex` 直路由，而是：
//! `Intent → Capability → Policy → Agent Selection`。
//! - intent 注册表：自然语言意图 → 能力（"modify_code" → "coding.modify"）
//! - Capability Registry：能力 → 提供方列表（priority 序；增删即动态选择）
//! - Policy 在 Router 前面：high_risk 能力 → RequiresApproval（复用现有
//!   Approval/Gate/Audit 机制裁决；v0.1 只留判定接口）
//! - fail-closed：未知意图 / 无提供方 → 拒绝

use std::collections::{HashMap, HashSet};

use super::FabricError;

/// 路由判定（Policy 输出）。
#[derive(Debug, Clone, PartialEq)]
pub enum RouteVerdict {
    Allow,
    RequiresApproval,
    Denied(String),
}

/// 路由结果（可解释：agent + capability + verdict）。
#[derive(Debug, Clone, PartialEq)]
pub struct RoutePlan {
    pub agent_id: String,
    pub capability: String,
    pub verdict: RouteVerdict,
}

/// 能力提供方（priority 升序 = 选择优先级；1 最高）。
#[derive(Debug, Clone)]
pub struct AgentOffer {
    pub agent_id: String,
    pub priority: u32,
}

/// Fabric Router：Intent → Capability → Policy → Agent Selection。
#[derive(Debug, Default)]
pub struct FabricRouter {
    intents: HashMap<String, String>,
    registry: HashMap<String, Vec<AgentOffer>>,
    /// HIGH risk 能力集（Policy：require_approval）。
    high_risk: HashSet<String>,
}

impl FabricRouter {
    pub fn new() -> Self {
        Self::default()
    }

    /// 注册意图 → 能力映射。
    pub fn register_intent(&mut self, intent: impl Into<String>, capability: impl Into<String>) {
        self.intents.insert(intent.into(), capability.into());
    }

    /// 注册能力提供方（priority 升序插入；1 = 首选）。
    pub fn register_agent(
        &mut self,
        capability: impl Into<String>,
        agent_id: impl Into<String>,
        priority: u32,
    ) {
        let cap = capability.into();
        let offers = self.registry.entry(cap).or_default();
        offers.push(AgentOffer {
            agent_id: agent_id.into(),
            priority,
        });
        offers.sort_by_key(|o| o.priority);
    }

    /// 提供方下线（动态选择：Codex 不是特殊公民）。
    pub fn unregister_agent(&mut self, capability: impl Into<String>, agent_id: &str) {
        if let Some(offers) = self.registry.get_mut(&capability.into()) {
            offers.retain(|o| o.agent_id != agent_id);
        }
    }

    /// 标记 HIGH risk 能力（Policy：require_approval）。
    pub fn mark_high_risk(&mut self, capability: impl Into<String>) {
        self.high_risk.insert(capability.into());
    }

    /// 路由（fail-closed）：Intent → Capability → Policy → Selection。
    pub fn route(&self, intent: &str) -> Result<RoutePlan, FabricError> {
        let capability = self
            .intents
            .get(intent)
            .ok_or_else(|| FabricError::UnknownIntent(intent.to_string()))?;
        let offers = self
            .registry
            .get(capability)
            .filter(|o| !o.is_empty())
            .ok_or_else(|| FabricError::NoProvider(capability.clone()))?;
        let verdict = if self.high_risk.contains(capability) {
            RouteVerdict::RequiresApproval
        } else {
            RouteVerdict::Allow
        };
        Ok(RoutePlan {
            agent_id: offers[0].agent_id.clone(),
            capability: capability.clone(),
            verdict,
        })
    }
}
