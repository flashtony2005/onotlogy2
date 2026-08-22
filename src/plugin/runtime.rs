//! PluginRuntime —— Capability 调度执行（Resolve → Policy → Provider → Plugin → Result）
//! + Permission Policy（fail-closed）+ Secret Isolation + Session Event 审计。

use serde_json::{Value, json};
use std::path::{Path, PathBuf};

use super::capability::{CapabilityRegistry, Router};
use super::manifest::PluginManifest;
use super::registry::PluginRegistry;
use crate::session::event::EventType;
use crate::session::log::SessionLog;

/// 权限请求（执行前检查；未声明 → DENY，fail-closed）。
#[derive(Debug, Clone, Default)]
pub struct PermissionRequest<'a> {
    pub network: Option<&'a str>,
    pub filesystem_read: Option<&'a str>,
    pub filesystem_write: Option<&'a str>,
    pub process: Option<&'a str>,
    pub secret: Option<&'a str>,
}

/// 运行时错误。
#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error("能力未解析（无实现）: {0}")]
    Unresolved(String),
    #[error("权限拒绝: {0}")]
    PermissionDenied(String),
    #[error("插件执行失败: {0}")]
    PluginFailed(String),
    #[error("能力契约未声明: {0}")]
    NoContract(String),
}

/// Capability Runtime。
pub struct PluginRuntime<'a> {
    plugins: &'a PluginRegistry,
    caps: &'a CapabilityRegistry,
    router: Router,
    log: Option<&'a mut SessionLog>,
}

impl<'a> PluginRuntime<'a> {
    pub fn new(plugins: &'a PluginRegistry, caps: &'a CapabilityRegistry, router: Router) -> Self {
        Self {
            plugins,
            caps,
            router,
            log: None,
        }
    }

    /// 挂接 Session Event Log（每次关键动作 append 审计）。
    pub fn with_log(mut self, log: &'a mut SessionLog) -> Self {
        self.log = Some(log);
        self
    }

    fn emit(&mut self, event_type: EventType, data: Value) {
        if let Some(log) = &mut self.log {
            log.append(event_type, data);
        }
    }

    /// 执行一个 Capability：
    /// Resolve → Policy Check → Provider Selection → Plugin Invocation → Result。
    pub fn execute(&mut self, capability: &str, input: Value) -> Result<Value, RuntimeError> {
        // 0) 契约存在（Input Schema 的载体）
        if self.caps.contract(capability).is_none() {
            self.emit(
                EventType::Error,
                json!({ "message": format!("no contract: {capability}") }),
            );
            return Err(RuntimeError::NoContract(capability.to_string()));
        }
        // 1) Resolve：有实现吗
        let binding = self.router.route(self.caps, capability).ok_or_else(|| {
            self.emit(
                EventType::Error,
                json!({ "message": format!("unresolved: {capability}") }),
            );
            RuntimeError::Unresolved(capability.to_string())
        })?;
        // 2) Policy Check：插件必须 ACTIVE
        let inst = self.plugins.get(&binding.plugin_id).ok_or_else(|| {
            RuntimeError::PluginFailed(format!("plugin missing: {}", binding.plugin_id))
        })?;
        self.emit(
            EventType::CapabilitySelected,
            json!({ "capability": capability }),
        );
        // 3) Provider Selection 已由 Router 完成
        // 4) Plugin Invocation（Secret Isolation：日志只记脱敏 input）
        let sanitized = sanitize_secrets(&input, inst.manifest());
        self.emit(
            EventType::PluginCall,
            json!({ "plugin": inst.manifest().id, "capability": capability, "input": sanitized }),
        );
        let result = inst
            .plugin
            .execute(capability, input)
            .map_err(|e| RuntimeError::PluginFailed(e.to_string()))?;
        self.emit(
            EventType::PluginResult,
            json!({ "plugin": inst.manifest().id, "ok": true }),
        );
        Ok(result)
    }

    /// 异步执行（真实网络插件：HTTP / MCP / AI Provider）。
    /// 与 execute 同一路径：Resolve → Policy → Provider → Invoke → Result + Session 审计。
    pub async fn execute_async(
        &mut self,
        capability: &str,
        input: Value,
    ) -> Result<Value, RuntimeError> {
        if self.caps.contract(capability).is_none() {
            self.emit(
                EventType::Error,
                json!({ "message": format!("no contract: {capability}") }),
            );
            return Err(RuntimeError::NoContract(capability.to_string()));
        }
        let binding = self.router.route(self.caps, capability).ok_or_else(|| {
            self.emit(
                EventType::Error,
                json!({ "message": format!("unresolved: {capability}") }),
            );
            RuntimeError::Unresolved(capability.to_string())
        })?;
        let inst = self.plugins.get(&binding.plugin_id).ok_or_else(|| {
            RuntimeError::PluginFailed(format!("plugin missing: {}", binding.plugin_id))
        })?;
        self.emit(
            EventType::CapabilitySelected,
            json!({ "capability": capability }),
        );
        let sanitized = sanitize_secrets(&input, inst.manifest());
        self.emit(
            EventType::PluginCall,
            json!({ "plugin": inst.manifest().id, "capability": capability, "input": sanitized }),
        );
        let async_plugin = inst.async_plugin.as_ref().ok_or_else(|| {
            RuntimeError::PluginFailed(format!("插件 {} 不是异步插件", inst.manifest().id))
        })?;
        let result = async_plugin
            .execute_async(capability, input)
            .await
            .map_err(|e| RuntimeError::PluginFailed(e.to_string()))?;
        self.emit(
            EventType::PluginResult,
            json!({ "plugin": inst.manifest().id, "ok": true }),
        );
        Ok(result)
    }

    /// 权限检查（执行前：请求超出 manifest 声明 → DENY，fail-closed）。
    pub fn check_permission(
        manifest: &PluginManifest,
        request: &PermissionRequest,
    ) -> Result<(), RuntimeError> {
        if let Some(n) = request.network
            && !manifest
                .permissions
                .network
                .iter()
                .any(|d| n.starts_with(d))
        {
            return Err(RuntimeError::PermissionDenied(format!("network {n}")));
        }
        if let Some(p) = request.filesystem_read
            && !manifest
                .permissions
                .filesystem_read
                .iter()
                .any(|d| std::path::Path::new(p).starts_with(std::path::Path::new(d)))
        {
            return Err(RuntimeError::PermissionDenied(format!(
                "filesystem_read {p}"
            )));
        }
        if let Some(p) = request.filesystem_write
            && !manifest
                .permissions
                .filesystem_write
                .iter()
                .any(|d| std::path::Path::new(p).starts_with(std::path::Path::new(d)))
        {
            return Err(RuntimeError::PermissionDenied(format!(
                "filesystem_write {p}"
            )));
        }
        if let Some(p) = request.process
            && !manifest.permissions.process.iter().any(|d| p == d)
        {
            return Err(RuntimeError::PermissionDenied(format!("process {p}")));
        }
        if let Some(s) = request.secret
            && !manifest.permissions.secrets.iter().any(|d| d == s)
        {
            return Err(RuntimeError::PermissionDenied(format!("secret {s}")));
        }
        Ok(())
    }

    /// 将待读取路径解析为 canonical 普通文件。
    pub fn canonicalize_filesystem_read(path: &Path) -> Result<PathBuf, RuntimeError> {
        if !path.is_absolute() {
            return Err(RuntimeError::PermissionDenied(
                "filesystem_read 路径必须使用绝对路径".into(),
            ));
        }
        let canonical = path.canonicalize().map_err(|error| {
            RuntimeError::PermissionDenied(format!("filesystem_read 文件不可用：{error}"))
        })?;
        if !canonical.is_file() {
            return Err(RuntimeError::PermissionDenied(format!(
                "filesystem_read 路径不是普通文件：{}",
                canonical.display()
            )));
        }
        Ok(canonical)
    }

    /// 在插件执行前完成文件读取的统一前置校验。
    ///
    /// 调用方只能拿到已经 canonicalize 且通过 manifest 权限声明的普通文件，
    /// 避免宿主在插件真正执行前绕过同一套权限边界读取数据源。
    pub fn authorize_filesystem_read(
        manifest: &PluginManifest,
        path: &Path,
    ) -> Result<PathBuf, RuntimeError> {
        let canonical = Self::canonicalize_filesystem_read(path)?;
        let canonical_string = canonical.to_string_lossy();
        Self::check_permission(
            manifest,
            &PermissionRequest {
                filesystem_read: Some(canonical_string.as_ref()),
                ..PermissionRequest::default()
            },
        )?;
        Ok(canonical)
    }
}

/// Secret Isolation：把 input 中与 manifest secret 配置同名的值打码（不进日志）。
pub fn sanitize_secrets(input: &Value, manifest: &PluginManifest) -> Value {
    let mut out = input.clone();
    for cfg in &manifest.config {
        if cfg.secret
            && let Some(obj) = out.as_object_mut()
            && obj.contains_key(&cfg.name)
        {
            obj.insert(cfg.name.clone(), json!("***"));
        }
    }
    out
}
