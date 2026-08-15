//! PluginRegistry —— 实现库（"谁来提供能力"）+ Lifecycle。

use std::collections::HashMap;

use super::manifest::PluginManifest;
use super::{AsyncPlugin, Plugin};

/// 插件生命周期。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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
    /// 真实网络执行器（HTTP/MCP/AI；None = 纯本地同步插件）。
    pub async_plugin: Option<Box<dyn AsyncPlugin>>,
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
                async_plugin: None,
            },
        );
        Ok(())
    }

    /// 安装异步插件（真实网络：HTTP / MCP / AI Provider）。
    pub fn install_async(&mut self, plugin: Box<dyn AsyncPlugin>) -> Result<(), RegistryError> {
        let manifest = plugin.manifest().clone();
        if self.plugins.contains_key(&manifest.id) {
            return Err(RegistryError::Duplicate(manifest.id));
        }
        self.plugins.insert(
            manifest.id.clone(),
            PluginInstance {
                manifest,
                status: PluginStatus::Installed,
                plugin: Box::new(crate::plugin::exec::NoopPlugin),
                async_plugin: Some(plugin),
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

    /// 状态快照（manifest + lifecycle；插件本体不序列化——重载时重建执行器）。
    pub fn snapshot(&self) -> PluginStateSnapshot {
        PluginStateSnapshot {
            plugins: self
                .plugins
                .values()
                .map(|p| (p.manifest.clone(), p.status))
                .collect(),
        }
    }

    /// 从快照恢复状态（插件本体需重新 install_async/install）。
    pub fn restore_state(&mut self, snap: PluginStateSnapshot) {
        self.plugins.clear();
        for (manifest, status) in snap.plugins {
            self.plugins.insert(
                manifest.id.clone(),
                PluginInstance {
                    manifest,
                    status,
                    plugin: Box::new(crate::plugin::exec::NoopPlugin),
                    async_plugin: None,
                },
            );
        }
    }
}

/// PluginRegistry 持久化快照。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PluginStateSnapshot {
    pub plugins: Vec<(PluginManifest, PluginStatus)>,
}
