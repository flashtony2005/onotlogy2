//! PluginRuntime —— Capability 调度执行（Resolve → Policy → Provider → Plugin → Result）
//! + Permission Policy（fail-closed）+ Secret Isolation + Session Event 审计。

use serde_json::{Value, json};

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
                .any(|d| p.starts_with(d))
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
                .any(|d| p.starts_with(d))
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
