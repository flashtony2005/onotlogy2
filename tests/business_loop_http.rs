//! Business Loop 端到端契约测试。
//!
//! 从真实 crm-retention 业务包出发，验证：
//! 目标差距 → 幂等发现机会 → 显式决策 → 执行与结果记录 → 目标反馈 →
//! 新状态进入后自动选择并执行下一最优动作。

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

    let (_, body) = common::http_call(
        addr,
        "GET",
        "/api/v1/business/goals?projectId=crm-retention",
        None,
    )
    .await;
    let goals = data(&body).await.as_array().cloned().unwrap_or_default();
    let goal = goals
        .iter()
        .find(|goal| goal["targetMetric"] == "monthly_reactivations")
        .unwrap_or_else(|| panic!("执行后仍应能看到业务目标：{goals:?}"));
    assert_eq!(goal["currentValue"], 13.0, "Outcome 应反馈到目标进度");
    assert_eq!(goal["gap"], 7.0);

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

    // 注入两类新企业状态：低余额客户适合低成本接触；高余额客户应比较出挽留路径。
    storage
        .apply_world(openaip_os::storage::WorldCommand::CreateObject {
            object_type: "Customer".into(),
            id: "customer:demo-vip-late".into(),
            data: json!({
                "name": "第二位超期 VIP 客户",
                "level": "VIP",
                "balance": 0,
                "days_since_last_contact": 61
            }),
            project_id: Some("crm-retention".into()),
        })
        .await
        .expect("应能写入新企业状态");
    storage
        .apply_world(openaip_os::storage::WorldCommand::CreateObject {
            object_type: "Customer".into(),
            id: "customer:demo-vip-rich".into(),
            data: json!({
                "name": "高余额超期 VIP 客户",
                "level": "VIP",
                "balance": 2_000_000.0,
                "days_since_last_contact": 75
            }),
            project_id: Some("crm-retention".into()),
        })
        .await
        .expect("应能写入高余额客户状态");

    let run_next_body = json!({ "projectId": "crm-retention" }).to_string();
    let (status, body) = common::http_call(
        addr,
        "POST",
        "/api/v1/business/discover-opportunities",
        Some(&run_next_body),
    )
    .await;
    assert_eq!(status, 200, "应能重新发现新状态机会：{body}");
    assert_eq!(data(&body).await.as_array().map(Vec::len), Some(2));

    let (status, body) = common::http_call(
        addr,
        "GET",
        "/api/v1/business/recommendations?projectId=crm-retention",
        None,
    )
    .await;
    assert_eq!(status, 200, "应能读取下一动作推荐：{body}");
    let recommendations = data(&body).await.as_array().cloned().unwrap_or_default();
    assert_eq!(recommendations.len(), 2);
    let recommendation_for = |subject: &str| {
        recommendations
            .iter()
            .find(|item| item["subject"] == json!(subject))
            .unwrap_or_else(|| panic!("缺少 {subject} 的推荐：{recommendations:?}"))
    };
    let low_recommendation = recommendation_for("customer:demo-vip-late");
    let rich_recommendation = recommendation_for("customer:demo-vip-rich");
    assert_eq!(low_recommendation["action"], json!("ContactCustomer"));
    assert_eq!(
        low_recommendation["candidates"][0]["action"],
        json!("ContactCustomer")
    );
    assert_eq!(rich_recommendation["action"], json!("RetainCustomer"));
    assert_eq!(
        rich_recommendation["candidates"][0]["action"],
        json!("RetainCustomer")
    );
    assert_eq!(
        rich_recommendation["candidates"][1]["action"],
        json!("ContactCustomer")
    );

    let (status, body) = common::http_call(
        addr,
        "POST",
        "/api/v1/business/run-next",
        Some(&run_next_body),
    )
    .await;
    assert_eq!(status, 200, "应能自动执行下一最优动作：{body}");
    let result = data(&body).await;
    assert_eq!(result["executed"], json!(true));
    assert_eq!(
        result["recommendation"]["subject"],
        json!("customer:demo-vip-rich")
    );
    assert_eq!(result["recommendation"]["action"], json!("RetainCustomer"));
    assert_eq!(result["decision"]["data"]["status"], json!("Selected"));
    let alternatives: Value =
        serde_json::from_str(result["decision"]["data"]["alternatives"].as_str().unwrap())
            .expect("候选比较应保存为 JSON 字符串");
    assert_eq!(alternatives.as_array().map(Vec::len), Some(2));
    let task_id = result["execution"]["createdObject"]["id"]
        .as_str()
        .expect("挽留动作应创建 RetentionTask")
        .to_string();
    assert_eq!(
        result["execution"]["outcome"]["data"]["success"],
        json!(true)
    );
    assert_eq!(
        result["goals"][0]["currentValue"],
        json!(13.0),
        "挽留不直接计入复购指标"
    );
    assert_eq!(result["goals"][0]["gap"], json!(7.0));

    let task = storage
        .get(&task_id)
        .await
        .expect("应能读取挽留任务")
        .expect("挽留任务应存在");
    assert_eq!(task.r#type, "RetentionTask");
    assert_eq!(task.data["status"], json!("Pending"));
    let current_relations = storage.list_all_relations().await.unwrap();
    assert!(current_relations.iter().any(|relation| {
        relation.source == "customer:demo-vip-rich"
            && relation.relation == "needs_retention"
            && relation.target == task_id
    }));

    // 高净值路径执行完后，低余额机会仍应由接触动作闭环并推进目标。
    let (status, body) = common::http_call(
        addr,
        "POST",
        "/api/v1/business/run-next",
        Some(&run_next_body),
    )
    .await;
    assert_eq!(status, 200, "应能自动执行剩余最优动作：{body}");
    let result = data(&body).await;
    assert_eq!(result["executed"], json!(true));
    assert_eq!(
        result["recommendation"]["subject"],
        json!("customer:demo-vip-late")
    );
    assert_eq!(result["recommendation"]["action"], json!("ContactCustomer"));
    assert_eq!(
        result["execution"]["outcome"]["data"]["success"],
        json!(true)
    );
    assert_eq!(result["goals"][0]["currentValue"], json!(14.0));
    assert_eq!(result["goals"][0]["gap"], json!(6.0));

    let late_vip = storage
        .get("customer:demo-vip-late")
        .await
        .expect("应能读取新 VIP 客户")
        .expect("新 VIP 客户仍存在");
    assert_eq!(late_vip.data["days_since_last_contact"], json!(0));

    let rich_vip = storage
        .get("customer:demo-vip-rich")
        .await
        .expect("应能读取高余额客户")
        .expect("高余额客户仍存在");
    assert_eq!(rich_vip.data["days_since_last_contact"], json!(75));

    let (status, body) = common::http_call(
        addr,
        "POST",
        "/api/v1/business/run-next",
        Some(&run_next_body),
    )
    .await;
    assert_eq!(status, 200, "没有机会时应返回空闲结果：{body}");
    assert_eq!(data(&body).await["executed"], json!(false));
}
