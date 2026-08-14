//! V0.7 运行态持久化集成测试（TC-persist-a~d）。
//!
//! 覆盖 design-persistence.md 测试矩阵：
//! - a 触发 run → wb_run 先 Running、完成 Completed
//! - b 重启后查 run → runs 内存重建，状态与 DB 一致
//! - c 崩溃时 Running 的 run → 重启标记 Interrupted
//! - d 待审批 Pending 跨重启 → orphan 翻 Expired（简化恢复分支）
//! - e 对象+事件同事务（with_tx）→ 独立 PR，本增量不做，见 design-persistence.md §3.5 / P4

mod common;

use common::FakeEngine;
use openaip_os::action::ActionRegistry;
use openaip_os::agent::AgentEngine;
use openaip_os::planner::generate_run_id;
use openaip_os::runtime::RuntimeKernel;
use openaip_os::server;
use openaip_os::storage::LibSQLStorage;
use serde_json::Value;
use serde_json::json;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// 与 v0_3 同源的可用引擎：每步固定返回决策，跑通声明式挽留闭环（无审批闸门）。
/// 启动一个服务（file db，支持跨重启持久化验证）。复用调用方 kernel 的同一存储连接。
async fn spawn_server(kernel: Arc<RuntimeKernel>) -> SocketAddr {
    let engine: Arc<dyn AgentEngine> = Arc::new(FakeEngine);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let ks = kernel.clone();
    tokio::spawn(async move {
        let _ = server::serve(
            ks,
            engine,
            listener,
            "ui/dist",
            retain_registry(),
            false,
            false,
            false,
        )
        .await;
    });
    tokio::time::sleep(Duration::from_millis(150)).await;
    addr
}

/// 简易 HTTP 调用（utf8 文本，Content-Length 裁剪），与 v0_3 同款。
async fn http_call(
    addr: SocketAddr,
    method: &str,
    path: &str,
    body: Option<&str>,
) -> (u16, String) {
    let mut stream = TcpStream::connect(addr).await.unwrap();
    let req = match body {
        Some(b) => format!(
            "{method} {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{b}",
            b.len()
        ),
        None => format!("{method} {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"),
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

/// 轮询 wb_run 直到终态（Completed/Failed/Interrupted）或超时。
async fn wait_run_done(kernel: &RuntimeKernel, run_id: &str) -> openaip_os::storage::RunRecord {
    for _ in 0..120 {
        if let Ok(Some(r)) = kernel.storage().get_run(run_id).await
            && r.status != "Running"
        {
            return r;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("run {run_id} 未在超时内进入终态");
}

/// 唯一临时 file db 路径。
fn temp_db() -> String {
    let dir = std::env::temp_dir();
    let name = format!(
        "oaip_persist_{}_{}.db",
        std::process::id(),
        generate_run_id()
    );
    dir.join(name).to_string_lossy().to_string()
}

// ── TC-persist-a：run 生命周期持久化（先 Running，完成 Completed） ──────────────────────

fn retain_registry() -> ActionRegistry {
    use openaip_os::parse_odl;
    ActionRegistry::from_ir(
        &parse_odl(
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
async fn tc_persist_a_run_lifecycle_persisted() {
    let path = temp_db();
    let _ = std::fs::remove_file(&path);
    let kernel = Arc::new(RuntimeKernel::new(
        LibSQLStorage::open(&path).await.unwrap(),
    ));
    // 种子一个客户，使声明式挽留闭环有候选
    kernel
        .create_object("Customer", "001", json!({"name": "Apple"}))
        .await
        .unwrap();
    let addr = spawn_server(kernel.clone()).await;

    let (st, body) = http_call(
        addr,
        "POST",
        "/api/v1/wb/agent/run",
        Some(r#"{"agentId":"retention-agent","goal":"reduce churn"}"#),
    )
    .await;
    assert_eq!(st, 200);
    let run_id = body.parse::<Value>().unwrap()["data"]["runId"]
        .as_str()
        .unwrap()
        .to_string();

    // wb_run 行应已存在（触发即 insert_run，落 Running）
    let early = kernel.storage().get_run(&run_id).await.unwrap();
    assert!(early.is_some(), "wb_run 应在触发时即插入（持久化生效）");

    // 完成时状态翻 Completed（DB 真相源）
    let rec = wait_run_done(&kernel, &run_id).await;
    assert_eq!(rec.status, "Completed");
    let _ = std::fs::remove_file(&path);
}

// ── TC-persist-b：重启后查 run → runs 内存重建，状态与 DB 一致 ──────────────────────────
#[tokio::test]
async fn tc_persist_b_restart_rebuilds_run_map() {
    let path = temp_db();
    let _ = std::fs::remove_file(&path);
    // 首次启动：触发并跑完一个 run
    let kernel1 = Arc::new(RuntimeKernel::new(
        LibSQLStorage::open(&path).await.unwrap(),
    ));
    kernel1
        .create_object("Customer", "001", json!({"name": "Apple"}))
        .await
        .unwrap();
    let addr1 = spawn_server(kernel1.clone()).await;
    let (_, body) = http_call(
        addr1,
        "POST",
        "/api/v1/wb/agent/run",
        Some(r#"{"agentId":"retention-agent","goal":"reduce churn"}"#),
    )
    .await;
    let run_id = body.parse::<Value>().unwrap()["data"]["runId"]
        .as_str()
        .unwrap()
        .to_string();
    let _ = wait_run_done(&kernel1, &run_id).await;

    // 模拟重启：同一 file db 开新内核 + 新服务（recover 重建 runs 映射）
    let kernel2 = Arc::new(RuntimeKernel::new(
        LibSQLStorage::open(&path).await.unwrap(),
    ));
    let addr2 = spawn_server(kernel2.clone()).await;
    // 新服务内存 runs 初始为空 → timeline 经 DB 回退得到 Completed
    let (st, body) = http_call(
        addr2,
        "GET",
        &format!("/api/v1/wb/agent/run/{run_id}/timeline?view=developer"),
        None,
    )
    .await;
    assert_eq!(st, 200);
    let tl = body.parse::<Value>().unwrap()["data"].clone();
    assert_eq!(
        tl["status"], "Completed",
        "重启后 timeline 应经 DB 回退得到 Completed"
    );
    // DB 真相源一致
    assert_eq!(
        kernel2
            .storage()
            .get_run(&run_id)
            .await
            .unwrap()
            .unwrap()
            .status,
        "Completed"
    );
    let _ = std::fs::remove_file(&path);
}

// ── TC-persist-c：崩溃时 Running 的 run → 重启标记 Interrupted ─────────────────────────
#[tokio::test]
async fn tc_persist_c_crash_running_to_interrupted() {
    let path = temp_db();
    let _ = std::fs::remove_file(&path);
    let kernel = Arc::new(RuntimeKernel::new(
        LibSQLStorage::open(&path).await.unwrap(),
    ));
    // 崩溃残留：直接以 Running 落一条 wb_run（无活执行器）
    kernel
        .storage()
        .insert_run("run_crash", "retention-agent", "restore me")
        .await
        .unwrap();
    assert_eq!(
        kernel
            .storage()
            .get_run("run_crash")
            .await
            .unwrap()
            .unwrap()
            .status,
        "Running"
    );
    // 启动服务触发恢复（Running → Interrupted）
    spawn_server(kernel.clone()).await;
    let rec = kernel
        .storage()
        .get_run("run_crash")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        rec.status, "Interrupted",
        "崩溃残留的 Running run 重启应标记 Interrupted"
    );
    let _ = std::fs::remove_file(&path);
}

// ── TC-persist-d：待审批 Pending 跨重启 → orphan 翻 Expired ────────────────────────────
#[tokio::test]
async fn tc_persist_d_orphan_pending_to_expired() {
    let path = temp_db();
    let _ = std::fs::remove_file(&path);
    let kernel = Arc::new(RuntimeKernel::new(
        LibSQLStorage::open(&path).await.unwrap(),
    ));
    // 崩溃残留：一条孤立 Pending 审批（无活 run）
    kernel
        .storage()
        .insert_approval(
            "apr_orphan",
            "run_x",
            "retention-agent",
            "action@t",
            "需审批",
        )
        .await
        .unwrap();
    assert_eq!(
        kernel
            .storage()
            .list_approvals_by_state("Pending")
            .await
            .unwrap()
            .len(),
        1
    );
    // 启动服务触发恢复（orphan Pending → Expired）
    spawn_server(kernel.clone()).await;
    let expired = kernel
        .storage()
        .list_approvals_by_state("Expired")
        .await
        .unwrap();
    assert_eq!(expired.len(), 1, "orphan Pending 重启应翻 Expired");
    assert_eq!(expired[0].id, "apr_orphan");
    assert_eq!(expired[0].state, "Expired");
    let _ = std::fs::remove_file(&path);
}
