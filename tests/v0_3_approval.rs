//! V0.4 挂起式 HTTP 审批端点测试契约（TC-023a~e）
mod common;
use common::*;

use openaip_os::action::ActionRegistry;
use openaip_os::agent::AgentEngine;
use openaip_os::parse_odl;
use openaip_os::runtime::RuntimeKernel;
use openaip_os::server;
use openaip_os::storage::LibSQLStorage;
use serde_json::{Value, json};
use std::time::Duration;
use tokio::net::TcpListener;

// ── TC-023a：server 挂起式 HTTP 审批端点 — 挂起待审批 → 浏览器批准 → 运行完成 ──────────
#[tokio::test]
async fn tc023a_server_http_approval_suspend_and_approve() {
    let s = LibSQLStorage::open(":memory:").await.unwrap();
    let kernel = std::sync::Arc::new(RuntimeKernel::new(s));
    kernel
        .create_object("Customer", "001", json!({"name": "Apple", "level": "VIP"}))
        .await
        .unwrap();
    let engine: std::sync::Arc<dyn AgentEngine> = std::sync::Arc::new(FakeEngine);
    let reg = ActionRegistry::from_ir(&parse_odl(ODL_APPROVE).unwrap());

    // 启动服务：auto_approve=false → require_approval 动作挂起，等待 HTTP 审批
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let ks = kernel.clone();
    let es = engine.clone();
    tokio::spawn(async move {
        let _ = server::serve(ks, es, listener, "ui/dist", reg, false, false, false).await;
    });
    tokio::time::sleep(Duration::from_millis(150)).await;

    // 触发运行（立即返回 runId + streamUrl）
    let (st, body) = http_call(
        addr,
        "POST",
        "/api/v1/wb/agent/run",
        Some(r#"{"agentId":"retention-agent","goal":"close account"}"#),
    )
    .await;
    assert_eq!(st, 200);
    let run_id = serde_json::from_str::<Value>(&body).unwrap()["data"]["runId"]
        .as_str()
        .unwrap()
        .to_string();

    // 运行应挂起：GET approvals 出现至少一条 Pending
    let mut pending: Vec<Value> = Vec::new();
    for _ in 0..100 {
        let (_, b) = http_call(
            addr,
            "GET",
            &format!("/api/v1/wb/agent/run/{run_id}/approvals"),
            None,
        )
        .await;
        pending = serde_json::from_str::<Value>(&b)
            .unwrap()
            .get("data")
            .and_then(|d| d.as_array())
            .cloned()
            .unwrap_or_default();
        if pending.iter().any(|p| p["state"] == "Pending") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let req = pending
        .iter()
        .find(|p| p["state"] == "Pending")
        .expect("应挂起出一条待审批请求");
    assert_eq!(req["state"], "Pending");
    assert!(req["reqId"].as_str().unwrap().starts_with("apr_"));
    let req_id = req["reqId"].as_str().unwrap().to_string();

    // 浏览器点批准
    let (st2, _) = http_call(
        addr,
        "POST",
        &format!("/api/v1/wb/agent/run/{run_id}/approval/{req_id}"),
        Some(r#"{"decision":"approve"}"#),
    )
    .await;
    assert_eq!(st2, 200);

    // 批准后运行应推进至 Completed
    let mut status = String::new();
    for _ in 0..100 {
        let (_, b) = http_call(
            addr,
            "GET",
            &format!("/api/v1/wb/agent/run/{run_id}/timeline?view=operator"),
            None,
        )
        .await;
        status = serde_json::from_str::<Value>(&b)
            .unwrap()
            .get("data")
            .and_then(|d| d.get("status"))
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .to_string();
        if status == "Completed" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(status, "Completed", "批准后运行应完成");

    // 动作被实际执行：产出 action.closeAccount world 事件
    let wevents = kernel.list_world_events().await.unwrap();
    assert!(
        wevents
            .iter()
            .any(|e| e.event_type == "action.closeAccount"),
        "批准后动作应被执行"
    );
}

// ── TC-023b：server 挂起式 HTTP 审批端点 — 拒绝 → WB2005 终止 ─────────────────────────
#[tokio::test]
async fn tc023b_server_http_approval_reject() {
    let s = LibSQLStorage::open(":memory:").await.unwrap();
    let kernel = std::sync::Arc::new(RuntimeKernel::new(s));
    kernel
        .create_object("Customer", "001", json!({"name": "Apple", "level": "VIP"}))
        .await
        .unwrap();
    let engine: std::sync::Arc<dyn AgentEngine> = std::sync::Arc::new(FakeEngine);
    let reg = ActionRegistry::from_ir(&parse_odl(ODL_APPROVE).unwrap());

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let ks = kernel.clone();
    let es = engine.clone();
    tokio::spawn(async move {
        let _ = server::serve(ks, es, listener, "ui/dist", reg, false, false, false).await;
    });
    tokio::time::sleep(Duration::from_millis(150)).await;

    let (_, body) = http_call(
        addr,
        "POST",
        "/api/v1/wb/agent/run",
        Some(r#"{"agentId":"retention-agent","goal":"close account"}"#),
    )
    .await;
    let run_id = serde_json::from_str::<Value>(&body).unwrap()["data"]["runId"]
        .as_str()
        .unwrap()
        .to_string();

    // 等待挂起
    let mut req_id = String::new();
    for _ in 0..100 {
        let (_, b) = http_call(
            addr,
            "GET",
            &format!("/api/v1/wb/agent/run/{run_id}/approvals"),
            None,
        )
        .await;
        let list = serde_json::from_str::<Value>(&b)
            .unwrap()
            .get("data")
            .and_then(|d| d.as_array())
            .cloned()
            .unwrap_or_default();
        if let Some(p) = list.iter().find(|p| p["state"] == "Pending") {
            req_id = p["reqId"].as_str().unwrap().to_string();
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(!req_id.is_empty(), "应挂起出一条待审批请求");

    // 浏览器点拒绝
    let (st2, _) = http_call(
        addr,
        "POST",
        &format!("/api/v1/wb/agent/run/{run_id}/approval/{req_id}"),
        Some(r#"{"decision":"reject"}"#),
    )
    .await;
    assert_eq!(st2, 200);

    // 拒绝后运行应终止（Failed）且人工审批标记置位
    let mut status = String::new();
    let mut human = String::new();
    for _ in 0..100 {
        let (_, b) = http_call(
            addr,
            "GET",
            &format!("/api/v1/wb/agent/run/{run_id}/timeline?view=operator"),
            None,
        )
        .await;
        let v = serde_json::from_str::<Value>(&b).unwrap();
        status = v["data"]["status"].as_str().unwrap_or("").to_string();
        human = v["data"]["humanApproval"]
            .as_str()
            .unwrap_or("")
            .to_string();
        if status == "Failed" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(status, "Failed", "拒绝后运行应终止");
    assert_eq!(human, "Required", "拒绝后应标记需人工审批");

    // 审批请求状态应记为 Rejected
    let (_, b) = http_call(
        addr,
        "GET",
        &format!("/api/v1/wb/agent/run/{run_id}/approvals"),
        None,
    )
    .await;
    let list = serde_json::from_str::<Value>(&b)
        .unwrap()
        .get("data")
        .and_then(|d| d.as_array())
        .cloned()
        .unwrap_or_default();
    assert!(
        list.iter()
            .any(|p| p["reqId"] == req_id && p["state"] == "Rejected"),
        "审批请求应记为 Rejected"
    );

    // 重复裁决同一请求应被拒（code=404，已裁决；HTTP 信封用 code 字段而非状态码）
    let (_, b3) = http_call(
        addr,
        "POST",
        &format!("/api/v1/wb/agent/run/{run_id}/approval/{req_id}"),
        Some(r#"{"decision":"approve"}"#),
    )
    .await;
    let code3 = serde_json::from_str::<Value>(&b3)
        .unwrap()
        .get("code")
        .and_then(|c| c.as_i64())
        .unwrap_or(0);
    assert_eq!(code3, 404, "已裁决请求不应再接受裁决");
}

// ── TC-023c：挂起超时回收 — 超时后 state=Expired（无幽灵 Pending），超时残留请求不可再裁决 ──
// 闭环 Critical-1/2：超时后 ApprovalStore 请求必须翻 Expired 并清 tx，否则 UI 持续显示
// 待审批且 resolve 误判成功；Critical-3：Expired 与人工 Rejected 状态可区分。
#[tokio::test]
async fn tc023c_server_http_approval_timeout_expired() {
    let s = LibSQLStorage::open(":memory:").await.unwrap();
    let kernel = std::sync::Arc::new(RuntimeKernel::new(s));
    kernel
        .create_object("Customer", "001", json!({"name": "Apple", "level": "VIP"}))
        .await
        .unwrap();
    let engine: std::sync::Arc<dyn AgentEngine> = std::sync::Arc::new(FakeEngine);
    let reg = ActionRegistry::from_ir(&parse_odl(ODL_APPROVE).unwrap());

    // 注入极短超时（50ms）触发超时回收路径
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let ks = kernel.clone();
    let es = engine.clone();
    tokio::spawn(async move {
        let _ = server::serve_with_timeout(
            ks,
            es,
            listener,
            "ui/dist",
            reg,
            false,
            false,
            false,
            Some(Duration::from_millis(50)),
        )
        .await;
    });
    tokio::time::sleep(Duration::from_millis(150)).await;

    let (_, body) = http_call(
        addr,
        "POST",
        "/api/v1/wb/agent/run",
        Some(r#"{"agentId":"retention-agent","goal":"close account"}"#),
    )
    .await;
    let run_id = serde_json::from_str::<Value>(&body).unwrap()["data"]["runId"]
        .as_str()
        .unwrap()
        .to_string();

    // 应挂起出一条 Pending（先抓 reqId）
    let mut req_id = String::new();
    for _ in 0..100 {
        let (_, b) = http_call(
            addr,
            "GET",
            &format!("/api/v1/wb/agent/run/{run_id}/approvals"),
            None,
        )
        .await;
        let list = serde_json::from_str::<Value>(&b)
            .unwrap()
            .get("data")
            .and_then(|d| d.as_array())
            .cloned()
            .unwrap_or_default();
        if let Some(p) = list.iter().find(|p| p["state"] == "Pending") {
            req_id = p["reqId"].as_str().unwrap().to_string();
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(!req_id.is_empty(), "应挂起出一条待审批请求");

    // 等超时回收（>50ms），运行应终止（Failed）
    let mut status = String::new();
    for _ in 0..100 {
        let (_, b) = http_call(
            addr,
            "GET",
            &format!("/api/v1/wb/agent/run/{run_id}/timeline?view=operator"),
            None,
        )
        .await;
        status = serde_json::from_str::<Value>(&b)
            .unwrap()
            .get("data")
            .and_then(|d| d.get("status"))
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .to_string();
        if status == "Failed" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(status, "Failed", "超时应终止运行");

    // 超时后状态必须翻 Expired（非 Pending 幽灵，也非 Rejected）
    let (_, b) = http_call(
        addr,
        "GET",
        &format!("/api/v1/wb/agent/run/{run_id}/approvals"),
        None,
    )
    .await;
    let list = serde_json::from_str::<Value>(&b)
        .unwrap()
        .get("data")
        .and_then(|d| d.as_array())
        .cloned()
        .unwrap_or_default();
    assert!(
        list.iter()
            .any(|p| p["reqId"] == req_id && p["state"] == "Expired"),
        "超时后审批请求应翻 Expired（无幽灵 Pending）"
    );

    // 超时残留请求不可再裁决：resolve 应返回 404（原为 Critical-2 误判成功）
    let (_, b3) = http_call(
        addr,
        "POST",
        &format!("/api/v1/wb/agent/run/{run_id}/approval/{req_id}"),
        Some(r#"{"decision":"approve"}"#),
    )
    .await;
    let code3 = serde_json::from_str::<Value>(&b3)
        .unwrap()
        .get("code")
        .and_then(|c| c.as_i64())
        .unwrap_or(0);
    assert_eq!(code3, 404, "超时(Expired)请求不应再接受裁决");

    // M2：超时状态须同步落库（DB 真相源），重启后不得残留 Pending 幽灵
    let db_rows = kernel
        .storage()
        .list_approvals_for_run(&run_id)
        .await
        .unwrap();
    let row = db_rows
        .iter()
        .find(|r| r.id == req_id)
        .expect("审批行应已持久化");
    assert_eq!(
        row.state, "Expired",
        "DB 中审批行应同步为 Expired（非 Pending 幽灵）"
    );
}

// ── TC-023d：超时 Expired 与人工 Rejected 状态可区分（Critical-3） ──────────────────
#[tokio::test]
async fn tc023d_timeout_expired_distinct_from_rejected() {
    use openaip_os::planner::{ApprovalState, ApprovalStore};
    let store = ApprovalStore::new();

    // 人工拒绝路径：resolve 置 Rejected
    let (tx, _rx) = tokio::sync::oneshot::channel();
    store.requests_insert_for_test("r_reject", ApprovalState::Pending, Some(tx));
    assert!(store.resolve("r_reject", false), "人工拒绝应成功裁决");
    assert_eq!(
        store.get("r_reject").unwrap().state,
        ApprovalState::Rejected,
        "人工拒绝应记为 Rejected"
    );

    // 超时路径：expire 置 Expired
    let (tx2, _rx2) = tokio::sync::oneshot::channel();
    store.requests_insert_for_test("r_expire", ApprovalState::Pending, Some(tx2));
    store.expire("r_expire");
    assert_eq!(
        store.get("r_expire").unwrap().state,
        ApprovalState::Expired,
        "超时回收应记为 Expired"
    );
    // Expired 不可再裁决
    assert!(!store.resolve("r_expire", true), "Expired 不应再接受裁决");
}

// ── TC-023e：裁决端点必须匹配 run_id（跨 run 裁决 → 404） ─────────────────────────────
#[tokio::test]
async fn tc023e_approval_wrong_run_id_rejected() {
    let s = LibSQLStorage::open(":memory:").await.unwrap();
    let kernel = std::sync::Arc::new(RuntimeKernel::new(s));
    kernel
        .create_object("Customer", "001", json!({"name": "Apple", "level": "VIP"}))
        .await
        .unwrap();
    let engine: std::sync::Arc<dyn AgentEngine> = std::sync::Arc::new(FakeEngine);
    let reg = ActionRegistry::from_ir(&parse_odl(ODL_APPROVE).unwrap());

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let ks = kernel.clone();
    let es = engine.clone();
    tokio::spawn(async move {
        let _ = server::serve(ks, es, listener, "ui/dist", reg, false, false, false).await;
    });
    tokio::time::sleep(Duration::from_millis(150)).await;

    let (_, body) = http_call(
        addr,
        "POST",
        "/api/v1/wb/agent/run",
        Some(r#"{"agentId":"retention-agent","goal":"close account"}"#),
    )
    .await;
    let run_id = serde_json::from_str::<Value>(&body).unwrap()["data"]["runId"]
        .as_str()
        .unwrap()
        .to_string();

    let mut req_id = String::new();
    for _ in 0..100 {
        let (_, b) = http_call(
            addr,
            "GET",
            &format!("/api/v1/wb/agent/run/{run_id}/approvals"),
            None,
        )
        .await;
        let list = serde_json::from_str::<Value>(&b)
            .unwrap()
            .get("data")
            .and_then(|d| d.as_array())
            .cloned()
            .unwrap_or_default();
        if let Some(p) = list.iter().find(|p| p["state"] == "Pending") {
            req_id = p["reqId"].as_str().unwrap().to_string();
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(!req_id.is_empty(), "应挂起出一条待审批请求");

    // 用错误 run_id 裁决 → JSON code=404
    let wrong_run = format!("{run_id}_wrong");
    let (_, b_wrong) = http_call(
        addr,
        "POST",
        &format!("/api/v1/wb/agent/run/{wrong_run}/approval/{req_id}"),
        Some(r#"{"decision":"approve"}"#),
    )
    .await;
    let code_wrong = serde_json::from_str::<Value>(&b_wrong)
        .unwrap()
        .get("code")
        .and_then(|c| c.as_i64())
        .unwrap_or(0);
    assert_eq!(code_wrong, 404, "跨 run 裁决应被拒绝");

    // 正确 run_id 裁决仍应成功
    let (_, b_ok) = http_call(
        addr,
        "POST",
        &format!("/api/v1/wb/agent/run/{run_id}/approval/{req_id}"),
        Some(r#"{"decision":"approve"}"#),
    )
    .await;
    let code_ok = serde_json::from_str::<Value>(&b_ok)
        .unwrap()
        .get("code")
        .and_then(|c| c.as_i64())
        .unwrap_or(-1);
    assert_eq!(code_ok, 0, "正确 run_id 应能裁决");
}
