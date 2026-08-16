//! V0.5 观察视图端点测试契约（TC-024a + Tasks）
mod common;
use common::*;

use openaip_os::runtime::RuntimeKernel;
use openaip_os::server;
use openaip_os::storage::LibSQLStorage;
use serde_json::{Value, json};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;

// ── TC-024a：server Memory 视图端点 — GET /memory 返回沉淀（hits 累加） ──────────────
#[tokio::test]
async fn tc024a_server_memory_endpoint() {
    let s = LibSQLStorage::open(":memory:").await.unwrap();
    let kernel = std::sync::Arc::new(RuntimeKernel::new(s));
    // 沉淀两条记忆：action.retain 反复复用以累加 hits
    kernel
        .record_memory("action.retain", json!({ "experience": "retain" }))
        .await
        .unwrap();
    kernel
        .record_memory("action.refund", json!({ "experience": "refund" }))
        .await
        .unwrap();
    kernel
        .record_memory("action.retain", json!({ "experience": "retain", "v": 2 }))
        .await
        .unwrap(); // hits: 1→2

    let addr = spawn_server(kernel).await;

    let (st, body) = http_call(addr, "GET", "/api/v1/wb/agent/memory", None).await;
    assert_eq!(st, 200);
    let v = serde_json::from_str::<Value>(&body).unwrap();
    assert_eq!(v["code"], 0, "记忆查询应成功");
    let list = v["data"]["list"].as_array().unwrap();
    assert_eq!(v["data"]["total"].as_i64().unwrap(), 2, "应去重为 2 条");

    // action.retain 命中两次 → hits=2，且按 hits 降序排首位
    assert_eq!(list[0]["label"], "action.retain", "hits 高者排前");
    assert_eq!(list[0]["hits"].as_i64().unwrap(), 2, "复用语义累加 hits");
    assert_eq!(list[1]["label"], "action.refund");
    assert_eq!(list[1]["hits"].as_i64().unwrap(), 1);
    assert!(
        list[0]["payload"].get("experience").is_some(),
        "payload 应回传"
    );
}

#[tokio::test]
async fn tc_server_tasks_endpoint() {
    let s = LibSQLStorage::open(":memory:").await.unwrap();
    let kernel = std::sync::Arc::new(RuntimeKernel::new(s));
    kernel
        .create_object(
            "RetentionTask",
            "001_retain",
            json!({
                "title": "客户挽留计划",
                "customer": "001",
                "status": "Pending",
                "priority": "MEDIUM"
            }),
        )
        .await
        .unwrap();
    let addr = spawn_server(kernel).await;
    let (st, body) = http_call(addr, "GET", "/api/v1/wb/agent/tasks", None).await;
    assert_eq!(st, 200);
    let v = serde_json::from_str::<Value>(&body).unwrap();
    assert_eq!(v["code"], 0);
    assert_eq!(v["data"]["total"].as_i64().unwrap(), 1);
    let item = &v["data"]["list"][0];
    assert_eq!(item["id"], "001_retain");
    assert_eq!(item["status"], "Pending");
    assert_eq!(item["title"], "客户挽留计划");
    assert_eq!(item["customer"], "001");
}

// ── T1：静态托管（serve_static / SPA fallback / Content-Type / API 404） ──
// 覆盖全项目最低点（覆盖率基线 static_files 0%）。

/// 启动带自定义 static_dir 的无认证 server（临时目录，不依赖 ui/dist）。
async fn spawn_server_with_dir(static_dir: &str) -> SocketAddr {
    let s = LibSQLStorage::open(":memory:").await.unwrap();
    let kernel = Arc::new(RuntimeKernel::new(s));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let dir = static_dir.to_string();
    tokio::spawn(async move {
        let _ = server::serve(
            kernel,
            retain_services(),
            retain_defs(),
            std::sync::Arc::new(v2_retain_engine()),
            listener,
            dir,
            false,
            false,
            false,
        )
        .await;
    });
    tokio::time::sleep(Duration::from_millis(150)).await;
    addr
}

#[tokio::test]
async fn tc_static_serves_index_and_assets() {
    // 造临时 static_dir：index.html + app.js + style.css
    let dir = std::env::temp_dir().join(format!("oaip_static_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("index.html"), "<!doctype html><h1>UI</h1>").unwrap();
    std::fs::write(dir.join("app.js"), "console.log(1)").unwrap();
    std::fs::write(dir.join("style.css"), "body{}").unwrap();

    let addr = spawn_server_with_dir(dir.to_str().unwrap()).await;

    // 根路径 → index.html + text/html
    let (st, body) = http_call(addr, "GET", "/", None).await;
    assert_eq!(st, 200);
    assert!(
        body.contains("<!doctype html>"),
        "根路径应返回 index.html，实际: {body:?}"
    );

    // JS 资产 → 内容 + application/javascript
    let (st2, body2) = http_call(addr, "GET", "/app.js", None).await;
    assert_eq!(st2, 200);
    assert_eq!(body2, "console.log(1)");

    // CSS 资产
    let (st3, body3) = http_call(addr, "GET", "/style.css", None).await;
    assert_eq!(st3, 200);
    assert_eq!(body3, "body{}");

    // 未知路径 → SPA fallback 到 index.html
    let (st4, body4) = http_call(addr, "GET", "/some/route", None).await;
    assert_eq!(st4, 200);
    assert!(
        body4.contains("<!doctype html>"),
        "未知路径应 fallback index.html"
    );

    // API 前缀未命中 → 404 JSON 信封
    let (st5, body5) = http_call(addr, "GET", "/api/v1/wb/nope", None).await;
    assert_eq!(st5, 404);
    assert!(
        body5.contains("\"code\":"),
        "API 404 应为 JSON 信封: {body5:?}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn tc_static_missing_dir_returns_hint() {
    // static_dir 不存在 → 返回构建提示 HTML（不 panic）
    let dir = std::env::temp_dir().join(format!("oaip_static_missing_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let addr = spawn_server_with_dir(dir.to_str().unwrap()).await;
    let (st, body) = http_call(addr, "GET", "/", None).await;
    assert_eq!(st, 200);
    assert!(
        body.contains("npm install") || body.contains("未检测到"),
        "缺 ui/dist 应返回构建提示，实际: {body:?}"
    );
}
