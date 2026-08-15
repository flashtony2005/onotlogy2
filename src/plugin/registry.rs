//! PluginRegistry —— 实现库（"谁来提供能力"）+ Lifecycle。

use std::collections::HashMap;

use super::Plugin;
use super::manifest::PluginManifest;

/// 插件生命周期。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginStatus {
    Discovered,
    Installed,
    Loaded,
    Validated,
    Registered,
    Active,
    Disabled,
    Uninstalled,
}

/// 注册表错误。
#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    #[error("重复安装: {0}")]
    Duplicate(String),
    #[error("插件不存在: {0}")]
    NotFound(String),
    #[error("非法生命周期转移: {from} → {to}")]
    InvalidTransition { from: String, to: String },
}

fn transition_allowed(from: PluginStatus, to: PluginStatus) -> bool {
    if from == to {
        return true;
    }
    matches!(
        (from, to),
        (PluginStatus::Discovered, PluginStatus::Installed)
            | (PluginStatus::Installed, PluginStatus::Loaded)
            | (PluginStatus::Installed, PluginStatus::Validated)
            | (PluginStatus::Loaded, PluginStatus::Validated)
            | (PluginStatus::Validated, PluginStatus::Registered)
            | (PluginStatus::Registered, PluginStatus::Active)
            | (PluginStatus::Active, PluginStatus::Disabled)
            | (PluginStatus::Disabled, PluginStatus::Active)
            | (PluginStatus::Disabled, PluginStatus::Uninstalled)
    )
}

/// 插件实例。
pub struct PluginInstance {
    pub manifest: PluginManifest,
    pub status: PluginStatus,
    pub plugin: Box<dyn Plugin>,
}

impl PluginInstance {
    pub fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }
}

/// Plugin Registry。
#[derive(Default)]
pub struct PluginRegistry {
    plugins: HashMap<String, PluginInstance>,
}

impl PluginRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// 安装（DISCOVERED → INSTALLED；重复拒绝）。
    pub fn install(&mut self, plugin: Box<dyn Plugin>) -> Result<(), RegistryError> {
        let manifest = plugin.manifest().clone();
        if self.plugins.contains_key(&manifest.id) {
            return Err(RegistryError::Duplicate(manifest.id));
        }
        self.plugins.insert(
            manifest.id.clone(),
            PluginInstance {
                manifest,
                status: PluginStatus::Installed,
                plugin,
            },
        );
        Ok(())
    }

    /// 四重校验之一环（Manifest/Capability/Permission/Runtime 校验的入口；
    /// MVP：Loaded → Validated）。
    pub fn validate(&mut self, id: &str) -> Result<(), RegistryError> {
        self.advance(id, PluginStatus::Validated)
    }

    /// Registered。
    pub fn register(&mut self, id: &str) -> Result<(), RegistryError> {
        self.advance(id, PluginStatus::Registered)
    }

    /// ACTIVE（进入可执行状态）。
    pub fn activate(&mut self, id: &str) -> Result<(), RegistryError> {
        self.advance(id, PluginStatus::Active)
    }

    pub fn disable(&mut self, id: &str) -> Result<(), RegistryError> {
        self.advance(id, PluginStatus::Disabled)
    }

    pub fn uninstall(&mut self, id: &str) -> Result<(), RegistryError> {
        self.advance(id, PluginStatus::Uninstalled)
    }

    fn advance(&mut self, id: &str, to: PluginStatus) -> Result<(), RegistryError> {
        let inst = self
            .plugins
            .get_mut(id)
            .ok_or_else(|| RegistryError::NotFound(id.to_string()))?;
        let from = inst.status;
        if !transition_allowed(from, to) {
            return Err(RegistryError::InvalidTransition {
                from: format!("{from:?}"),
                to: format!("{to:?}"),
            });
        }
        inst.status = to;
        Ok(())
    }

    pub fn get(&self, id: &str) -> Option<&PluginInstance> {
        self.plugins.get(id)
    }

    pub fn status(&self, id: &str) -> Option<PluginStatus> {
        self.plugins.get(id).map(|p| p.status)
    }

    pub fn active_plugins(&self) -> Vec<&PluginInstance> {
        self.plugins
            .values()
            .filter(|p| p.status == PluginStatus::Active)
            .collect()
    }
}
