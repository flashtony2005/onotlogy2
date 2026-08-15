//! ProcessWorker —— 通用 CLI Worker（真实进程执行）。
//!
//! 适配 Codex / Claude Code / git / ffmpeg / python 等一切可命令行启动的 Worker：
//! execute = spawn(command + args) 捕获 stdout/stderr/exit。

use serde_json::{Value, json};

use super::manifest::WorkerManifest;
use super::sdk::{Worker, WorkerError, WorkerResult};

/// Process Worker：command 为可执行文件；input 可携带 `args` 数组附加参数。
pub struct ProcessWorker {
    pub manifest: WorkerManifest,
    command: String,
}

impl ProcessWorker {
    pub fn new(manifest: WorkerManifest, command: &str) -> Self {
        Self {
            manifest,
            command: command.to_string(),
        }
    }

    /// 命令（测试断言用；正常路径经 execute 使用）。
    pub fn command(&self) -> &str {
        &self.command
    }

    /// 可变 manifest（测试/注册前调参）。
    pub fn manifest_mut(&mut self) -> &mut WorkerManifest {
        &mut self.manifest
    }
}

#[async_trait::async_trait]
impl Worker for ProcessWorker {
    fn manifest(&self) -> &WorkerManifest {
        &self.manifest
    }

    async fn execute(&self, _capability: &str, input: Value) -> Result<WorkerResult, WorkerError> {
        let start = std::time::Instant::now();
        let extra: Vec<String> = input
            .get("args")
            .and_then(|a| a.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        let mut cmd = tokio::process::Command::new(&self.command);
        cmd.args(&extra);
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());
        let out = cmd
            .output()
            .await
            .map_err(|e| WorkerError::Execution(format!("进程启动失败 {}: {e}", self.command)))?;
        let stdout = String::from_utf8_lossy(&out.stdout).to_string();
        let stderr = String::from_utf8_lossy(&out.stderr).to_string();
        Ok(WorkerResult {
            ok: out.status.success(),
            output: json!({ "stdout": stdout }),
            stderr: Some(stderr),
            exit_code: out.status.code(),
            duration_ms: start.elapsed().as_millis() as u64,
        })
    }
}
