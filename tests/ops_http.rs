//! Phase 3 operational endpoint contract tests.

mod common;
use common::*;

use openaip_os::server;
use openaip_os::storage::LibSQLStorage;
use openaip_os::trace::{TraceEvent, TraceKind};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;

async fn spawn_ops_server() -> std::net::SocketAddr {
    let storage = Arc::new(LibSQLStorage::open(":memory:").await.unwrap());
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = server::serve(
            storage,
            retain_services(),
            retain_defs(),
            Arc::new(v2_retain_engine()),
            listener,
            "ui/dist",
            false,
            false,
            true,
        )
        .await;
    });
    tokio::time::sleep(Duration::from_millis(150)).await;
    address
}

#[tokio::test]
async fn probes_are_public_and_metrics_are_prometheus_text() {
    let address = spawn_ops_server().await;

    let (status, body) = http_call_auth(address, "GET", "/healthz", None, None).await;
    assert_eq!(status, 200);
    assert!(body.contains("\"status\":\"ok\""));

    let (status, body) = http_call_auth(address, "GET", "/readyz", None, None).await;
    assert_eq!(status, 200);
    assert!(body.contains("\"status\":\"ready\""));

    let (status, body) = http_call_auth(address, "GET", "/metrics", None, None).await;
    assert_eq!(status, 200);
    assert!(body.contains("# TYPE oaip_ready gauge"));
    assert!(body.contains("oaip_ready 1"));
    assert!(body.contains("oaip_pending_approvals 0"));
}

#[tokio::test]
async fn protected_api_remains_protected_while_probes_are_public() {
    let address = spawn_ops_server().await;
    let (status, _) = http_call_auth(address, "GET", "/api/v1/wb/agent/health", None, None).await;
    assert_eq!(status, 401);
}

#[tokio::test]
async fn operational_counts_are_read_without_loading_full_rows() {
    let storage = LibSQLStorage::open(":memory:").await.unwrap();
    assert_eq!(storage.count_runs().await.unwrap(), 0);
    assert_eq!(storage.count_trace_events().await.unwrap(), 0);

    storage
        .insert_run("run-ops", "agent-ops", "metrics")
        .await
        .unwrap();
    storage
        .append_trace(&TraceEvent {
            id: "trace-ops".into(),
            kind: TraceKind::Cognitive,
            event_type: "Goal".into(),
            object_refs: vec!["ops".into()],
            project_id: None,
            run_id: Some("run-ops".into()),
            agent_id: Some("agent-ops".into()),
            reasoning: None,
            input: None,
            output: None,
            confidence: None,
            timestamp: "1".into(),
        })
        .await
        .unwrap();

    assert_eq!(storage.count_runs().await.unwrap(), 1);
    assert_eq!(storage.count_trace_events().await.unwrap(), 1);
}
