//! Fabric 审批恢复契约：文件数据库重启、异常事件和分页扫描。

mod common;

use openaip_os::fabric::adapters::ProcessAdapter;
use openaip_os::fabric::router::FabricRouter;
use openaip_os::fabric::{AgentMessage, MessageKind};
use openaip_os::facade::Facade;
use openaip_os::storage::LibSQLStorage;
use openaip_os::trace::{TraceEvent, TraceKind};
use openaip_os::worker::manifest::WorkerManifest;
use openaip_os::worker::process::ProcessWorker;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;

fn temp_db_path(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "openaip-{label}-{}-{}.db",
        std::process::id(),
        openaip_os::trace::new_trace_id()
    ))
}

async fn spawn_file_server(path: &Path) -> (std::net::SocketAddr, Arc<LibSQLStorage>) {
    let storage = Arc::new(LibSQLStorage::open(path.to_str().unwrap()).await.unwrap());
    let services = common::retain_services();
    let defs = common::retain_defs();
    let engine: Arc<dyn openaip_os::cognitive::CognitiveEngine> =
        Arc::new(common::v2_retain_engine());

    let facade = tokio::task::block_in_place(|| {
        let mut router = FabricRouter::new();
        router.register_intent("run_shell", "shell.execute");
        router.register_agent("shell.execute", "sh-agent", 1);
        router.mark_high_risk("shell.execute");
        let manifest =
            WorkerManifest::new("sh-agent", "agent").with_capabilities(&["shell.execute"]);
        let adapter = Arc::new(ProcessAdapter::new(
            "sh-agent",
            ProcessWorker::new(manifest, "/bin/sh"),
        ));
        Facade::new(
            storage.clone(),
            tokio::runtime::Handle::current(),
            router,
            vec![adapter],
            &[("fabric.task_completed", "fabric.status")],
        )
        .unwrap()
    });

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let storage_for_server = storage.clone();
    tokio::spawn(async move {
        let _ = openaip_os::server::serve_with_facade(
            storage_for_server,
            services,
            defs,
            engine,
            listener,
            "ui/dist",
            false,
            false,
            false,
            None,
            Some(Arc::new(facade)),
        )
        .await;
    });
    tokio::time::sleep(Duration::from_millis(150)).await;
    (addr, storage)
}

async fn submit_shell(addr: std::net::SocketAddr, session_id: &str, command: &str) -> Value {
    let body = json!({
        "sessionId": session_id,
        "sender": "dsh",
        "intent": "run_shell",
        "payload": {"args": ["-c", command], "object": "recovery-test"},
        "executionContext": {
            "workspace": { "root": "/tmp", "mode": "isolated" }
        }
    });
    let (_, response) = common::http_call(
        addr,
        "POST",
        "/api/v1/facade/fabric/submit",
        Some(&body.to_string()),
    )
    .await;
    serde_json::from_str(&response).unwrap()
}

async fn wait_for_pending(addr: std::net::SocketAddr, run_id: &str) -> Value {
    for _ in 0..50 {
        let (_, body) = common::http_call(
            addr,
            "GET",
            &format!("/api/v1/wb/agent/run/{run_id}/approvals"),
            None,
        )
        .await;
        let value: Value = serde_json::from_str(&body).unwrap();
        if value["data"]
            .as_array()
            .is_some_and(|entries| !entries.is_empty())
        {
            return value;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("审批未恢复: run_id={run_id}");
}

fn task_request(run_id: &str, session_id: &str) -> AgentMessage {
    AgentMessage {
        id: openaip_os::trace::new_trace_id(),
        session_id: session_id.to_string(),
        sender: "dsh".into(),
        receiver: "sh-agent".into(),
        kind: MessageKind::TaskRequest,
        intent: Some("run_shell".into()),
        payload: json!({"args": ["-c", "echo restored-ok"], "object": "recovery-test"}),
        context: json!({
            "workspace": { "root": "/tmp", "mode": "isolated" }
        }),
        capability: Some("shell.execute".into()),
        correlation_id: Some(run_id.into()),
        causation_id: None,
        timestamp: openaip_os::kernel::now_secs(),
    }
}

fn trace(event_type: &str, input: Value, object_ref: &str) -> TraceEvent {
    TraceEvent {
        id: openaip_os::trace::new_trace_id(),
        kind: TraceKind::Cognitive,
        event_type: event_type.into(),
        object_refs: vec![object_ref.into()],
        project_id: None,
        run_id: None,
        agent_id: Some("sh-agent".into()),
        reasoning: None,
        input: Some(input),
        output: None,
        confidence: None,
        timestamp: openaip_os::trace::now_ts(),
    }
}

async fn append_task_and_approval(storage: &LibSQLStorage, run_id: &str, session_id: &str) {
    let request = task_request(run_id, session_id);
    storage
        .append_trace(&trace(
            "fabric.task_requested",
            json!({"request": request}),
            run_id,
        ))
        .await
        .unwrap();
    storage
        .append_trace(&trace(
            "fabric.approval_requested",
            json!({"run_id": run_id, "session_id": session_id}),
            run_id,
        ))
        .await
        .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn tc_fabric_recovery_rehydrates_file_db_approval_and_resumes() {
    let path = temp_db_path("fabric-restart");
    let (first_addr, _) = spawn_file_server(&path).await;
    let submitted = submit_shell(first_addr, "sess-restart", "echo recovered-after-restart").await;
    let run_id = submitted["data"]["result"]["correlation_id"]
        .as_str()
        .unwrap()
        .to_string();
    let first_pending = wait_for_pending(first_addr, &run_id).await;
    assert_eq!(first_pending["data"][0]["runId"], run_id);

    // 第二个服务实例使用同一文件数据库，模拟进程重启后的恢复窗口。
    let (second_addr, storage) = spawn_file_server(&path).await;
    let recovered = wait_for_pending(second_addr, &run_id).await;
    assert_eq!(recovered["data"][0]["runId"], run_id);
    assert_eq!(recovered["data"][0]["state"], "Pending");

    let (_, body) = common::http_call(
        second_addr,
        "POST",
        &format!("/api/v1/wb/agent/run/{run_id}/approval/{run_id}"),
        Some(r#"{"decision":"approve"}"#),
    )
    .await;
    let resolved: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(resolved["code"], 0, "恢复后的审批应可裁决: {resolved}");
    assert_eq!(resolved["data"]["decision"], "Approved");
    assert_eq!(resolved["data"]["status"], "completed");

    let events = storage
        .list_events(openaip_os::storage::TraceQuery {
            event_type: Some("fabric.task_completed".into()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(
        events.iter().any(|event| {
            event
                .input
                .as_ref()
                .and_then(|input| input["correlation_id"].as_str())
                == Some(run_id.as_str())
        }),
        "恢复后必须追加 task_completed 终局审计"
    );

    let _ = std::fs::remove_file(path);
}

#[tokio::test(flavor = "multi_thread")]
async fn tc_fabric_recovery_ignores_orphans_terminals_and_scans_past_first_page() {
    let path = temp_db_path("fabric-recovery-cases");
    let storage = LibSQLStorage::open(path.to_str().unwrap()).await.unwrap();

    // 先写入超过恢复单页上限的噪声，再写挂起请求，验证 after_id 分页不会漏旧挂起。
    for index in 0..1002 {
        storage
            .append_trace(&trace(
                "fabric.noise",
                json!({"index": index}),
                &format!("noise-{index}"),
            ))
            .await
            .unwrap();
    }
    append_task_and_approval(&storage, "valid-pending", "sess-valid").await;

    // 缺少 task.request 的审批事件不得凭空生成 pending。
    storage
        .append_trace(&trace(
            "fabric.approval_requested",
            json!({"run_id": "orphan-approval", "session_id": "sess-orphan"}),
            "orphan-approval",
        ))
        .await
        .unwrap();

    for (run_id, terminal_event) in [
        ("terminal-completed", "fabric.task_completed"),
        ("terminal-failed", "fabric.task_failed"),
        ("terminal-cancelled", "fabric.task_cancelled"),
    ] {
        append_task_and_approval(&storage, run_id, &format!("sess-{run_id}")).await;
        storage
            .append_trace(&trace(
                terminal_event,
                json!({"correlation_id": run_id}),
                run_id,
            ))
            .await
            .unwrap();
    }
    append_task_and_approval(&storage, "terminal-rejected", "sess-terminal-rejected").await;
    storage
        .append_trace(&trace(
            "fabric.approval_rejected",
            json!({"run_id": "terminal-rejected"}),
            "terminal-rejected",
        ))
        .await
        .unwrap();
    drop(storage);

    let (addr, _) = spawn_file_server(&path).await;
    let recovered = wait_for_pending(addr, "valid-pending").await;
    assert_eq!(recovered["data"].as_array().unwrap().len(), 1);

    for run_id in [
        "orphan-approval",
        "terminal-completed",
        "terminal-failed",
        "terminal-cancelled",
        "terminal-rejected",
    ] {
        let (_, body) = common::http_call(
            addr,
            "GET",
            &format!("/api/v1/wb/agent/run/{run_id}/approvals"),
            None,
        )
        .await;
        let value: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(
            value["data"].as_array().unwrap().len(),
            0,
            "异常/终结 run 不得恢复为 pending: {run_id}: {value}"
        );
    }

    let _ = std::fs::remove_file(path);
}
