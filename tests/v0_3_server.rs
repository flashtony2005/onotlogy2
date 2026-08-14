//! V0.5 观察视图端点测试契约（TC-024a + Tasks）
mod common;
use common::*;

use openaip_os::runtime::RuntimeKernel;
use openaip_os::storage::LibSQLStorage;
use serde_json::{Value, json};

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
