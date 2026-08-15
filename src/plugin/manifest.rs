//! PluginManifest —— 插件的"身份证"（告诉系统：我是谁、我提供什么、需要什么权限、如何运行）。

use serde::{Deserialize, Serialize};

/// 运行时类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RuntimeType {
    Http,
    Mcp,
    Local,
    Ai,
}

/// 权限声明（fail-closed：未声明 = 无权限）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Permissions {
    /// 允许访问的网络域名白名单。
    pub network: Vec<String>,
    pub filesystem_read: Vec<String>,
    pub filesystem_write: Vec<String>,
    /// 允许的进程。
    pub process: Vec<String>,
    /// 允许使用的 secret 配置名。
    pub secrets: Vec<String>,
    pub database: Vec<String>,
}

/// 配置字段。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConfigField {
    pub name: String,
    /// secret 标记：值不进审计/日志（Secret Isolation）。
    pub secret: bool,
}

/// 能力声明。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CapabilityDecl {
    pub id: String,
    pub version: String,
}

/// Plugin Manifest（可序列化）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub runtime: RuntimeType,
    pub capabilities: Vec<CapabilityDecl>,
    pub config: Vec<ConfigField>,
    pub permissions: Permissions,
    pub sandbox: bool,
}

impl PluginManifest {
    pub fn new(id: &str, name: &str, version: &str, runtime: RuntimeType) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            version: version.to_string(),
            runtime,
            capabilities: Vec::new(),
            config: Vec::new(),
            permissions: Permissions::default(),
            sandbox: false,
        }
    }

    pub fn with_capability(mut self, id: &str) -> Self {
        self.capabilities.push(CapabilityDecl {
            id: id.to_string(),
            version: "1".into(),
        });
        self
    }

    pub fn with_config(mut self, field: ConfigField) -> Self {
        self.config.push(field);
        self
    }

    pub fn with_permissions(mut self, p: Permissions) -> Self {
        self.permissions = p;
        self
    }

    pub fn declares(&self, capability: &str) -> bool {
        self.capabilities.iter().any(|c| c.id == capability)
    }
}
