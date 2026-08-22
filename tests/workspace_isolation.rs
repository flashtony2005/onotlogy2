//! Phase 2 Workspace/Process 契约：真实进程、路径边界与干净环境。

use openaip_os::fabric::adapter::AgentAdapter;
use openaip_os::fabric::adapters::{ProcessAdapter, dsh_adapter};
use openaip_os::fabric::{AgentMessage, MessageKind};
use openaip_os::worker::manifest::{ExecutionProtocol, WorkerManifest};
use openaip_os::worker::process::ProcessWorker;
use openaip_os::worker::sdk::{Worker, WorkerError};
use serde_json::json;
use std::path::PathBuf;

fn workspace(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("oaip_phase2_{name}_{}", std::process::id()));
    std::fs::remove_dir_all(&root).ok();
    std::fs::create_dir_all(root.join("nested")).unwrap();
    root
}

fn shell_worker() -> ProcessWorker {
    let mut manifest = WorkerManifest::new("shell-contract", "cli");
    manifest.execution = ExecutionProtocol::Process;
    ProcessWorker::new(manifest, "/bin/sh")
}

#[tokio::test]
async fn isolated_process_uses_workspace_cwd_and_explicit_environment() {
    let root = workspace("context");
    let root = root.canonicalize().unwrap();
    let nested = root.join("nested");
    let result = shell_worker()
        .execute(
            "shell.execute",
            json!({
                "execution_context": {
                    "workspace": { "root": root, "mode": "isolated" },
                    "cwd": "nested",
                    "env": { "OAIP_PHASE2": "ok" }
                },
                "args": ["-c", "printf '%s|%s|%s' \"$PWD\" \"$HOME\" \"$OAIP_PHASE2\""]
            }),
        )
        .await
        .unwrap();

    let stdout = result.output["stdout"].as_str().unwrap();
    assert!(
        stdout.starts_with(&format!("{}|", nested.display())),
        "cwd: {stdout}"
    );
    assert!(
        stdout.contains(&format!("|{}|", root.display())),
        "HOME: {stdout}"
    );
    assert!(stdout.ends_with("|ok"), "explicit env: {stdout}");
    std::fs::remove_dir_all(root).ok();
}

#[tokio::test]
async fn isolated_process_rejects_workspace_escape_and_missing_root() {
    let root = workspace("boundary");
    let outside = std::env::temp_dir().join(format!("oaip_phase2_outside_{}", std::process::id()));
    std::fs::create_dir_all(&outside).unwrap();

    let escaped = shell_worker()
        .execute(
            "shell.execute",
            json!({
                "execution_context": {
                    "workspace": { "root": root, "mode": "isolated" },
                    "cwd": outside
                },
                "args": ["-c", "echo must-not-run"]
            }),
        )
        .await;
    let Err(WorkerError::Execution(error)) = escaped else {
        panic!("workspace 越界必须 fail-closed: {escaped:?}");
    };
    assert!(error.contains("越出 workspace.root"), "error: {error}");

    let missing = root.join("missing");
    let failed = shell_worker()
        .execute(
            "shell.execute",
            json!({
                "execution_context": {
                    "workspace": { "root": missing, "mode": "isolated" }
                },
                "args": ["-c", "echo must-not-run"]
            }),
        )
        .await;
    assert!(matches!(failed, Err(WorkerError::Execution(_))));

    std::fs::remove_dir_all(root).ok();
    std::fs::remove_dir_all(outside).ok();
}

#[test]
fn fabric_process_adapter_applies_the_same_workspace_contract() {
    let root = workspace("fabric");
    let root = root.canonicalize().unwrap();
    let nested = root.join("nested");
    let mut manifest = WorkerManifest::new("agent", "agent").with_capabilities(&["agent.task"]);
    manifest.execution = ExecutionProtocol::Process;
    let adapter = ProcessAdapter::new("agent", ProcessWorker::new(manifest, "/bin/sh"));
    let message = AgentMessage {
        id: "request-1".into(),
        session_id: "session-1".into(),
        sender: "test".into(),
        receiver: "agent".into(),
        kind: MessageKind::TaskRequest,
        intent: Some("agent_task".into()),
        payload: json!({
            "args": ["-c", "printf '%s|%s' \"$PWD\" \"$OAIP_PHASE2\""]
        }),
        context: json!({
            "workspace": { "root": root, "mode": "isolated" },
            "cwd": "nested",
            "env": { "OAIP_PHASE2": "fabric" }
        }),
        capability: Some("agent.task".into()),
        correlation_id: Some("corr-1".into()),
        causation_id: None,
        timestamp: 0,
    };

    let events = adapter.stream(&message).unwrap();
    let result = events.last().unwrap();
    assert_eq!(result.kind, MessageKind::TaskResult);
    assert_eq!(result.payload["status"], "completed");
    assert_eq!(
        result.payload["stdout"],
        format!("{}|fabric\n", nested.display())
    );

    std::fs::remove_dir_all(root).ok();
}

#[cfg(unix)]
#[test]
fn dsh_adapter_keeps_provider_args_and_isolates_workspace() {
    use std::os::unix::fs::PermissionsExt;

    let root = workspace("dsh").canonicalize().unwrap();
    let script = root.join("dsh-shim");
    std::fs::write(
        &script,
        "#!/bin/sh\nprintf '%s|%s|%s|%s' \"$1\" \"$2\" \"$PWD\" \"$NO_COLOR\"\n",
    )
    .unwrap();
    let mut permissions = std::fs::metadata(&script).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&script, permissions).unwrap();

    let adapter = dsh_adapter(Some(script.to_str().unwrap()));
    let message = AgentMessage {
        id: "dsh-request-1".into(),
        session_id: "session-dsh".into(),
        sender: "test".into(),
        receiver: "dsh".into(),
        kind: MessageKind::TaskRequest,
        intent: Some("agent_task".into()),
        payload: json!({ "task": "workspace-task" }),
        context: json!({
            "workspace": { "root": root, "mode": "isolated" },
            "env": { "NO_COLOR": "isolated" }
        }),
        capability: Some("agent.task".into()),
        correlation_id: Some("dsh-corr-1".into()),
        causation_id: None,
        timestamp: 0,
    };

    let events = adapter.stream(&message).unwrap();
    let result = events.last().unwrap();
    assert_eq!(result.payload["status"], "completed");
    assert_eq!(
        result.payload["stdout"],
        format!("--profile|headless|{}|isolated\n", root.display())
    );
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn fabric_process_adapter_enforces_timeout_without_leaking_process() {
    let root = workspace("timeout").canonicalize().unwrap();
    let mut manifest =
        WorkerManifest::new("agent-timeout", "agent").with_capabilities(&["agent.task"]);
    manifest.execution = ExecutionProtocol::Process;
    let adapter = ProcessAdapter::new("agent-timeout", ProcessWorker::new(manifest, "/bin/sh"));
    let message = AgentMessage {
        id: "timeout-request".into(),
        session_id: "session-timeout".into(),
        sender: "test".into(),
        receiver: "agent-timeout".into(),
        kind: MessageKind::TaskRequest,
        intent: Some("agent_task".into()),
        payload: json!({ "args": ["-c", "sleep 1"] }),
        context: json!({
            "workspace": { "root": root, "mode": "isolated" },
            "timeout_ms": 20
        }),
        capability: Some("agent.task".into()),
        correlation_id: Some("timeout-corr".into()),
        causation_id: None,
        timestamp: 0,
    };

    let started = std::time::Instant::now();
    let events = adapter.stream(&message).unwrap();
    assert!(started.elapsed() < std::time::Duration::from_millis(500));
    let result = events.last().unwrap();
    assert_eq!(result.payload["status"], "failed");
    assert!(result.payload["stderr"].as_str().unwrap().contains("超时"));

    std::fs::remove_dir_all(root).ok();
}
