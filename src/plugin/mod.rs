//! Plugin —— 外部能力进入 OpenAIP-OS 的标准入口（Phase 10）。
//!
//! 核心抽象边界：**Skill 是能力需求（requires），Tool/Plugin 是能力实现（provides）**。
//! Plugin = Tool / AI / API / MCP / 代码执行器 / Agent 的适配与扩展机制。

pub mod capability;
pub mod exec;
pub mod manifest;
pub mod registry;
pub mod runtime;

pub use manifest::{ConfigField, Permissions, PluginManifest, RuntimeType};

use serde_json::Value;

/// 插件执行错误。
#[derive(Debug, thiserror::Error)]
pub enum PluginError {
    #[error("插件执行失败: {0}")]
    Failed(String),
}

/// Plugin SDK：第三方实现此 trait 即可接入系统（install → register → execute）。
pub trait Plugin {
    fn manifest(&self) -> &PluginManifest;
    fn execute(&self, capability: &str, input: Value) -> Result<Value, PluginError>;
}
