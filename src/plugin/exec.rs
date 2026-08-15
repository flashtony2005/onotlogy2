//! Executors —— 插件执行器（MVP：Local Function；Http/AI 为声明式 mock，接口就位）。

use serde_json::Value;

use super::manifest::{PluginManifest, RuntimeType};
use super::{Plugin, PluginError};

/// 插件处理器（capability + input → result）。
pub type PluginHandler = Box<dyn Fn(&str, Value) -> Result<Value, PluginError> + Send + Sync>;

/// Local Function 插件：确定性 Rust 函数执行（测试主力 + 无外部依赖）。
pub struct LocalFunctionPlugin {
    pub manifest: PluginManifest,
    handler: PluginHandler,
}

impl LocalFunctionPlugin {
    pub fn new(manifest: PluginManifest, handler: PluginHandler) -> Self {
        Self { manifest, handler }
    }
}

impl Plugin for LocalFunctionPlugin {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    fn execute(&self, capability: &str, input: Value) -> Result<Value, PluginError> {
        // 只执行声明的能力（未声明 → 拒绝——P-Rev）
        if !self.manifest.declares(capability) {
            return Err(PluginError::Failed(format!(
                "插件 {} 未声明能力 {capability}",
                self.manifest.id
            )));
        }
        (self.handler)(capability, input)
    }
}

/// 声明式 Http/AI 插件（MVP：与 Local 同构——真实网络接入列 Non-goal；
/// runtime 类型字段保留，供未来适配器使用）。
pub fn declare_plugin(
    id: &str,
    runtime: RuntimeType,
    capabilities: &[&str],
    handler: PluginHandler,
) -> LocalFunctionPlugin {
    let mut m = PluginManifest::new(id, id, "1.0.0", runtime);
    for c in capabilities {
        m = m.with_capability(c);
    }
    LocalFunctionPlugin::new(m, handler)
}
