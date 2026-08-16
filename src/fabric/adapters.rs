//! 真实 Agent Adapter（ABI-3 落地）：桥接现有 Worker 层——ProcessWorker
//! 作为执行连接器（manifest 归属），Adapter 管协议（AgentMessage ↔ 进程原语）。
//!
//! 执行模型（OAB v0.1）：
//! - **同步 std::process**：阻塞上下文约定（server=spawn_blocking、
//!   CLI=block_in_place，与 StorageSink 同约定）；不依赖 async runtime。
//! - `send`：spawn → 等结束 → task.result（status=completed/failed）。
//! - `stream`：spawn 拿句柄 → 逐行 stdout → TaskProgress 序列 → 终局 TaskResult。
//! - `cancel`：按 correlation_id kill 运行中子进程。
//! - payload 约定：`args` 数组透传为命令行参数；`task` 文本追加为末参。
//!
//! Codex / DSH 都是 **Capability Provider**（discover 来自 WorkerManifest），
//! 不是特殊公民——新 Agent 接入 = 一个 adapter 工厂函数。

use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;

use serde_json::{Value, json};

use super::adapter::{AgentAdapter, AgentCapability};
use super::{AgentMessage, FabricError, MessageKind};
use crate::worker::adapters::{codex_worker, dsh_worker};
use crate::worker::process::ProcessWorker;
use crate::worker_core::sdk::Worker;

/// 进程型适配器（Codex / DSH / 任意 CLI Agent 共用执行骨架）。
pub struct ProcessAdapter {
    id: String,
    worker: ProcessWorker,
    /// 运行中的流式任务（correlation_id → 子进程句柄；cancel 依据）。
    running: Mutex<HashMap<String, Child>>,
}

impl ProcessAdapter {
    pub fn new(id: impl Into<String>, worker: ProcessWorker) -> Self {
        Self {
            id: id.into(),
            worker,
            running: Mutex::new(HashMap::new()),
        }
    }

    fn running(&self) -> Result<std::sync::MutexGuard<'_, HashMap<String, Child>>, FabricError> {
        self.running
            .lock()
            .map_err(|_| FabricError::Adapter("运行表锁中毒".into()))
    }

    /// payload → 命令行参数（契约：args 数组透传；task 文本追加末参）。
    fn args_of(payload: &Value) -> Vec<String> {
        let mut args: Vec<String> = payload
            .get("args")
            .and_then(|a| a.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        if let Some(t) = payload.get("task").and_then(|v| v.as_str()) {
            args.push(t.to_string());
        }
        args
    }

    /// 命令是否可解析（PATH 探测；connect 握手用——不启动进程）。
    fn command_exists(cmd: &str) -> bool {
        if cmd.contains('/') {
            std::path::Path::new(cmd).exists()
        } else {
            std::env::var_os("PATH")
                .map(|p| std::env::split_paths(&p).any(|d| d.join(cmd).exists()))
                .unwrap_or(false)
        }
    }

    /// 同步执行（send 底层）：spawn + 等结束 → (stdout, stderr, exit_code)。
    fn run(&self, args: &[String]) -> Result<(String, Option<String>, Option<i32>), FabricError> {
        let out = Command::new(self.worker.command())
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .map_err(|e| {
                FabricError::Adapter(format!("进程启动失败 {}: {e}", self.worker.command()))
            })?;
        Ok((
            String::from_utf8_lossy(&out.stdout).to_string(),
            Some(String::from_utf8_lossy(&out.stderr).to_string()),
            out.status.code(),
        ))
    }

    /// Worker 输出 → task.result（协议翻译：进程退出码 → 语义 status）。
    fn result_of(
        &self,
        req: &AgentMessage,
        stdout: String,
        stderr: Option<String>,
        exit_code: Option<i32>,
        progress_count: usize,
    ) -> AgentMessage {
        let status = if exit_code == Some(0) {
            "completed"
        } else {
            "failed"
        };
        AgentMessage {
            id: crate::trace::new_trace_id(),
            session_id: req.session_id.clone(),
            sender: self.id.clone(),
            receiver: req.sender.clone(),
            kind: MessageKind::TaskResult,
            intent: req.intent.clone(),
            payload: json!({
                "status": status,
                "stdout": stdout,
                "stderr": stderr,
                "exit_code": exit_code,
                "progress_count": progress_count,
            }),
            context: Value::Null,
            capability: req.capability.clone(),
            correlation_id: req.correlation_id.clone(),
            causation_id: Some(req.id.clone()),
            timestamp: crate::kernel::now_secs(),
        }
    }
}

impl AgentAdapter for ProcessAdapter {
    fn id(&self) -> &str {
        &self.id
    }

    fn connect(&self) -> Result<(), FabricError> {
        if Self::command_exists(self.worker.command()) {
            Ok(())
        } else {
            Err(FabricError::Adapter(format!(
                "命令不可用: {}",
                self.worker.command()
            )))
        }
    }

    fn discover(&self) -> Vec<AgentCapability> {
        let m = self.worker.manifest();
        m.capabilities
            .iter()
            .map(|c| AgentCapability {
                name: c.clone(),
                agent_id: self.id.clone(),
                quality: m.quality,
                risk: m.risk,
            })
            .collect()
    }

    fn send(&self, msg: &AgentMessage) -> Result<AgentMessage, FabricError> {
        let args = Self::args_of(&msg.payload);
        let (stdout, stderr, exit) = self.run(&args)?;
        Ok(self.result_of(msg, stdout, stderr, exit, 0))
    }

    /// 真实流式：逐行 stdout → TaskProgress；进程结束 → 终局 TaskResult。
    fn stream(&self, msg: &AgentMessage) -> Result<Vec<AgentMessage>, FabricError> {
        let corr = msg
            .correlation_id
            .clone()
            .unwrap_or_else(crate::trace::new_trace_id);
        let mut child = Command::new(self.worker.command())
            .args(Self::args_of(&msg.payload))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                FabricError::Adapter(format!("进程启动失败 {}: {e}", self.worker.command()))
            })?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| FabricError::Adapter("stdout 管道不可用".into()))?;
        self.running()?.insert(corr.clone(), child);

        let mut out: Vec<AgentMessage> = Vec::new();
        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            let line = line.map_err(|e| FabricError::Adapter(format!("读取 stdout: {e}")))?;
            if line.trim().is_empty() {
                continue;
            }
            out.push(AgentMessage {
                id: crate::trace::new_trace_id(),
                session_id: msg.session_id.clone(),
                sender: self.id.clone(),
                receiver: msg.sender.clone(),
                kind: MessageKind::TaskProgress,
                intent: msg.intent.clone(),
                payload: json!({ "progress": line }),
                context: Value::Null,
                capability: msg.capability.clone(),
                correlation_id: msg.correlation_id.clone(),
                causation_id: Some(msg.id.clone()),
                timestamp: crate::kernel::now_secs(),
            });
        }
        // 进程结束：取回句柄 wait（cancel 后 kill 的进程 exit 非 0 → failed）
        let exit = self
            .running()?
            .remove(&corr)
            .and_then(|mut c| c.wait().ok())
            .and_then(|s| s.code());
        out.push(self.result_of(msg, String::new(), None, exit, out.len()));
        Ok(out)
    }

    /// 取消：按 correlation_id kill 运行中的子进程（未在跑 = 幂等 Ok）。
    fn cancel(&self, correlation_id: &str) -> Result<(), FabricError> {
        if let Some(mut child) = self.running()?.remove(correlation_id) {
            child
                .kill()
                .map_err(|e| FabricError::Adapter(format!("kill 失败: {e}")))?;
            let _ = child.wait();
        }
        Ok(())
    }
}

/// Codex 适配器（能力市场与 codex_worker 同源：code.read/write/test/search）。
pub fn codex_adapter(command: Option<&str>) -> ProcessAdapter {
    ProcessAdapter::new("codex", codex_worker(command))
}

/// DSH 适配器（能力市场与 dsh_worker 同源：agent.task + code.* + shell.execute）。
pub fn dsh_adapter(command: Option<&str>) -> ProcessAdapter {
    ProcessAdapter::new("dsh", dsh_worker(command))
}
