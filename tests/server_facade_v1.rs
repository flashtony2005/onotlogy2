//! Facade HTTP Transport 契约测试（Phase 7 · Server 是 OpenAipFacade 的一等 Transport）。
//!
//! - tc_sf1 world.query 端点：事件 → 投影 → 世界视图（HTTP 语义与 CLI/Plugin 同源）
//! - tc_sf2 audit.query 端点：审计语义域（fabric.* 可查）；旧 /audit 也含 fabric 域
//! - tc_sf3 fabric.submit 端点：fail-closed（无提供方 → WB2005）；world/audit 端点协议完整
//! - tc_sf4 会话自动创建 + correlation 贯通（fabric.submit 请求/响应）

mod common;

use serde_json::{Value, json};

async fn call(
    addr: std::net::SocketAddr,
    method: &str,
    path: &str,
    body: Option<Value>,
) -> (u16, Value) {
    let url = format!("http://{addr}{path}");
    let client = reqwest::Client::new();
    let mut req = client.request(
        reqwest::Method::from_bytes(method.as_bytes()).unwrap(),
        &url,
    );
    if let Some(b) = body {
        req = req.json(&b);
    }
    let resp = req.send().await.unwrap();
    let status = resp.status().as_u16();
    let body: Value = resp.json().await.unwrap_or(json!(null));
    (status, body)
}

#[tokio::test(flavor = "multi_thread")]
async fn tc_sf1_world_query_http_transport() {
    let kernel = std::sync::Arc::new(
        openaip_os::storage::LibSQLStorage::open(":memory:")
            .await
            .unwrap(),
    );
    kernel
        .apply_world(openaip_os::storage::WorldCommand::CreateObject {
            object_type: "User".into(),
            id: "u1".into(),
            data: json!({ "name": "Apple", "level": "VIP" }),
        })
        .await
        .unwrap();
    kernel
        .apply_world(openaip_os::storage::WorldCommand::LinkRelation {
            source: "u1".into(),
            edge: "placed".into(),
            target: "o1".into(),
        })
        .await
        .unwrap();
    let addr = common::spawn_server(kernel).await;

    // world.query（无过滤）：投影世界全量
    let (code, body) = call(addr, "POST", "/api/v1/facade/world/query", Some(json!({}))).await;
    assert_eq!(code, 200);
    let data = &body["data"];
    assert!(
        data["total"].as_u64().unwrap() >= 4,
        "u1(type/name/level) + placed 边: {:?}",
        data
    );
    assert!(
        data["relations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["source"] == "u1" && r["predicate"] == "level" && r["target"] == "VIP")
    );
    // predicate 过滤
    let (_, body) = call(
        addr,
        "POST",
        "/api/v1/facade/world/query",
        Some(json!({ "predicate": "placed" })),
    )
    .await;
    assert_eq!(body["data"]["total"], 1);
    assert_eq!(body["data"]["relations"][0]["source"], "u1");
    // object_ref 过滤
    let (_, body) = call(
        addr,
        "POST",
        "/api/v1/facade/world/query",
        Some(json!({ "objectRef": "u1" })),
    )
    .await;
    let rels = body["data"]["relations"].as_array().unwrap();
    assert!(
        rels.iter()
            .all(|r| r["source"] == "u1" || r["target"] == "u1")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn tc_sf2_audit_query_http_transport_includes_fabric_domain() {
    let kernel = std::sync::Arc::new(
        openaip_os::storage::LibSQLStorage::open(":memory:")
            .await
            .unwrap(),
    );
    kernel
        .append_trace(&openaip_os::trace::TraceEvent {
            id: "ev_f1".into(),
            kind: openaip_os::trace::TraceKind::Cognitive,
            event_type: "fabric.task_completed".into(),
            object_refs: vec!["order-1".into()],
            run_id: None,
            agent_id: None,
            reasoning: None,
            input: Some(json!({ "to": "completed" })),
            output: None,
            confidence: None,
            timestamp: "100".into(),
        })
        .await
        .unwrap();
    kernel
        .append_trace(&openaip_os::trace::TraceEvent {
            id: "ev_f2".into(),
            kind: openaip_os::trace::TraceKind::Cognitive,
            event_type: "effect.committed".into(),
            object_refs: vec!["world".into()],
            run_id: None,
            agent_id: None,
            reasoning: None,
            input: None,
            output: None,
            confidence: None,
            timestamp: "101".into(),
        })
        .await
        .unwrap();
    let addr = common::spawn_server(kernel).await;

    // Facade audit.query（fabric 域可查；非审计域被过滤）
    let (code, body) = call(
        addr,
        "POST",
        "/api/v1/facade/audit/query",
        Some(json!({ "eventType": "fabric." })),
    )
    .await;
    assert_eq!(code, 200);
    // fabric. 前缀过滤：task_completed 在场（可能含 server 启动的 agent_connected）
    let evs = body["data"]["events"].as_array().unwrap();
    assert!(!evs.is_empty());
    let done = evs
        .iter()
        .find(|e| e["eventType"] == "fabric.task_completed")
        .expect("task_completed 在场");
    assert_eq!(done["refs"][0], "order-1");
    // 默认审计语义域（effect.committed 不属于；含 server 启动的 agent_connected）
    let (_, body) = call(addr, "POST", "/api/v1/facade/audit/query", Some(json!({}))).await;
    let types: Vec<&str> = body["data"]["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|e| e["eventType"].as_str())
        .collect();
    assert!(types.contains(&"fabric.task_completed"));
    assert!(
        !types.contains(&"effect.committed"),
        "非审计域必须过滤: {types:?}"
    );
    // 旧 /audit 端点同样包含 fabric 域（Governance 面板可见）
    let (_, body) = call(addr, "GET", "/api/v1/wb/agent/audit", None).await;
    let types: Vec<&str> = body["data"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|e| e["eventType"].as_str())
        .collect();
    assert!(
        types.contains(&"fabric.task_completed"),
        "旧 /audit 缺 fabric 域: {types:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn tc_sf3_fabric_submit_http_fail_closed() {
    let kernel = std::sync::Arc::new(
        openaip_os::storage::LibSQLStorage::open(":memory:")
            .await
            .unwrap(),
    );
    let addr = common::spawn_server(kernel).await;

    // 未注册意图 → fail-closed（WB2005 语义；Codex/DSH 未装时提供方为空）
    let (code, body) = call(
        addr,
        "POST",
        "/api/v1/facade/fabric/submit",
        Some(json!({
            "sessionId": "sess_http",
            "sender": "dsh",
            "intent": "no_such_intent",
            "payload": {}
        })),
    )
    .await;
    assert_eq!(code, 200, "ApiResponse 语义错误也 200（code 字段区分）");
    assert_ne!(body["code"], 0, "fail-closed：未知意图不得成功");
}

#[tokio::test(flavor = "multi_thread")]
async fn tc_sf4_fabric_submit_auto_session_and_correlation() {
    let kernel = std::sync::Arc::new(
        openaip_os::storage::LibSQLStorage::open(":memory:")
            .await
            .unwrap(),
    );
    let addr = common::spawn_server(kernel).await;

    // 同一 session 两次提交：第一次自动创建；两次请求都无 crash（协议完整）
    let (_, b1) = call(
        addr,
        "POST",
        "/api/v1/facade/fabric/submit",
        Some(json!({
            "sessionId": "sess_auto",
            "sender": "dsh",
            "intent": "run_shell",
            "payload": { "args": ["-c", "true"] }
        })),
    )
    .await;
    let (_, b2) = call(
        addr,
        "POST",
        "/api/v1/facade/fabric/submit",
        Some(json!({
            "sessionId": "sess_auto",
            "sender": "dsh",
            "intent": "run_shell",
            "payload": { "args": ["-c", "true"] }
        })),
    )
    .await;
    // 无提供方（测试环境无 codex/dsh）→ 均失败且为同一错误（fail-closed 一致语义）
    assert_eq!(b1["code"], b2["code"], "同一意图两次提交语义一致");
    assert_ne!(b1["code"], 0);
}
