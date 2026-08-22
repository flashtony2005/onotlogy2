//! Phase 2 Facade/Fabric execution_context contract and fail-closed policy.

use openaip_os::fabric::router::FabricRouter;
use openaip_os::fabric::session::SessionManager;
use openaip_os::fabric::{Bridge, FabricError};
use openaip_os::worker::process::{
    ExecutionContext, ExecutionWorkspace, MAX_EXECUTION_TIMEOUT_MS, ProcessExecutionContext,
    WorkspaceMode,
};
use serde_json::json;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

fn bridge() -> Bridge {
    let mut router = FabricRouter::new();
    router.register_intent("agent_task", "agent.task");
    router.register_intent("modify_code", "code.write");
    router.register_intent("run_shell", "shell.execute");
    router.register_agent("agent.task", "dsh", 1);
    router.register_agent("code.write", "codex", 1);
    router.register_agent("shell.execute", "shell", 1);
    Bridge::new(router, None)
}

fn session() -> (Arc<Mutex<SessionManager>>, String) {
    let sessions = Arc::new(Mutex::new(SessionManager::new()));
    let id = sessions.lock().unwrap().create(vec!["oaip-test".into()]).id;
    (sessions, id)
}

fn isolated_context(root: &str) -> ExecutionContext {
    ExecutionContext {
        workspace: ExecutionWorkspace {
            root: root.to_string(),
            mode: WorkspaceMode::Isolated,
        },
        cwd: None,
        env: BTreeMap::new(),
        timeout_ms: Some(30_000),
    }
}

#[test]
fn facade_execution_context_serializes_as_formal_nested_contract() {
    let context = isolated_context("/tmp/project");
    let value = serde_json::to_value(&context).unwrap();
    assert_eq!(value["workspace"]["mode"], "isolated");
    assert_eq!(value["timeout_ms"], 30_000);
    assert!(value.get("timeoutMs").is_none());

    let request = openaip_os::facade::FabricRequest {
        session_id: "sess-1".into(),
        sender: "dsh".into(),
        intent: "agent_task".into(),
        payload: json!({ "task": "check" }),
        execution_context: Some(context),
        project_id: None,
    };
    let value = serde_json::to_value(request).unwrap();
    assert!(value.get("executionContext").is_some());
    assert!(value["payload"].get("workspace").is_none());
}

#[test]
fn execution_capabilities_require_isolated_workspace() {
    let bridge = bridge();
    let (sessions, session_id) = session();

    for intent in ["agent_task", "modify_code", "run_shell"] {
        let result = bridge.submit(
            sessions.clone(),
            &session_id,
            "oaip-test",
            intent,
            json!({ "task": "must not run" }),
            None,
            None,
        );
        let error = match result {
            Ok(_) => panic!("执行能力缺少 execution_context 必须拒绝"),
            Err(error) => error,
        };
        assert!(
            matches!(error, FabricError::Denied(message) if message.contains("execution_context"))
        );
    }
}

#[test]
fn shell_execute_without_workspace_is_denied_before_job_creation() {
    let bridge = bridge();
    let (sessions, session_id) = session();
    let result = bridge.submit(
        sessions,
        &session_id,
        "oaip-test",
        "run_shell",
        json!({ "args": ["-c", "echo should-not-run"] }),
        None,
        None,
    );

    let error = match result {
        Ok(_) => panic!("shell.execute 缺少隔离 workspace 必须拒绝"),
        Err(error) => error,
    };
    assert!(matches!(error, FabricError::Denied(message) if message.contains("shell.execute")));
}

#[test]
fn execution_context_is_carried_on_task_request_not_payload() {
    let root = std::env::temp_dir().canonicalize().unwrap();
    let bridge = bridge();
    let (sessions, session_id) = session();
    let context = isolated_context(root.to_str().unwrap());
    let (request, job) = bridge
        .submit(
            sessions.clone(),
            &session_id,
            "oaip-test",
            "agent_task",
            json!({ "task": "check" }),
            Some(context.clone()),
            None,
        )
        .unwrap();

    assert!(job.is_some());
    assert!(request.payload.get("workspace").is_none());
    assert!(request.payload.get("timeout_ms").is_none());

    let stored = sessions
        .lock()
        .unwrap()
        .get(&session_id)
        .unwrap()
        .messages()
        .last()
        .unwrap()
        .clone();
    assert_eq!(stored.context["workspace"]["mode"], "isolated");
    assert_eq!(stored.context["timeout_ms"], 30_000);
    assert!(stored.payload.get("workspace").is_none());
    assert!(stored.payload.get("timeout_ms").is_none());
}

#[test]
fn execution_timeout_is_hard_bounded() {
    let root = std::env::temp_dir().canonicalize().unwrap();
    let mut context = isolated_context(root.to_str().unwrap());
    context.timeout_ms = Some(MAX_EXECUTION_TIMEOUT_MS + 1);

    let error = ProcessExecutionContext::from_execution_context(&context).unwrap_err();
    assert!(error.contains("硬上限"), "错误必须明确说明硬上限: {error}");
}

#[test]
fn process_execution_without_context_still_has_hard_timeout() {
    let context = ProcessExecutionContext::from_input(&serde_json::json!({})).unwrap();
    assert_eq!(context.timeout_ms, Some(MAX_EXECUTION_TIMEOUT_MS));
}
