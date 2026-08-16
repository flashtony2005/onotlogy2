//! M1c HTTP 审批端点契约（TC-023a~e · V2 Waiting 注册表驱动）。
//!
//! V1 读 ApprovalStore（planner 内存 + wb_approval 表）；M1c 起读 AppState.pending
//! （run 路径 Waiting 时注册，裁决经 mpsc 通道唤醒 resolver）。行为契约不变：
//! 挂起列表 / 批准恢复完成 / 拒绝终止 / 超时 Expired / 错 run 与二次裁决拒绝。

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
    openaip_os::compose::defs::ir_to_services(&openaip_os::abi::odl::parse_odl(ODL_REFUND).unwrap())
}

fn refund_defs() -> Vec<openaip_os::compose::defs::RuntimeDefinition> {
    openaip_os::compose::defs::ir_to_runtime(&openaip_os::abi::odl::parse_odl(ODL_REFUND).unwrap())
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

async fn spawn(storage: LibSQLStorage, timeout: Option<Duration>) -> SocketAddr {
    let storage = std::sync::Arc::new(storage);
    let services = refund_services();
    let defs = refund_defs();
    let engine: std::sync::Arc<dyn openaip_os::cognitive::CognitiveEngine> =
        std::sync::Arc::new(refund_engine());
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = server::serve_with_timeout(
            storage.clone(),
            services,
            defs,
            engine,
            listener,
            "ui/dist",
            false,
            false,
            false,
            timeout,
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

fn body_code(body: &str) -> i64 {
    serde_json::from_str::<Value>(body)
        .unwrap()
        .get("code")
        .and_then(|c| c.as_i64())
        .unwrap_or(-1)
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

// ── TC-023a：挂起 → 浏览器批准 → 运行完成（动作真实执行）────────────────────
#[tokio::test]
async fn tc023a_server_http_approval_suspend_and_approve() {
    let storage = LibSQLStorage::open(":memory:").await.unwrap();
    let addr = spawn(storage.clone(), None).await;
    let run_id = start_run(addr).await;

    let req = wait_pending(addr, &run_id).await;
    assert_eq!(req["state"], "Pending");
    assert_eq!(req["capability"], "Customer.refund");
    let req_id = req["reqId"].as_str().unwrap().to_string();

    let (st2, b2) = http_call(
        addr,
        "POST",
        &format!("/api/v1/wb/agent/run/{run_id}/approval/{req_id}"),
        Some(r#"{"decision":"approve"}"#),
    )
    .await;
    assert_eq!(st2, 200);
    assert_eq!(body_code(&b2), 0, "批准应成功: {b2}");

    assert!(
        wait_status(&storage, &run_id, "Completed").await,
        "批准后运行应完成"
    );
    // 动作被实际执行：V2 事件流含 Action（Success）
    let events = storage
        .list_events(openaip_os::storage::TraceQuery::for_run(&run_id))
        .await
        .unwrap();
    assert!(
        events.iter().any(|e| e.event_type == "Action"),
        "批准后动作应执行（Action 事件）"
    );
}

// ── TC-023b：拒绝 → 运行终止（Failed，无 Action）────────────────────────────
#[tokio::test]
async fn tc023b_server_http_approval_reject() {
    let storage = LibSQLStorage::open(":memory:").await.unwrap();
    let addr = spawn(storage.clone(), None).await;
    let run_id = start_run(addr).await;
    let req = wait_pending(addr, &run_id).await;
    let req_id = req["reqId"].as_str().unwrap().to_string();

    let (st2, b2) = http_call(
        addr,
        "POST",
        &format!("/api/v1/wb/agent/run/{run_id}/approval/{req_id}"),
        Some(r#"{"decision":"reject"}"#),
    )
    .await;
    assert_eq!(st2, 200);
    assert_eq!(body_code(&b2), 0, "拒绝应成功: {b2}");

    assert!(
        wait_status(&storage, &run_id, "Failed").await,
        "拒绝后运行应终止"
    );
    let events = storage
        .list_events(openaip_os::storage::TraceQuery::for_run(&run_id))
        .await
        .unwrap();
    assert!(
        !events.iter().any(|e| e.event_type == "Action"),
        "拒绝后动作不得执行"
    );
}

// ── TC-023c：挂起超时 → Expired 回收（运行终止）────────────────────────────
#[tokio::test]
async fn tc023c_server_http_approval_timeout_expired() {
    let storage = LibSQLStorage::open(":memory:").await.unwrap();
    let addr = spawn(storage.clone(), Some(Duration::from_millis(400))).await;
    let run_id = start_run(addr).await;
    let _ = wait_pending(addr, &run_id).await;

    assert!(
        wait_status(&storage, &run_id, "Failed").await,
        "超时后运行应终止（Expire 终局）"
    );
}

// ── TC-023d：超时回收后拒绝二次裁决（已从注册表移除 → 404）─────────────────
#[tokio::test]
async fn tc023d_timeout_expired_distinct_from_rejected() {
    let storage = LibSQLStorage::open(":memory:").await.unwrap();
    let addr = spawn(storage.clone(), Some(Duration::from_millis(400))).await;
    let run_id = start_run(addr).await;
    let req = wait_pending(addr, &run_id).await;
    let req_id = req["reqId"].as_str().unwrap().to_string();

    assert!(wait_status(&storage, &run_id, "Failed").await, "超时应终止");
    // 超时条目已移除：二次裁决 code=404；列表为空
    let (_, b2) = http_call(
        addr,
        "POST",
        &format!("/api/v1/wb/agent/run/{run_id}/approval/{req_id}"),
        Some(r#"{"decision":"approve"}"#),
    )
    .await;
    assert_eq!(body_code(&b2), 404, "已超时条目拒绝二次裁决: {b2}");
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
    assert!(pending.is_empty(), "超时后挂起列表应为空");
}

// ── TC-023e：错 run 裁决拒绝；正确 run 可继续裁决 ───────────────────────────
#[tokio::test]
async fn tc023e_approval_wrong_run_id_rejected() {
    let storage = LibSQLStorage::open(":memory:").await.unwrap();
    let addr = spawn(storage.clone(), None).await;
    let run_id = start_run(addr).await;
    let req = wait_pending(addr, &run_id).await;
    let req_id = req["reqId"].as_str().unwrap().to_string();

    // 错误 run id → 404，请求仍待裁决
    let (_, b1) = http_call(
        addr,
        "POST",
        &format!("/api/v1/wb/agent/run/wrong_run/approval/{req_id}"),
        Some(r#"{"decision":"approve"}"#),
    )
    .await;
    assert_eq!(body_code(&b1), 404, "错 run 拒绝裁决: {b1}");

    // 正确 run id → code=0 → 完成
    let (st2, b2) = http_call(
        addr,
        "POST",
        &format!("/api/v1/wb/agent/run/{run_id}/approval/{req_id}"),
        Some(r#"{"decision":"approve"}"#),
    )
    .await;
    assert_eq!(st2, 200);
    assert_eq!(body_code(&b2), 0);
    assert!(wait_status(&storage, &run_id, "Completed").await);
}
