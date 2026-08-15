//! HttpPlugin —— 真实 HTTP 网络接入（Phase 10 · 真实世界连接）。
//!
//! 声明式：manifest.endpoint 为 base URL；execute(capability, input) →
//! `POST {endpoint}/{capability}` body `{ "input": ... }` → JSON 响应。
//! AI Provider 同构：llm.generate 指向 OpenAI 形态端点即可。

use serde_json::Value;

use super::manifest::PluginManifest;
use super::{AsyncPlugin, PluginError};

/// 真实 HTTP 插件（reqwest client；TLS 由 rustls 提供）。
pub struct HttpPlugin {
    pub manifest: PluginManifest,
    client: reqwest::Client,
}

impl HttpPlugin {
    pub fn new(manifest: PluginManifest) -> Self {
        Self {
            manifest,
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait::async_trait]
impl AsyncPlugin for HttpPlugin {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    async fn execute_async(&self, capability: &str, input: Value) -> Result<Value, PluginError> {
        let base = self
            .manifest
            .endpoint
            .as_deref()
            .ok_or_else(|| PluginError::Failed("HttpPlugin 缺 endpoint".into()))?;
        let url = format!("{}/{}", base.trim_end_matches('/'), capability);
        let resp = self
            .client
            .post(&url)
            .json(&input)
            .send()
            .await
            .map_err(|e| PluginError::Failed(format!("HTTP 请求失败: {e}")))?;
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_else(|_| "unknown".to_string());
            return Err(PluginError::Failed(format!("HTTP {status}: {text}")));
        }
        let body: Value = resp
            .json()
            .await
            .map_err(|e| PluginError::Failed(format!("响应解析失败: {e}")))?;
        Ok(body)
    }
}
