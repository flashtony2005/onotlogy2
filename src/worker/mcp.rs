//! McpWorker —— MCP Server 作为外部能力来源（JSON-RPC tools/call）。

use serde_json::Value;

use super::manifest::WorkerManifest;
use super::sdk::{Worker, WorkerError, WorkerResult};
use crate::plugin::mcp::McpClient;

/// MCP Worker：capability = MCP tool 名。
pub struct McpWorker {
    pub manifest: WorkerManifest,
    client: McpClient,
}

impl McpWorker {
    pub fn new(manifest: WorkerManifest, mcp_url: &str) -> Self {
        Self {
            manifest,
            client: McpClient::new(mcp_url),
        }
    }
}

#[async_trait::async_trait]
impl Worker for McpWorker {
    fn manifest(&self) -> &WorkerManifest {
        &self.manifest
    }

    async fn execute(&self, capability: &str, input: Value) -> Result<WorkerResult, WorkerError> {
        self.client
            .initialize()
            .await
            .map_err(|e| WorkerError::Execution(e.to_string()))?;
        let result = self
            .client
            .tools_call(capability, input)
            .await
            .map_err(|e| WorkerError::Execution(e.to_string()))?;
        Ok(WorkerResult::ok(result))
    }
}
