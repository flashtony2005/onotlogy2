//! Worker SDK —— 执行主体抽象（第三方实现此 trait 即可被调度）。

use serde_json::Value;

use crate::manifest::WorkerManifest;

/// Worker 执行错误。
#[derive(Debug, thiserror::Error)]
pub enum WorkerError {
    #[error("策略拒绝（fail-closed）: {0}")]
    Denied(String),
    #[error("需要审批: {0}")]
    RequireApproval(String),
    #[error("无可用 Worker: {0}")]
    NoWorker(String),
    #[error("Worker 执行失败: {0}")]
    Execution(String),
}

/// Worker 执行结果（Execution Protocol 的结果契约）。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WorkerResult {
    pub ok: bool,
    pub output: Value,
    pub stderr: Option<String>,
    pub exit_code: Option<i32>,
    pub duration_ms: u64,
}

impl WorkerResult {
    pub fn ok(output: Value) -> Self {
        Self {
            ok: true,
            output,
            stderr: None,
            exit_code: Some(0),
            duration_ms: 0,
        }
    }
}

/// Worker：Capability 的执行主体。
#[async_trait::async_trait]
pub trait Worker: Send + Sync {
    fn manifest(&self) -> &WorkerManifest;

    /// 执行一个 capability（输入 → 结果）。
    async fn execute(&self, capability: &str, input: Value) -> Result<WorkerResult, WorkerError>;

    /// 健康探测（可用性；MVP 默认 true）。
    async fn health(&self) -> bool {
        true
    }
}
