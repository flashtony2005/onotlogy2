//! V2.0 Universal Worker & Plugin Runtime（Phase 10 · V10.1）契约测试。
//!
//! TDD：先写测试（红），再实现（绿）。
//!
//! W1 WorkerManifest 序列化 roundtrip
//! W2 WorkerRegistry：注册/重复拒绝/candidates
//! W3 WorkerRouter：候选 → Policy Filter → Score → Selected
//! W4 HTTP Worker：POST 真实调用（axum mock）
//! W5 MCP Worker：JSON-RPC tools/call
//! R4 Process Worker（Generic CLI 真实进程）
//! R5 Codex Worker Adapter 壳
//! R7 Generic CLI（git 真实执行）
//! R8 Policy / Approval（allow / require_approval / deny fail-closed）
//! R9 Session Event Log（worker.* 全事件链）
//! R10 Failure / Retry（失败捕获 + 重试一次）

use axum::routing::post;
use axum::{Json, Router as AxumRouter};
use serde_json::{Value, json};
use tokio::net::TcpListener;

use openaip_os::session::event::{EventType, SessionId};
use openaip_os::session::log::SessionLog;
use openaip_os::worker::exec::WorkerRuntime;
use openaip_os::worker::manifest::{ExecutionProtocol, WorkerManifest};
use openaip_os::worker::policy::WorkerPolicy;
use openaip_os::worker::registry::{WorkerRegistry, WorkerStatus};
use openaip_os::worker::router::{RouterCriteria, SelectedWorker, WorkerRouter};
use openaip_os::worker::sdk::{Worker, WorkerError, WorkerResult};

/// echo CLI Worker（Generic Process Worker）。
fn echo_worker(id: &str, caps: &[&str]) -> openaip_os::worker::process::ProcessWorker {
    let mut m = WorkerManifest::new(id, "cli").with_capabilities(caps);
    m.execution = ExecutionProtocol::Process;
    openaip_os::worker::process::ProcessWorker::new(m, "echo")
}

// ── W1：WorkerManifest 序列化 roundtrip ───────────────────────────────
#[tokio::test]
async fn tc_w1_manifest_roundtrip() {
    let m = WorkerManifest::new("codex", "agent")
        .with_capabilities(&["code.read", "code.write", "code.test"])
        .with_quality(0.95);
    let s = serde_json::to_string(&m).unwrap();
    let back: WorkerManifest = serde_json::from_str(&s).unwrap();
    assert_eq!(m.id, back.id);
    assert_eq!(m.capabilities.len(), 3);
    assert_eq!(m.execution, back.execution);
}

// ── W2：WorkerRegistry ─────────────────────────────────────────────────
#[tokio::test]
async fn tc_w2_registry() {
    let mut reg = WorkerRegistry::new();
    reg.register(Box::new(echo_worker("echo", &["text.echo"])))
        .unwrap();
    // 重复注册拒绝
    assert!(
        reg.register(Box::new(echo_worker("echo", &["text.echo"])))
            .is_err()
    );
    // candidates：按 capability 发现
    let cands = reg.candidates("text.echo");
    assert_eq!(cands.len(), 1);
    assert!(reg.candidates("missing.cap").is_empty());
    // 状态
    assert_eq!(reg.status("echo"), Some(WorkerStatus::Registered));
    assert_eq!(reg.status("nope"), None);
}

// ── W3：WorkerRouter（候选 → Policy Filter → Score → Selected）─────────
#[tokio::test]
async fn tc_w3_router_selection() {
    let mut reg = WorkerRegistry::new();
    // 两个 code.write 候选：质量 0.9（cost 5）与 0.95（cost 10）
    let mut w1 = echo_worker("worker-a", &["code.write"]);
    w1.manifest_mut().quality = 0.9;
    w1.manifest_mut().cost = 5;
    reg.register(Box::new(w1)).unwrap();
    let mut w2 = echo_worker("worker-b", &["code.write"]);
    w2.manifest_mut().quality = 0.95;
    w2.manifest_mut().cost = 10;
    reg.register(Box::new(w2)).unwrap();

    let router = WorkerRouter::new(RouterCriteria::default());
    let policy = WorkerPolicy::new().allow("code.write");
    let sel: SelectedWorker = router.route(&reg, "code.write", &policy).expect("路由成功");
    assert_eq!(sel.worker_id, "worker-b", "质量优先");
    assert!(sel.reason.contains("quality"), "路由理由可解释");

    // 显式指定
    let router2 = WorkerRouter::new(RouterCriteria {
        explicit: Some("worker-a".into()),
        ..Default::default()
    });
    assert_eq!(
        router2
            .route(&reg, "code.write", &policy)
            .unwrap()
            .worker_id,
        "worker-a"
    );

    // Policy 过滤：deny → 无候选
    let deny = WorkerPolicy::new().deny("code.write");
    assert!(router.route(&reg, "code.write", &deny).is_err());
}

// ── R4：Process Worker（Generic CLI 真实进程执行）──────────────────────
#[tokio::test]
async fn tc_r4_process_worker_real_execution() {
    let mut reg = WorkerRegistry::new();
    reg.register(Box::new(echo_worker("echo", &["text.echo"])))
        .unwrap();
    let w = reg.get("echo").unwrap();
    let result: WorkerResult = w
        .execute("text.echo", json!({ "args": ["hello worker"] }))
        .await
        .expect("真实进程执行");
    assert!(result.ok);
    assert_eq!(result.exit_code, Some(0));
    let stdout = result.output["stdout"].as_str().unwrap_or("");
    assert!(stdout.contains("hello worker"), "stdout 应含输入: {stdout}");
}

// ── R5：Codex Worker Adapter 壳 ────────────────────────────────────────
#[tokio::test]
async fn tc_r5_codex_adapter_shell() {
    let codex = openaip_os::worker::adapters::codex_worker(Some("echo"));
    let m = codex.manifest().clone();
    assert!(
        m.capabilities.contains(&"code.read".to_string()),
        "code.read 能力"
    );
    assert!(
        m.capabilities.contains(&"code.test".to_string()),
        "code.test 能力"
    );
    // 壳可真实执行（命令替换为 echo——适配器不绑定具体二进制）
    let result = codex
        .execute("code.read", json!({ "file": "a.rs" }))
        .await
        .expect("适配器执行");
    assert!(result.ok);
}

// ── R7：Generic CLI（git 真实执行）────────────────────────────────────
#[tokio::test]
async fn tc_r7_generic_cli_git() {
    let mut m = WorkerManifest::new("git", "cli").with_capabilities(&["git.version"]);
    m.execution = ExecutionProtocol::Process;
    let git = openaip_os::worker::process::ProcessWorker::new(m, "git");
    let result = git
        .execute("git.version", json!({ "args": ["--version"] }))
        .await
        .expect("git 真实执行");
    assert!(result.ok, "git --version 应成功");
    assert!(
        result.output["stdout"]
            .as_str()
            .unwrap_or("")
            .contains("git"),
        "输出含 git 版本"
    );
}

// ── R8：Policy / Approval（fail-closed）────────────────────────────────
#[tokio::test]
async fn tc_r8_policy_approval() {
    let mut reg = WorkerRegistry::new();
    reg.register(Box::new(echo_worker("echo", &["code.read", "db.delete"])))
        .unwrap();

    // allow → 执行成功
    let policy = WorkerPolicy::new().allow("code.read");
    let mut log = SessionLog::new(SessionId::new("s8"));
    let mut rt = WorkerRuntime::new(&reg, &policy, WorkerRouter::new(RouterCriteria::default()))
        .with_log(&mut log);
    let r = rt
        .run("code.read", json!({ "text": "ok" }))
        .await
        .expect("allow 执行");
    assert!(r.ok);

    // require_approval → 不执行，明确错误
    let policy2 = WorkerPolicy::new().require_approval("db.delete");
    let mut rt2 = WorkerRuntime::new(&reg, &policy2, WorkerRouter::new(RouterCriteria::default()))
        .with_log(&mut log);
    let err = rt2.run("db.delete", json!({})).await.unwrap_err();
    assert!(
        matches!(err, WorkerError::RequireApproval(_)),
        "require_approval → 等待审批: {err}"
    );
    assert!(
        log.events()
            .iter()
            .any(|e| e.event_type == EventType::ApprovalRequested),
        "审批事件已记录"
    );

    // deny → 拒绝且不执行
    let policy3 = WorkerPolicy::new().deny("db.delete");
    let mut rt3 = WorkerRuntime::new(&reg, &policy3, WorkerRouter::new(RouterCriteria::default()));
    assert!(matches!(
        rt3.run("db.delete", json!({})).await.unwrap_err(),
        WorkerError::Denied(_)
    ));

    // 未配置 → Deny（Fail Closed，不是 TRY）
    let policy4 = WorkerPolicy::new();
    let mut rt4 = WorkerRuntime::new(&reg, &policy4, WorkerRouter::new(RouterCriteria::default()));
    assert!(matches!(
        rt4.run("code.read", json!({})).await.unwrap_err(),
        WorkerError::Denied(_)
    ));
}

// ── R9：Session Event Log（worker.* 全事件链）──────────────────────────
#[tokio::test]
async fn tc_r9_event_log_chain() {
    let mut reg = WorkerRegistry::new();
    reg.register(Box::new(echo_worker("echo", &["text.echo"])))
        .unwrap();
    let policy = WorkerPolicy::new().allow("text.echo");
    let mut log = SessionLog::new(SessionId::new("s9"));
    let mut rt = WorkerRuntime::new(&reg, &policy, WorkerRouter::new(RouterCriteria::default()))
        .with_log(&mut log);
    rt.run("text.echo", json!({ "text": "x" })).await.unwrap();

    let tys: Vec<&str> = log.events().iter().map(|e| e.event_type.as_str()).collect();
    assert!(tys.contains(&"policy.checked"), "policy.checked 事件");
    assert!(tys.contains(&"worker.selected"), "worker.selected 事件");
    assert!(tys.contains(&"worker.started"), "worker.started 事件");
    assert!(tys.contains(&"worker.completed"), "worker.completed 事件");
    // 顺序：selected → started → completed
    let idx_sel = tys.iter().position(|t| *t == "worker.selected").unwrap();
    let idx_start = tys.iter().position(|t| *t == "worker.started").unwrap();
    let idx_done = tys.iter().position(|t| *t == "worker.completed").unwrap();
    assert!(idx_sel < idx_start && idx_start < idx_done, "事件顺序正确");
    // selected 事件带 worker/capability/reason
    let sel = log
        .events()
        .iter()
        .find(|e| e.event_type == EventType::WorkerSelected)
        .unwrap();
    assert_eq!(sel.data["worker"], "echo");
    assert_eq!(sel.data["capability"], "text.echo");
}

// ── R10：Failure / Retry（失败捕获 + 重试一次）─────────────────────────
#[tokio::test]
async fn tc_r10_failure_and_retry() {
    // 一个会失败的 worker：命令不存在（第一次失败）→ Runtime 重试一次（仍失败 → FailedAfterRetry）
    let mut m = WorkerManifest::new("broken", "cli").with_capabilities(&["x.run"]);
    m.execution = ExecutionProtocol::Process;
    let broken = openaip_os::worker::process::ProcessWorker::new(m, "no-such-cmd-xyz");
    let mut reg = WorkerRegistry::new();
    reg.register(Box::new(broken)).unwrap();
    let policy = WorkerPolicy::new().allow("x.run");
    let mut log = SessionLog::new(SessionId::new("s10"));
    let mut rt = WorkerRuntime::new(&reg, &policy, WorkerRouter::new(RouterCriteria::default()))
        .with_log(&mut log);

    let err = rt.run("x.run", json!({})).await.unwrap_err();
    assert!(
        matches!(err, WorkerError::Execution(_)),
        "失败可捕获: {err}"
    );
    // 失败事件 + 重试（两次尝试均 failed）
    let fails = log
        .events()
        .iter()
        .filter(|e| e.event_type == EventType::WorkerFailed)
        .count();
    assert!(fails >= 2, "失败记录 + 重试记录: {fails}");
    let starts = log
        .events()
        .iter()
        .filter(|e| e.event_type == EventType::WorkerStarted)
        .count();
    assert_eq!(starts, 2, "重试一次（共两次启动）");
}

// ── W4：HTTP Worker（真实 POST 调用）──────────────────────────────────
#[tokio::test]
async fn tc_w4_http_worker_real_post() {
    let app = AxumRouter::new().route(
        "/execute",
        post(|Json(body): Json<Value>| async move {
            let cap = body.get("capability").cloned().unwrap_or(json!(""));
            Json(json!({ "ok": true, "echo_cap": cap }))
        }),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let base = format!("http://{addr}");

    let worker = openaip_os::worker::http::HttpWorker::new(
        WorkerManifest::new("http-remote", "http").with_capabilities(&["remote.run"]),
        &base,
    );
    let result = worker
        .execute("remote.run", json!({ "x": 1 }))
        .await
        .expect("HTTP worker 真实调用");
    assert!(result.ok);
    assert_eq!(result.output["echo_cap"], "remote.run");
}

// ── W5：MCP Worker（JSON-RPC tools/call）──────────────────────────────
#[tokio::test]
async fn tc_w5_mcp_worker_tools_call() {
    let app = AxumRouter::new().route(
        "/mcp",
        post(|Json(req): Json<Value>| async move {
            let id = req.get("id").cloned().unwrap_or(json!(0));
            let method = req.get("method").and_then(|m| m.as_str()).unwrap_or("");
            match method {
                "initialize" => Json(json!({
                    "jsonrpc": "2.0", "id": id,
                    "result": { "protocolVersion": "2024-11-05", "capabilities": { "tools": {} }, "serverInfo": { "name": "mock", "version": "1.0" } }
                })),
                "tools/call" => {
                    let name = req.get("params").and_then(|p| p.get("name")).and_then(|n| n.as_str()).unwrap_or("");
                    Json(json!({
                        "jsonrpc": "2.0", "id": id,
                        "result": { "content": [ { "type": "text", "text": format!("mcp:{name}") } ] }
                    }))
                }
                _ => Json(json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32601, "message": "unknown" } })),
            }
        }),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let worker = openaip_os::worker::mcp::McpWorker::new(
        WorkerManifest::new("mcp-server", "mcp").with_capabilities(&["mcp.tool"]),
        &format!("http://{addr}/mcp"),
    );
    let result = worker
        .execute("mcp.tool", json!({ "a": 1 }))
        .await
        .expect("MCP worker 调用");
    assert!(result.ok);
    assert!(
        result.output["content"][0]["text"]
            .as_str()
            .unwrap_or("")
            .contains("mcp.tool")
    );
}
