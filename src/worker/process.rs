//! ProcessWorker —— 通用 CLI Worker（真实进程执行）。
//!
//! 适配 Codex / Claude Code / git / ffmpeg / python 等一切可命令行启动的 Worker：
//! execute = spawn(command + args) 捕获 stdout/stderr/exit。

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::manifest::WorkerManifest;
use super::sdk::{Worker, WorkerError, WorkerResult};

/// Every process execution has a bounded lifetime, including callers that omit
/// an explicit execution context. Five minutes matches the approval lease and
/// prevents an unbounded child from consuming the local execution plane.
pub const MAX_EXECUTION_TIMEOUT_MS: u64 = 300_000;

/// Facade/Fabric 的正式执行上下文 DTO。
///
/// 执行上下文不再混在任务 payload 中。文件型任务必须显式声明
/// `workspace.mode=isolated`，Process 层会再次做路径与文件系统校验。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ExecutionContext {
    pub workspace: ExecutionWorkspace,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ExecutionWorkspace {
    pub root: String,
    pub mode: WorkspaceMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WorkspaceMode {
    Isolated,
}

/// Workspace 约束下的进程执行上下文。
#[derive(Debug, Clone)]
pub struct ProcessExecutionContext {
    cwd: Option<PathBuf>,
    workspace_root: Option<PathBuf>,
    env: Vec<(String, String)>,
    isolated: bool,
    pub timeout_ms: Option<u64>,
}

impl ProcessExecutionContext {
    /// 从统一输入解析正式 `execution_context`。旧的 payload 顶层执行字段直接拒绝，
    /// 不保留兼容路径，避免调用方误以为上下文已被执行器采纳。
    pub fn from_input(input: &Value) -> Result<Self, String> {
        for key in ["workspace", "cwd", "env", "timeout_ms"] {
            if input.get(key).is_some() {
                return Err(format!("执行上下文必须位于 execution_context.{key}"));
            }
        }
        let Some(value) = input.get("execution_context") else {
            return Ok(Self::unisolated());
        };
        Self::from_context_value(value)
    }

    /// 从 AgentMessage.context 解析正式执行上下文。
    pub fn from_context_value(value: &Value) -> Result<Self, String> {
        if value.is_null() {
            return Ok(Self::unisolated());
        }
        let context: ExecutionContext = serde_json::from_value(value.clone())
            .map_err(|error| format!("execution_context 无效: {error}"))?;
        Self::from_execution_context(&context)
    }

    fn unisolated() -> Self {
        Self {
            cwd: None,
            workspace_root: None,
            env: Vec::new(),
            isolated: false,
            timeout_ms: Some(MAX_EXECUTION_TIMEOUT_MS),
        }
    }

    /// 将 Facade/Fabric DTO 解析为已验证的进程上下文。
    pub fn from_execution_context(context: &ExecutionContext) -> Result<Self, String> {
        let root_path = Path::new(&context.workspace.root);
        if !root_path.is_absolute() {
            return Err("execution_context.workspace.root 必须是绝对路径".to_string());
        }
        let root = root_path
            .canonicalize()
            .map_err(|error| format!("workspace.root 不可用: {error}"))?;
        if !root.is_dir() {
            return Err(format!("workspace.root 不是目录: {}", root.display()));
        }

        if let Some(timeout_ms) = context.timeout_ms {
            if timeout_ms == 0 {
                return Err("execution_context.timeout_ms 必须是正整数".to_string());
            }
            if timeout_ms > MAX_EXECUTION_TIMEOUT_MS {
                return Err(format!(
                    "execution_context.timeout_ms 超过硬上限 {}ms",
                    MAX_EXECUTION_TIMEOUT_MS
                ));
            }
        }
        let requested_cwd = context
            .cwd
            .as_deref()
            .map(PathBuf::from)
            .unwrap_or_else(|| root.clone());
        let candidate = if requested_cwd.is_absolute() {
            requested_cwd
        } else {
            root.join(requested_cwd)
        };
        let cwd = candidate
            .canonicalize()
            .map_err(|error| format!("cwd 不可用: {error}"))?;
        if !cwd.is_dir() {
            return Err(format!("cwd 不是目录: {}", cwd.display()));
        }
        if cwd != root && !cwd.starts_with(&root) {
            return Err(format!(
                "cwd 越出 workspace.root: {} 不在 {} 内",
                cwd.display(),
                root.display()
            ));
        }

        Ok(Self {
            cwd: Some(cwd),
            workspace_root: Some(root),
            env: context
                .env
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect(),
            isolated: true,
            timeout_ms: Some(context.timeout_ms.unwrap_or(MAX_EXECUTION_TIMEOUT_MS)),
        })
    }

    pub fn cwd(&self) -> Option<&Path> {
        self.cwd.as_deref()
    }

    pub fn workspace_root(&self) -> Option<&Path> {
        self.workspace_root.as_deref()
    }

    pub fn isolated(&self) -> bool {
        self.isolated
    }

    /// Reject environment variables outside a provider-owned allowlist.
    ///
    /// The generic process worker intentionally keeps explicit environment
    /// forwarding flexible. Providers that execute privileged or stateful
    /// agents can opt into a narrower policy without changing the shared
    /// execution-context format.
    pub fn validate_env_allowlist(&self, allowed: &[&str]) -> Result<(), String> {
        if let Some((key, _)) = self
            .env
            .iter()
            .find(|(key, _)| !allowed.contains(&key.as_str()))
        {
            return Err(format!("执行上下文环境变量不被允许: {key}"));
        }
        Ok(())
    }

    fn env_value(&self, key: &str) -> Option<&str> {
        self.env
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.as_str())
    }

    /// 应用到 Tokio 进程命令。isolated 模式显式清理宿主环境，只保留安全 PATH、
    /// workspace 作为默认 HOME，以及 payload 明确声明的环境变量。
    pub fn apply_tokio(&self, command: &mut tokio::process::Command) {
        if let Some(cwd) = &self.cwd {
            command.current_dir(cwd);
        }
        if self.isolated {
            command.env_clear();
            command.env("PATH", self.env_value("PATH").unwrap_or(SAFE_PATH));
            command.env(
                "HOME",
                self.env_value("HOME")
                    .or_else(|| self.workspace_root.as_deref().and_then(Path::to_str))
                    .unwrap_or("/"),
            );
        }
        for (key, value) in &self.env {
            command.env(key, value);
        }
    }

    /// 应用到标准库进程命令。
    pub fn apply_std(&self, command: &mut std::process::Command) {
        if let Some(cwd) = &self.cwd {
            command.current_dir(cwd);
        }
        if self.isolated {
            command.env_clear();
            command.env("PATH", self.env_value("PATH").unwrap_or(SAFE_PATH));
            command.env(
                "HOME",
                self.env_value("HOME")
                    .or_else(|| self.workspace_root.as_deref().and_then(Path::to_str))
                    .unwrap_or("/"),
            );
        }
        for (key, value) in &self.env {
            command.env(key, value);
        }
    }
}

const SAFE_PATH: &str = "/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin";

/// Process Worker：command 为可执行文件；input 可携带 `args` 与正式的
/// `execution_context`。每次执行都受 `MAX_EXECUTION_TIMEOUT_MS` 硬上限约束；
/// Provider 自己的固定参数始终先于调用方参数。
pub struct ProcessWorker {
    pub manifest: WorkerManifest,
    command: String,
    fixed_args: Vec<String>,
    env_allowlist: Option<&'static [&'static str]>,
    requires_isolated_context: bool,
}

impl ProcessWorker {
    pub fn new(manifest: WorkerManifest, command: &str) -> Self {
        Self {
            manifest,
            command: command.to_string(),
            fixed_args: Vec::new(),
            env_allowlist: None,
            requires_isolated_context: false,
        }
    }

    /// Create a process worker with provider-owned arguments that are always
    /// placed before caller-supplied arguments.
    pub fn with_fixed_args<I, S>(manifest: WorkerManifest, command: &str, fixed_args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            manifest,
            command: command.to_string(),
            fixed_args: fixed_args.into_iter().map(Into::into).collect(),
            env_allowlist: None,
            requires_isolated_context: false,
        }
    }

    /// Restrict the environment accepted by this provider before a process is
    /// spawned. The same policy is used by both the Worker and Fabric paths.
    pub fn with_env_allowlist(mut self, allowed: &'static [&'static str]) -> Self {
        self.env_allowlist = Some(allowed);
        self
    }

    /// Require callers to provide an explicit isolated execution context.
    pub fn requiring_isolated_context(mut self) -> Self {
        self.requires_isolated_context = true;
        self
    }

    pub fn env_allowlist(&self) -> Option<&'static [&'static str]> {
        self.env_allowlist
    }

    pub fn requires_isolated_context(&self) -> bool {
        self.requires_isolated_context
    }

    /// 命令（测试断言用；正常路径经 execute 使用）。
    pub fn command(&self) -> &str {
        &self.command
    }

    /// Provider-owned command arguments. These are not caller-controlled.
    pub fn fixed_args(&self) -> &[String] {
        &self.fixed_args
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
        if self.requires_isolated_context
            && input
                .get("execution_context")
                .is_none_or(serde_json::Value::is_null)
        {
            return Err(WorkerError::Execution(
                "执行上下文必须提供 workspace(mode=isolated)".to_string(),
            ));
        }
        let context =
            ProcessExecutionContext::from_input(&input).map_err(WorkerError::Execution)?;
        if let Some(allowed) = self.env_allowlist {
            context
                .validate_env_allowlist(allowed)
                .map_err(WorkerError::Execution)?;
        }
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
        cmd.args(&self.fixed_args);
        cmd.args(&extra);
        context.apply_tokio(&mut cmd);
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());
        // The output future owns the child process. kill_on_drop makes a timeout
        // terminate the process instead of leaving a detached child behind.
        cmd.kill_on_drop(true);
        #[cfg(unix)]
        cmd.process_group(0);
        let child = cmd
            .spawn()
            .map_err(|e| WorkerError::Execution(format!("进程启动失败 {}: {e}", self.command)))?;
        let child_pid = child.id();
        let output = if let Some(timeout_ms) = context.timeout_ms {
            match tokio::time::timeout(
                std::time::Duration::from_millis(timeout_ms),
                child.wait_with_output(),
            )
            .await
            {
                Ok(output) => output,
                Err(_) => {
                    #[cfg(unix)]
                    if let Some(pid) = child_pid {
                        terminate_process_group(pid);
                    }
                    return Err(WorkerError::Execution(format!(
                        "进程执行超时 {}ms: {}",
                        timeout_ms, self.command
                    )));
                }
            }
        } else {
            child.wait_with_output().await
        };
        let out = output
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

#[cfg(unix)]
fn terminate_process_group(pid: u32) {
    let _ = std::process::Command::new("/bin/kill")
        .args(["-9", &format!("-{pid}")])
        .stderr(std::process::Stdio::null())
        .status();
}
