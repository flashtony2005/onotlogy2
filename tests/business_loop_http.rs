//! Business Loop v1 端到端契约测试。
//!
//! 从真实 crm-retention 业务包出发，验证：
//! 目标差距 → 幂等发现机会 → 显式决策 → 执行与结果记录 → 再规划不再重复发现。

mod common;

use openaip_os::storage::{LibSQLStorage, ObjectEngine};
use serde_json::{Value, json};
use std::sync::Arc;

async fn data(body: &str) -> Value {
    let value: Value = serde_json::from_str(body).expect("响应必须是 JSON");
    assert_eq!(value["code"], 0, "API 应成功：{body}");
    value["data"].clone()
}

#[tokio::test]
async fn business_loop_discovers_decides_executes_and_replans() {
    let storage = Arc::new(LibSQLStorage::open(":memory:").await.unwrap());
    let addr = common::spawn_server(storage.clone()).await;

    let (status, body) = common::http_call(
        addr,
        "POST",
        "/api/v1/domain-packs/crm-retention/install",
        None,
    )
    .await;
    assert_eq!(status, 200, "业务包应安装成功：{body}");

    let (status, body) = common::http_call(
        addr,
        "GET",
        "/api/v1/business/goals?projectId=crm-retention",
        None,
    )
    .await;
    assert_eq!(status, 200, "应能读取目标差距：{body}");
    let goals = data(&body).await.as_array().cloned().unwrap_or_default();
    let goal = goals
        .iter()
        .find(|goal| goal["targetMetric"] == "monthly_reactivations")
        .unwrap_or_else(|| panic!("应包含月度复购目标：{goals:?}"));
    assert_eq!(goal["targetValue"], 20.0);
    assert_eq!(goal["currentValue"], 12.0);
    assert_eq!(goal["gap"], 8.0);

    let discover_body = json!({ "projectId": "crm-retention" }).to_string();
    let (status, body) = common::http_call(
        addr,
        "POST",
        "/api/v1/business/discover-opportunities",
        Some(&discover_body),
    )
    .await;
    assert_eq!(status, 200, "应能发现复购机会：{body}");
    let opportunities = data(&body).await.as_array().cloned().unwrap_or_default();
    assert_eq!(opportunities.len(), 1, "种子数据只应产生一个 VIP 复购机会");
    let opportunity_id = opportunities[0]["id"]
        .as_str()
        .expect("机会必须有 ID")
        .to_string();
    assert_eq!(
        opportunities[0]["data"]["subject"],
        json!("customer:demo-vip")
    );
    assert_eq!(
        opportunities[0]["data"]["opportunity_type"],
        json!("Reactivation")
    );

    let (_, repeat_body) = common::http_call(
        addr,
        "POST",
        "/api/v1/business/discover-opportunities",
        Some(&discover_body),
    )
    .await;
    assert_eq!(
        data(&repeat_body).await.as_array().map(Vec::len),
        Some(0),
        "同一 Open 机会不能重复创建：{repeat_body}"
    );

    let decide_path = format!("/api/v1/business/opportunities/{opportunity_id}/decide");
    let (status, body) = common::http_call(
        addr,
        "POST",
        &decide_path,
        Some(r#"{"action":"ContactCustomer","reason":"关闭 VIP 复购风险"}"#),
    )
    .await;
    assert_eq!(status, 200, "应能建立决策：{body}");
    let decision = data(&body).await;
    assert_eq!(decision["data"]["action"], json!("ContactCustomer"));
    assert_eq!(decision["data"]["status"], json!("Selected"));

    let execute_path = format!("/api/v1/business/opportunities/{opportunity_id}/execute");
    let (status, body) = common::http_call(addr, "POST", &execute_path, Some("{}")).await;
    assert_eq!(status, 200, "应能执行已决策动作：{body}");
    let result = data(&body).await;
    assert_eq!(result["decision"]["data"]["status"], json!("Executed"));
    let decision_id = result["decision"]["id"].as_str().unwrap().to_string();
    let outcome_id = result["outcome"]["id"].as_str().unwrap().to_string();
    let transition_id = result["stateTransition"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(result["outcome"]["data"]["success"], json!(true));
    assert_eq!(
        result["stateTransition"]["data"]["subject"],
        json!("customer:demo-vip")
    );

    let vip = storage
        .get("customer:demo-vip")
        .await
        .expect("应能读取 VIP 客户")
        .expect("执行后 VIP 客户仍存在");
    assert_eq!(vip.data["days_since_last_contact"], json!(0));

    let relations = storage.list_all_relations().await.unwrap();
    let has_edge = |source: &str, edge: &str, target: &str| {
        relations.iter().any(|relation| {
            relation.source == source && relation.relation == edge && relation.target == target
        })
    };
    assert!(has_edge("customer:demo-vip", "targets", &opportunity_id));
    assert!(has_edge(&decision_id, "decided_from", &opportunity_id));
    assert!(has_edge(&decision_id, "produced_outcome", &outcome_id));
    assert!(has_edge(
        &transition_id,
        "changed_state",
        "customer:demo-vip"
    ));

    let (_, body) = common::http_call(
        addr,
        "POST",
        "/api/v1/business/discover-opportunities",
        Some(&discover_body),
    )
    .await;
    assert_eq!(
        data(&body).await.as_array().map(Vec::len),
        Some(0),
        "状态变化后不应再次发现同一机会"
    );
}
