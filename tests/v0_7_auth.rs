//! V0.7 认证与 RBAC 测试契约（对应 design-auth.md §3.9 TC-auth-a~g）。
//!
//! 验证「服务端强制认证 + 角色能力门禁 + 视图钳制」：无令牌/错令牌 401，越权角色 403，
//! 前端 `?view=` 被钳制到角色上限，登录/登出/禁用生命周期正确。

mod common;
use common::*;

use openaip_os::auth::{IdentityProvider, LocalIdentityProvider};
use openaip_os::runtime::RuntimeKernel;
use openaip_os::server;
use openaip_os::storage::{LibSQLStorage, ObjectEngine, UserRecord};
use serde_json::{Value, json};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;

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
    let services = retain_services();
    let defs = retain_defs();
    let engine: Arc<dyn openaip_os::cognitive::CognitiveEngine> =
        Arc::new(openaip_os::cognitive::NoOpEngine);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = server::serve_with_timeout(
            kernel, services, defs, engine, listener, "ui/dist", false, false,
            true, // enforce_auth
            None,
        )
        .await;
    });
    tokio::time::sleep(Duration::from_millis(150)).await;
    (addr, storage)
}

// ── TC-auth-a：无令牌访问受保护端点 → 401 ───────────────────────────────

#[tokio::test]
async fn tc_auth_a_no_token_401() {
    let (addr, _s) = spawn_auth_server().await;
    let (st, _) = http_call_auth(
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
    let (st, _) = http_call_auth(
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
    let (st, _) = http_call_auth(
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
    let (st, body) = http_call_auth(
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
    let (st_me, body) = http_call_auth(addr, "GET", "/api/v1/wb/auth/me", None, Some(&token)).await;
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
    let (st_lo, _) =
        http_call_auth(addr, "POST", "/api/v1/wb/auth/logout", None, Some(&token)).await;
    assert_eq!(st_lo, 200);
    let (st_reuse, _) = http_call_auth(addr, "GET", "/api/v1/wb/auth/me", None, Some(&token)).await;
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
    let (st_reuse, _) = http_call_auth(addr, "GET", "/api/v1/wb/auth/me", None, Some(&token)).await;
    assert_eq!(st_reuse, 401);
}

// ── TC-auth-h：/graph 按角色钳制（Executive 剥 data，Developer 全量） ─────
// 防止 /graph 成为 V0.7 视图钳制的旁路：Executive 是最低权限角色，不得经图端点读到对象全量字段。
#[tokio::test]
async fn tc_auth_h_graph_clamped_to_role() {
    let (addr, storage) = spawn_auth_server().await;
    // 造一个带敏感 data 的对象
    storage
        .create(
            "Customer",
            "001",
            json!({"name": "Apple", "balance": 99999.0}),
        )
        .await
        .unwrap();

    // Executive：200，但节点不含 data
    let (_, exec_token) = login(addr, "exec", "execpw").await;
    let exec_token = exec_token.expect("exec 应登录成功");
    let (st, body) = http_call_auth(addr, "GET", "/api/v1/wb/graph", None, Some(&exec_token)).await;
    assert_eq!(st, 200);
    let v: Value = serde_json::from_str(&body).unwrap();
    let node = &v["data"]["nodes"][0];
    assert_eq!(node["id"], "001");
    assert_eq!(node["type"], "Customer");
    assert!(
        node.get("data").is_none(),
        "Executive 不得经 /graph 读到对象 data"
    );

    // Developer（admin）：节点含全量 data
    let (_, admin_token) = login(addr, "admin", "adminpw").await;
    let admin_token = admin_token.expect("admin 应登录成功");
    let (st2, body2) =
        http_call_auth(addr, "GET", "/api/v1/wb/graph", None, Some(&admin_token)).await;
    assert_eq!(st2, 200);
    let v2: Value = serde_json::from_str(&body2).unwrap();
    assert_eq!(v2["data"]["nodes"][0]["data"]["balance"], json!(99999.0));
}

// ── TC-auth-i：读接口钳制 — Executive 经 /trace 列表看不到事件（M2） ──────
// list_trace/get_trace 与 timeline/WS 同一套 from_view 钳制，不得成为三层视图旁路。
#[tokio::test]
async fn tc_auth_i_trace_list_clamped_to_role() {
    let (addr, storage) = spawn_auth_server().await;
    // 经同一存储句柄造一条带推理细节的认知事件（server 与测试共享连接）
    let kernel = Arc::new(RuntimeKernel::new(storage.clone()));
    kernel
        .emit_cognitive(
            "Reasoning",
            "run_1",
            "retention-agent",
            &["001".to_string()],
            Some("内部推理：高流失风险"),
            Some(serde_json::json!({"candidates": ["001"]})),
            Some(serde_json::json!({"risk": "high"})),
            Some(0.4),
        )
        .await
        .unwrap();

    // Developer：能看到事件且带细节
    let (_, admin_token) = login(addr, "admin", "adminpw").await;
    let admin_token = admin_token.expect("admin 应登录成功");
    let (st, body) = http_call_auth(
        addr,
        "GET",
        "/api/v1/wb/agent/trace",
        None,
        Some(&admin_token),
    )
    .await;
    assert_eq!(st, 200);
    let v: Value = serde_json::from_str(&body).unwrap();
    let list = v["data"]["list"].as_array().unwrap();
    assert_eq!(list.len(), 1, "Developer 应能看到事件");
    assert!(
        list[0]["reasoning"].is_string(),
        "Developer 视图应保留 reasoning 细节"
    );

    // Executive：非 Goal/Action 事件被钳制过滤（列表为空）
    let (_, exec_token) = login(addr, "exec", "execpw").await;
    let exec_token = exec_token.expect("exec 应登录成功");
    let (st2, body2) = http_call_auth(
        addr,
        "GET",
        "/api/v1/wb/agent/trace",
        None,
        Some(&exec_token),
    )
    .await;
    assert_eq!(st2, 200);
    let v2: Value = serde_json::from_str(&body2).unwrap();
    assert!(
        v2["data"]["list"].as_array().unwrap().is_empty(),
        "Executive 不得经 /trace 读到事件细节（M2 钳制）"
    );
}

// ── TC-auth-j：登录限流 — 连续失败 5 次后锁定 429（M3） ─────────────────
#[tokio::test]
async fn tc_auth_j_login_rate_limited() {
    let (addr, _s) = spawn_auth_server().await;
    for _ in 0..5 {
        let (st, _) = login(addr, "admin", "wrongpw").await;
        assert_eq!(st, 401, "前 5 次应为普通认证失败");
    }
    // 锁定窗口内：错误口令与正确口令一律 429
    let (st6, _) = login(addr, "admin", "wrongpw").await;
    assert_eq!(st6, 429, "连续失败 5 次后应锁定（429）");
    let (st7, _) = login(addr, "admin", "adminpw").await;
    assert_eq!(st7, 429, "锁定窗口内正确口令也不放行");
}

// ── TC-auth-k：会话令牌哈希存储 — 库中无明文令牌（M4） ─────────────────
#[tokio::test]
async fn tc_auth_k_session_token_hashed_at_rest() {
    let (addr, storage) = spawn_auth_server().await;
    let (_, token) = login(addr, "admin", "adminpw").await;
    let token = token.expect("admin 应登录成功");
    // 明文令牌不可直接查库（wb_session 存 SHA-256 哈希而非明文）
    let row = storage.get_session(&token).await.unwrap();
    assert!(row.is_none(), "库中不得存明文令牌（M4 哈希存储）");
    // 令牌本身仍可正常鉴权（内部哈希比对）
    let (st, _) = http_call_auth(addr, "GET", "/api/v1/wb/auth/me", None, Some(&token)).await;
    assert_eq!(st, 200, "哈希存储不应破坏令牌校验");
}
