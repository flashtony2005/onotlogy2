//! McpPlugin —— MCP（Model Context Protocol）轻量客户端（JSON-RPC 2.0 over HTTP）。
//!
//! 协议序列：`initialize` → `tools/list`（可选）→ `tools/call`。
//! MCP Server 的工具即 Capability 实现：`tools/call { name: capability, arguments: input }`。

use serde_json::{Value, json};

use super::manifest::PluginManifest;
use super::{AsyncPlugin, PluginError};

/// JSON-RPC 响应。
#[derive(Debug, Clone, serde::Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: u64,
    #[serde(default)]
    pub result: Option<Value>,
    #[serde(default)]
    pub error: Option<Value>,
}

/// 轻量 MCP 客户端（HTTP transport）。
pub struct McpClient {
    url: String,
    client: reqwest::Client,
    next_id: std::sync::atomic::AtomicU64,
}

impl McpClient {
    pub fn new(url: &str) -> Self {
        Self {
            url: url.to_string(),
            client: reqwest::Client::new(),
            next_id: std::sync::atomic::AtomicU64::new(1),
        }
    }

    async fn call(&self, method: &str, params: Value) -> Result<Value, PluginError> {
        let id = self
            .next_id
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let resp: JsonRpcResponse = self
            .client
            .post(&self.url)
            .json(&json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": method,
                "params": params,
            }))
            .send()
            .await
            .map_err(|e| PluginError::Failed(format!("MCP 请求失败: {e}")))?
            .json()
            .await
            .map_err(|e| PluginError::Failed(format!("MCP 响应解析失败: {e}")))?;
        if let Some(err) = resp.error {
            return Err(PluginError::Failed(format!(
                "MCP {method} 错误: {}",
                err.get("message")
                    .and_then(|m| m.as_str())
                    .unwrap_or("unknown")
            )));
        }
        resp.result
            .ok_or_else(|| PluginError::Failed(format!("MCP {method} 无结果")))
    }

    /// initialize（协议握手）。
    pub async fn initialize(&self) -> Result<Value, PluginError> {
        self.call(
            "initialize",
            json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {
                    "name": "openaip-os",
                    "version": env!("CARGO_PKG_VERSION")
                }
            }),
        )
        .await
    }

    /// tools/list（能力发现）。
    pub async fn tools_list(&self) -> Result<Vec<Value>, PluginError> {
        let r = self.call("tools/list", json!({})).await?;
        Ok(r.get("tools")
            .cloned()
            .unwrap_or_else(|| json!([]))
            .as_array()
            .cloned()
            .unwrap_or_default())
    }

    /// tools/call（执行工具 = capability）。
    pub async fn tools_call(&self, name: &str, arguments: Value) -> Result<Value, PluginError> {
        self.call(
            "tools/call",
            json!({ "name": name, "arguments": arguments }),
        )
        .await
    }
}

/// MCP 插件：manifest.endpoint 指向 MCP Server；capability 映射为 tool 名。
pub struct McpPlugin {
    pub manifest: PluginManifest,
    client: McpClient,
}

impl McpPlugin {
    pub fn new(manifest: PluginManifest) -> Self {
        let url = manifest.endpoint.clone().unwrap_or_default();
        Self {
            manifest,
            client: McpClient::new(&url),
        }
    }

    pub fn client(&self) -> &McpClient {
        &self.client
    }
}

#[async_trait::async_trait]
impl AsyncPlugin for McpPlugin {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    async fn execute_async(&self, capability: &str, input: Value) -> Result<Value, PluginError> {
        if self.manifest.endpoint.is_none() {
            return Err(PluginError::Failed("McpPlugin 缺 endpoint".into()));
        }
        // 完整协议序列：握手 → 调用
        self.client.initialize().await?;
        self.client.tools_call(capability, input).await
    }
}
