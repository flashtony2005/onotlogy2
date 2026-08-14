//! Service —— 组合层（= Context + Long-lived Fiber + Capabilities）。
//!
//! Service 不是 Kernel Primitive：它是能力集合挂载在某个 Context 作用域的产物。
//! Phase 2 简化：Service = id + 能力注册表；Context 绑定由 TaskExecutor 执行时指定。

use std::collections::HashMap;

use super::capability::Capability;

/// 服务：一组能力的集合（用户服务 / 订单服务 / 分析服务…）。
#[derive(Debug, Clone, Default)]
pub struct Service {
    pub id: String,
    capabilities: HashMap<String, Capability>,
}

impl Service {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            capabilities: HashMap::new(),
        }
    }

    pub fn with_capability(mut self, cap: Capability) -> Self {
        self.capabilities.insert(cap.name.clone(), cap);
        self
    }

    pub fn capability(&self, name: &str) -> Option<&Capability> {
        self.capabilities.get(name)
    }

    pub fn capability_names(&self) -> impl Iterator<Item = &String> {
        self.capabilities.keys()
    }
}
