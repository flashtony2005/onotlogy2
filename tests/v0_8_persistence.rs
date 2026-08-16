//! V0.7 运行态持久化集成测试（TC-persist-a~d）。
//!
//! 覆盖 design-persistence.md 测试矩阵：
//! - a 触发 run → wb_run 先 Running、完成 Completed
//! - b 重启后查 run → runs 内存重建，状态与 DB 一致
//! - c 崩溃时 Running 的 run → 重启标记 Interrupted
//! - d 待审批 Pending 跨重启 → orphan 翻 Expired（简化恢复分支）
//! - e 对象+事件同事务（with_tx）→ 独立 PR，本增量不做，见 design-persistence.md §3.5 / P4

mod common;

use common::*;
use openaip_os::session::run::generate_run_id;
use openaip_os::storage::{LibSQLStorage, ObjectEngine};
use serde_json::Value;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;

/// 与 v0_3 同源的可用引擎：每步固定返回决策，跑通声明式挽留闭环（无审批闸门）。
/// 启动一个服务（file db，支持跨重启持久化验证）。复用调用方 kernel 的同一存储连接。
/// 简易 HTTP 调用（utf8 文本，Content-Length 裁剪），与 v0_3 同款。
/// 轮询 wb_run 直到终态（Completed/Failed/Interrupted）或超时。
async fn wait_run_done(kernel: &LibSQLStorage, run_id: &str) -> openaip_os::storage::RunRecord {
    for _ in 0..120 {
        if let Ok(Some(r)) = kernel.get_run(run_id).await
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

#[tokio::test]
async fn tc_persist_a_run_lifecycle_persisted() {
    let path = temp_db();
    let _ = std::fs::remove_file(&path);
    let kernel = Arc::new(LibSQLStorage::open(&path).await.unwrap());
    // 种子一个客户，使声明式挽留闭环有候选
    kernel
        .create("Customer", "001", json!({"name": "Apple"}))
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
    let early = kernel.get_run(&run_id).await.unwrap();
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
    let kernel1 = Arc::new(LibSQLStorage::open(&path).await.unwrap());
    kernel1
        .create("Customer", "001", json!({"name": "Apple"}))
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
    let kernel2 = Arc::new(LibSQLStorage::open(&path).await.unwrap());
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
        kernel2.get_run(&run_id).await.unwrap().unwrap().status,
        "Completed"
    );
    let _ = std::fs::remove_file(&path);
}

// ── TC-persist-c：崩溃时 Running 的 run → 重启标记 Interrupted ─────────────────────────
#[tokio::test]
async fn tc_persist_c_crash_running_to_interrupted() {
    let path = temp_db();
    let _ = std::fs::remove_file(&path);
    let kernel = Arc::new(LibSQLStorage::open(&path).await.unwrap());
    // 崩溃残留：直接以 Running 落一条 wb_run（无活执行器）
    kernel
        .insert_run("run_crash", "retention-agent", "restore me")
        .await
        .unwrap();
    assert_eq!(
        kernel.get_run("run_crash").await.unwrap().unwrap().status,
        "Running"
    );
    // 启动服务触发恢复（Running → Interrupted）
    spawn_server(kernel.clone()).await;
    let rec = kernel.get_run("run_crash").await.unwrap().unwrap();
    assert_eq!(
        rec.status, "Interrupted",
        "崩溃残留的 Running run 重启应标记 Interrupted"
    );
    let _ = std::fs::remove_file(&path);
}

// ── TC-persist-d：M1d 删除——orphan 审批恢复依赖 wb_approval 表（已随 V1 门面退役）；
//    超时/Expired 语义由 v2_parity g2c + v0_3_approval tc023c/d 覆盖（Waiting 注册表路径）。
