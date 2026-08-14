//! V0.9 WS 视图钳制测试契约（对应 WS 实时流最小权限，与 REST timeline 同一套 clamp_view）。
//!
//! 验证：Developer 经 WS 看全量（reasoning/input/output）；Executive 经 WS 仅 Goal/Action 且无细节；
//! 无令牌连接 WS → 被拒（401）。

use async_trait::async_trait;
use futures_util::StreamExt;
use openaip_os::agent::{AgentEngine, AgentError, Context, Decision};
use openaip_os::auth::{IdentityProvider, LocalIdentityProvider};
use openaip_os::planner::generate_run_id;
use openaip_os::runtime::RuntimeKernel;
use openaip_os::server;
use openaip_os::storage::{LibSQLStorage, UserRecord};
use serde_json::{Value, json};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::{HeaderValue, Request};

/// 确定性 Fake 认知引擎：每个认知步都产出 reasoning + output（用于验证钳制是否剥离细节）。
struct FakeEngine;

#[async_trait]
impl AgentEngine for FakeEngine {
    fn name(&self) -> &str {
        "fake"
    }
    async fn reason(&self, ctx: &Context) -> Result<Decision, AgentError> {
        Ok(Decision {
            intent: format!("intent for {}", ctx.step),
            reasoning: format!("fake reasoning for {}", ctx.step),
            confidence: 0.9,
            object_refs: vec!["001".into()],
            output: json!({ "step": ctx.step }),
        })
    }
}

/// 原始 HTTP 调用（带可选 Bearer 令牌）。
async fn http_call(
    addr: SocketAddr,
    method: &str,
    path: &str,
    body: Option<&str>,
    token: Option<&str>,
) -> (u16, String) {
    let mut stream = TcpStream::connect(addr).await.unwrap();
    let auth_line = token
        .map(|t| format!("Authorization: Bearer {t}\r\n"))
        .unwrap_or_default();
    let req = match body {
        Some(b) => format!(
            "{method} {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Type: application/json\r\n{auth_line}Content-Length: {}\r\n\r\n{b}",
            b.len()
        ),
        None => format!(
            "{method} {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n{auth_line}\r\n"
        ),
    };
    stream.write_all(req.as_bytes()).await.unwrap();
    let mut buf = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(5), stream.read_to_end(&mut buf)).await;
    let text = String::from_utf8_lossy(&buf);
    let (head, rest) = text.split_once("\r\n\r\n").unwrap_or(("", ""));
    let status: u16 = head
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let cl = head.lines().find_map(|l| {
        l.to_ascii_lowercase()
            .strip_prefix("content-length: ")
            .and_then(|v| v.trim().parse::<usize>().ok())
    });
    let body = match cl {
        Some(n) => rest.get(..n).unwrap_or(rest).to_string(),
        None => rest.to_string(),
    };
    (status, body)
}

/// 登录并返回 (status, token)。
async fn login(addr: SocketAddr, username: &str, password: &str) -> (u16, Option<String>) {
    let body = json!({ "username": username, "password": password }).to_string();
    let (st, resp) = http_call(addr, "POST", "/api/v1/wb/auth/login", Some(&body), None).await;
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

fn retain_registry() -> openaip_os::action::ActionRegistry {
    openaip_os::action::ActionRegistry::from_ir(
        &openaip_os::parse_odl(
            r#"
object Customer:
  properties:
    name: string
    level: string
  actions:
    retain:
      creates:
        - type: RetentionTask
          data:
            title: "客户挽留计划"
            customer: self
          link: needs_retention
"#,
        )
        .expect("retain ODL"),
    )
}

#[tokio::test]
async fn tc_ws_developer_full_detail() {
    let (addr, storage) = spawn_ws_server_raw().await;
    let kernel = Arc::new(RuntimeKernel::new(storage.clone()));
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
    let kernel = Arc::new(RuntimeKernel::new(storage.clone()));
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
    let kernel = Arc::new(RuntimeKernel::new(storage.clone()));
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
    let kernel = Arc::new(RuntimeKernel::new(storage.clone()));
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
    let kernel = Arc::new(RuntimeKernel::new(storage.clone()));
    let engine: Arc<dyn AgentEngine> = Arc::new(FakeEngine);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = server::serve_with_timeout(
            kernel,
            engine,
            listener,
            "ui/dist",
            retain_registry(),
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

/// 种子客户并跑通 Planner 闭环（事件落库，供 WS 历史回放）。
async fn seed_and_run(kernel: &Arc<RuntimeKernel>) {
    use openaip_os::planner::Planner;
    kernel
        .create_object("Customer", "001", json!({"name": "Apple", "level": "VIP"}))
        .await
        .unwrap();
    let run_id = generate_run_id();
    Planner::with_actions(kernel.as_ref(), retain_registry())
        .run_goal(&FakeEngine, "reduce churn", "retention-agent", &run_id)
        .await
        .expect("planner 应跑通");
}

/// 取最近一次运行的 run_id（按 trace 事件反查）。
async fn last_run_id(kernel: &Arc<RuntimeKernel>) -> String {
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
