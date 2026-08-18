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
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;

#[cfg(unix)]
use std::os::unix::process::CommandExt;

use serde_json::{Value, json};

use super::adapter::{AgentAdapter, AgentCapability};
use super::{AgentMessage, FabricError, MessageKind};
use crate::worker::adapters::{codex_worker, dsh_worker, shell_worker};
use crate::worker::process::ProcessWorker;
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

/// 进程型适配器（Codex / DSH / 任意 CLI Agent 共用执行骨架）。
pub struct ProcessAdapter {
    id: String,
    worker: ProcessWorker,
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
        Self {
            id: id.into(),
            worker,
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
            Self::args_of(payload)
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
        status_override: Option<&str>,
    ) -> AgentMessage {
        let status = match status_override {
            Some(s) => s,
            None if exit_code == Some(0) => "completed",
            None => "failed",
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
        let args = self.effective_args(&msg.payload);
        let (stdout, stderr, exit) = self.run(&args)?;
        Ok(self.result_of(msg, stdout, stderr, exit, 0, None))
    }

    /// 真实流式：逐行 stdout → TaskProgress；进程结束 → 终局 TaskResult。
    fn stream(&self, msg: &AgentMessage) -> Result<Vec<AgentMessage>, FabricError> {
        let corr = msg
            .correlation_id
            .clone()
            .unwrap_or_else(crate::trace::new_trace_id);
        let mut cmd = Command::new(self.worker.command());
        cmd.args(self.effective_args(&msg.payload))
            .stdout(Stdio::piped())
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
        // 聚合 stdout：逐行进度行的同时也要让终局结果携带完整 stdout（契约：
        // 下游按 result.stdout 读取执行输出；stream 不丢该字段）。
        let mut captured = String::new();
        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            let line = line.map_err(|e| FabricError::Adapter(format!("读取 stdout: {e}")))?;
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
        // 进程结束：取回句柄 wait（cancel 后 kill 的进程 exit 非 0 → failed）
        let exit = self
            .running()?
            .remove(&corr)
            .and_then(|mut c| c.wait().ok())
            .and_then(|s| s.code());
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
        } else if exit == Some(0) {
            "completed"
        } else {
            "failed"
        };
        out.push(self.result_of(msg, captured, None, exit, out.len(), Some(status)));
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
