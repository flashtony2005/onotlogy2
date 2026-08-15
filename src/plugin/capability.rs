//! CapabilityRegistry —— 契约库（"需要什么能力"）+ Binding + Router。

use std::collections::HashMap;

use super::manifest::PluginManifest;

/// 能力契约（Contract = input/output schema + effects）。
#[derive(Debug, Clone)]
pub struct CapabilityContract {
    pub capability: String,
    pub input_schema: serde_json::Value,
    pub output_schema: serde_json::Value,
    pub effects: Vec<String>,
}

/// Provider 绑定：capability ↔ (plugin, tool) 实现。
#[derive(Debug, Clone)]
pub struct ProviderBinding {
    pub capability: String,
    pub plugin_id: String,
    pub tool_id: String,
    pub cost: u32,
    pub latency_ms: u32,
    pub reliability: f64,
}

/// Capability Registry（契约 + 实现绑定）。
#[derive(Debug, Clone, Default)]
pub struct CapabilityRegistry {
    contracts: HashMap<String, CapabilityContract>,
    providers: HashMap<String, Vec<ProviderBinding>>,
}

impl CapabilityRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// 声明能力契约。
    pub fn declare(&mut self, contract: CapabilityContract) {
        self.contracts.insert(contract.capability.clone(), contract);
    }

    pub fn contract(&self, capability: &str) -> Option<&CapabilityContract> {
        self.contracts.get(capability)
    }

    /// 绑定插件声明的全部能力（安装后自动注册提供能力）。
    /// 只绑定 manifest.capabilities——**未声明能力绝不产生实现**（P-Rev）。
    pub fn bind(&mut self, manifest: &PluginManifest) -> usize {
        let mut n = 0;
        for decl in &manifest.capabilities {
            self.bind_provider(ProviderBinding {
                capability: decl.id.clone(),
                plugin_id: manifest.id.clone(),
                tool_id: format!("{}.{}", manifest.id, decl.id),
                cost: 1,
                latency_ms: 100,
                reliability: 0.9,
            });
            n += 1;
        }
        n
    }

    pub fn bind_provider(&mut self, binding: ProviderBinding) {
        self.providers
            .entry(binding.capability.clone())
            .or_default()
            .push(binding);
    }

    pub fn providers_of(&self, capability: &str) -> Vec<&ProviderBinding> {
        self.providers
            .get(capability)
            .map(|v| v.iter().collect())
            .unwrap_or_default()
    }
}

/// Router 选择策略。
#[derive(Debug, Clone)]
pub enum RouterStrategy {
    /// 显式指定 plugin_id。
    Explicit(String),
    /// 注册序第一个。
    First,
    /// 简单评分：reliability 优先（确定性）。
    BestEffort,
}

/// Capability Router：Capability + Policy + Cost/Latency/Quality/Availability → Provider。
pub struct Router {
    pub strategy: RouterStrategy,
}

impl Router {
    pub fn new(strategy: RouterStrategy) -> Self {
        Self { strategy }
    }

    pub fn route<'a>(
        &self,
        registry: &'a CapabilityRegistry,
        capability: &str,
    ) -> Option<&'a ProviderBinding> {
        let providers = registry.providers_of(capability);
        if providers.is_empty() {
            return None;
        }
        match &self.strategy {
            RouterStrategy::Explicit(plugin_id) => providers
                .iter()
                .copied()
                .find(|p| &p.plugin_id == plugin_id),
            RouterStrategy::First => providers.first().copied(),
            RouterStrategy::BestEffort => {
                // 确定性评分：reliability 降序（同分 cost 升序）
                providers.into_iter().max_by(|a, b| {
                    a.reliability
                        .partial_cmp(&b.reliability)
                        .unwrap_or(std::cmp::Ordering::Equal)
                        .then_with(|| b.cost.cmp(&a.cost))
                })
            }
        }
    }
}
