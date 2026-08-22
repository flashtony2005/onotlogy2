//! Phase 2.3 业务包管理 API 契约测试。
//!
//! 验证网页端依赖的发现、声明式 Skill 关联、安装状态和幂等安装行为。

mod common;

use openaip_os::storage::LibSQLStorage;
use serde_json::{Value, json};
use std::sync::Arc;

#[tokio::test]
async fn lists_lottery_pack_and_declared_skill() {
    let storage = Arc::new(LibSQLStorage::open(":memory:").await.unwrap());
    let addr = common::spawn_server(storage).await;

    let (status, body) = common::http_call(addr, "GET", "/api/v1/domain-packs", None).await;
    assert_eq!(status, 200, "业务包列表应成功返回：{body}");
    let value: Value = serde_json::from_str(&body).unwrap();
    let packs = value["data"].as_array().expect("data 应为业务包数组");
    let lottery = packs
        .iter()
        .find(|pack| pack["id"] == "lottery-analysis")
        .expect("应发现彩票业务包");

    assert_eq!(lottery["valid"], true);
    assert_eq!(lottery["installed"], false);
    assert_eq!(lottery["status"], "available");
    assert_eq!(lottery["skill_ids"], json!(["business-pack-operations"]));
    assert!(lottery["ontology_types"].as_u64().unwrap() > 0);
    assert!(lottery["bootstrap_objects"].as_u64().unwrap() > 0);
    assert!(lottery["bootstrap_relations"].as_u64().unwrap() > 0);
}

#[tokio::test]
async fn installs_lottery_pack_idempotently_and_refreshes_status() {
    let storage = Arc::new(LibSQLStorage::open(":memory:").await.unwrap());
    let addr = common::spawn_server(storage).await;

    let (first_status, first_body) = common::http_call(
        addr,
        "POST",
        "/api/v1/domain-packs/lottery-analysis/install",
        None,
    )
    .await;
    assert_eq!(first_status, 200, "首次安装应成功：{first_body}");
    let first: Value = serde_json::from_str(&first_body).unwrap();
    assert_eq!(first["data"]["pack"]["installed"], true);
    assert_eq!(first["data"]["pack"]["status"], "installed");
    assert_eq!(first["data"]["project_id"], "lottery-analysis");
    assert!(first["data"]["objects_created"].as_u64().unwrap() > 0);
    assert!(first["data"]["relations_created"].as_u64().unwrap() > 0);

    let (second_status, second_body) = common::http_call(
        addr,
        "POST",
        "/api/v1/domain-packs/lottery-analysis/install",
        None,
    )
    .await;
    assert_eq!(second_status, 200, "重复安装应幂等：{second_body}");
    let second: Value = serde_json::from_str(&second_body).unwrap();
    assert_eq!(second["data"]["pack"]["installed"], true);
    assert_eq!(second["data"]["objects_created"], 0);
    assert_eq!(second["data"]["relations_created"], 0);
    assert!(second["data"]["objects_skipped"].as_u64().unwrap() > 0);
    assert!(second["data"]["relations_skipped"].as_u64().unwrap() > 0);
}

#[tokio::test]
async fn rejects_unknown_domain_pack_install() {
    let storage = Arc::new(LibSQLStorage::open(":memory:").await.unwrap());
    let addr = common::spawn_server(storage).await;

    let (status, body) =
        common::http_call(addr, "POST", "/api/v1/domain-packs/not-found/install", None).await;
    assert_eq!(status, 404, "未知业务包应返回 404：{body}");
    let value: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(value["code"], 4040);
}
