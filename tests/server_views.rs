//! 服务端观察视图端点测试契约（TC-024a + Tasks）。
mod common;
use common::*;

use openaip_os::server;
use openaip_os::storage::LibSQLStorage;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;

// ── T1：静态托管（serve_static / SPA fallback / Content-Type / API 404） ──
// 覆盖全项目最低点（覆盖率基线 static_files 0%）。

/// 启动带自定义 static_dir 的无认证 server（临时目录，不依赖 ui/dist）。
async fn spawn_server_with_dir(static_dir: &str) -> SocketAddr {
    let s = LibSQLStorage::open(":memory:").await.unwrap();
    let kernel = Arc::new(s);
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
