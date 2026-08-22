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

use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Read};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;

#[cfg(unix)]
use std::os::unix::process::CommandExt;

use serde_json::{Value, json};

use super::adapter::{AgentAdapter, AgentCapability};
use super::task_protocol;
use super::{AgentMessage, FabricError, MessageKind};
use crate::worker::adapters::{codex_worker, dsh_worker, shell_worker};
use crate::worker::process::{ProcessExecutionContext, ProcessWorker};
use crate::worker_core::sdk::Worker;

/// 杀掉进程及其整个进程组（Unix）。
///
/// `Child::kill()` 只杀直接子进程；当命令是 `sh -c "..."` 时，shell 派生的孙子进程
/// （如 `sleep`）会继承 stdout 管道并保持打开——直接子进程被杀后孙子仍存活，管道不会
/// 关闭 → 父进程 `read_line` 等不到 EOF → `stream` 永久阻塞；且 `shutdown`/`cancel`
/// 会遗留孤儿后台 job（与 S2 目标相悖）。故 spawn 时以 `process_group(0)` 把子进程设为
/// 进程组 leader（pgid == pid），此处对整个组发 SIGKILL（`/bin/kill -9 -<pgid>` 即 killpg）。
/// `/bin/kill` 不可用则退化为 `Child::kill()`。`process_group` 仅 Unix 可用，非 Unix 直接
/// `Child::kill()`。
#[cfg(unix)]
fn kill_child_tree(child: &mut Child) {
    let pid = child.id();
    let killed = Command::new("/bin/kill")
        .args(["-9", &format!("-{}", pid)])
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !killed {
        let _ = child.kill();
    }
    let _ = child.wait();
}

#[cfg(not(unix))]
fn kill_child_tree(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

struct ResultMetadata<'a> {
    progress_count: usize,
    duration_ms: u128,
    status_override: Option<&'a str>,
    task_spec: Value,
    workspace_diff: Value,
}

fn structured_output(stdout: &str) -> Value {
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return json!({ "format": "empty", "value": null });
    }
    match serde_json::from_str::<Value>(trimmed) {
        Ok(value) => json!({ "format": "json", "value": value }),
        Err(_) => json!({ "format": "text", "value": stdout }),
    }
}

/// 进程型适配器（Codex / DSH / 任意 CLI Agent 共用执行骨架）。
pub struct ProcessAdapter {
    id: String,
    worker: ProcessWorker,
    requires_isolated_context: bool,
    /// shell 模式：payload 经 shell_args_of 收敛成 `sh -c "<cmd>"`（专用 shell-exec 提供方）。
    shell: bool,
    /// 运行中的流式任务（correlation_id → 子进程句柄；cancel 依据）。
    running: Mutex<HashMap<String, Child>>,
    /// 已被 cancel kill 的 correlation：cancel 在 kill 子进程时写入，stream 在子进程
    /// 结束后读取以确定终态 `killed` vs `failed`/`completed`。以 `running` map 的存在性
    /// 为互斥判据——cancel 与 stream 都先操作 `running`，谁先 remove 谁赢，故「已结束
    /// 任务被延迟 cancel 误标」竞态消除（见 mod.rs `finish` 注释）。
    killed: Mutex<HashSet<String>>,
}

impl ProcessAdapter {
    pub fn new(id: impl Into<String>, worker: ProcessWorker) -> Self {
        let requires_isolated_context = worker.requires_isolated_context()
            || worker.manifest.capabilities.iter().any(|capability| {
                matches!(
                    capability.as_str(),
                    "agent.task" | "code.write" | "shell.execute"
                )
            });
        Self {
            id: id.into(),
            worker,
            requires_isolated_context,
            shell: false,
            running: Mutex::new(HashMap::new()),
            killed: Mutex::new(HashSet::new()),
        }
    }

    fn running(&self) -> Result<std::sync::MutexGuard<'_, HashMap<String, Child>>, FabricError> {
        self.running
            .lock()
            .map_err(|_| FabricError::Adapter("运行表锁中毒".into()))
    }

    fn context(&self, value: &Value) -> Result<ProcessExecutionContext, FabricError> {
        if self.requires_isolated_context && (value.is_null() || value.get("workspace").is_none()) {
            return Err(FabricError::Adapter(
                "执行上下文必须提供 workspace(mode=isolated)".into(),
            ));
        }
        let context =
            ProcessExecutionContext::from_context_value(value).map_err(FabricError::Adapter)?;
        if let Some(allowed) = self.worker.env_allowlist() {
            context
                .validate_env_allowlist(allowed)
                .map_err(FabricError::Adapter)?;
        }
        Ok(context)
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
        if let Some(task) = task_protocol::task_text(payload) {
            args.push(task);
        }
        args
    }

    /// payload → shell 命令行（shell 模式）：收敛成 `sh -c "<cmd>"`。
    /// - payload.command（字符串）→ 直接当 shell 命令（`sh -c "<cmd>"`）
    /// - args[0] == "-c" → 原样透传（已是 `sh -c` 形态，harness 透传兼容）
    /// - 否则 → 空格 join args 后包 `-c`（支持裸 args 形态）
    /// - 退化：无 command/args → task 文本当命令
    fn shell_args_of(payload: &Value) -> Vec<String> {
        if let Some(cmd) = payload.get("command").and_then(|v| v.as_str()) {
            return vec!["-c".into(), cmd.to_string()];
        }
        let args: Vec<String> = payload
            .get("args")
            .and_then(|a| a.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        if args.first().map(|s| s == "-c").unwrap_or(false) {
            return args; // ["-c", "echo ok"] → sh -c "echo ok"
        }
        let joined = args.join(" ");
        if joined.is_empty()
            && let Some(t) = payload.get("task").and_then(|v| v.as_str())
        {
            return vec!["-c".into(), t.to_string()];
        }
        vec!["-c".into(), joined]
    }

    /// 选择参数构造器：shell 模式走 shell_args_of，否则 args_of（契约透传）。
    fn effective_args(&self, payload: &Value) -> Vec<String> {
        if self.shell {
            Self::shell_args_of(payload)
        } else {
            let mut args = self.worker.fixed_args().to_vec();
            args.extend(Self::args_of(payload));
            args
        }
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
    fn run(
        &self,
        args: &[String],
        _payload: &Value,
        context_value: &Value,
    ) -> Result<(String, Option<String>, Option<i32>), FabricError> {
        let context = self.context(context_value)?;
        let mut command = Command::new(self.worker.command());
        command.args(args);
        context.apply_std(&mut command);
        command.stdout(Stdio::piped()).stderr(Stdio::piped());
        if let Some(timeout_ms) = context.timeout_ms {
            #[cfg(unix)]
            command.process_group(0);
            let mut child = command.spawn().map_err(|error| {
                FabricError::Adapter(format!("进程启动失败 {}: {error}", self.worker.command()))
            })?;
            let stdout = child
                .stdout
                .take()
                .ok_or_else(|| FabricError::Adapter("stdout 管道不可用".into()))?;
            let stderr = child
                .stderr
                .take()
                .ok_or_else(|| FabricError::Adapter("stderr 管道不可用".into()))?;
            let stdout_thread = std::thread::spawn(move || {
                let mut bytes = Vec::new();
                let result = BufReader::new(stdout).read_to_end(&mut bytes);
                (result, bytes)
            });
            let stderr_thread = std::thread::spawn(move || {
                let mut bytes = Vec::new();
                let result = BufReader::new(stderr).read_to_end(&mut bytes);
                (result, bytes)
            });
            let deadline = std::time::Instant::now()
                .checked_add(std::time::Duration::from_millis(timeout_ms))
                .unwrap_or_else(std::time::Instant::now);
            let status = loop {
                if let Some(status) = child.try_wait().map_err(|error| {
                    FabricError::Adapter(format!("等待进程失败 {}: {error}", self.worker.command()))
                })? {
                    break status;
                }
                if std::time::Instant::now() >= deadline {
                    kill_child_tree(&mut child);
                    let _ = stdout_thread.join();
                    let _ = stderr_thread.join();
                    return Err(FabricError::Adapter(format!(
                        "进程执行超时 {timeout_ms}ms: {}",
                        self.worker.command()
                    )));
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            };
            let (stdout_result, stdout) = stdout_thread
                .join()
                .map_err(|_| FabricError::Adapter("读取 stdout 线程崩溃".into()))?;
            let (stderr_result, stderr) = stderr_thread
                .join()
                .map_err(|_| FabricError::Adapter("读取 stderr 线程崩溃".into()))?;
            stdout_result.map_err(|error| FabricError::Adapter(format!("读取 stdout: {error}")))?;
            stderr_result.map_err(|error| FabricError::Adapter(format!("读取 stderr: {error}")))?;
            return Ok((
                String::from_utf8_lossy(&stdout).to_string(),
                Some(String::from_utf8_lossy(&stderr).to_string()),
                status.code(),
            ));
        }
        let out = command.output().map_err(|e| {
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
        metadata: ResultMetadata<'_>,
    ) -> AgentMessage {
        let status = match metadata.status_override {
            Some(s) => s,
            None if exit_code == Some(0) => "completed",
            None => "failed",
        };
        let structured_output = structured_output(&stdout);
        let test_summary = task_protocol::test_summary(&stdout);
        let failure_category = match status {
            "completed" => Value::Null,
            "killed" => json!("cancelled"),
            _ if stderr
                .as_deref()
                .is_some_and(|value| value.starts_with("进程执行超时")) =>
            {
                json!("timeout")
            }
            _ => json!("process_exit"),
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
                "progress_count": metadata.progress_count,
                "duration_ms": metadata.duration_ms,
                "structured_output": structured_output,
                "test_summary": test_summary,
                "task_spec": metadata.task_spec,
                "workspace_diff": metadata.workspace_diff,
                "failure_category": failure_category,
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
        let task_spec =
            task_protocol::validate_payload(&msg.payload).map_err(FabricError::Adapter)?;
        let context = self.context(&msg.context)?;
        let before = task_protocol::workspace_snapshot(&context);
        let started = std::time::Instant::now();
        let args = self.effective_args(&msg.payload);
        let (stdout, stderr, exit) = self.run(&args, &msg.payload, &msg.context)?;
        let workspace_diff = task_protocol::workspace_diff(&before, &context);
        Ok(self.result_of(
            msg,
            stdout,
            stderr,
            exit,
            ResultMetadata {
                progress_count: 0,
                duration_ms: started.elapsed().as_millis(),
                status_override: None,
                task_spec,
                workspace_diff,
            },
        ))
    }

    /// 真实流式：逐行 stdout → TaskProgress；进程结束 → 终局 TaskResult。
    fn stream(&self, msg: &AgentMessage) -> Result<Vec<AgentMessage>, FabricError> {
        let corr = msg
            .correlation_id
            .clone()
            .unwrap_or_else(crate::trace::new_trace_id);
        let task_spec =
            task_protocol::validate_payload(&msg.payload).map_err(FabricError::Adapter)?;
        let context = self.context(&msg.context)?;
        let before = task_protocol::workspace_snapshot(&context);
        let started = std::time::Instant::now();
        let mut cmd = Command::new(self.worker.command());
        cmd.args(self.effective_args(&msg.payload));
        context.apply_std(&mut cmd);
        cmd.stdout(Stdio::piped())
            // P0 修复：stderr 若 piped 且子进程大量写 stderr 会双向死锁（OS 管道缓冲满 →
            // 子进程阻塞 → 父进程读 stdout 等不到 EOF）。stream 语义本不捕获 stderr
            // （result_of 已传 None），直接丢弃即可。
            .stderr(Stdio::null());
        // 子进程设为独立进程组 leader（pgid == pid）。cancel/shutdown 时对整个组发
        // SIGKILL，连 `sh -c` 派生的孙子进程（如 `sleep`）一并回收——否则孙子持有
        // stdout 管道 → EOF 不来 → `stream` 永久阻塞，且留下孤儿后台 job（违背 S2 目标）。
        #[cfg(unix)]
        cmd.process_group(0);
        let mut child = cmd.spawn().map_err(|e| {
            FabricError::Adapter(format!("进程启动失败 {}: {e}", self.worker.command()))
        })?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| FabricError::Adapter("stdout 管道不可用".into()))?;
        self.running()?.insert(corr.clone(), child);

        let mut out: Vec<AgentMessage> = Vec::new();
        // 读取线程将阻塞式 stdout 转成可轮询 channel，使 timeout_ms 在 Fabric
        // stream 路径也能生效，而不会被 BufRead::read_line 永久阻塞。
        let (line_tx, line_rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                if line_tx.send(line).is_err() {
                    break;
                }
            }
        });
        // 聚合 stdout：逐行进度行的同时也要让终局结果携带完整 stdout（契约：
        // 下游按 result.stdout 读取执行输出；stream 不丢该字段）。
        let mut captured = String::new();
        let deadline = context.timeout_ms.map(|timeout_ms| {
            std::time::Instant::now()
                .checked_add(std::time::Duration::from_millis(timeout_ms))
                .unwrap_or_else(std::time::Instant::now)
        });
        let mut timed_out = false;
        loop {
            let wait = deadline
                .map(|at| {
                    at.saturating_duration_since(std::time::Instant::now())
                        .min(std::time::Duration::from_millis(100))
                })
                .unwrap_or_else(|| std::time::Duration::from_millis(100));
            match line_rx.recv_timeout(wait) {
                Ok(Ok(line)) => {
                    if line.trim().is_empty() {
                        continue;
                    }
                    captured.push_str(&line);
                    captured.push('\n');
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
                Ok(Err(error)) => {
                    if let Some(mut child) = self.running()?.remove(&corr) {
                        kill_child_tree(&mut child);
                    }
                    return Err(FabricError::Adapter(format!("读取 stdout: {error}")));
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    if deadline.is_some_and(|at| std::time::Instant::now() >= at) {
                        timed_out = true;
                        if let Some(mut child) = self.running()?.remove(&corr) {
                            kill_child_tree(&mut child);
                        }
                        break;
                    }
                }
            }
        }
        // 进程结束：取回句柄 wait（cancel 后 kill 的进程 exit 非 0 → failed）
        let exit = if timed_out {
            None
        } else {
            self.running()?
                .remove(&corr)
                .and_then(|mut c| c.wait().ok())
                .and_then(|s| s.code())
        };
        // 终态判定：被 cancel kill → killed（与 Bridge 选 fabric.task_cancelled 一致）；
        // 否则按退出码（0=completed / 非0=failed）。killed 标记由 cancel 写入，与 running
        // map 移除互斥——stream 已先 remove 则 cancel 不标记，消除「延迟 cancel 误标」竞态。
        let killed = self
            .killed
            .lock()
            .map_err(|_| FabricError::Adapter("killed 锁中毒".into()))?
            .remove(&corr);
        let status = if killed {
            "killed"
        } else if timed_out {
            "failed"
        } else if exit == Some(0) {
            "completed"
        } else {
            "failed"
        };
        let stderr = context
            .timeout_ms
            .filter(|_| timed_out)
            .map(|timeout_ms| format!("进程执行超时 {timeout_ms}ms: {}", self.worker.command()));
        let workspace_diff = task_protocol::workspace_diff(&before, &context);
        out.push(self.result_of(
            msg,
            captured,
            stderr,
            exit,
            ResultMetadata {
                progress_count: out.len(),
                duration_ms: started.elapsed().as_millis(),
                status_override: Some(status),
                task_spec,
                workspace_diff,
            },
        ));
        Ok(out)
    }

    /// 取消：按 correlation_id kill 运行中的子进程（未在跑 = 幂等 Ok）。
    fn cancel(&self, correlation_id: &str) -> Result<bool, FabricError> {
        if let Some(mut child) = self.running()?.remove(correlation_id) {
            // 先标记 killed（供 stream 终态判定），再 kill——二者通过 running map 移除互斥：
            // 若 stream 已先 remove（任务正常结束），此处 running 为空 → 不标记、返回 false（幂等）。
            self.killed
                .lock()
                .map_err(|_| FabricError::Adapter("killed 锁中毒".into()))?
                .insert(correlation_id.to_string());
            // 杀整个进程组（含 shell 派生的孙子），避免孤儿持有 stdout 管道阻塞 stream。
            kill_child_tree(&mut child);
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// 关闭回收（S2）：遍历运行中的后台子进程并 kill（孤儿后台 job 回收）。
    /// 与 cancel 同机制（`std::process::Child::kill` → SIGKILL）；服务退出前由
    /// Facade 优雅关闭透传调用。running map 被 drain 清空（子进程句柄 owned 取出后 kill+wait）。
    fn shutdown(&self) {
        let children: Vec<Child> = self
            .running()
            .map(|mut g| g.drain().map(|(_, c)| c).collect())
            .unwrap_or_default();
        for mut c in children {
            kill_child_tree(&mut c);
        }
    }
}

/// Codex 适配器（能力市场与 codex_worker 同源：code.read/write/test/search）。
pub fn codex_adapter(command: Option<&str>) -> ProcessAdapter {
    ProcessAdapter::new("codex", codex_worker(command))
}

/// DSH 适配器（能力市场与 dsh_worker 同源：agent.task + code.*）。
///
/// 注：dsh 不再声明 shell.execute（B1 决策——harness 不真提供 shell，执行归专用
/// shell-exec；避免「用自身工具链做任务」的假广告与路由竞态）。
pub fn dsh_adapter(command: Option<&str>) -> ProcessAdapter {
    ProcessAdapter::new("dsh", dsh_worker(command))
}

/// Shell Executor 适配器（专用 shell.execute 提供方；B1 决策：harness 撤下
/// shell.execute，执行归专用 shell-exec——复用 tests 已验证的 /bin/sh 通道）。
///
/// command 默认 `/bin/sh`（POSIX 必在）；payload 经 `shell_args_of` 收敛成
/// `sh -c "<cmd>"`：兼容 harness 透传的 `-c "<cmd>"` 形态与裸 args 形态。
pub fn shell_adapter(command: Option<&str>) -> ProcessAdapter {
    let mut a = ProcessAdapter::new("shell", shell_worker(command));
    a.shell = true;
    a
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn structured_output_distinguishes_json_text_and_empty() {
        assert_eq!(
            structured_output(" {\"ok\":true}\n"),
            json!({ "format": "json", "value": { "ok": true } })
        );
        assert_eq!(
            structured_output("plain output\n"),
            json!({ "format": "text", "value": "plain output\n" })
        );
        assert_eq!(
            structured_output(" \n"),
            json!({ "format": "empty", "value": null })
        );
    }

    #[cfg(unix)]
    fn test_script(body: &str) -> std::path::PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = std::env::temp_dir().join(format!(
            "oaip_dsh_adapter_test_{}_{}",
            std::process::id(),
            crate::trace::new_trace_id()
        ));
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        let mut permissions = std::fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&path, permissions).unwrap();
        path
    }

    #[cfg(unix)]
    fn test_request(context: Value) -> AgentMessage {
        AgentMessage {
            id: "dsh-test-request".into(),
            session_id: "dsh-test-session".into(),
            sender: "test".into(),
            receiver: "dsh".into(),
            kind: MessageKind::TaskRequest,
            intent: Some("agent_task".into()),
            payload: json!({}),
            context,
            capability: Some("agent.task".into()),
            correlation_id: Some("dsh-test-correlation".into()),
            causation_id: None,
            timestamp: 0,
        }
    }

    #[cfg(unix)]
    #[test]
    fn dsh_stream_reports_structured_output_and_duration() {
        let path = test_script("printf '%s\n' '{\"ok\":true}'");
        let root = std::env::temp_dir().to_string_lossy().into_owned();
        let request = test_request(json!({
            "workspace": { "root": root, "mode": "isolated" }
        }));
        let adapter = dsh_adapter(Some(path.to_str().unwrap()));
        let messages = adapter.stream(&request).unwrap();
        let result = messages.last().unwrap();
        assert_eq!(result.payload["status"], "completed");
        assert_eq!(result.payload["exit_code"], 0);
        assert!(result.payload["duration_ms"].is_number());
        assert_eq!(
            result.payload["structured_output"],
            json!({
                "format": "json",
                "value": { "ok": true }
            })
        );
        assert_eq!(result.payload["failure_category"], Value::Null);
        std::fs::remove_file(path).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn dsh_structured_task_reports_workspace_diff_and_test_summary() {
        let root = std::env::temp_dir().join(format!("oaip_dsh_workspace_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("changed.txt"), "before").unwrap();
        std::fs::write(root.join("deleted.txt"), "gone").unwrap();
        let path = test_script(
            "printf after > changed.txt; printf new > added.txt; rm deleted.txt; printf '%s\n' '{\"test_summary\":{\"passed\":3,\"failed\":0,\"skipped\":1}}'",
        );
        let request = AgentMessage {
            id: "dsh-structured-request".into(),
            session_id: "dsh-structured-session".into(),
            sender: "test".into(),
            receiver: "dsh".into(),
            kind: MessageKind::TaskRequest,
            intent: Some("modify_code".into()),
            payload: json!({
                "task_spec": {
                    "description": "修复订单金额计算",
                    "kind": "code_change",
                    "acceptance": { "run_tests": true }
                }
            }),
            context: json!({
                "workspace": { "root": root, "mode": "isolated" }
            }),
            capability: Some("code.write".into()),
            correlation_id: Some("dsh-structured-correlation".into()),
            causation_id: None,
            timestamp: 0,
        };
        let result = dsh_adapter(Some(path.to_str().unwrap()))
            .stream(&request)
            .unwrap()
            .pop()
            .unwrap();
        assert_eq!(result.payload["task_spec"]["kind"], "code_change");
        assert_eq!(
            result.payload["workspace_diff"]["added"],
            json!(["added.txt"])
        );
        assert_eq!(
            result.payload["workspace_diff"]["modified"],
            json!(["changed.txt"])
        );
        assert_eq!(
            result.payload["workspace_diff"]["deleted"],
            json!(["deleted.txt"])
        );
        assert_eq!(result.payload["test_summary"]["passed"], 3);
        assert_eq!(result.payload["test_summary"]["skipped"], 1);
        std::fs::remove_file(path).unwrap();
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn dsh_rejects_invalid_task_protocol_before_spawn() {
        let request = test_request(json!({
            "workspace": { "root": std::env::temp_dir(), "mode": "isolated" }
        }));
        let mut request = request;
        request.payload = json!({ "task_spec": { "description": "", "kind": "code_change" } });
        let error = dsh_adapter(Some("/bin/echo")).stream(&request).unwrap_err();
        assert!(error.to_string().contains("task_spec.description"));
    }

    #[cfg(unix)]
    #[test]
    fn dsh_stream_timeout_is_terminal_and_classified() {
        let path = test_script("sleep 2");
        let root = std::env::temp_dir().to_string_lossy().into_owned();
        let request = test_request(json!({
            "workspace": { "root": root, "mode": "isolated" },
            "timeout_ms": 40
        }));
        let adapter = dsh_adapter(Some(path.to_str().unwrap()));
        let messages = adapter.stream(&request).unwrap();
        let result = messages.last().unwrap();
        assert_eq!(result.payload["status"], "failed");
        assert_eq!(result.payload["exit_code"], Value::Null);
        assert_eq!(result.payload["failure_category"], "timeout");
        assert!(result.payload["duration_ms"].is_number());
        std::fs::remove_file(path).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn dsh_cancel_reports_killed_without_completed_race() {
        use std::sync::Arc;
        use std::time::Duration;
        let path = test_script("sleep 2");
        let root = std::env::temp_dir().to_string_lossy().into_owned();
        let request = test_request(json!({
            "workspace": { "root": root, "mode": "isolated" }
        }));
        let adapter = Arc::new(dsh_adapter(Some(path.to_str().unwrap())));
        let running = Arc::clone(&adapter);
        let handle = std::thread::spawn(move || running.stream(&request));
        std::thread::sleep(Duration::from_millis(80));
        assert!(adapter.cancel("dsh-test-correlation").unwrap());
        let messages = handle.join().unwrap().unwrap();
        let result = messages.last().unwrap();
        assert_eq!(result.payload["status"], "killed");
        assert_eq!(result.payload["failure_category"], "cancelled");
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn dsh_context_rejects_unallowlisted_environment() {
        let root = std::env::temp_dir().to_string_lossy().into_owned();
        let adapter = dsh_adapter(Some("/bin/echo"));
        let allowed = json!({
            "workspace": { "root": root, "mode": "isolated" },
            "env": {
                "PATH": "/usr/bin",
                "HOME": "/tmp",
                "DSH_HOME": "/tmp/dsh",
                "DEEPSEEK_API_KEY": "test",
                "NO_COLOR": "1",
                "TERM": "dumb",
                "LANG": "C",
                "LC_ALL": "C"
            }
        });
        adapter
            .context(&allowed)
            .expect("allowlisted environment should pass");

        let rejected = json!({
            "workspace": { "root": root, "mode": "isolated" },
            "env": { "SECRET_TOKEN": "must-not-cross-provider-boundary" }
        });
        let error = adapter
            .context(&rejected)
            .expect_err("unknown env must fail closed");
        assert!(error.to_string().contains("SECRET_TOKEN"));
    }
}
