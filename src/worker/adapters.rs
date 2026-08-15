//! Worker 适配器壳 —— Codex / Claude Code 等现成 Agent Worker。
//!
//! 壳 = WorkerManifest（能力声明）+ ProcessWorker（真实进程执行）。
//! **不绑定具体二进制**：command 可配置；未安装时由调用方提供替代命令。

use super::manifest::ExecutionProtocol;
use super::process::ProcessWorker;
use super::sdk::Worker;

/// Codex Worker 适配器（code.read / code.write / code.test / code.search）。
pub fn codex_worker(command: Option<&str>) -> ProcessWorker {
    let mut m = super::manifest::WorkerManifest::new("codex", "agent")
        .with_capabilities(&["code.read", "code.write", "code.test", "code.search"])
        .with_quality(0.95);
    m.execution = ExecutionProtocol::Process;
    ProcessWorker::new(m, command.unwrap_or("codex"))
}

/// Claude Code Worker 适配器（同能力集，独立执行主体）。
pub fn claude_worker(command: Option<&str>) -> ProcessWorker {
    let mut m = super::manifest::WorkerManifest::new("claude-code", "agent")
        .with_capabilities(&["code.read", "code.write", "code.test", "code.search"])
        .with_quality(0.93);
    m.execution = ExecutionProtocol::Process;
    ProcessWorker::new(m, command.unwrap_or("claude"))
}

/// DeepSeek Harness Worker 适配器壳（Phase 11D：Harness 是 Worker，不是内核）。
/// 经 Process 协议接入（MCP/HTTP 为可选接入协议）；command 可配置为
/// deepseek-harness CLI / 包装脚本；未安装时由调用方提供替代命令。
pub fn deepseek_worker(command: Option<&str>) -> ProcessWorker {
    let mut m = super::manifest::WorkerManifest::new("deepseek-harness", "agent")
        .with_capabilities(&[
            "code.read",
            "code.write",
            "code.test",
            "code.search",
            "shell.execute",
        ])
        .with_quality(0.9)
        .with_risk(3)
        .with_trust(0.9)
        .with_availability("local");
    m.execution = ExecutionProtocol::Process;
    ProcessWorker::new(m, command.unwrap_or("deepseek"))
}

/// 便捷断言（防未使用）。
#[allow(dead_code)]
fn _is_worker(w: &ProcessWorker) -> bool {
    w.manifest().worker_type == "agent"
}
