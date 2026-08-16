//! M2 · 审计查询入口契约（审批审计事实轨 → HTTP 端点）。
//!
//! 契约：run 终局把 Kernel 审计事件（approval.requested/granted/rejected/expired、
//! evolution.gate_approved）桥进 wb_trace_event（I1：State = Reduce(EventLog)，可回放）；
//! `GET /api/v1/wb/agent/audit` 按审计种类过滤返回。

mod common;
use common::*;

use openaip_os::server;
use openaip_os::storage::LibSQLStorage;
use serde_json::{Value, json};
use std::net::SocketAddr;
use std::time::Duration;
use tokio::net::TcpListener;

const ODL_REFUND: &str = r#"
object Customer:
  properties:
    name: string
    level: string
  actions:
    refund:
      params:
        target: string
        amount: float
      policy:
        require_approval: true
      creates:
        - type: Refund
          link: refunded
"#;

fn refund_services() -> Vec<openaip_os::compose::Service> {
    openaip_os::compose::defs::ir_to_services(&openaip_os::parse_odl(ODL_REFUND).unwrap())
}

fn refund_defs() -> Vec<openaip_os::compose::defs::RuntimeDefinition> {
    openaip_os::compose::defs::ir_to_runtime(&openaip_os::parse_odl(ODL_REFUND).unwrap())
}

fn refund_engine() -> openaip_os::cognitive::FakeEngine {
    use openaip_os::cognitive::engine::PlanStep;
    openaip_os::cognitive::FakeEngine::new(vec![PlanStep {
        capability: "Customer.refund".into(),
        args: json!({ "target": "001", "amount": 100 }),
        when: Some(Box::new(|v: &openaip_os::cognitive::AgentView| {
            !v.related.iter().any(|(_, p, _)| p == "refunded")
        })),
    }])
}

async fn spawn(storage: LibSQLStorage) -> SocketAddr {
    let kernel = std::sync::Arc::new(storage);
    let services = refund_services();
    let defs = refund_defs();
    let engine: std::sync::Arc<dyn openaip_os::cognitive::CognitiveEngine> =
        std::sync::Arc::new(refund_engine());
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = server::serve(
            kernel, services, defs, engine, listener, "ui/dist", false, false, false,
        )
        .await;
    });
    tokio::time::sleep(Duration::from_millis(150)).await;
    addr
}

async fn start_run(addr: SocketAddr) -> String {
    let (st, body) = http_call(
        addr,
        "POST",
        "/api/v1/wb/agent/run",
        Some(r#"{"agentId":"retention-agent","goal":"refund 001"}"#),
    )
    .await;
    assert_eq!(st, 200, "run 启动: {body}");
    serde_json::from_str::<Value>(&body).unwrap()["data"]["runId"]
        .as_str()
        .unwrap()
        .to_string()
}

async fn wait_pending(addr: SocketAddr, run_id: &str) -> Value {
    for _ in 0..100 {
        let (_, b) = http_call(
            addr,
            "GET",
            &format!("/api/v1/wb/agent/run/{run_id}/approvals"),
            None,
        )
        .await;
        let pending = serde_json::from_str::<Value>(&b)
            .unwrap()
            .get("data")
            .and_then(|d| d.as_array())
            .cloned()
            .unwrap_or_default();
        if !pending.is_empty() {
            return pending[0].clone();
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("未挂起出审批请求")
}

async fn wait_status(storage: &LibSQLStorage, run_id: &str, want: &str) -> bool {
    for _ in 0..100 {
        if let Ok(runs) = storage.list_runs().await
            && let Some(r) = runs.iter().find(|r| r.id == run_id)
            && r.status == want
        {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    false
}

async fn audit_types(addr: SocketAddr) -> Vec<String> {
    let (_, b) = http_call(addr, "GET", "/api/v1/wb/agent/audit", None).await;
    serde_json::from_str::<Value>(&b)
        .unwrap()
        .get("data")
        .and_then(|d| d.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|e| {
                    e.get("eventType")
                        .and_then(|t| t.as_str())
                        .map(String::from)
                })
                .collect()
        })
        .unwrap_or_default()
}

// ── M2a：批准 run → /audit 含 requested + granted ──────────────────────
#[tokio::test]
async fn m2a_approved_run_audit_trail() {
    let storage = LibSQLStorage::open(":memory:").await.unwrap();
    let addr = spawn(storage.clone()).await;
    let run_id = start_run(addr).await;
    let req = wait_pending(addr, &run_id).await;
    let req_id = req["reqId"].as_str().unwrap().to_string();

    let (_, b) = http_call(
        addr,
        "POST",
        &format!("/api/v1/wb/agent/run/{run_id}/approval/{req_id}"),
        Some(r#"{"decision":"approve"}"#),
    )
    .await;
    assert_eq!(serde_json::from_str::<Value>(&b).unwrap()["code"], 0);
    assert!(wait_status(&storage, &run_id, "Completed").await);

    let types = audit_types(addr).await;
    assert!(
        types.iter().any(|t| t == "approval.requested"),
        "应有 approval.requested: {types:?}"
    );
    assert!(
        types.iter().any(|t| t == "approval.granted"),
        "应有 approval.granted（人工批准）: {types:?}"
    );
}

// ── M2b：拒绝 run → /audit 含 rejected ─────────────────────────────────
#[tokio::test]
async fn m2b_rejected_run_audit_trail() {
    let storage = LibSQLStorage::open(":memory:").await.unwrap();
    let addr = spawn(storage.clone()).await;
    let run_id = start_run(addr).await;
    let req = wait_pending(addr, &run_id).await;
    let req_id = req["reqId"].as_str().unwrap().to_string();

    let (_, b) = http_call(
        addr,
        "POST",
        &format!("/api/v1/wb/agent/run/{run_id}/approval/{req_id}"),
        Some(r#"{"decision":"reject"}"#),
    )
    .await;
    assert_eq!(serde_json::from_str::<Value>(&b).unwrap()["code"], 0);
    assert!(wait_status(&storage, &run_id, "Failed").await);

    let types = audit_types(addr).await;
    assert!(
        types.iter().any(|t| t == "approval.rejected"),
        "应有 approval.rejected: {types:?}"
    );
}
