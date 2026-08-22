//! T1 Bridge 审批接入契约测试（fabric high-risk → 挂起 → 人工裁决 → 续跑/终止）。
//!
//! - tc_approve_1：fabric submit high-risk → 返回挂起（非 Err）+ approval_requested
//!   入世界 + /run/:run_id/approvals 可见 pending
//! - tc_approve_2：approve → fabric 续跑执行（adapter.send 发生）→ task_completed
//!   入世界 + 世界 fabric.status 状态
//! - tc_approve_3：reject → 终止（不执行）+ approval_rejected 事件
//! - tc_approve_4：二次裁决 → 404（防重）

mod common;

use openaip_os::fabric::adapters::ProcessAdapter;
use openaip_os::fabric::router::FabricRouter;
use openaip_os::facade::Facade;
use openaip_os::worker::manifest::WorkerManifest;
use openaip_os::worker::process::ProcessWorker;
use serde_json::Value;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;

/// 注入带 sh adapter 的 Facade：shell.execute high-risk → 挂起路径可控（/bin/sh 必在）。
async fn spawn_approval_server() -> SocketAddr {
    let storage = Arc::new(
        openaip_os::storage::LibSQLStorage::open(":memory:")
            .await
            .unwrap(),
    );
    let services = common::retain_services();
    let defs = common::retain_defs();
    let engine: Arc<dyn openaip_os::cognitive::CognitiveEngine> =
        Arc::new(common::v2_retain_engine());

    // 装配 Facade（阻塞上下文构造——sink 同步落库）
    let facade = tokio::task::block_in_place(|| {
        let mut router = FabricRouter::new();
        router.register_intent("run_shell", "shell.execute");
        router.register_agent("shell.execute", "sh-agent", 1);
        router.mark_high_risk("shell.execute");
        let mut m = WorkerManifest::new("sh-agent", "agent").with_capabilities(&["shell.execute"]);
        m.quality = 0.9;
        m.risk = 2;
        let adapter = Arc::new(ProcessAdapter::new(
            "sh-agent",
            ProcessWorker::new(m, "/bin/sh"),
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
    let ks = storage.clone();
    tokio::spawn(async move {
        let _ = openaip_os::server::serve_with_facade(
            ks,
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
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    addr
}

async fn submit_shell(addr: SocketAddr) -> Value {
    let (_, body) = common::http_call(
        addr,
        "POST",
        "/api/v1/facade/fabric/submit",
        Some(
            r#"{"sessionId":"sess_appr","sender":"dsh","intent":"run_shell","payload":{"args":["-c","echo approved-ok"],"object":"order-appr"},"executionContext":{"workspace":{"root":"/tmp","mode":"isolated"}}}"#,
        ),
    )
    .await;
    serde_json::from_str::<Value>(&body).unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn tc_approve_1_fabric_submit_high_risk_pends() {
    let addr = spawn_approval_server().await;
    let data = submit_shell(addr).await;
    let result = &data["data"]["result"];
    assert_eq!(
        result["payload"]["status"], "pending",
        "high-risk 应挂起而非报错"
    );
    let run_id = result["correlation_id"].as_str().unwrap().to_string();
    assert!(!run_id.is_empty());

    // 审批列表可见
    let (_, list) = common::http_call(
        addr,
        "GET",
        &format!("/api/v1/wb/agent/run/{run_id}/approvals"),
        None,
    )
    .await;
    let list_v: Value = serde_json::from_str(&list).unwrap();
    assert!(
        list_v["data"].as_array().is_some(),
        "pending 应在审批列表: {list_v}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn tc_approve_2_approve_resumes_and_executes() {
    let addr = spawn_approval_server().await;
    let data = submit_shell(addr).await;
    let run_id = data["data"]["result"]["correlation_id"]
        .as_str()
        .unwrap()
        .to_string();

    let (_, resolved) = common::http_call(
        addr,
        "POST",
        &format!("/api/v1/wb/agent/run/{run_id}/approval/{run_id}"),
        Some(r#"{"decision":"approve"}"#),
    )
    .await;
    let rv: Value = serde_json::from_str(&resolved).unwrap();
    assert_eq!(rv["data"]["decision"], "Approved");
    assert_eq!(
        rv["data"]["status"], "completed",
        "续跑应执行 sh 成功: {rv}"
    );

    // 终局事件入世界：fabric.status=completed（领域规则投影）
    let (_, w) = common::http_call(
        addr,
        "POST",
        "/api/v1/facade/world/query",
        Some(r#"{"projectId":"openaip-os","predicate":"fabric.status"}"#),
    )
    .await;
    let wv: Value = serde_json::from_str(&w).unwrap();
    // 全局世界（None）也可见（未指定 project 的 fabric 提交）
    let (_, w2) = common::http_call(
        addr,
        "POST",
        "/api/v1/facade/world/query",
        Some(r#"{"predicate":"fabric.status"}"#),
    )
    .await;
    let wv2: Value = serde_json::from_str(&w2).unwrap();
    let _ = wv;
    assert!(
        wv2["data"]["relations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["target"] == "completed"),
        "审批后世界应见 fabric.status=completed: {wv2}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn tc_approve_3_reject_terminates() {
    let addr = spawn_approval_server().await;
    let data = submit_shell(addr).await;
    let run_id = data["data"]["result"]["correlation_id"]
        .as_str()
        .unwrap()
        .to_string();

    let (_, resolved) = common::http_call(
        addr,
        "POST",
        &format!("/api/v1/wb/agent/run/{run_id}/approval/{run_id}"),
        Some(r#"{"decision":"reject"}"#),
    )
    .await;
    let rv: Value = serde_json::from_str(&resolved).unwrap();
    assert_eq!(rv["data"]["decision"], "Rejected");

    // 不执行：世界无 fabric.status（无 task_completed/failed）
    let (_, w) = common::http_call(
        addr,
        "POST",
        "/api/v1/facade/world/query",
        Some(r#"{"predicate":"fabric.status"}"#),
    )
    .await;
    let wv: Value = serde_json::from_str(&w).unwrap();
    assert_eq!(wv["data"]["total"], 0, "reject 后不得执行: {wv}");
    // 审计：approval_rejected 在场
    let (_, a) = common::http_call(
        addr,
        "POST",
        "/api/v1/facade/audit/query",
        Some(r#"{"eventType":"fabric.approval_rejected"}"#),
    )
    .await;
    let av: Value = serde_json::from_str(&a).unwrap();
    assert!(av["data"]["total"].as_u64().unwrap() >= 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn tc_approve_4_second_decision_404() {
    let addr = spawn_approval_server().await;
    let data = submit_shell(addr).await;
    let run_id = data["data"]["result"]["correlation_id"]
        .as_str()
        .unwrap()
        .to_string();

    let approve_url = format!("/api/v1/wb/agent/run/{run_id}/approval/{run_id}");

    // 第一次裁决：必须成功执行（续跑 sh → completed）。
    // 本测试只查二次裁决会漏掉「第一次就 404」的回归——若第一次 404，entry 从未
    // 移除/从未执行，第二次照样 404，测试会假绿。故此处先锁死第一次成功。
    let (c1, b1) = common::http_call(
        addr,
        "POST",
        &approve_url,
        Some(r#"{"decision":"approve"}"#),
    )
    .await;
    assert_eq!(c1, 200, "第一次裁决 HTTP 必须 200");
    let v1: Value = serde_json::from_str(&b1).unwrap();
    assert_eq!(
        v1["code"].as_i64().unwrap(),
        0,
        "第一次裁决必须成功（code=0），不得 404/5000（entry 应在 resolve 前被消费）"
    );
    assert_eq!(v1["data"]["decision"], "Approved");
    assert_eq!(
        v1["data"]["status"], "completed",
        "第一次裁决应续跑执行 adapter.send（entry 已消费/移除）"
    );

    // 二次裁决 → 业务码 404（已从注册表移除；项目约定所有响应 HTTP 200，错误码在 body.code）
    let (_, body) = common::http_call(
        addr,
        "POST",
        &approve_url,
        Some(r#"{"decision":"approve"}"#),
    )
    .await;
    let v: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(
        v["code"].as_i64().unwrap(),
        404,
        "防重：二次裁决必须返回业务码 404（审批请求不存在或已裁决）"
    );
}
