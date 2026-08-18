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
            project_id: None,
        })
        .await
        .unwrap();
    kernel
        .apply_world(openaip_os::storage::WorldCommand::LinkRelation {
            source: "u1".into(),
            edge: "placed".into(),
            target: "o1".into(),
            project_id: None,
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
            project_id: None,
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
            project_id: None,
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
    // 无审批/无提供方 → 均不得成功执行（fail-closed 一致语义）。
    // T1：high-risk 意图进入挂起（code=0, status=pending）而非执行，故改判"不成功执行"。
    assert_eq!(b1["code"], b2["code"], "同一意图两次提交语义一致");
    assert_ne!(
        b1["data"]["result"]["status"], "completed",
        "fail-closed：run_shell 无审批/无提供方不得执行成功"
    );
}

// ── tc_sf5：fabric.* 事件实时达 WS（C1 完整性——过滤层放行双 None 世界级事实）──
// C1 修复把 Bridge sink 接上 event_tx 广播、AppState 复用同一频道后，WS 过滤层
// （run_id/agent_id 精确匹配）仍会丢弃 fabric 事件（P5 约定 run_id=None, agent_id=None）
// ——广播到了也到不了 /stream。本测试钉死过滤层放行：订阅任意 id 即应收 fabric 事件。
#[tokio::test(flavor = "multi_thread")]
async fn tc_sf5_fabric_events_reach_ws_realtime() {
    use futures_util::StreamExt;
    use std::time::Duration;
    use tokio_tungstenite::tungstenite::Message;
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;

    let kernel = std::sync::Arc::new(
        openaip_os::storage::LibSQLStorage::open(":memory:")
            .await
            .unwrap(),
    );
    let addr = common::spawn_server(kernel).await;

    // 订阅 WS（id 任意——fabric 世界级事实应放行到所有订阅者）
    let url = format!("ws://{addr}/api/v1/wb/agent/observer/stream");
    let (mut ws, _resp) = tokio_tungstenite::connect_async(url.into_client_request().unwrap())
        .await
        .expect("WS 连接应成功");

    // 提交 fabric 任务（run_shell 高风险管理 → 一串 fabric.* 事件：capability_invoked /
    // task_requested / message_sent / approval_requested，同步落库 + 广播）
    let (code, body) = call(
        addr,
        "POST",
        "/api/v1/facade/fabric/submit",
        Some(json!({
            "sessionId": "sess_ws",
            "sender": "dsh",
            "intent": "run_shell",
            "payload": { "args": ["-c", "echo ws-ok"], "object": "order-ws" }
        })),
    )
    .await;
    assert_eq!(code, 200);
    assert_eq!(body["data"]["result"]["payload"]["status"], "pending");

    // 收集 WS 事件直到看到 fabric.approval_requested（5s 超时）
    let mut got: Vec<String> = Vec::new();
    for _ in 0..100 {
        if let Ok(Some(Ok(Message::Text(t)))) =
            tokio::time::timeout(Duration::from_millis(50), ws.next()).await
        {
            let v: Value = serde_json::from_str(&t).unwrap();
            let ty = v["eventType"].as_str().unwrap_or("").to_string();
            if ty.starts_with("fabric.") {
                got.push(ty.clone());
                if ty == "fabric.approval_requested" {
                    break;
                }
            }
        }
    }
    assert!(
        got.iter().any(|t| t == "fabric.approval_requested"),
        "fabric 事件必须实时达 WS（C1 完整性：过滤层放行双 None 世界级事实），实际收到: {got:?}"
    );
}

// ── tc_sf6：B1 修复——run_shell 真实执行（专用 shell-exec 提供方，非 harness 假广告）──
// build_facade 现接线 shell_adapter（/bin/sh，POSIX 必在）；harness（dsh/pi/deepseek）
// 已撤下 shell.execute 声明。本测试钉死：submit run_shell → 高风险管理挂起 → approve →
// 真跑 /bin/sh → 终态 completed + sender=="shell"（判定执行器身份，修复前落 dsh 必 failed）；
// 并交叉验世界投影 fabric.status=completed（领域规则 task_completed→status 已落地）。
#[tokio::test(flavor = "multi_thread")]
async fn tc_sf6_run_shell_executes_via_shell_exec_provider() {
    let kernel = std::sync::Arc::new(
        openaip_os::storage::LibSQLStorage::open(":memory:")
            .await
            .unwrap(),
    );
    let addr = common::spawn_server(kernel).await;

    // 提交 run_shell（高风险管理 → 挂起，不立即执行）
    let (code, body) = call(
        addr,
        "POST",
        "/api/v1/facade/fabric/submit",
        Some(json!({
            "sessionId": "sess_shell",
            "sender": "dsh",
            "intent": "run_shell",
            "payload": { "args": ["-c", "echo hi-shell"] }
        })),
    )
    .await;
    assert_eq!(code, 200);
    assert_eq!(body["code"], 0, "submit 应成功创建挂起运行: {body}");
    assert_eq!(
        body["data"]["result"]["payload"]["status"], "pending",
        "high-risk 应挂起（不立即执行）: {body}"
    );
    let run_id = body["data"]["result"]["correlation_id"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(!run_id.is_empty());

    // 审批续跑（spawn_server 默认角色可裁决；真跑 /bin/sh）
    let (acode, av) = call(
        addr,
        "POST",
        &format!("/api/v1/wb/agent/run/{run_id}/approval/{run_id}"),
        Some(json!({"decision":"approve"})),
    )
    .await;
    assert_eq!(acode, 200);
    assert_eq!(av["code"], 0, "审批续跑应成功: {av}");
    assert_eq!(
        av["data"]["status"], "completed",
        "真跑 /bin/sh 应 completed（修复前落 dsh 必 failed）: {av}"
    );
    // 执行器身份判定：必须是专用 shell-exec（B1：harness 已撤下 shell.execute）
    assert_eq!(
        av["data"]["sender"], "shell",
        "执行器必须是 shell-exec（B1 修复核心：route 到真 /bin/sh）: {av}"
    );

    // 交叉验：执行结果入世界（领域规则 fabric.task_completed → fabric.status=completed）
    let (_, w) = call(
        addr,
        "POST",
        "/api/v1/facade/world/query",
        Some(json!({"predicate":"fabric.status"})),
    )
    .await;
    assert!(
        w["data"]["relations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["target"] == "completed"),
        "审批后世界应见 fabric.status=completed: {w}"
    );
}
