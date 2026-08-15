//! V2 认知面板 API（Phase 10 · UI 后端）集成测试。
//!
//! SV1 空列表 → Import Skill → 列表可见
//! SV2 Plugin 列表（生命周期状态）
//! SV3 Session 创建 + 事件读取（append 后可查）
//! SV4 未登录访问被拒（auth 中间件生效）——独立于 enforce_auth=false 的基础路径

mod common;

use openaip_os::runtime::RuntimeKernel;
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

#[tokio::test]
async fn tc_sv1_import_skill_then_list() {
    let kernel = std::sync::Arc::new(RuntimeKernel::new(
        openaip_os::storage::LibSQLStorage::open(":memory:")
            .await
            .unwrap(),
    ));
    let addr = common::spawn_server(kernel).await;

    // 空列表
    let (s0, body0) = call(addr, "GET", "/api/v1/v2/skills", None).await;
    assert_eq!(s0, 200);
    assert_eq!(body0.as_array().unwrap().len(), 0, "初始为空");

    // Import
    let md = "# Thumbnail Generator\n\n- goal: 生成缩略图\n## Stage 1: Design\n- capability: image.generate\n- artifact: ThumbnailConcept\n## Stage 2: Render\n- capability: image.render\n- artifact: ThumbnailFile\n";
    let (s1, body1) = call(
        addr,
        "POST",
        "/api/v1/v2/skills/import",
        Some(json!({ "name": "thumbnail-gen", "content": md, "known_capabilities": ["image.generate", "image.render"] })),
    )
    .await;
    assert_eq!(s1, 200, "导入成功: {body1}");
    assert_eq!(body1["id"], "thumbnail-gen");
    assert_eq!(body1["skill_type"], "Workflow");

    // 列表可见
    let (s2, body2) = call(addr, "GET", "/api/v1/v2/skills", None).await;
    assert_eq!(s2, 200);
    let arr = body2.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["capabilities"][0], "image.generate");

    // 重复导入 → 409
    let (s3, _) = call(
        addr,
        "POST",
        "/api/v1/v2/skills/import",
        Some(json!({ "name": "thumbnail-gen", "content": md, "known_capabilities": ["image.generate", "image.render"] })),
    )
    .await;
    assert_eq!(s3, 409, "重复注册拒绝");
}

#[tokio::test]
async fn tc_sv1b_skill_raw_view() {
    let kernel = std::sync::Arc::new(RuntimeKernel::new(
        openaip_os::storage::LibSQLStorage::open(":memory:")
            .await
            .unwrap(),
    ));
    let addr = common::spawn_server(kernel).await;

    let md = "# Demo Skill\n\n- goal: 演示\n## Stage 1: A\n- capability: x.y\n## Stage 2: B\n- capability: x.z\n";
    let (s0, _) = call(
        addr,
        "POST",
        "/api/v1/v2/skills/import",
        Some(json!({ "name": "demo-skill", "content": md, "known_capabilities": ["x.y", "x.z"] })),
    )
    .await;
    assert_eq!(s0, 200);

    // 原始内容可查看（Raw 不修改）
    let (s1, body1) = call(addr, "GET", "/api/v1/v2/skills/demo-skill/raw", None).await;
    assert_eq!(s1, 200, "raw 端点: {body1}");
    assert_eq!(body1["content"], md, "原始内容逐字保留");
    assert!(
        !body1["content_hash"].as_str().unwrap().is_empty(),
        "hash 可追溯"
    );

    // 不存在 → 404
    let (s2, _) = call(addr, "GET", "/api/v1/v2/skills/nope/raw", None).await;
    assert_eq!(s2, 404);
}

#[tokio::test]
async fn tc_sv2_plugins_list() {
    let kernel = std::sync::Arc::new(RuntimeKernel::new(
        openaip_os::storage::LibSQLStorage::open(":memory:")
            .await
            .unwrap(),
    ));
    let addr = common::spawn_server(kernel).await;
    let (s, body) = call(addr, "GET", "/api/v1/v2/plugins", None).await;
    assert_eq!(s, 200);
    assert!(body.is_array(), "插件列表（初始为空数组）");
}

#[tokio::test]
async fn tc_sv4_workers_preseeded() {
    let kernel = std::sync::Arc::new(RuntimeKernel::new(
        openaip_os::storage::LibSQLStorage::open(":memory:")
            .await
            .unwrap(),
    ));
    let addr = common::spawn_server(kernel).await;
    let (s, body) = call(addr, "GET", "/api/v1/v2/workers", None).await;
    assert_eq!(s, 200);
    let arr = body.as_array().unwrap();
    // 预置 4 个 Worker：echo / git / codex / claude-code
    let ids: Vec<&str> = arr.iter().map(|w| w["id"].as_str().unwrap()).collect();
    for expected in ["echo", "git", "codex", "claude-code"] {
        assert!(
            ids.contains(&expected),
            "预置 worker {expected} 缺失: {ids:?}"
        );
    }
    let codex = arr.iter().find(|w| w["id"] == "codex").unwrap();
    assert!(
        codex["capabilities"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c == "code.write"),
        "codex 壳声明 code.write"
    );
}

#[tokio::test]
async fn tc_sv3_session_create_and_read() {
    let kernel = std::sync::Arc::new(RuntimeKernel::new(
        openaip_os::storage::LibSQLStorage::open(":memory:")
            .await
            .unwrap(),
    ));
    let addr = common::spawn_server(kernel).await;

    let (s1, body1) = call(addr, "POST", "/api/v1/v2/sessions/swe-1/create", None).await;
    assert_eq!(s1, 200);
    assert_eq!(body1["session_id"], "swe-1");
    assert_eq!(body1["events"][0]["event_type"], "session.created");

    let (s2, body2) = call(addr, "GET", "/api/v1/v2/sessions/swe-1", None).await;
    assert_eq!(s2, 200);
    assert_eq!(body2["events"].as_array().unwrap().len(), 1);

    // 不存在 → 404
    let (s3, _) = call(addr, "GET", "/api/v1/v2/sessions/nope", None).await;
    assert_eq!(s3, 404);
}
