//! Phase 2.4 业务包运行能力契约测试。
//!
//! 验证运行配置通过 WorldCommand 持久化、业务包启停状态、插件关联和策略注册表。

mod common;

use openaip_os::storage::LibSQLStorage;
use serde_json::{Value, json};
use std::sync::Arc;

#[tokio::test]
async fn exposes_plugin_association_and_default_runtime() {
    let storage = Arc::new(LibSQLStorage::open(":memory:").await.unwrap());
    let addr = common::spawn_server(storage).await;

    let (status, body) = common::http_call(addr, "GET", "/api/v1/domain-packs", None).await;
    assert_eq!(status, 200, "业务包列表应成功返回：{body}");
    let value: Value = serde_json::from_str(&body).unwrap();
    let lottery = value["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|pack| pack["id"] == "lottery-analysis")
        .unwrap();
    assert_eq!(lottery["plugin_ids"], json!(["lottery-analysis"]));
    assert!(
        lottery["plugin_capabilities"]
            .as_array()
            .unwrap()
            .iter()
            .any(|capability| capability == "lottery.analyze")
    );

    let (status, body) = common::http_call(
        addr,
        "GET",
        "/api/v1/domain-packs/lottery-analysis/runtime",
        None,
    )
    .await;
    assert_eq!(status, 200, "未安装业务包仍可读取默认配置：{body}");
    let value: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(value["data"]["enabled"], true);
    assert_eq!(value["data"]["poll_interval_seconds"], 30);
}

#[tokio::test]
async fn updates_runtime_disables_pack_and_lists_strategy_registry() {
    let storage = Arc::new(LibSQLStorage::open(":memory:").await.unwrap());
    let addr = common::spawn_server(storage).await;

    let (status, body) = common::http_call(
        addr,
        "POST",
        "/api/v1/domain-packs/lottery-analysis/install",
        None,
    )
    .await;
    assert_eq!(status, 200, "安装应成功：{body}");

    let update = json!({
        "enabled": true,
        "source_path": "/tmp/dlt.csv",
        "watch_enabled": true,
        "poll_interval_seconds": 60,
        "auto_sync": true,
        "auto_analyze": true,
        "auto_backtest": true,
        "auto_review": false,
        "strategy_ids": ["statistics", "heuristic"]
    })
    .to_string();
    let (status, body) = common::http_call(
        addr,
        "PUT",
        "/api/v1/domain-packs/lottery-analysis/runtime",
        Some(&update),
    )
    .await;
    assert_eq!(status, 200, "运行配置保存应成功：{body}");
    let value: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(value["data"]["source_path"], "/tmp/dlt.csv");
    assert_eq!(
        value["data"]["strategy_ids"],
        json!(["statistics", "heuristic"])
    );

    let (status, body) = common::http_call(
        addr,
        "POST",
        "/api/v1/domain-packs/lottery-analysis/disable",
        None,
    )
    .await;
    assert_eq!(status, 200, "停用应成功：{body}");
    let value: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(value["data"]["enabled"], false);

    let (status, body) = common::http_call(addr, "GET", "/api/v1/domain-packs", None).await;
    assert_eq!(status, 200);
    let value: Value = serde_json::from_str(&body).unwrap();
    let lottery = value["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|pack| pack["id"] == "lottery-analysis")
        .unwrap();
    assert_eq!(lottery["status"], "disabled");
    assert_eq!(lottery["enabled"], false);

    let (status, body) = common::http_call(
        addr,
        "GET",
        "/api/v1/domain-packs/lottery-analysis/strategies",
        None,
    )
    .await;
    assert_eq!(status, 200, "策略注册表应成功返回：{body}");
    let value: Value = serde_json::from_str(&body).unwrap();
    let strategies = value["data"].as_array().unwrap();
    assert_eq!(strategies.len(), 3);
    assert!(
        strategies
            .iter()
            .any(|strategy| strategy["code"] == "statistics")
    );
    assert!(
        strategies
            .iter()
            .any(|strategy| strategy["code"] == "heuristic")
    );
    assert!(
        strategies
            .iter()
            .any(|strategy| strategy["code"] == "metaphysics")
    );

    let (status, body) = common::http_call(
        addr,
        "GET",
        "/api/v1/domain-packs/lottery-analysis/operations",
        None,
    )
    .await;
    assert_eq!(status, 200, "运行能力汇总应成功返回：{body}");
    let value: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(value["data"]["plugins"][0]["id"], "lottery-analysis");
    assert_eq!(value["data"]["runtime"]["enabled"], false);
}

#[tokio::test]
async fn rejects_runtime_update_before_install() {
    let storage = Arc::new(LibSQLStorage::open(":memory:").await.unwrap());
    let addr = common::spawn_server(storage).await;
    let update = json!({"enabled": true}).to_string();
    let (status, body) = common::http_call(
        addr,
        "PUT",
        "/api/v1/domain-packs/lottery-analysis/runtime",
        Some(&update),
    )
    .await;
    assert_eq!(status, 409, "未安装业务包不应保存运行配置：{body}");
}
