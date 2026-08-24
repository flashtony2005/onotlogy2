//! Business Loop 端到端契约测试。
//!
//! 从真实 crm-retention 业务包出发，验证：
//! 目标差距 → 幂等发现机会 → 显式决策 → 执行与结果记录 → 目标反馈 →
//! 新状态进入后自动选择并执行下一最优动作。

mod common;

use openaip_os::business_loop::{
    decide_opportunity, discover_opportunities, recommend_next_actions,
};
use openaip_os::storage::{LibSQLStorage, ObjectEngine};
use serde_json::{Value, json};
use std::sync::Arc;

async fn data(body: &str) -> Value {
    let value: Value = serde_json::from_str(body).expect("响应必须是 JSON");
    assert_eq!(value["code"], 0, "API 应成功：{body}");
    value["data"].clone()
}

async fn insert_rich_vip(storage: &LibSQLStorage, id: &str) {
    storage
        .apply_world(openaip_os::storage::WorldCommand::CreateObject {
            object_type: "Customer".into(),
            id: id.into(),
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
}

async fn seed_odl_project(storage: &LibSQLStorage, project_id: &str, ontology: &str) -> String {
    storage
        .create_project(project_id, "ODL 契约", "验证动作目录由 ODL 驱动", ontology)
        .await
        .expect("应能创建项目");
    let customer_id = format!("customer:{project_id}-vip");
    storage
        .apply_world(openaip_os::storage::WorldCommand::CreateObject {
            object_type: "Customer".into(),
            id: customer_id.clone(),
            data: json!({
                "name": "ODL 契约 VIP",
                "level": "VIP",
                "balance": 100_000.0,
                "days_since_last_contact": 75
            }),
            project_id: Some(project_id.into()),
        })
        .await
        .expect("应能写入客户状态");
    storage
        .apply_world(openaip_os::storage::WorldCommand::CreateObject {
            object_type: "BusinessGoal".into(),
            id: format!("business-goal:{project_id}"),
            data: json!({
                "target_metric": "monthly_reactivations",
                "baseline_value": 0.0,
                "target_value": 1.0,
                "current_value": 0.0,
                "status": "active"
            }),
            project_id: Some(project_id.into()),
        })
        .await
        .expect("应能写入经营目标");
    customer_id
}

#[tokio::test]
async fn odl_planning_metadata_drives_action_catalog() {
    let storage = LibSQLStorage::open(":memory:")
        .await
        .expect("内存库应可打开");
    let pack_ontology = std::fs::read_to_string("domain-packs/crm-retention/ontology.odl")
        .expect("业务包 ODL 应存在");

    seed_odl_project(&storage, "odl-base", &pack_ontology).await;
    let discovered = discover_opportunities(&storage, "odl-base")
        .await
        .expect("基础 ODL 应能发现机会");
    assert_eq!(discovered.len(), 1, "ODL 规则应发现机会：{discovered:?}");
    let stored_opportunity = storage
        .get("opportunity:odl-base:reactivation:customer-odl-base-vip")
        .await
        .expect("应能查询 ODL 发现的机会")
        .expect("ODL 规则应创建机会");
    assert_eq!(stored_opportunity.data["estimated_value"], json!(180_000.0));
    assert_eq!(
        stored_opportunity.data["evidence"],
        json!("VIP 客户已 75 天未联系，超过复购预警阈值")
    );
    let baseline = recommend_next_actions(&storage, "odl-base")
        .await
        .expect("基础 ODL 应能推荐动作");
    assert_eq!(baseline.len(), 1);
    assert_eq!(baseline[0].action, "ContactCustomer");
    assert_eq!(baseline[0].confidence, 0.7);
    let contact = baseline[0]
        .candidates
        .iter()
        .find(|candidate| candidate.action == "ContactCustomer")
        .expect("推荐应保留候选明细");
    assert_eq!(contact.cost, 5_000.0);
    assert!((contact.net_value - 121_000.0).abs() < 1e-9);
    let retain = baseline[0]
        .candidates
        .iter()
        .find(|candidate| candidate.action == "RetainCustomer")
        .expect("推荐应包含 ODL 挽留动作");
    assert_eq!(retain.confidence, 0.6);
    assert!((retain.net_value - 1_000.0).abs() < 1e-9);

    let expensive_contact = pack_ontology.replace("cost: 5000", "cost: 500000");
    assert_ne!(expensive_contact, pack_ontology, "契约测试必须修改成功");
    seed_odl_project(&storage, "odl-changed", &expensive_contact).await;
    discover_opportunities(&storage, "odl-changed")
        .await
        .expect("变更 ODL 应能发现机会");
    let changed = recommend_next_actions(&storage, "odl-changed")
        .await
        .expect("变更 ODL 应能重新推荐动作");
    assert_eq!(
        changed[0].action, "RetainCustomer",
        "ODL 成本变化应直接改变最优决策"
    );

    let inactive_rule = pack_ontology.replace("value: 45", "value: 1000");
    assert_ne!(inactive_rule, pack_ontology, "契约测试必须修改成功");
    seed_odl_project(&storage, "odl-inactive", &inactive_rule).await;
    let none = discover_opportunities(&storage, "odl-inactive")
        .await
        .expect("修改机会条件后仍应可执行发现");
    assert!(none.is_empty(), "ODL 条件不满足时不能发现机会：{none:?}");

    let error = decide_opportunity(
        &storage,
        "odl-base",
        &baseline[0].opportunity_id,
        Some("refund"),
        "contract-test",
        None,
        None,
    )
    .await
    .expect_err("没有 planning 元数据的 refund 不能进入业务规划");
    assert!(error.to_string().contains("不支持的业务动作 refund"));
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
    let action_outcome_id = result["actionOutcome"]["id"]
        .as_str()
        .expect("接触动作应有执行证据")
        .to_string();
    let business_result_id = result["businessResult"]["id"]
        .as_str()
        .expect("直接业务结果应立即产生")
        .to_string();
    let transition_id = result["stateTransition"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(
        result["actionOutcome"]["data"]["stage"],
        json!("ActionExecuted")
    );
    assert_eq!(
        result["businessResult"]["data"]["stage"],
        json!("BusinessResult")
    );
    assert_eq!(result["businessResult"]["data"]["success"], json!(true));
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
    assert!(has_edge(
        &decision_id,
        "produced_outcome",
        &action_outcome_id
    ));
    assert!(has_edge(
        &decision_id,
        "produced_outcome",
        &business_result_id
    ));
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
    insert_rich_vip(&storage, "customer:demo-vip-rich").await;

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
    assert_eq!(rich_recommendation["confidence"], json!(0.6));
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
        result["execution"]["actionOutcome"]["data"]["stage"],
        json!("ActionExecuted")
    );
    assert_eq!(
        result["execution"]["businessResult"],
        json!(null),
        "创建挽留任务只是执行证据，不是客户被留住的业务结果"
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

    // 挽留结果必须经历显式生命周期；只有终局才成为可学习 BusinessResult。
    let start_path = format!("/api/v1/business/retention-tasks/{task_id}/start");
    let (status, body) = common::http_call(addr, "POST", &start_path, Some("{}")).await;
    assert_eq!(status, 200, "应能启动挽留任务：{body}");
    assert_eq!(data(&body).await["data"]["status"], json!("InProgress"));

    let retention_decision_id = result["decision"]["id"].as_str().unwrap().to_string();
    let result_path = format!("/api/v1/business/retention-tasks/{task_id}/result");
    let (status, body) =
        common::http_call(addr, "POST", &result_path, Some(r#"{"success":true}"#)).await;
    assert_eq!(status, 200, "应能记录挽留终局：{body}");
    let settled = data(&body).await;
    assert_eq!(settled["task"]["data"]["status"], json!("Completed"));
    assert_eq!(settled["outcome"]["data"]["stage"], json!("BusinessResult"));
    assert_eq!(settled["outcome"]["data"]["success"], json!(true));
    assert_eq!(
        settled["goals"][0]["currentValue"],
        json!(13.0),
        "挽留成功也不误报为复购指标"
    );
    let current_relations = storage.list_all_relations().await.unwrap();
    assert!(current_relations.iter().any(|relation| {
        relation.source == retention_decision_id
            && relation.relation == "produced_task"
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
        result["execution"]["businessResult"]["data"]["stage"],
        json!("BusinessResult")
    );
    assert_eq!(
        result["execution"]["businessResult"]["data"]["success"],
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

    // 失败终局也是学习样本：第二次挽留沿用成功置信度，失败后概率回到 1/2。
    insert_rich_vip(&storage, "customer:demo-vip-rich2").await;
    common::http_call(
        addr,
        "POST",
        "/api/v1/business/discover-opportunities",
        Some(&run_next_body),
    )
    .await;
    let (status, body) = common::http_call(
        addr,
        "GET",
        "/api/v1/business/recommendations?projectId=crm-retention",
        None,
    )
    .await;
    assert_eq!(status, 200, "应能读取失败前推荐：{body}");
    let retry = data(&body)
        .await
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .find(|item| item["subject"] == json!("customer:demo-vip-rich2"))
        .unwrap_or_else(|| panic!("缺少第二位高余额客户的推荐"));
    assert_eq!(
        retry["confidence"],
        json!(1.0),
        "只有 BusinessResult 参与学习"
    );

    let (status, body) = common::http_call(
        addr,
        "POST",
        "/api/v1/business/run-next",
        Some(&run_next_body),
    )
    .await;
    assert_eq!(status, 200, "应能执行第二位高余额客户的动作：{body}");
    let result = data(&body).await;
    assert_eq!(
        result["recommendation"]["subject"],
        json!("customer:demo-vip-rich2")
    );
    let second_task_id = result["execution"]["createdObject"]["id"]
        .as_str()
        .expect("第二次挽留也应创建任务")
        .to_string();
    common::http_call(
        addr,
        "POST",
        &format!("/api/v1/business/retention-tasks/{second_task_id}/start"),
        Some("{}"),
    )
    .await;
    let (status, body) = common::http_call(
        addr,
        "POST",
        &format!("/api/v1/business/retention-tasks/{second_task_id}/result"),
        Some(r#"{"success":false}"#),
    )
    .await;
    assert_eq!(status, 200, "应能记录挽留失败：{body}");
    let failed = data(&body).await;
    assert_eq!(failed["task"]["data"]["status"], json!("Failed"));
    assert_eq!(failed["outcome"]["data"]["success"], json!(false));

    insert_rich_vip(&storage, "customer:demo-vip-rich3").await;
    common::http_call(
        addr,
        "POST",
        "/api/v1/business/discover-opportunities",
        Some(&run_next_body),
    )
    .await;
    let (status, body) = common::http_call(
        addr,
        "GET",
        "/api/v1/business/recommendations?projectId=crm-retention",
        None,
    )
    .await;
    assert_eq!(status, 200, "应能读取失败后推荐：{body}");
    let learned = data(&body)
        .await
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .find(|item| item["subject"] == json!("customer:demo-vip-rich3"))
        .unwrap_or_else(|| panic!("缺少第三位高余额客户的推荐"));
    assert_eq!(learned["confidence"], json!(0.5), "负样本应降低置信度");

    let (status, body) = common::http_call(
        addr,
        "POST",
        "/api/v1/business/run-next",
        Some(&run_next_body),
    )
    .await;
    assert_eq!(status, 200, "应能执行降低置信度后的推荐：{body}");
    let result = data(&body).await;
    assert_eq!(result["executed"], json!(true));
    assert_eq!(
        result["recommendation"]["subject"],
        json!("customer:demo-vip-rich3")
    );
    assert_eq!(result["execution"]["businessResult"], json!(null));

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
