//! V0.9 WS 视图钳制测试契约（对应 WS 实时流最小权限，与 REST timeline 同一套 clamp_view）。
//!
//! 验证：Developer 经 WS 看全量（reasoning/input/output）；Executive 经 WS 仅 Goal/Action 且无细节；
//! 无令牌连接 WS → 被拒（401）。

mod common;
use common::*;

use futures_util::StreamExt;
use openaip_os::auth::{IdentityProvider, LocalIdentityProvider};
use openaip_os::server;
use openaip_os::storage::{LibSQLStorage, UserRecord};
use serde_json::{Value, json};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::{HeaderValue, Request};

/// 登录并返回 (status, token)。
async fn login(addr: SocketAddr, username: &str, password: &str) -> (u16, Option<String>) {
    let body = json!({ "username": username, "password": password }).to_string();
    let (st, resp) = http_call_auth(addr, "POST", "/api/v1/wb/auth/login", Some(&body), None).await;
    if st == 200 {
        let v: Value = serde_json::from_str(&resp).unwrap();
        (st, v["data"]["token"].as_str().map(|s| s.to_string()))
    } else {
        (st, None)
    }
}

/// 连接 WS 并收集消息（2s 超时），带可选 Bearer 令牌。成功返回消息列表；无令牌被拒返回 Err。
async fn collect_ws(addr: &SocketAddr, path: &str, token: Option<&str>) -> Result<Vec<Value>, ()> {
    let url = format!("ws://{addr}{path}");
    let mut req = url.into_client_request().map_err(|_| ())?;
    if let Some(t) = token {
        req.headers_mut().insert(
            "Authorization",
            HeaderValue::from_str(&format!("Bearer {t}")).unwrap(),
        );
    }
    collect_ws_req(req).await
}

/// 连接 WS（不带 Authorization 头）——浏览器 WebSocket 场景：令牌经 `?token=` 查询参数传递。
async fn collect_ws_query(addr: &SocketAddr, path: &str) -> Result<Vec<Value>, ()> {
    let url = format!("ws://{addr}{path}");
    let req = url.into_client_request().map_err(|_| ())?;
    collect_ws_req(req).await
}

/// 共享收集逻辑：连接后收集消息至 2s 超时或关闭。
async fn collect_ws_req(req: Request<()>) -> Result<Vec<Value>, ()> {
    let (mut ws, _resp) = tokio_tungstenite::connect_async(req)
        .await
        .map_err(|_| ())?;
    let mut out = Vec::new();
    let sleep = tokio::time::sleep(Duration::from_secs(2));
    tokio::pin!(sleep);
    loop {
        tokio::select! {
            msg = ws.next() => {
                match msg {
                    Some(Ok(Message::Text(t))) => out.push(serde_json::from_str(&t).unwrap()),
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => {}
                }
            }
            _ = &mut sleep => break,
        }
    }
    let _ = ws.close(None).await;
    Ok(out)
}

// ── TC-ws-a：Developer 经 WS 看全量（reasoning/input/output） ───────────────

#[tokio::test]
async fn tc_ws_developer_full_detail() {
    let (addr, storage) = spawn_ws_server_raw().await;
    let kernel = Arc::new(storage.clone());
    seed_and_run(&kernel).await;
    let run_id = last_run_id(&kernel).await;

    let (_, dev_tok) = login(addr, "admin", "adminpw").await;
    let dev_tok = dev_tok.expect("admin 应登录");
    let msgs = collect_ws(
        &addr,
        &format!("/api/v1/wb/agent/{run_id}/stream"),
        Some(&dev_tok),
    )
    .await
    .expect("Developer WS 应连上");

    assert!(!msgs.is_empty(), "Developer WS 应收到事件");
    assert!(
        msgs.iter().any(|m| m["reasoning"].is_string()),
        "Developer WS 应保留 reasoning 细节"
    );
    assert!(
        msgs.iter().any(|m| m["output"].is_object()),
        "Developer WS 应保留 output 细节"
    );
    // 8 个认知步类型俱在（全量视图）
    let types: Vec<&str> = msgs
        .iter()
        .map(|m| m["eventType"].as_str().unwrap())
        .collect();
    for step in ["Goal", "Intent", "Reasoning", "Planning", "Action"] {
        assert!(types.contains(&step), "Developer WS 应含 {step} 事件");
    }
}

// ── TC-ws-b：Executive 经 WS 仅 Goal/Action 且无细节 ────────────────────────
#[tokio::test]
async fn tc_ws_executive_clamped() {
    let (addr, storage) = spawn_ws_server_raw().await;
    let kernel = Arc::new(storage.clone());
    seed_and_run(&kernel).await;
    let run_id = last_run_id(&kernel).await;

    let (_, exec_tok) = login(addr, "exec", "execpw").await;
    let exec_tok = exec_tok.expect("exec 应登录");
    let msgs = collect_ws(
        &addr,
        &format!("/api/v1/wb/agent/{run_id}/stream"),
        Some(&exec_tok),
    )
    .await
    .expect("Executive WS 应连上");

    assert!(!msgs.is_empty(), "Executive WS 应收到事件");
    // 无越权细节字段
    assert!(
        msgs.iter().all(|m| !m["reasoning"].is_string()
            && !m["input"].is_object()
            && !m["output"].is_object()),
        "Executive WS 不得含 reasoning/input/output"
    );
    // 仅 Goal/Action（与 REST Executive 视图一致）
    assert!(
        msgs.iter()
            .all(|m| m["eventType"] == "Goal" || m["eventType"] == "Action"),
        "Executive WS 仅应含 Goal/Action 事件"
    );
}

// ── TC-ws-c：无令牌连接 WS → 被拒（401，connect 失败） ─────────────────────
#[tokio::test]
async fn tc_ws_no_token_rejected() {
    let (addr, storage) = spawn_ws_server_raw().await;
    let kernel = Arc::new(storage.clone());
    seed_and_run(&kernel).await;
    let run_id = last_run_id(&kernel).await;

    let res = collect_ws(&addr, &format!("/api/v1/wb/agent/{run_id}/stream"), None).await;
    assert!(res.is_err(), "无令牌 WS 应被拒（401）");
}

// ── TC-ws-d：浏览器场景 — 令牌经 ?token= 查询参数（无 Authorization 头）可连接 ──
// 浏览器 WebSocket 无法设置请求头；服务端须仅对 WS 流路径接受 `?token=`（REST 仍只认 Bearer）。
#[tokio::test]
async fn tc_ws_query_token_authenticates() {
    let (addr, storage) = spawn_ws_server_raw().await;
    let kernel = Arc::new(storage.clone());
    seed_and_run(&kernel).await;
    let run_id = last_run_id(&kernel).await;

    let (_, tok) = login(addr, "admin", "adminpw").await;
    let tok = tok.expect("admin 应登录");
    let msgs = collect_ws_query(
        &addr,
        &format!("/api/v1/wb/agent/{run_id}/stream?token={tok}"),
    )
    .await;
    assert!(msgs.is_ok(), "?token= 应通过 WS 鉴权");
    assert!(
        !msgs.unwrap().is_empty(),
        "应收到历史事件（?token= 连接有效）"
    );
}

/// 启动强制认证服务并直接返回 storage（供测试构造 kernel 触发运行）。
async fn spawn_ws_server_raw() -> (SocketAddr, LibSQLStorage) {
    let storage = LibSQLStorage::open(":memory:").await.unwrap();
    let idp = LocalIdentityProvider::new(storage.clone());
    idp.bootstrap_admin("admin", "adminpw").await.unwrap();
    let hash = LocalIdentityProvider::hash_password("execpw").unwrap();
    storage
        .create_user(&UserRecord {
            id: "user.exec".into(),
            username: "exec".into(),
            password_hash: hash,
            role: "executive".into(),
            display_name: "Exec".into(),
            disabled: false,
        })
        .await
        .unwrap();
    let kernel = Arc::new(storage.clone());
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = server::serve_with_timeout(
            kernel,
            retain_services(),
            retain_defs(),
            std::sync::Arc::new(v2_retain_engine()),
            listener,
            "ui/dist",
            false,
            false,
            true, // enforce_auth
            None,
        )
        .await;
    });
    tokio::time::sleep(Duration::from_millis(150)).await;
    (addr, storage)
}

/// 种子客户并跑通 V2 认知闭环（事件落库，供 WS 历史回放；确定性 FakeEngine）。
async fn seed_and_run(kernel: &Arc<LibSQLStorage>) {
    let _ = kernel
        .create("Customer", "001", json!({"name": "Apple", "level": "VIP"}))
        .await;
    use openaip_os::cognitive::runner::run_agent;
    use openaip_os::cognitive::{Agent, FakeEngine};
    use openaip_os::kernel::{Kernel, Scope};
    use openaip_os::storage::ObjectEngine;
    let mut k = Kernel::new();
    let mut agent = Agent::new(
        "retention-agent",
        k.create_context(None, Scope::World, "world").unwrap(),
    );
    let services = retain_services();
    let defs = retain_defs();
    let mut trace: Vec<openaip_os::trace::TraceEvent> = Vec::new();
    let mut approvals: Vec<openaip_os::cognitive::ApprovalRequest> = Vec::new();
    let _ = run_agent(
        &mut k,
        &mut agent,
        &services,
        &FakeEngine::new(vec![openaip_os::cognitive::PlanStep {
            capability: "Customer.retain".into(),
            args: json!({ "target": "001" }),
            when: Some(Box::new(|v: &openaip_os::cognitive::AgentView| {
                !v.related.iter().any(|(_, p, _)| p == "needs_retention")
            })),
        }]),
        Some(defs),
        None,
        "run_seed",
        "reduce churn",
        &mut trace,
        &mut approvals,
        &mut |_| panic!("无审批预期"),
    );
    for e in &trace {
        let _ = kernel.append_trace(e).await;
    }
}

/// 取最近一次运行的 run_id（按 trace 事件反查）。
async fn last_run_id(kernel: &Arc<LibSQLStorage>) -> String {
    use openaip_os::storage::TraceQuery;
    let events = kernel
        .list_events(TraceQuery::for_agent("retention-agent"))
        .await
        .unwrap();
    events
        .iter()
        .rev()
        .find_map(|e| e.run_id.clone())
        .expect("应存在 run_id")
}
