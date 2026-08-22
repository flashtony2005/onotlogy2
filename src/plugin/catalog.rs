//! 宿主级插件目录：一次装配、共享查询、统一能力调用。
//!
//! HTTP、CLI、自动化 Job 都通过这个目录解析插件与能力；传输层不感知彩票
//! 领域实现，新增插件只需要在宿主启动装配阶段注册。

use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use serde::Serialize;
use serde_json::Value;
use thiserror::Error;

use super::AsyncPlugin;
use super::capability::{CapabilityRegistry, Router, RouterStrategy};
use super::manifest::PluginManifest;
use super::registry::{PluginRegistry, PluginStatus, RegistryError};
use super::runtime::{PluginRuntime, RuntimeError};
use crate::lottery_plugin::{LotteryPlugin, PLUGIN_ID, declare_capabilities};
use crate::storage::LibSQLStorage;

/// 宿主共享插件目录。
///
/// 装配完成后只读共享，执行时仅创建短生命周期 Runtime，不在每个请求中
/// 重复安装插件、声明契约和绑定 Provider。
pub struct PluginCatalog {
    plugins: PluginRegistry,
    capabilities: CapabilityRegistry,
    lottery_source_roots: Option<Arc<RwLock<Vec<PathBuf>>>>,
}

impl Default for PluginCatalog {
    fn default() -> Self {
        Self::new()
    }
}

impl PluginCatalog {
    pub fn new() -> Self {
        Self {
            plugins: PluginRegistry::new(),
            capabilities: CapabilityRegistry::new(),
            lottery_source_roots: None,
        }
    }

    /// 注册一个异步插件及其能力契约。
    ///
    /// 业务插件只负责提供 Provider 与 Contract；HTTP 层无需增加任何业务
    /// 分支。外部插件包接入时复用这个方法即可。
    pub fn register_async(
        &mut self,
        plugin: Box<dyn AsyncPlugin>,
        declare_contracts: impl FnOnce(&mut CapabilityRegistry),
    ) -> Result<(), CatalogError> {
        let manifest = plugin.manifest().clone();
        self.plugins.install_async(plugin)?;
        self.plugins.validate(&manifest.id)?;
        self.plugins.register(&manifest.id)?;
        self.plugins.activate(&manifest.id)?;
        declare_contracts(&mut self.capabilities);
        self.capabilities.bind(&manifest);
        Ok(())
    }

    /// 创建内置插件目录。当前内置彩票能力，后续可替换为插件发现结果。
    pub fn with_builtins(
        storage: LibSQLStorage,
        lottery_source_roots: impl IntoIterator<Item = PathBuf>,
    ) -> Result<Self, CatalogError> {
        let mut catalog = Self::new();
        let plugin = LotteryPlugin::with_source_roots(storage, lottery_source_roots);
        catalog.lottery_source_roots = Some(plugin.source_roots());
        catalog.register_async(Box::new(plugin), declare_capabilities)?;
        Ok(catalog)
    }

    pub fn list(&self) -> Vec<PluginInfo> {
        let mut plugins: Vec<_> = self
            .plugins
            .snapshot()
            .plugins
            .into_iter()
            .map(|(manifest, status)| PluginInfo {
                manifest: self.effective_manifest(&manifest.id.clone(), manifest),
                status,
            })
            .collect();
        plugins.sort_by(|a, b| a.manifest.id.cmp(&b.manifest.id));
        plugins
    }

    pub fn manifest(&self, plugin_id: &str) -> Option<PluginInfo> {
        self.plugins.get(plugin_id).map(|plugin| PluginInfo {
            manifest: self.effective_manifest(plugin_id, plugin.manifest().clone()),
            status: plugin.status,
        })
    }

    fn effective_manifest(&self, plugin_id: &str, mut manifest: PluginManifest) -> PluginManifest {
        if plugin_id == PLUGIN_ID
            && let Some(roots) = &self.lottery_source_roots
            && let Ok(roots) = roots.read()
        {
            manifest.permissions.filesystem_read = roots
                .iter()
                .map(|root| root.to_string_lossy().into_owned())
                .collect();
        }
        manifest
    }

    pub fn has_capability(&self, capability: &str) -> bool {
        self.capabilities.contract(capability).is_some()
    }

    /// 对宿主侧数据源执行插件 manifest 权限前置校验。
    ///
    /// watcher、HTTP 和 CLI 在读取业务包数据前都必须通过这里；权限规则
    /// 由插件 manifest 提供，宿主不复制具体业务包的目录约束。
    pub fn authorize_filesystem_read(
        &self,
        plugin_id: &str,
        path: &Path,
    ) -> Result<PathBuf, CatalogError> {
        let plugin = self
            .plugins
            .get(plugin_id)
            .ok_or_else(|| CatalogError::PluginNotFound(plugin_id.to_string()))?;
        if plugin.status != PluginStatus::Active {
            return Err(CatalogError::PluginNotActive(plugin_id.to_string()));
        }
        if plugin_id == PLUGIN_ID
            && let Some(roots) = &self.lottery_source_roots
        {
            let canonical =
                PluginRuntime::canonicalize_filesystem_read(path).map_err(CatalogError::Runtime)?;
            let allowed = roots
                .read()
                .map_err(|_| {
                    CatalogError::Runtime(RuntimeError::PermissionDenied(
                        "数据源授权状态不可用".into(),
                    ))
                })?
                .iter()
                .any(|root| canonical.starts_with(root));
            if !allowed {
                return Err(CatalogError::Runtime(RuntimeError::PermissionDenied(
                    format!("filesystem_read {}", canonical.display()),
                )));
            }
            return Ok(canonical);
        }
        PluginRuntime::authorize_filesystem_read(plugin.manifest(), path)
            .map_err(CatalogError::Runtime)
    }

    pub fn allow_filesystem_read_root(
        &self,
        plugin_id: &str,
        root: &Path,
    ) -> Result<(), CatalogError> {
        if plugin_id != PLUGIN_ID {
            return Err(CatalogError::PluginNotFound(plugin_id.to_string()));
        }
        let canonical = root.canonicalize().map_err(|error| {
            CatalogError::Runtime(RuntimeError::PermissionDenied(format!(
                "filesystem_read 目录不可用：{error}"
            )))
        })?;
        if !canonical.is_dir() {
            return Err(CatalogError::Runtime(RuntimeError::PermissionDenied(
                format!("filesystem_read 路径不是目录：{}", canonical.display()),
            )));
        }
        let Some(roots) = &self.lottery_source_roots else {
            let plugin = self
                .plugins
                .get(plugin_id)
                .ok_or_else(|| CatalogError::PluginNotFound(plugin_id.to_string()))?;
            let allowed = plugin
                .manifest()
                .permissions
                .filesystem_read
                .iter()
                .any(|allowed_root| canonical.starts_with(Path::new(allowed_root)));
            return if allowed {
                Ok(())
            } else {
                Err(CatalogError::Runtime(RuntimeError::PermissionDenied(
                    format!("filesystem_read {}", canonical.display()),
                )))
            };
        };
        let mut roots = roots.write().map_err(|_| {
            CatalogError::Runtime(RuntimeError::PermissionDenied(
                "彩票插件数据源授权状态不可用".into(),
            ))
        })?;
        if !roots.iter().any(|root| root == &canonical) {
            roots.push(canonical);
        }
        Ok(())
    }

    pub async fn invoke(
        &self,
        plugin_id: &str,
        capability: &str,
        input: Value,
    ) -> Result<Value, CatalogError> {
        let plugin = self
            .plugins
            .get(plugin_id)
            .ok_or_else(|| CatalogError::PluginNotFound(plugin_id.to_string()))?;
        if plugin.status != PluginStatus::Active {
            return Err(CatalogError::PluginNotActive(plugin_id.to_string()));
        }
        if !plugin.manifest().declares(capability) {
            return Err(CatalogError::CapabilityNotDeclared {
                plugin_id: plugin_id.to_string(),
                capability: capability.to_string(),
            });
        }

        let mut runtime = PluginRuntime::new(
            &self.plugins,
            &self.capabilities,
            Router::new(RouterStrategy::Explicit(plugin_id.to_string())),
        );
        runtime
            .execute_async(capability, input)
            .await
            .map_err(CatalogError::Runtime)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct PluginInfo {
    pub manifest: PluginManifest,
    pub status: PluginStatus,
}

#[derive(Debug, Error)]
pub enum CatalogError {
    #[error("插件不存在：{0}")]
    PluginNotFound(String),
    #[error("插件未激活：{0}")]
    PluginNotActive(String),
    #[error("插件 {plugin_id} 未声明能力：{capability}")]
    CapabilityNotDeclared {
        plugin_id: String,
        capability: String,
    },
    #[error("插件注册失败：{0}")]
    Registry(#[from] RegistryError),
    #[error("插件执行失败：{0}")]
    Runtime(#[from] RuntimeError),
}

pub type SharedPluginCatalog = Arc<PluginCatalog>;
