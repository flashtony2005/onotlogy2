//! HttpWorker —— 远程 Agent / 企业服务 / AI Provider 接入（POST /execute）。

use serde_json::{Value, json};

use super::manifest::WorkerManifest;
use super::sdk::{Worker, WorkerError, WorkerResult};

/// HTTP Worker：POST {base}/execute，body {capability, input}。
pub struct HttpWorker {
    pub manifest: WorkerManifest,
    url: String,
    client: reqwest::Client,
}

impl HttpWorker {
    pub fn new(manifest: WorkerManifest, base_url: &str) -> Self {
        Self {
            manifest,
            url: format!("{}/execute", base_url.trim_end_matches('/')),
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait::async_trait]
impl Worker for HttpWorker {
    fn manifest(&self) -> &WorkerManifest {
        &self.manifest
    }

    async fn execute(&self, capability: &str, input: Value) -> Result<WorkerResult, WorkerError> {
        let start = std::time::Instant::now();
        let resp = self
            .client
            .post(&self.url)
            .json(&json!({ "capability": capability, "input": input }))
            .send()
            .await
            .map_err(|e| WorkerError::Execution(format!("HTTP 调用失败: {e}")))?;
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(WorkerError::Execution(format!("HTTP {status}: {text}")));
        }
        let body: Value = resp
            .json()
            .await
            .map_err(|e| WorkerError::Execution(format!("响应解析失败: {e}")))?;
        Ok(WorkerResult {
            ok: body.get("ok").and_then(|v| v.as_bool()).unwrap_or(true),
            output: body,
            stderr: None,
            exit_code: Some(0),
            duration_ms: start.elapsed().as_millis() as u64,
        })
    }
}
