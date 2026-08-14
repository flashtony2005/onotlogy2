//! V0.7 认证与 RBAC 测试契约（对应 design-auth.md §3.9 TC-auth-a~g）。
//!
//! 验证「服务端强制认证 + 角色能力门禁 + 视图钳制」：无令牌/错令牌 401，越权角色 403，
//! 前端 `?view=` 被钳制到角色上限，登录/登出/禁用生命周期正确。

use async_trait::async_trait;
use openaip_os::agent::{AgentEngine, AgentError, Context, Decision};
use openaip_os::auth::{IdentityProvider, LocalIdentityProvider};
use openaip_os::runtime::RuntimeKernel;
use openaip_os::server;
use openaip_os::storage::{LibSQLStorage, UserRecord};
use serde_json::{Value, json};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// 测试用空引擎（认证测试不触发真实 Planner；401/403 在引擎前返回）。
struct NoopEngine;

#[async_trait]
impl AgentEngine for NoopEngine {
    fn name(&self) -> &str {
        "noop"
    }
    async fn reason(&self, _ctx: &Context) -> Result<Decision, AgentError> {
        Err(AgentError::Unavailable("test noop".into()))
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

/// 启动强制认证的服务（内存库 + admin(Developer) + exec(Executive)）。
/// 返回地址与共享存储句柄（供测试直接造/禁用账号）。
async fn spawn_auth_server() -> (SocketAddr, LibSQLStorage) {
    let storage = LibSQLStorage::open(":memory:").await.unwrap();
    let idp = LocalIdentityProvider::new(storage.clone());
    idp.bootstrap_admin("admin", "adminpw").await.unwrap();
    // Executive 账号（越权测试）
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
    let engine: Arc<dyn AgentEngine> = Arc::new(NoopEngine);
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

// ── TC-auth-a：无令牌访问受保护端点 → 401 ───────────────────────────────

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
async fn tc_auth_a_no_token_401() {
    let (addr, _s) = spawn_auth_server().await;
    let (st, _) = http_call(
        addr,
        "POST",
        "/api/v1/wb/agent/run",
        Some(r#"{"agentId":"a","goal":"g"}"#),
        None,
    )
    .await;
    assert_eq!(st, 401);
}

// ── TC-auth-b：错误令牌 → 401 ───────────────────────────────────────────
#[tokio::test]
async fn tc_auth_b_wrong_token_401() {
    let (addr, _s) = spawn_auth_server().await;
    let (st, _) = http_call(
        addr,
        "POST",
        "/api/v1/wb/agent/run",
        Some(r#"{"agentId":"a","goal":"g"}"#),
        Some("not.a.real.token"),
    )
    .await;
    assert_eq!(st, 401);
}

// ── TC-auth-c：Executive 触发运行 → 403（需 Operator 及以上） ────────────
#[tokio::test]
async fn tc_auth_c_executive_run_forbidden() {
    let (addr, _s) = spawn_auth_server().await;
    let (_, token) = login(addr, "exec", "execpw").await;
    let token = token.expect("exec 应登录成功");
    let (st, _) = http_call(
        addr,
        "POST",
        "/api/v1/wb/agent/run",
        Some(r#"{"agentId":"a","goal":"g"}"#),
        Some(&token),
    )
    .await;
    assert_eq!(st, 403);
}

// ── TC-auth-d：Executive 请求 ?view=developer 被服务端钳制为 executive ──
#[tokio::test]
async fn tc_auth_d_view_clamped_to_role() {
    let (addr, _s) = spawn_auth_server().await;
    let (_, token) = login(addr, "exec", "execpw").await;
    let token = token.expect("exec 应登录成功");
    let (st, body) = http_call(
        addr,
        "GET",
        "/api/v1/wb/agent/run/anyrun/timeline?view=developer",
        None,
        Some(&token),
    )
    .await;
    assert_eq!(st, 200);
    let v: Value = serde_json::from_str(&body).unwrap();
    // 有效视图被钳制到 Executive（不信任前端 view 参数）
    assert_eq!(v["data"]["view"], "executive");
    // Executive 视图不含 reasoning/input/output 细节
    let events = v["data"]["events"].as_array().unwrap();
    assert!(events.iter().all(|e| e.get("reasoning").is_none()
        && e.get("input").is_none()
        && e.get("output").is_none()));
}

// ── TC-auth-e：正确登录 → 200 + token + role，令牌可访问受保护端点 ──────
#[tokio::test]
async fn tc_auth_e_login_ok_and_token_works() {
    let (addr, _s) = spawn_auth_server().await;
    let (st, token) = login(addr, "admin", "adminpw").await;
    assert_eq!(st, 200);
    let token = token.expect("admin 应拿到令牌");
    let (st_me, body) = http_call(addr, "GET", "/api/v1/wb/auth/me", None, Some(&token)).await;
    assert_eq!(st_me, 200);
    let v: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["data"]["role"], "developer");
}

// ── TC-auth-f：登出后复用旧令牌 → 401 ───────────────────────────────────
#[tokio::test]
async fn tc_auth_f_logout_invalidates_token() {
    let (addr, _s) = spawn_auth_server().await;
    let (_, token) = login(addr, "admin", "adminpw").await;
    let token = token.expect("admin 应拿到令牌");
    let (st_lo, _) = http_call(addr, "POST", "/api/v1/wb/auth/logout", None, Some(&token)).await;
    assert_eq!(st_lo, 200);
    let (st_reuse, _) = http_call(addr, "GET", "/api/v1/wb/auth/me", None, Some(&token)).await;
    assert_eq!(st_reuse, 401);
}

// ── TC-auth-g：用户被禁用后其令牌 → 401（级联清会话） ───────────────────
#[tokio::test]
async fn tc_auth_g_disabled_user_token_401() {
    let (addr, storage) = spawn_auth_server().await;
    let hash = LocalIdentityProvider::hash_password("dupw").unwrap();
    storage
        .create_user(&UserRecord {
            id: "user.du".into(),
            username: "du".into(),
            password_hash: hash,
            role: "developer".into(),
            display_name: "DU".into(),
            disabled: false,
        })
        .await
        .unwrap();
    let (st_login, token) = login(addr, "du", "dupw").await;
    assert_eq!(st_login, 200);
    let token = token.expect("du 应拿到令牌");
    // 禁用账号（级联清其会话）
    let u = storage.get_user_by_username("du").await.unwrap().unwrap();
    storage.set_user_disabled(&u.id, true).await.unwrap();
    // 复用旧令牌 → 401
    let (st_reuse, _) = http_call(addr, "GET", "/api/v1/wb/auth/me", None, Some(&token)).await;
    assert_eq!(st_reuse, 401);
}
