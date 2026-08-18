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

/// Pi Agent Worker 适配器（`@earendil-works/pi-coding-agent`）。
///
/// 角色边界（与 Cognitive Engine 形态互补）：
/// - **Thinker 形态**（`PiAgentEngine`，src/agent.rs）：`--no-tools` 禁工具，只做 NLU/推理
/// - **Worker 形态**（本适配器）：`--mode text --print --no-session` 放开 read/bash/edit/write 工具，
///   经 Process 协议被 Router 调度去执行任务
///
/// 已验证真实可执行（非交互单次任务模式）。能力对应 pi 的工具面（code.*）；
/// shell.execute 已撤下（B1——shell 执行归专用 shell-exec，harness 不假广告）。
pub fn pi_worker(command: Option<&str>) -> ProcessWorker {
    let mut m = super::manifest::WorkerManifest::new("pi-agent", "agent")
        .with_capabilities(&["code.read", "code.write", "code.test", "code.search"])
        .with_quality(0.88)
        .with_risk(3)
        .with_trust(0.85)
        .with_availability("local");
    m.execution = ExecutionProtocol::Process;
    ProcessWorker::new(m, command.unwrap_or("pi"))
}

/// DSH（DeepSeek Harness）Worker 适配器（Phase 11D：Harness 是 Worker，不是内核）。
///
/// 经 Process 协议接入：`dsh --profile headless "<任务>"`（answer one task, print, exit）——
/// 任务文本经 input.args 传入。已验证真实可执行（headless 单次任务模式）。
/// MCP/HTTP 为可选接入协议；command 可配置，不绑定具体二进制。
pub fn dsh_worker(command: Option<&str>) -> ProcessWorker {
    let mut m = super::manifest::WorkerManifest::new("dsh-harness", "agent")
        .with_capabilities(&[
            "agent.task",
            "code.read",
            "code.write",
            "code.test",
            "code.search",
        ])
        .with_quality(0.9)
        .with_risk(3)
        .with_trust(0.9)
        .with_availability("local");
    m.execution = ExecutionProtocol::Process;
    ProcessWorker::new(m, command.unwrap_or("dsh"))
}

/// DeepSeek Harness Worker 适配器壳（Phase 11D：Harness 是 Worker，不是内核）。
/// 经 Process 协议接入（MCP/HTTP 为可选接入协议）；command 可配置为
/// deepseek-harness CLI / 包装脚本；未安装时由调用方提供替代命令。
pub fn deepseek_worker(command: Option<&str>) -> ProcessWorker {
    let mut m = super::manifest::WorkerManifest::new("deepseek-harness", "agent")
        .with_capabilities(&["code.read", "code.write", "code.test", "code.search"])
        .with_quality(0.9)
        .with_risk(3)
        .with_trust(0.9)
        .with_availability("local");
    m.execution = ExecutionProtocol::Process;
    ProcessWorker::new(m, command.unwrap_or("deepseek"))
}

/// Shell Executor Worker（专用 shell.execute 提供方；B1 决策：harness 撤下 shell.execute，
/// 执行归专用 shell-exec——复用 tests 已验证的 /bin/sh 通道）。
///
/// 只声明 `shell.execute`，不做「用自身工具链做任务」的假广告；命令可配置，
/// 默认 `/bin/sh`（POSIX 必在）。与 codex/dsh 同为 Capability Provider，由 Router 调度。
pub fn shell_worker(command: Option<&str>) -> ProcessWorker {
    let mut m = super::manifest::WorkerManifest::new("shell-exec", "cli")
        .with_capabilities(&["shell.execute"])
        .with_quality(0.99)
        .with_risk(3)
        .with_trust(0.9)
        .with_availability("local");
    m.execution = ExecutionProtocol::Process;
    ProcessWorker::new(m, command.unwrap_or("/bin/sh"))
}

/// 便捷断言（防未使用）。
#[allow(dead_code)]
fn _is_worker(w: &ProcessWorker) -> bool {
    w.manifest().worker_type == "agent"
}

#[cfg(test)]
mod tests {
    use super::*;

    /// DSH 适配器声明（能力市场身份证）——纯内存，可进 CI。
    #[test]
    fn dsh_worker_manifest_declares_capability_market() {
        let w = dsh_worker(None);
        let m = w.manifest();
        assert_eq!(m.id, "dsh-harness");
        assert_eq!(m.worker_type, "agent");
        assert!(m.supports("agent.task"), "通用 agent 任务入口");
        assert!(
            m.supports("code.write"),
            "代码能力就位（shell.execute 已撤下）"
        );
        assert_eq!(m.risk, 3);
        assert!((m.trust - 0.9).abs() < 1e-9);
        assert_eq!(m.availability, "local");
        assert_eq!(m.execution, ExecutionProtocol::Process);
        // 命令可覆盖（不绑定二进制）
        let overridden = dsh_worker(Some("/custom/dsh"));
        assert_eq!(overridden.command(), "/custom/dsh");
    }

    /// Codex 适配器声明（能力市场身份证）——纯内存，可进 CI。
    #[test]
    fn codex_worker_manifest_declares_capability_market() {
        let w = codex_worker(None);
        let m = w.manifest();
        assert_eq!(m.id, "codex");
        assert!(m.supports("code.read") && m.supports("code.write") && m.supports("code.test"));
        assert_eq!(m.execution, ExecutionProtocol::Process);
        let overridden = codex_worker(Some("/custom/codex"));
        assert_eq!(overridden.command(), "/custom/codex");
    }

    /// Pi Agent 适配器声明（Worker 形态身份；Thinker 形态在 agent.rs）——进 CI。
    #[test]
    fn pi_worker_manifest_declares_capability_market() {
        let w = pi_worker(None);
        let m = w.manifest();
        assert_eq!(m.id, "pi-agent");
        assert!(
            m.supports("code.read"),
            "代码能力就位（shell.execute 已撤下）"
        );
        assert_eq!(m.risk, 3);
        assert_eq!(m.availability, "local");
        assert_eq!(m.execution, ExecutionProtocol::Process);
        let overridden = pi_worker(Some("/custom/pi"));
        assert_eq!(overridden.command(), "/custom/pi");
    }
}
