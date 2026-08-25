//! Business Loop 端到端契约测试。
//!
//! 从真实 crm-retention 业务包出发，验证：
//! 目标差距 → 幂等发现机会 → 显式决策 → 执行与结果记录 → 目标反馈 →
//! 新状态进入后自动选择并执行下一最优动作。

mod common;

use openaip_os::abi::ir::ActorKind;
use openaip_os::abi::odl::parse_odl;
use openaip_os::business_loop::{
    DecisionOptions, decide_business_approval, decide_opportunity, discover_opportunities,
    execute_opportunity, plan_opportunity_paths, recommend_next_actions, run_next_best_action,
    settle_follow_up, start_follow_up,
};
use openaip_os::plugin::catalog::PluginCatalog;
use openaip_os::storage::{LibSQLStorage, ObjectEngine};
use openaip_os::worker_core::registry::WorkerRegistry;
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

async fn seed_odl_project(
    storage: &LibSQLStorage,
    project_id: &str,
    ontology: &str,
    target_metric: &str,
) -> String {
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
                "target_metric": target_metric,
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

    seed_odl_project(
        &storage,
        "odl-base",
        &pack_ontology,
        "monthly_reactivations",
    )
    .await;
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

    let expensive_contact = pack_ontology
        .replace("cost: 5000", "cost: 500000")
        .replace("require_approval: true", "require_approval: false");
    assert_ne!(expensive_contact, pack_ontology, "契约测试必须修改成功");
    seed_odl_project(
        &storage,
        "odl-changed",
        &expensive_contact,
        "monthly_reactivations",
    )
    .await;
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
    seed_odl_project(
        &storage,
        "odl-inactive",
        &inactive_rule,
        "monthly_reactivations",
    )
    .await;
    let none = discover_opportunities(&storage, "odl-inactive")
        .await
        .expect("修改机会条件后仍应可执行发现");
    assert!(none.is_empty(), "ODL 条件不满足时不能发现机会：{none:?}");

    let error = decide_opportunity(
        &storage,
        "odl-base",
        &baseline[0].opportunity_id,
        "contract-test",
        DecisionOptions {
            action: Some("refund"),
            reason: None,
            alternatives: None,
            params: None,
        },
    )
    .await
    .expect_err("声明参数的动作不能缺少输入");
    assert!(error.to_string().contains("动作 refund 缺少参数 amount"));
}

#[tokio::test]
async fn goal_metric_is_declared_and_must_agree_across_evidence_chain() {
    let storage = LibSQLStorage::open(":memory:")
        .await
        .expect("内存库应可打开");
    let pack_ontology = std::fs::read_to_string("domain-packs/crm-retention/ontology.odl")
        .expect("业务包 ODL 应存在");

    let custom_ontology = pack_ontology.replace(
        "goal_metric: monthly_reactivations",
        "goal_metric: qualified_reactivations",
    );
    assert_ne!(custom_ontology, pack_ontology, "契约测试必须修改成功");
    seed_odl_project(
        &storage,
        "custom-goal",
        &custom_ontology,
        "qualified_reactivations",
    )
    .await;
    let discovered = discover_opportunities(&storage, "custom-goal")
        .await
        .expect("自定义指标项目应能发现机会");
    assert_eq!(discovered.len(), 1);
    let opportunity_id = discovered[0]["id"].as_str().expect("机会必须有 ID");
    decide_opportunity(
        &storage,
        "custom-goal",
        opportunity_id,
        "contract-test",
        DecisionOptions {
            action: Some("ContactCustomer"),
            reason: None,
            alternatives: None,
            params: None,
        },
    )
    .await
    .expect("应能决策接触动作");
    let execution = execute_opportunity(
        &storage,
        &PluginCatalog::new(),
        &WorkerRegistry::new(),
        "custom-goal",
        opportunity_id,
    )
    .await
    .expect("应能执行接触动作");
    assert_eq!(
        execution["goals"][0]["targetMetric"],
        json!("qualified_reactivations")
    );
    assert_eq!(
        execution["goals"][0]["currentValue"],
        json!(1.0),
        "规则与动作声明同一自定义指标时，成功结果应推进目标"
    );

    let mismatched_ontology = pack_ontology.replacen(
        "goal_metric: monthly_reactivations",
        "goal_metric: awareness_contacts",
        1,
    );
    assert_ne!(mismatched_ontology, pack_ontology, "契约测试必须修改成功");
    seed_odl_project(
        &storage,
        "mismatched-goal",
        &mismatched_ontology,
        "monthly_reactivations",
    )
    .await;
    discover_opportunities(&storage, "mismatched-goal")
        .await
        .expect("指标不一致仍可发现机会");
    let opportunities = storage
        .list_objects_for_project("mismatched-goal", "Opportunity")
        .await
        .expect("应能读取机会");
    let opportunity_id = opportunities[0].id.clone();
    decide_opportunity(
        &storage,
        "mismatched-goal",
        &opportunity_id,
        "contract-test",
        DecisionOptions {
            action: Some("ContactCustomer"),
            reason: None,
            alternatives: None,
            params: None,
        },
    )
    .await
    .expect("应能决策接触动作");
    let execution = execute_opportunity(
        &storage,
        &PluginCatalog::new(),
        &WorkerRegistry::new(),
        "mismatched-goal",
        &opportunity_id,
    )
    .await
    .expect("指标不一致不阻止执行，但不应计入目标");
    assert_eq!(
        execution["goals"][0]["currentValue"],
        json!(0.0),
        "规则与动作 goal_metric 不一致时，目标进度必须保持不变"
    );
}

#[tokio::test]
async fn opportunity_cost_budget_blocks_unaffordable_actions() {
    let storage = LibSQLStorage::open(":memory:")
        .await
        .expect("内存库应可打开");
    let pack_ontology = std::fs::read_to_string("domain-packs/crm-retention/ontology.odl")
        .expect("业务包 ODL 应存在");
    let budgeted_ontology = pack_ontology
        .replace(
            "goal_metric: monthly_reactivations\n      evidence:",
            "goal_metric: monthly_reactivations\n      cost_budget: 4500\n      evidence:",
        )
        .replace("require_approval: true", "require_approval: false");
    assert_ne!(budgeted_ontology, pack_ontology, "契约测试必须修改成功");
    seed_odl_project(
        &storage,
        "budget-loop",
        &budgeted_ontology,
        "monthly_reactivations",
    )
    .await;
    discover_opportunities(&storage, "budget-loop")
        .await
        .expect("预算不阻止机会发现");
    assert!(
        recommend_next_actions(&storage, "budget-loop")
            .await
            .expect("预算应过滤不可负担候选")
            .is_empty()
    );
    assert!(
        plan_opportunity_paths(&storage, "budget-loop")
            .await
            .expect("预算应过滤不可行路径")
            .is_empty()
    );
    let run = run_next_best_action(
        &storage,
        &PluginCatalog::new(),
        &WorkerRegistry::new(),
        "budget-loop",
        "contract-test",
    )
    .await
    .expect("没有可负担动作时应空闲");
    assert!(!run.executed);

    let opportunities = storage
        .list_objects_for_project("budget-loop", "Opportunity")
        .await
        .expect("应能读取机会");
    let error = decide_opportunity(
        &storage,
        "budget-loop",
        &opportunities[0].id,
        "contract-test",
        DecisionOptions {
            action: Some("ContactCustomer"),
            reason: None,
            alternatives: None,
            params: None,
        },
    )
    .await
    .expect_err("超过预算的动作必须拒绝决策");
    assert!(error.to_string().contains("超出机会成本预算"));
}

#[tokio::test]
async fn goal_progress_uses_declared_subject_benefit_change() {
    let storage = LibSQLStorage::open(":memory:")
        .await
        .expect("内存库应可打开");
    let pack_ontology = std::fs::read_to_string("domain-packs/crm-retention/ontology.odl")
        .expect("业务包 ODL 应存在");
    let ontology = pack_ontology
        .replace(
            "    balance: float\n    days_since_last_contact: int",
            "    balance: float\n    lifetime_value: float\n    days_since_last_contact: int",
        )
        .replace(
            "    ContactCustomer:\n      planning:\n        cost: 5000",
            "    ContactCustomer:\n      planning:\n        cost: 5000\n        benefit_field: lifetime_value\n        benefit_factor: 0.5",
        )
        .replacen(
            "      updates:\n        - field: days_since_last_contact\n          value: 0",
            "      updates:\n        - field: days_since_last_contact\n          value: 0\n        - field: lifetime_value\n          value: 1500",
            1,
        );
    assert_ne!(ontology, pack_ontology, "收益契约测试必须修改成功");

    let customer_id = seed_odl_project(
        &storage,
        "quantified-goal",
        &ontology,
        "monthly_reactivations",
    )
    .await;
    let mut customer = storage
        .get(&customer_id)
        .await
        .expect("应能读取客户")
        .expect("客户应存在")
        .data;
    customer["lifetime_value"] = json!(1000.0);
    storage
        .apply_world(openaip_os::storage::WorldCommand::UpdateObject {
            id: customer_id.clone(),
            data: customer,
            project_id: Some("quantified-goal".into()),
        })
        .await
        .expect("应能写入量化收益基线");

    let discovered = discover_opportunities(&storage, "quantified-goal")
        .await
        .expect("应发现量化收益机会");
    let opportunity_id = discovered[0]["id"].as_str().expect("机会必须有 ID");
    let decision = decide_opportunity(
        &storage,
        "quantified-goal",
        opportunity_id,
        "contract-test",
        DecisionOptions {
            action: Some("ContactCustomer"),
            reason: None,
            alternatives: None,
            params: None,
        },
    )
    .await
    .expect("应能决策量化收益动作");
    assert_eq!(decision["data"]["action"], json!("ContactCustomer"));
    let execution = execute_opportunity(
        &storage,
        &PluginCatalog::new(),
        &WorkerRegistry::new(),
        "quantified-goal",
        opportunity_id,
    )
    .await
    .expect("应能执行量化收益动作");
    assert_eq!(execution["goals"][0]["currentValue"], json!(250.0));

    let outcomes = storage
        .list_objects_for_project("quantified-goal", "Outcome")
        .await
        .expect("应能读取结果账本");
    assert!(outcomes.iter().any(|outcome| {
        outcome.data["stage"] == json!("BusinessResult")
            && outcome.data["goal_delta"] == json!(250.0)
    }));
}

#[tokio::test]
async fn deferred_follow_up_evidence_is_declared_and_type_generic() {
    let storage = LibSQLStorage::open(":memory:")
        .await
        .expect("内存库应可打开");
    let pack_ontology = std::fs::read_to_string("domain-packs/crm-retention/ontology.odl")
        .expect("业务包 ODL 应存在");
    let parsed = parse_odl(&pack_ontology).expect("完整 Deferred 证据契约应可解析");
    let effect = &parsed
        .find("Customer")
        .expect("应存在 Customer")
        .actions
        .iter()
        .find(|action| action.name == "RetainCustomer")
        .expect("应存在挽留动作")
        .creates[0];
    assert_eq!(effect.subject_field.as_deref(), Some("customer"));
    assert_eq!(effect.decision_edge.as_deref(), Some("produced_task"));

    let missing = pack_ontology.replace(
        "          subject_field: customer\n          decision_edge: produced_task\n",
        "",
    );
    assert!(parse_odl(&missing).is_err(), "缺少证据链必须解析失败");
    let immediate = pack_ontology
        .replace("outcome_timing: deferred", "outcome_timing: immediate")
        .replace("RetentionTask", "FollowUpPlan");
    let error = parse_odl(&immediate).expect_err("immediate 创建不能携带终局元数据");
    assert!(error.to_string().contains("deferred 证据元数据"));

    let generic = pack_ontology
        .replace("RetentionTask", "FollowUpPlan")
        .replace(
            "          decision_edge: produced_task",
            "          decision_edge: produced_task\n          result_benefit_field: recovered_amount\n          result_benefit_factor: 0.5",
        )
        .replace(
            "    priority: string\n  actions:",
            "    priority: string\n    recovered_amount: float\n  actions:",
        )
        .replace(
            "            priority: \"MEDIUM\"",
            "            priority: \"MEDIUM\"\n            recovered_amount: 500",
        );
    assert_ne!(generic, pack_ontology, "契约测试必须修改成功");
    seed_odl_project(
        &storage,
        "generic-follow-up",
        &generic,
        "monthly_reactivations",
    )
    .await;
    let discovered = discover_opportunities(&storage, "generic-follow-up")
        .await
        .expect("应发现通用跟进机会");
    let opportunity_id = discovered[0]["id"].as_str().expect("机会必须有 ID");
    decide_opportunity(
        &storage,
        "generic-follow-up",
        opportunity_id,
        "contract-test",
        DecisionOptions {
            action: Some("RetainCustomer"),
            reason: None,
            alternatives: None,
            params: None,
        },
    )
    .await
    .expect("应能决策 Deferred 动作");
    let execution = execute_opportunity(
        &storage,
        &PluginCatalog::new(),
        &WorkerRegistry::new(),
        "generic-follow-up",
        opportunity_id,
    )
    .await
    .expect("应能执行创建效应");
    let created = &execution["createdObject"];
    assert_eq!(created["type"], json!("FollowUpPlan"));
    let task_id = created["id"].as_str().expect("跟进对象必须有 ID");

    start_follow_up(&storage, "generic-follow-up", task_id)
        .await
        .expect("应能启动任意声明的跟进对象");
    let mut running = storage
        .get(task_id)
        .await
        .expect("应能读取跟进对象")
        .expect("跟进对象应存在");
    running.data["recovered_amount"] = json!(500.0);
    storage
        .apply_world(openaip_os::storage::WorldCommand::UpdateObject {
            id: task_id.to_string(),
            data: running.data,
            project_id: Some("generic-follow-up".into()),
        })
        .await
        .expect("应能记录终局收益");
    let settled = settle_follow_up(&storage, "generic-follow-up", task_id, true)
        .await
        .expect("应能为任意声明对象记录终局");
    assert_eq!(settled["task"]["type"], json!("FollowUpPlan"));
    assert_eq!(settled["outcome"]["data"]["goal_delta"], json!(250.0));
    assert_eq!(settled["goals"][0]["currentValue"], json!(250.0));

    // 第二个同类机会必须继承终局收益学习，而不是继续使用静态期望。
    storage
        .apply_world(openaip_os::storage::WorldCommand::CreateObject {
            object_type: "Customer".into(),
            id: "customer:generic-follow-up-vip-2".into(),
            data: json!({
                "name": "第二位 VIP",
                "level": "VIP",
                "balance": 100000.0,
                "days_since_last_contact": 80
            }),
            project_id: Some("generic-follow-up".into()),
        })
        .await
        .expect("应能写入第二个客户");
    discover_opportunities(&storage, "generic-follow-up")
        .await
        .expect("应发现第二个同类机会");
    let recommendations = recommend_next_actions(&storage, "generic-follow-up")
        .await
        .expect("应能为新机会推荐动作");
    let retained = recommendations[0]
        .candidates
        .iter()
        .find(|candidate| candidate.action == "RetainCustomer")
        .expect("推荐应包含挽留候选");
    assert_eq!(retained.expected_benefit, json!(250.0));
    assert_eq!(retained.outcome_runs, 1);
    assert_eq!(retained.outcome_successes, 1);
    let reason = recommendations[0].reason.as_str();
    assert!(reason.contains("样本1成功1"), "学习证据应可解释：{reason}");
}

#[tokio::test]
async fn metric_budget_is_shared_across_opportunities() {
    let storage = LibSQLStorage::open(":memory:")
        .await
        .expect("内存库应可打开");
    let pack_ontology = std::fs::read_to_string("domain-packs/crm-retention/ontology.odl")
        .expect("业务包 ODL 应存在");
    let shared_budget = pack_ontology.replace(
        "goal_metric: monthly_reactivations\n      evidence:",
        "goal_metric: monthly_reactivations\n      metric_budget: 5000\n      evidence:",
    );
    assert_ne!(shared_budget, pack_ontology, "契约测试必须修改成功");
    seed_odl_project(
        &storage,
        "shared-budget",
        &shared_budget,
        "monthly_reactivations",
    )
    .await;
    storage
        .apply_world(openaip_os::storage::WorldCommand::CreateObject {
            object_type: "Customer".into(),
            id: "customer:shared-budget-second".into(),
            data: json!({
                "name": "第二个 VIP",
                "level": "VIP",
                "balance": 100_000.0,
                "days_since_last_contact": 75
            }),
            project_id: Some("shared-budget".into()),
        })
        .await
        .expect("应能写入第二个客户状态");

    let discovered = discover_opportunities(&storage, "shared-budget")
        .await
        .expect("应发现两个共享预算机会");
    assert_eq!(discovered.len(), 2);
    let first_run = run_next_best_action(
        &storage,
        &PluginCatalog::new(),
        &WorkerRegistry::new(),
        "shared-budget",
        "contract-test",
    )
    .await
    .expect("第一笔动作应消耗指标预算");
    let execution = first_run.execution.expect("应有执行结果");
    assert_eq!(
        execution["actionOutcome"]["data"]["cost"],
        json!(5_000.0),
        "ActionExecuted 必须记录资源消耗"
    );

    discover_opportunities(&storage, "shared-budget")
        .await
        .expect("第二个机会仍可发现");
    assert!(
        recommend_next_actions(&storage, "shared-budget")
            .await
            .expect("共享预算耗尽后不应推荐")
            .is_empty()
    );
    assert!(
        plan_opportunity_paths(&storage, "shared-budget")
            .await
            .expect("共享预算耗尽后不应规划路径")
            .is_empty()
    );
    let idle_run = run_next_best_action(
        &storage,
        &PluginCatalog::new(),
        &WorkerRegistry::new(),
        "shared-budget",
        "contract-test",
    )
    .await
    .expect("预算耗尽后自动运行应空闲");
    assert!(!idle_run.executed);

    let opportunities = storage
        .list_objects_for_project("shared-budget", "Opportunity")
        .await
        .expect("应能读取机会");
    let second = opportunities
        .iter()
        .find(|item| item.data["status"] == json!("Open"))
        .expect("第二个机会应保持 Open");
    let error = decide_opportunity(
        &storage,
        "shared-budget",
        &second.id,
        "contract-test",
        DecisionOptions {
            action: Some("ContactCustomer"),
            reason: None,
            alternatives: None,
            params: None,
        },
    )
    .await
    .expect_err("显式决策不能绕过共享预算");
    assert!(error.to_string().contains("指标剩余成本预算"));
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

    let (status, body) = common::http_call(
        addr,
        "GET",
        "/api/v1/business/opportunity-paths?projectId=crm-retention",
        None,
    )
    .await;
    assert_eq!(status, 200, "应能读取机会路径：{body}");
    let paths = data(&body).await.as_array().cloned().unwrap_or_default();
    assert_eq!(paths[0]["actions"], json!(["ContactCustomer"]));
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
        "/api/v1/business/portfolio?projectId=crm-retention",
        None,
    )
    .await;
    assert_eq!(status, 200, "应能读取业务组合：{body}");
    let portfolio = data(&body).await.as_array().cloned().unwrap_or_default();
    assert_eq!(portfolio.len(), 2, "两个主体应可组合：{portfolio:?}");
    assert!(
        portfolio
            .iter()
            .any(|path| path["subject"] == json!("customer:demo-vip-late"))
    );
    assert!(
        portfolio
            .iter()
            .any(|path| path["subject"] == json!("customer:demo-vip-rich"))
    );

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
        "GET",
        "/api/v1/business/opportunity-paths?projectId=crm-retention",
        None,
    )
    .await;
    assert_eq!(status, 200, "应能读取创建类流程路径：{body}");
    let paths = data(&body).await.as_array().cloned().unwrap_or_default();
    let rich_path = paths
        .iter()
        .find(|path| path["subject"] == json!("customer:demo-vip-rich"))
        .unwrap_or_else(|| panic!("缺少高余额客户路径：{paths:?}"));
    assert_eq!(rich_path["actions"], json!(["RetainCustomer"]));

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
    assert_eq!(alternatives.as_array().map(Vec::len), Some(3));
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
    let start_path = format!("/api/v1/business/follow-ups/{task_id}/start");
    let (status, body) = common::http_call(addr, "POST", &start_path, Some("{}")).await;
    assert_eq!(status, 200, "应能启动挽留任务：{body}");
    assert_eq!(data(&body).await["data"]["status"], json!("InProgress"));

    let retention_decision_id = result["decision"]["id"].as_str().unwrap().to_string();
    let result_path = format!("/api/v1/business/follow-ups/{task_id}/result");
    let (status, body) =
        common::http_call(addr, "POST", &result_path, Some(r#"{"success":true}"#)).await;
    assert_eq!(status, 200, "应能记录挽留终局：{body}");
    let settled = data(&body).await;
    assert_eq!(settled["task"]["data"]["status"], json!("Completed"));
    assert_eq!(settled["outcome"]["data"]["stage"], json!("BusinessResult"));
    assert_eq!(settled["outcome"]["data"]["success"], json!(true));
    assert_eq!(
        settled["goals"][0]["currentValue"],
        json!(14.0),
        "挽留成功按 ODL 声明的同一目标指标计入进度"
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
    assert_eq!(result["goals"][0]["currentValue"], json!(15.0));
    assert_eq!(result["goals"][0]["gap"], json!(5.0));

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
        &format!("/api/v1/business/follow-ups/{second_task_id}/start"),
        Some("{}"),
    )
    .await;
    let (status, body) = common::http_call(
        addr,
        "POST",
        &format!("/api/v1/business/follow-ups/{second_task_id}/result"),
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

#[tokio::test]
async fn generic_opportunity_engine_supports_non_customer_subjects() {
    let storage = LibSQLStorage::open(":memory:")
        .await
        .expect("内存库应可打开");
    let ontology = r#"
object Contract:
  properties:
    title: string
    status: string one_of [Expiring,Negotiated,Active]
    days_to_expiry: int
  opportunities:
    RenewalDue:
      subject_type: Contract
      estimated_value: 12000
      urgency_field: days_to_expiry
      urgency_horizon: 30
      link_edge: targets_contract
      goal_metric: contract_renewals
      evidence: "合同 {title} 将在 {days_to_expiry} 天内到期"
      when:
        - field: self.status
          op: eq
          value: "Expiring"
    ReadyToRenew:
      subject_type: Contract
      estimated_value: 12000
      urgency_field: days_to_expiry
      urgency_horizon: 30
      link_edge: targets_contract
      goal_metric: contract_renewals
      evidence: "合同 {title} 已完成谈判，可以签约"
      when:
        - field: self.status
          op: eq
          value: "Negotiated"
  actions:
    QuickClose:
      planning:
        cost: 4000
        prior_confidence: 0.5
        outcome_timing: immediate
        goal_metric: contract_renewals
      updates:
        - field: status
          value: "Active"
    DraftTerms:
      planning:
        cost: 500
        prior_confidence: 0.8
        outcome_timing: deferred
      updates:
        - field: status
          value: "Negotiated"
    RenewContract:
      policy:
        precondition:
          - field: self.status
            op: eq
            value: "Negotiated"
      planning:
        cost: 1000
        prior_confidence: 0.8
        outcome_timing: immediate
        goal_metric: contract_renewals
      updates:
        - field: status
          value: "Active"

object BusinessGoal:
  properties:
    target_metric: string
    baseline_value: float
    target_value: float
    current_value: float
    status: string

object Opportunity:
  properties:
    subject: string
    opportunity_type: string
    status: string
    estimated_value: float
    evidence: string

object Decision:
  properties:
    opportunity: string
    action: string
    reason: string
    alternatives: string
    decided_by: string
    status: string

object Outcome:
  properties:
    action: string
    before: string
    after: string
    success: bool
    cost: float
    metric: string
    stage: string one_of [ActionExecuted,BusinessResult]
    goal_delta: float

object StateTransition:
  properties:
    subject: string
    before_data: string
    after_data: string
    at_action: string

relation targets_contract:
  source: Opportunity
  target: Contract

relation decided_from:
  source: Decision
  target: Opportunity

relation produced_outcome:
  source: Decision
  target: Outcome

relation changed_state:
  source: StateTransition
  target: Contract
"#
    .to_owned();
    storage
        .create_project("contract-loop", "合同闭环", "非 Customer 主体", &ontology)
        .await
        .expect("应能创建项目");
    storage
        .apply_world(openaip_os::storage::WorldCommand::CreateObject {
            object_type: "Contract".into(),
            id: "contract:acme".into(),
            data: json!({
                "title": "ACME 年度合同",
                "status": "Expiring",
                "days_to_expiry": 20
            }),
            project_id: Some("contract-loop".into()),
        })
        .await
        .expect("应能写入合同状态");
    storage
        .apply_world(openaip_os::storage::WorldCommand::CreateObject {
            object_type: "BusinessGoal".into(),
            id: "business-goal:contract-loop".into(),
            data: json!({
                "target_metric": "contract_renewals",
                "baseline_value": 0.0,
                "target_value": 1.0,
                "current_value": 0.0,
                "status": "active"
            }),
            project_id: Some("contract-loop".into()),
        })
        .await
        .expect("应能写入经营目标");

    let discovered = discover_opportunities(&storage, "contract-loop")
        .await
        .expect("应能发现合同机会");
    assert_eq!(discovered.len(), 1);
    assert_eq!(discovered[0]["data"]["subject"], json!("contract:acme"));
    let _opportunity_id = discovered[0]["id"].as_str().expect("机会必须有 ID");

    let recommendations = recommend_next_actions(&storage, "contract-loop")
        .await
        .expect("应能推荐合同动作");
    assert_eq!(recommendations.len(), 1);
    assert_eq!(recommendations[0].action, "QuickClose");
    assert!(recommendations[0].reason.contains("days_to_expiry=20"));

    let paths = plan_opportunity_paths(&storage, "contract-loop")
        .await
        .expect("应能规划合同动作路径");
    assert!(paths.len() > 1);
    assert!(paths.iter().any(|path| path.actions == vec!["QuickClose"]));
    assert_eq!(paths[0].actions, vec!["DraftTerms", "RenewContract"]);
    assert!((paths[0].projected_net_value - 6180.0).abs() < 1e-9);

    let first_run = run_next_best_action(
        &storage,
        &PluginCatalog::new(),
        &WorkerRegistry::new(),
        "contract-loop",
        "contract-test",
    )
    .await
    .expect("路径规划应驱动第一轮执行");
    assert!(first_run.executed);
    assert_eq!(
        first_run.recommendation.as_ref().expect("应有推荐").action,
        "DraftTerms",
        "两步路径应优于贪心快捷关闭"
    );
    assert_eq!(
        first_run.decision.expect("应有决策")["data"]["action"],
        json!("DraftTerms")
    );
    assert_eq!(
        first_run.execution.expect("应有执行")["businessResult"],
        json!(null),
        "谈判是前置步骤，不能虚构终局结果"
    );
    let contract = storage
        .get("contract:acme")
        .await
        .expect("应能查询合同")
        .expect("合同应存在");
    assert_eq!(contract.data["status"], json!("Negotiated"));

    discover_opportunities(&storage, "contract-loop")
        .await
        .expect("状态变化后应再次发现机会");
    let recommendations = recommend_next_actions(&storage, "contract-loop")
        .await
        .expect("应能为新状态推荐动作");
    assert_eq!(recommendations.len(), 1);
    assert_eq!(recommendations[0].action, "RenewContract");
    decide_opportunity(
        &storage,
        "contract-loop",
        &recommendations[0].opportunity_id,
        "contract-test",
        DecisionOptions {
            action: Some(&recommendations[0].action),
            reason: None,
            alternatives: None,
            params: None,
        },
    )
    .await
    .expect("应能决策签约动作");
    let execution = execute_opportunity(
        &storage,
        &PluginCatalog::new(),
        &WorkerRegistry::new(),
        "contract-loop",
        &recommendations[0].opportunity_id,
    )
    .await
    .expect("应能执行签约动作");
    let contract = storage
        .get("contract:acme")
        .await
        .expect("应能查询合同")
        .expect("合同应存在");
    assert_eq!(contract.data["status"], json!("Active"));
    assert_eq!(execution["goals"][0]["currentValue"], json!(1.0));

    let budgeted = ontology.replace("cost: 4000", "cost: 6000").replace(
        "goal_metric: contract_renewals\n      evidence:",
        "goal_metric: contract_renewals\n      cost_budget: 5000\n      evidence:",
    );
    storage
        .create_project("contract-budget", "预算合同闭环", "成本约束", &budgeted)
        .await
        .expect("应能创建预算项目");
    storage
        .apply_world(openaip_os::storage::WorldCommand::CreateObject {
            object_type: "Contract".into(),
            id: "contract:beta".into(),
            data: json!({
                "title": "BETA 年度合同",
                "status": "Expiring",
                "days_to_expiry": 20
            }),
            project_id: Some("contract-budget".into()),
        })
        .await
        .expect("应能写入预算合同状态");
    storage
        .apply_world(openaip_os::storage::WorldCommand::CreateObject {
            object_type: "BusinessGoal".into(),
            id: "business-goal:contract-budget".into(),
            data: json!({
                "target_metric": "contract_renewals",
                "baseline_value": 0.0,
                "target_value": 1.0,
                "current_value": 0.0,
                "status": "active"
            }),
            project_id: Some("contract-budget".into()),
        })
        .await
        .expect("应能写入预算目标");
    discover_opportunities(&storage, "contract-budget")
        .await
        .expect("预算项目应能发现机会");
    let recommendations = recommend_next_actions(&storage, "contract-budget")
        .await
        .expect("预算项目应能推荐可负担动作");
    assert_eq!(recommendations[0].action, "DraftTerms");
    let paths = plan_opportunity_paths(&storage, "contract-budget")
        .await
        .expect("预算内组合路径应保留");
    assert_eq!(paths[0].actions, vec!["DraftTerms", "RenewContract"]);
    let run = run_next_best_action(
        &storage,
        &PluginCatalog::new(),
        &WorkerRegistry::new(),
        "contract-budget",
        "contract-test",
    )
    .await
    .expect("预算内路径应可执行");
    assert_eq!(
        run.recommendation.expect("应有推荐").action,
        "DraftTerms",
        "超过预算的快捷路径必须让位给低成本组合"
    );

    let resource_ontology = ontology
        .replace("cost: 4000", "cost: 4000\n        resource: labor=6")
        .replace("cost: 500", "cost: 500\n        resource: labor=2")
        .replace("cost: 1000", "cost: 1000\n        resource: labor=3")
        .replacen(
            "link_edge: targets_contract",
            "resource_budget: labor=5\n      link_edge: targets_contract",
            2,
        );
    storage
        .create_project(
            "contract-resource",
            "资源预算合同闭环",
            "命名资源约束",
            &resource_ontology,
        )
        .await
        .expect("应能创建资源预算项目");
    storage
        .apply_world(openaip_os::storage::WorldCommand::CreateObject {
            object_type: "Contract".into(),
            id: "contract:delta".into(),
            data: json!({
                "title": "DELTA 年度合同",
                "status": "Expiring",
                "days_to_expiry": 20
            }),
            project_id: Some("contract-resource".into()),
        })
        .await
        .expect("应能写入资源预算合同状态");
    storage
        .apply_world(openaip_os::storage::WorldCommand::CreateObject {
            object_type: "BusinessGoal".into(),
            id: "business-goal:contract-resource".into(),
            data: json!({
                "target_metric": "contract_renewals",
                "baseline_value": 0.0,
                "target_value": 1.0,
                "current_value": 0.0,
                "status": "active"
            }),
            project_id: Some("contract-resource".into()),
        })
        .await
        .expect("应能写入资源预算目标");
    let discovered = discover_opportunities(&storage, "contract-resource")
        .await
        .expect("资源预算项目应能发现机会");
    let recommendations = recommend_next_actions(&storage, "contract-resource")
        .await
        .expect("资源预算应过滤不可负担候选");
    assert_eq!(recommendations.len(), 1);
    assert_eq!(recommendations[0].action, "DraftTerms");
    let candidate = &recommendations[0].candidates[0];
    assert_eq!(candidate.resources.get("labor"), Some(&2.0));
    assert!(
        !recommendations[0]
            .candidates
            .iter()
            .any(|candidate| candidate.action == "QuickClose")
    );

    let error = decide_opportunity(
        &storage,
        "contract-resource",
        discovered[0]["id"].as_str().expect("机会必须有 ID"),
        "contract-test",
        DecisionOptions {
            action: Some("QuickClose"),
            reason: None,
            alternatives: None,
            params: None,
        },
    )
    .await
    .expect_err("超资源动作必须拒绝决策");
    assert!(error.to_string().contains("超出机会资源 labor"));

    let paths = plan_opportunity_paths(&storage, "contract-resource")
        .await
        .expect("资源内路径应保留");
    assert_eq!(paths[0].actions, vec!["DraftTerms", "RenewContract"]);
    let run = run_next_best_action(
        &storage,
        &PluginCatalog::new(),
        &WorkerRegistry::new(),
        "contract-resource",
        "contract-test",
    )
    .await
    .expect("资源预算内路径应可执行");
    assert!(run.executed);
    assert_eq!(
        run.recommendation.expect("应有推荐").action,
        "DraftTerms",
        "超资源的快捷路径必须让位给低资源组合"
    );
}

fn shared_resource_project_ontology() -> &'static str {
    r#"
object Customer:
  properties:
    name: string
    level: string
    balance: float
    days_since_last_contact: int
  opportunities:
    Reactivation:
      subject_type: Customer
      estimated_value: 10000
      urgency_field: days_since_last_contact
      urgency_horizon: 90
      link_edge: targets
      goal_metric: monthly_reactivations
      shared_resource_budget: labor=6
      shared_resource_window_days: 30
      evidence: "客户已 {days_since_last_contact} 天未联系"
      when:
        - field: self.level
          op: eq
          value: "VIP"
  actions:
    TouchCustomer:
      planning:
        cost: 100
        prior_confidence: 0.9
        outcome_timing: immediate
        goal_metric: monthly_reactivations
        resource: labor=6
      updates:
        - field: days_since_last_contact
          value: 0

object BusinessGoal:
  properties:
    target_metric: string
    baseline_value: float
    target_value: float
    current_value: float
    status: string

object Opportunity:
  properties:
    subject: string
    opportunity_type: string
    status: string
    estimated_value: float
    evidence: string

object Decision:
  properties:
    opportunity: string
    action: string
    reason: string
    alternatives: string
    decided_by: string
    status: string

object Outcome:
  properties:
    action: string
    before: string
    after: string
    success: bool
    cost: float
    metric: string
    stage: string
    goal_delta: float
    resources: string
    executed_at: datetime

object StateTransition:
  properties:
    subject: string
    before_data: string
    after_data: string
    at_action: string

relation targets:
  source: Opportunity
  target: Customer

relation decided_from:
  source: Decision
  target: Opportunity

relation produced_outcome:
  source: Decision
  target: Outcome

relation changed_state:
  source: StateTransition
  target: Customer
"#
}

#[tokio::test]
async fn shared_resource_budget_spans_projects() {
    let storage = LibSQLStorage::open(":memory:")
        .await
        .expect("内存库应可打开");
    for project_id in ["enterprise-one", "enterprise-two"] {
        seed_odl_project(
            &storage,
            project_id,
            shared_resource_project_ontology(),
            "monthly_reactivations",
        )
        .await;
    }

    let first = run_next_best_action(
        &storage,
        &PluginCatalog::new(),
        &WorkerRegistry::new(),
        "enterprise-one",
        "agent",
    )
    .await
    .expect("第一笔机会应消耗企业共享资源");
    assert!(first.executed);

    let second = run_next_best_action(
        &storage,
        &PluginCatalog::new(),
        &WorkerRegistry::new(),
        "enterprise-two",
        "agent",
    )
    .await
    .expect("第二笔机会必须看到企业共享资源账本");
    assert!(!second.executed);
    assert!(second.recommendation.is_none());

    let opportunities = storage
        .list_objects_for_project("enterprise-two", "Opportunity")
        .await
        .expect("应能查询机会");
    let open = opportunities
        .iter()
        .find(|item| item.data["status"] == json!("Open"))
        .expect("应保留未执行机会");
    let error = decide_opportunity(
        &storage,
        "enterprise-two",
        &open.id,
        "agent",
        DecisionOptions {
            action: Some("TouchCustomer"),
            reason: None,
            alternatives: None,
            params: None,
        },
    )
    .await
    .expect_err("企业共享资源耗尽后显式决策必须拒绝");
    assert!(error.to_string().contains("超出共享资源 labor"));
}

#[tokio::test]
async fn concurrent_runs_cannot_oversubscribe_enterprise_resource() {
    let storage = LibSQLStorage::open(":memory:")
        .await
        .expect("内存库应可打开");
    for project_id in ["concurrent-one", "concurrent-two"] {
        seed_odl_project(
            &storage,
            project_id,
            shared_resource_project_ontology(),
            "monthly_reactivations",
        )
        .await;
    }

    let runs = ["concurrent-one", "concurrent-two"].map(|project_id| {
        let storage = storage.clone();
        tokio::spawn(async move {
            run_next_best_action(
                &storage,
                &PluginCatalog::new(),
                &WorkerRegistry::new(),
                project_id,
                "agent",
            )
            .await
            .expect("并发闭环应返回结果")
        })
    });
    let executed = futures_util::future::join_all(runs)
        .await
        .into_iter()
        .filter(|result| result.as_ref().expect("任务不应崩溃").executed)
        .count();
    assert_eq!(executed, 1, "企业共享池只允许一笔 labor=6 执行");
}

#[tokio::test]
async fn autonomous_run_gates_actions_by_declared_actor() {
    let storage = LibSQLStorage::open(":memory:")
        .await
        .expect("内存库应可打开");
    let pack_ontology = std::fs::read_to_string("domain-packs/crm-retention/ontology.odl")
        .expect("业务包 ODL 应存在");
    let ontology = pack_ontology
        .replacen(
            "ContactCustomer:\n      planning:\n        cost: 5000",
            "ContactCustomer:\n      planning:\n        actor: human\n        cost: 100",
            1,
        )
        .replace(
            "    RetainCustomer:",
            r#"    AutomatedNudge:
      planning:
        cost: 1000
        prior_confidence: 0.5
        outcome_timing: immediate
        goal_metric: monthly_reactivations
      updates:
        - field: days_since_last_contact
          value: 1
    RetainCustomer:"#,
        );
    assert_ne!(ontology, pack_ontology, "契约测试必须修改成功");

    seed_odl_project(&storage, "actor-auto", &ontology, "monthly_reactivations").await;
    discover_opportunities(&storage, "actor-auto")
        .await
        .expect("应能发现人工动作机会");
    let recommendations = recommend_next_actions(&storage, "actor-auto")
        .await
        .expect("应能推荐声明主体的动作");
    assert_eq!(recommendations[0].action, json!("ContactCustomer"));
    assert_eq!(recommendations[0].actor, ActorKind::Human);

    let paths = plan_opportunity_paths(&storage, "actor-auto")
        .await
        .expect("应能规划声明主体的路径");
    assert_eq!(paths[0].actions, vec!["ContactCustomer"]);
    assert!(paths[0].requires_human);
    assert!(!paths.iter().all(|path| path.requires_human));

    let run = run_next_best_action(
        &storage,
        &PluginCatalog::new(),
        &WorkerRegistry::new(),
        "actor-auto",
        "contract-test",
    )
    .await
    .expect("自动运行必须能选择 Agent 动作");
    assert!(run.executed);
    assert_eq!(
        run.recommendation.expect("应有推荐").action,
        "AutomatedNudge",
        "自动执行不能选择 Human 路径"
    );

    seed_odl_project(
        &storage,
        "actor-explicit",
        &ontology,
        "monthly_reactivations",
    )
    .await;
    let discovered = discover_opportunities(&storage, "actor-explicit")
        .await
        .expect("应能发现显式人工动作机会");
    let opportunity_id = discovered[0]["id"].as_str().expect("机会必须有 ID");
    decide_opportunity(
        &storage,
        "actor-explicit",
        opportunity_id,
        "contract-test",
        DecisionOptions {
            action: Some("ContactCustomer"),
            reason: None,
            alternatives: None,
            params: None,
        },
    )
    .await
    .expect("显式决策可以选择 Human 动作");
    execute_opportunity(
        &storage,
        &PluginCatalog::new(),
        &WorkerRegistry::new(),
        "actor-explicit",
        opportunity_id,
    )
    .await
    .expect("显式执行可以执行 Human 动作");
}

#[tokio::test]
async fn explicit_decision_validates_and_executes_declared_action_params() {
    let storage = LibSQLStorage::open(":memory:")
        .await
        .expect("内存库应可打开");
    let pack_ontology = std::fs::read_to_string("domain-packs/crm-retention/ontology.odl")
        .expect("业务包 ODL 应存在");
    // 参数执行契约与审批网关相互独立；这里关闭审批只为了隔离测试参数求值。
    let ontology = pack_ontology.replace("require_approval: true", "require_approval: false");
    assert_ne!(ontology, pack_ontology, "契约测试必须修改成功");
    seed_odl_project(
        &storage,
        "action-params",
        &ontology,
        "monthly_reactivations",
    )
    .await;
    let discovered = discover_opportunities(&storage, "action-params")
        .await
        .expect("应能发现机会");
    let opportunity_id = discovered[0]["id"].as_str().expect("机会必须有 ID");

    for (params, expected) in [
        (None, "缺少参数 amount"),
        (Some(json!({"amount": "250"})), "参数 amount 类型不匹配"),
        (Some(json!({"amount": 1001})), "当前不可用"),
        (
            Some(json!({"amount": 250, "unexpected": 1})),
            "不支持参数 unexpected",
        ),
    ] {
        let error = decide_opportunity(
            &storage,
            "action-params",
            opportunity_id,
            "contract-test",
            DecisionOptions {
                action: Some("refund"),
                reason: None,
                alternatives: None,
                params,
            },
        )
        .await
        .expect_err("非法动作输入必须 fail-closed");
        assert!(
            error.to_string().contains(expected),
            "expected={expected} error={error}"
        );
    }

    decide_opportunity(
        &storage,
        "action-params",
        opportunity_id,
        "contract-test",
        DecisionOptions {
            action: Some("refund"),
            reason: None,
            alternatives: None,
            params: Some(json!({"amount": 250})),
        },
    )
    .await
    .expect("合法参数应能显式决策");
    let execution = execute_opportunity(
        &storage,
        &PluginCatalog::new(),
        &WorkerRegistry::new(),
        "action-params",
        opportunity_id,
    )
    .await
    .expect("合法参数应能执行");
    assert_eq!(execution["businessResult"]["data"]["success"], json!(true));
    let customer = storage
        .get("customer:action-params-vip")
        .await
        .expect("应能查询客户")
        .expect("客户应存在");
    assert_eq!(customer.data["balance"], json!(250));
}

#[tokio::test]
async fn require_approval_action_waits_for_separate_human_evidence() {
    let storage = LibSQLStorage::open(":memory:")
        .await
        .expect("内存库应可打开");
    let ontology = std::fs::read_to_string("domain-packs/crm-retention/ontology.odl")
        .expect("业务包 ODL 应存在");
    seed_odl_project(
        &storage,
        "approved-refund",
        &ontology,
        "monthly_reactivations",
    )
    .await;
    let discovered = discover_opportunities(&storage, "approved-refund")
        .await
        .expect("应能发现机会");
    let opportunity_id = discovered[0]["id"].as_str().expect("机会必须有 ID");
    let recommendations = recommend_next_actions(&storage, "approved-refund")
        .await
        .expect("审批动作应能进入推荐明细");
    let refund = recommendations[0]
        .candidates
        .iter()
        .find(|candidate| candidate.action == "refund")
        .expect("审批候选必须可见");
    assert!(refund.requires_approval);
    assert_eq!(refund.params.len(), 1);
    assert!(
        !plan_opportunity_paths(&storage, "approved-refund")
            .await
            .expect("路径规划应成功")
            .iter()
            .any(|path| path.actions.contains(&"refund".to_string()))
    );
    let decision = decide_opportunity(
        &storage,
        "approved-refund",
        opportunity_id,
        "contract-test",
        DecisionOptions {
            action: Some("refund"),
            reason: None,
            alternatives: None,
            params: Some(json!({"amount": 250})),
        },
    )
    .await
    .expect("通过前置条件的审批动作应能创建待审决策");
    assert_eq!(decision["data"]["status"], json!("PendingApproval"));
    assert_eq!(decision["approval"]["data"]["status"], json!("Pending"));

    let early_execute = execute_opportunity(
        &storage,
        &PluginCatalog::new(),
        &WorkerRegistry::new(),
        "approved-refund",
        opportunity_id,
    )
    .await
    .expect_err("未批准动作不能执行");
    assert!(early_execute.to_string().contains("机会缺少 Selected 决策"));

    let self_error = decide_business_approval(
        &storage,
        decision["id"].as_str().expect("决策必须有 ID"),
        true,
        "contract-test",
        Some("自批"),
    )
    .await
    .expect_err("审批人不能与决策人相同");

    assert!(self_error.to_string().contains("审批人不能与决策人相同"));

    let decision_id = decision["id"].as_str().expect("决策必须有 ID").to_owned();
    let reviewed = decide_business_approval(
        &storage,
        &decision_id,
        true,
        "human-reviewer",
        Some("金额在授权范围内"),
    )
    .await
    .expect("独立审批人应能放行");
    assert_eq!(reviewed["approval"]["data"]["status"], json!("Approved"));
    assert_eq!(reviewed["decision"]["data"]["status"], json!("Selected"));
    let execution = execute_opportunity(
        &storage,
        &PluginCatalog::new(),
        &WorkerRegistry::new(),
        "approved-refund",
        opportunity_id,
    )
    .await
    .expect("Approved 审批后应能执行");
    assert_eq!(execution["businessResult"]["data"]["success"], json!(true));

    seed_odl_project(
        &storage,
        "rejected-refund",
        &ontology,
        "monthly_reactivations",
    )
    .await;
    let discovered = discover_opportunities(&storage, "rejected-refund")
        .await
        .expect("拒绝项目应能发现机会");
    let opportunity_id = discovered[0]["id"].as_str().expect("机会必须有 ID");
    let decision = decide_opportunity(
        &storage,
        "rejected-refund",
        opportunity_id,
        "contract-test",
        DecisionOptions {
            action: Some("refund"),
            reason: None,
            alternatives: None,
            params: Some(json!({"amount": 100})),
        },
    )
    .await
    .expect("应能创建第二个待审决策");
    let reviewed = decide_business_approval(
        &storage,
        decision["id"].as_str().expect("决策必须有 ID"),
        false,
        "human-reviewer",
        Some("证据不足"),
    )
    .await
    .expect("独立审批人应能拒绝");
    assert_eq!(reviewed["approval"]["data"]["status"], json!("Rejected"));
    assert_eq!(reviewed["decision"]["data"]["status"], json!("Rejected"));
    assert_eq!(reviewed["opportunity"]["data"]["status"], json!("Open"));
}

#[tokio::test]
async fn whole_world_inconsistency_blocks_business_execution() {
    let storage = LibSQLStorage::open(":memory:")
        .await
        .expect("内存库应可打开");
    let pack_ontology = std::fs::read_to_string("domain-packs/crm-retention/ontology.odl")
        .expect("业务包 ODL 应存在");
    seed_odl_project(
        &storage,
        "reasoning",
        &pack_ontology,
        "monthly_reactivations",
    )
    .await;
    let discovered = discover_opportunities(&storage, "reasoning")
        .await
        .expect("尚未冲突的世界应能发现机会");
    let opportunity_id = discovered[0]["id"].as_str().expect("机会必须有 ID");
    decide_opportunity(
        &storage,
        "reasoning",
        opportunity_id,
        "contract-test",
        DecisionOptions {
            action: Some("ContactCustomer"),
            reason: None,
            alternatives: None,
            params: None,
        },
    )
    .await
    .expect("显式决策应能创建 Selected 决策");

    // 直接注入绕过业务门禁的非法世界行，证明执行前仍会做整状态推理。
    storage
        .apply_world(openaip_os::storage::WorldCommand::CreateObject {
            object_type: "RetentionTask".into(),
            id: "task:invalid-world".into(),
            data: json!({
                "title": "非法状态",
                "customer": "customer:reasoning-vip",
                "status": "INVALID",
                "priority": "MEDIUM"
            }),
            project_id: Some("reasoning".into()),
        })
        .await
        .expect("原始存储写入应成功以模拟外部漂移");

    let error = execute_opportunity(
        &storage,
        &PluginCatalog::new(),
        &WorkerRegistry::new(),
        "reasoning",
        opportunity_id,
    )
    .await
    .expect_err("不一致的整状态必须阻止行动执行");
    assert!(error.to_string().contains("企业世界状态不一致"));
}

#[tokio::test]
async fn autonomous_run_skips_approval_only_candidate() {
    let storage = LibSQLStorage::open(":memory:")
        .await
        .expect("内存库应可打开");
    let pack_ontology = std::fs::read_to_string("domain-packs/crm-retention/ontology.odl")
        .expect("业务包 ODL 应存在");
    let ontology = pack_ontology.replace(
        "goal_metric: monthly_reactivations\n      evidence:",
        "goal_metric: monthly_reactivations\n      cost_budget: 4999\n      evidence:",
    );
    seed_odl_project(
        &storage,
        "approval-only",
        &ontology,
        "monthly_reactivations",
    )
    .await;
    discover_opportunities(&storage, "approval-only")
        .await
        .expect("应能发现审批机会");
    let recommendations = recommend_next_actions(&storage, "approval-only")
        .await
        .expect("审批动作应能进入推荐明细");
    assert_eq!(recommendations.len(), 1);
    assert_eq!(recommendations[0].action, "refund");
    assert!(
        recommendations[0]
            .candidates
            .iter()
            .find(|candidate| candidate.action == "refund")
            .is_some_and(|candidate| candidate.requires_approval)
    );
    assert!(
        plan_opportunity_paths(&storage, "approval-only")
            .await
            .expect("路径规划应成功")
            .is_empty()
    );
    let run = run_next_best_action(
        &storage,
        &PluginCatalog::new(),
        &WorkerRegistry::new(),
        "approval-only",
        "agent",
    )
    .await
    .expect("自动运行必须安全跳过审批动作");
    assert!(!run.executed);
    assert!(run.decision.is_none());
}

fn capability_ontology(host_declaration: &str) -> String {
    r#"
object Customer:
  properties:
    name: string
    level: string
    balance: float
    days_since_last_contact: int
  opportunities:
    Reactivation:
      subject_type: Customer
      estimated_value: 10000
      urgency_field: days_since_last_contact
      urgency_horizon: 90
      link_edge: targets
      goal_metric: monthly_reactivations
      evidence: "客户已 {days_since_last_contact} 天未联系"
      when:
        - field: self.level
          op: eq
          value: "VIP"
  actions:
    TouchCustomer:
      planning:
        cost: 100
        prior_confidence: 0.9
        outcome_timing: immediate
        goal_metric: monthly_reactivations
        HOST_DECLARATION
        capability: touch_customer
      updates:
        - field: days_since_last_contact
          value: 0

object BusinessGoal:
  properties:
    target_metric: string
    baseline_value: float
    target_value: float
    current_value: float
    status: string

object Opportunity:
  properties:
    subject: string
    opportunity_type: string
    status: string
    estimated_value: float
    evidence: string

object Decision:
  properties:
    opportunity: string
    action: string
    reason: string
    alternatives: string
    decided_by: string
    status: string

object Outcome:
  properties:
    action: string
    before: string
    after: string
    success: bool
    cost: float
    metric: string
    stage: string
    goal_delta: float
    provider: string
    capability: string

object StateTransition:
  properties:
    subject: string
    before_data: string
    after_data: string
    at_action: string

relation targets:
  source: Opportunity
  target: Customer

relation decided_from:
  source: Decision
  target: Opportunity

relation produced_outcome:
  source: Decision
  target: Outcome

relation changed_state:
  source: StateTransition
  target: Customer
"#
    .replace("HOST_DECLARATION", host_declaration)
}

struct CapturingPlugin {
    manifest: openaip_os::plugin::PluginManifest,
    input: Arc<tokio::sync::Mutex<Option<Value>>>,
    output: Value,
}

#[async_trait::async_trait]
impl openaip_os::plugin::AsyncPlugin for CapturingPlugin {
    fn manifest(&self) -> &openaip_os::plugin::PluginManifest {
        &self.manifest
    }

    async fn execute_async(
        &self,
        _capability: &str,
        input: Value,
    ) -> Result<Value, openaip_os::plugin::PluginError> {
        *self.input.lock().await = Some(input);
        Ok(self.output.clone())
    }
}

struct CapturingWorker {
    manifest: openaip_os::worker_core::manifest::WorkerManifest,
    input: Arc<tokio::sync::Mutex<Option<Value>>>,
    worker_ok: bool,
    output: Value,
}

#[async_trait::async_trait]
impl openaip_os::worker_core::sdk::Worker for CapturingWorker {
    fn manifest(&self) -> &openaip_os::worker_core::manifest::WorkerManifest {
        &self.manifest
    }

    async fn execute(
        &self,
        capability: &str,
        input: Value,
    ) -> Result<openaip_os::worker_core::sdk::WorkerResult, openaip_os::worker_core::sdk::WorkerError>
    {
        let _ = capability;
        *self.input.lock().await = Some(input);
        if !self.worker_ok {
            return Err(openaip_os::worker_core::sdk::WorkerError::Execution(
                "captured failure".into(),
            ));
        }
        Ok(openaip_os::worker_core::sdk::WorkerResult {
            ok: true,
            output: self.output.clone(),
            stderr: None,
            exit_code: Some(0),
            duration_ms: 7,
        })
    }
}

#[tokio::test]
async fn declared_capability_invokes_plugin_and_projects_declared_output() {
    let ontology = capability_ontology("plugin: crm-plugin");
    let storage = LibSQLStorage::open(":memory:")
        .await
        .expect("内存库应可打开");
    let mut plugins = PluginCatalog::new();
    let captured = Arc::new(tokio::sync::Mutex::new(None));
    let mut manifest = openaip_os::plugin::PluginManifest::new(
        "crm-plugin",
        "CRM",
        "1",
        openaip_os::plugin::RuntimeType::Local,
    );
    manifest = manifest.with_capability("touch_customer");
    plugins
        .register_async(
            Box::new(CapturingPlugin {
                manifest,
                input: captured.clone(),
                output: json!({ "days_since_last_contact": 7 }),
            }),
            |capabilities| {
                capabilities.declare(openaip_os::plugin::capability::CapabilityContract {
                    capability: "touch_customer".into(),
                    input_schema: json!({}),
                    output_schema: json!({}),
                    effects: Vec::new(),
                });
            },
        )
        .expect("应能注册外部能力插件");
    let customer_id = seed_odl_project(
        &storage,
        "capability-loop",
        &ontology,
        "monthly_reactivations",
    )
    .await;
    let discovered = discover_opportunities(&storage, "capability-loop")
        .await
        .expect("应能发现机会");
    let recommendation = recommend_next_actions(&storage, "capability-loop")
        .await
        .expect("应能推荐外部能力动作");
    assert_eq!(recommendation[0].action, "TouchCustomer");
    let candidate = &recommendation[0].candidates[0];
    assert_eq!(candidate.plugin.as_deref(), Some("crm-plugin"));
    assert_eq!(candidate.capability.as_deref(), Some("touch_customer"));
    let opportunity_id = discovered[0]["id"].as_str().expect("机会必须有 ID");
    decide_opportunity(
        &storage,
        "capability-loop",
        opportunity_id,
        "contract-test",
        DecisionOptions {
            action: Some("TouchCustomer"),
            reason: None,
            alternatives: None,
            params: None,
        },
    )
    .await
    .expect("应能决策外部能力动作");

    let execution = execute_opportunity(
        &storage,
        &plugins,
        &WorkerRegistry::new(),
        "capability-loop",
        opportunity_id,
    )
    .await
    .expect("外部能力调用和状态投影应成功");
    assert_eq!(
        execution["actionOutcome"]["data"]["provider"],
        json!("crm-plugin")
    );
    assert_eq!(
        execution["actionOutcome"]["data"]["capability"],
        json!("touch_customer")
    );
    assert_eq!(
        execution["businessResult"]["data"]["provider"],
        json!("crm-plugin")
    );
    let input = captured.lock().await.clone().expect("插件应被调用");
    assert_eq!(input["subject"], json!(customer_id));
    assert_eq!(input["objectType"], json!("Customer"));
    assert_eq!(input["params"], json!({}));
    let customer = storage
        .get(&customer_id)
        .await
        .expect("查询客户失败")
        .expect("客户应存在");
    assert_eq!(
        customer.data["days_since_last_contact"],
        json!(7),
        "外部能力输出应作为声明字段的权威投影"
    );
}

#[tokio::test]
async fn capability_output_schema_blocks_invalid_plugin_result() {
    let ontology = capability_ontology("plugin: crm-plugin");
    let storage = LibSQLStorage::open(":memory:")
        .await
        .expect("内存库应可打开");
    let mut plugins = PluginCatalog::new();
    let mut manifest = openaip_os::plugin::PluginManifest::new(
        "crm-plugin",
        "CRM",
        "1",
        openaip_os::plugin::RuntimeType::Local,
    );
    manifest = manifest.with_capability("touch_customer");
    plugins
        .register_async(
            Box::new(CapturingPlugin {
                manifest,
                input: Arc::new(tokio::sync::Mutex::new(None)),
                output: json!({ "days_since_last_contact": "seven" }),
            }),
            |capabilities| {
                capabilities.declare(openaip_os::plugin::capability::CapabilityContract {
                    capability: "touch_customer".into(),
                    input_schema: json!({}),
                    output_schema: json!({
                        "type": "object",
                        "required": ["days_since_last_contact"],
                        "properties": {
                            "days_since_last_contact": { "type": "integer" }
                        }
                    }),
                    effects: Vec::new(),
                });
            },
        )
        .expect("应能注册外部能力插件");

    let customer_id = seed_odl_project(
        &storage,
        "capability-schema",
        &ontology,
        "monthly_reactivations",
    )
    .await;
    let discovered = discover_opportunities(&storage, "capability-schema")
        .await
        .expect("应能发现机会");
    let opportunity_id = discovered[0]["id"].as_str().expect("机会必须有 ID");
    decide_opportunity(
        &storage,
        "capability-schema",
        opportunity_id,
        "contract-test",
        DecisionOptions {
            action: Some("TouchCustomer"),
            reason: None,
            alternatives: None,
            params: None,
        },
    )
    .await
    .expect("应能决策外部能力动作");

    let error = execute_opportunity(
        &storage,
        &plugins,
        &WorkerRegistry::new(),
        "capability-schema",
        opportunity_id,
    )
    .await
    .expect_err("不符合输出契约的能力结果必须阻止执行");
    assert!(error.to_string().contains("输出不符合契约"));

    let customer = storage
        .get(&customer_id)
        .await
        .expect("查询客户失败")
        .expect("客户应存在");
    assert_eq!(
        customer.data["days_since_last_contact"],
        json!(75),
        "无效能力输出不得改变企业状态"
    );
}

#[tokio::test]
async fn capability_input_schema_blocks_invocation_without_side_effect() {
    let ontology = capability_ontology("plugin: crm-plugin");
    let storage = LibSQLStorage::open(":memory:")
        .await
        .expect("内存库应可打开");
    let mut plugins = PluginCatalog::new();
    let captured = Arc::new(tokio::sync::Mutex::new(None));
    let mut manifest = openaip_os::plugin::PluginManifest::new(
        "crm-plugin",
        "CRM",
        "1",
        openaip_os::plugin::RuntimeType::Local,
    );
    manifest = manifest.with_capability("touch_customer");
    plugins
        .register_async(
            Box::new(CapturingPlugin {
                manifest,
                input: captured.clone(),
                output: json!({}),
            }),
            |capabilities| {
                capabilities.declare(openaip_os::plugin::capability::CapabilityContract {
                    capability: "touch_customer".into(),
                    input_schema: json!({
                        "type": "object",
                        "required": ["operator_note"]
                    }),
                    output_schema: json!({}),
                    effects: Vec::new(),
                });
            },
        )
        .expect("应能注册外部能力插件");

    let customer_id = seed_odl_project(
        &storage,
        "capability-input",
        &ontology,
        "monthly_reactivations",
    )
    .await;
    let discovered = discover_opportunities(&storage, "capability-input")
        .await
        .expect("应能发现机会");
    let opportunity_id = discovered[0]["id"].as_str().expect("机会必须有 ID");
    decide_opportunity(
        &storage,
        "capability-input",
        opportunity_id,
        "contract-test",
        DecisionOptions {
            action: Some("TouchCustomer"),
            reason: None,
            alternatives: None,
            params: None,
        },
    )
    .await
    .expect("应能决策外部能力动作");

    let error = execute_opportunity(
        &storage,
        &plugins,
        &WorkerRegistry::new(),
        "capability-input",
        opportunity_id,
    )
    .await
    .expect_err("不符合输入契约时必须阻止外部调用");
    assert!(error.to_string().contains("输入不符合契约"));
    assert!(captured.lock().await.is_none(), "无效输入不得触达插件");

    let customer = storage
        .get(&customer_id)
        .await
        .expect("查询客户失败")
        .expect("客户应存在");
    assert_eq!(customer.data["days_since_last_contact"], json!(75));
}

#[tokio::test]
async fn declared_worker_invokes_registry_and_projects_output() {
    let ontology = capability_ontology("worker: crm-worker");
    let storage = LibSQLStorage::open(":memory:")
        .await
        .expect("内存库应可打开");
    let mut workers = WorkerRegistry::new();
    let captured = Arc::new(tokio::sync::Mutex::new(None));
    let manifest = openaip_os::worker_core::manifest::WorkerManifest::new("crm-worker", "test")
        .with_capabilities(&["touch_customer"]);
    workers
        .register(Box::new(CapturingWorker {
            manifest,
            input: captured.clone(),
            worker_ok: true,
            output: json!({ "days_since_last_contact": 7 }),
        }))
        .expect("应能注册外部能力 Worker");

    let customer_id =
        seed_odl_project(&storage, "worker-loop", &ontology, "monthly_reactivations").await;
    let discovered = discover_opportunities(&storage, "worker-loop")
        .await
        .expect("应能发现机会");
    let recommendation = recommend_next_actions(&storage, "worker-loop")
        .await
        .expect("应能推荐外部 Worker 动作");
    assert_eq!(recommendation[0].action, "TouchCustomer");
    let candidate = &recommendation[0].candidates[0];
    assert_eq!(candidate.worker.as_deref(), Some("crm-worker"));
    assert_eq!(candidate.capability.as_deref(), Some("touch_customer"));
    let opportunity_id = discovered[0]["id"].as_str().expect("机会必须有 ID");
    decide_opportunity(
        &storage,
        "worker-loop",
        opportunity_id,
        "contract-test",
        DecisionOptions {
            action: Some("TouchCustomer"),
            reason: None,
            alternatives: None,
            params: None,
        },
    )
    .await
    .expect("应能决策外部能力动作");

    let execution = execute_opportunity(
        &storage,
        &PluginCatalog::new(),
        &workers,
        "worker-loop",
        opportunity_id,
    )
    .await
    .expect("Worker 调用和状态投影应成功");
    assert_eq!(
        execution["actionOutcome"]["data"]["provider"],
        json!("crm-worker")
    );
    let input = captured.lock().await.clone().expect("Worker 应被调用");
    assert_eq!(input["subject"], json!(customer_id));
    assert_eq!(input["objectType"], json!("Customer"));
    assert_eq!(input["params"], json!({}));
    let customer = storage
        .get(&customer_id)
        .await
        .expect("查询客户失败")
        .expect("客户应存在");
    assert_eq!(
        customer.data["days_since_last_contact"],
        json!(7),
        "Worker 输出应作为声明字段的权威投影"
    );
    let stats = storage.list_worker_stats().await.expect("应能读取执行统计");
    assert_eq!(stats.len(), 1);
    assert_eq!(stats[0].capability, "touch_customer");
    assert_eq!(stats[0].worker_id, "crm-worker");
    assert_eq!(stats[0].runs, 1);
    assert_eq!(stats[0].success, 1);
}

#[tokio::test]
async fn failed_worker_is_recorded_and_blocks_state_change() {
    let ontology = capability_ontology("worker: crm-worker");
    let storage = LibSQLStorage::open(":memory:")
        .await
        .expect("内存库应可打开");
    let mut workers = WorkerRegistry::new();
    let manifest = openaip_os::worker_core::manifest::WorkerManifest::new("crm-worker", "test")
        .with_capabilities(&["touch_customer"]);
    let captured = Arc::new(tokio::sync::Mutex::new(None));
    workers
        .register(Box::new(CapturingWorker {
            manifest,
            input: captured,
            worker_ok: false,
            output: json!({}),
        }))
        .expect("应能注册失败 Worker");

    let customer_id = seed_odl_project(
        &storage,
        "worker-failure",
        &ontology,
        "monthly_reactivations",
    )
    .await;
    let discovered = discover_opportunities(&storage, "worker-failure")
        .await
        .expect("应能发现机会");
    let opportunity_id = discovered[0]["id"].as_str().expect("机会必须有 ID");
    decide_opportunity(
        &storage,
        "worker-failure",
        opportunity_id,
        "contract-test",
        DecisionOptions {
            action: Some("TouchCustomer"),
            reason: None,
            alternatives: None,
            params: None,
        },
    )
    .await
    .expect("应能决策外部能力动作");

    let error = execute_opportunity(
        &storage,
        &PluginCatalog::new(),
        &workers,
        "worker-failure",
        opportunity_id,
    )
    .await
    .expect_err("失败 Worker 必须阻止业务执行");
    assert!(error.to_string().contains("执行失败"));

    let customer = storage
        .get(&customer_id)
        .await
        .expect("查询客户失败")
        .expect("客户应存在");
    assert_eq!(customer.data["days_since_last_contact"], json!(75));
    let outcomes = storage
        .list_objects_for_project("worker-failure", "Outcome")
        .await
        .expect("应能读取 Outcome");
    assert!(outcomes.is_empty(), "失败 Worker 不能产生业务证据");

    let stats = storage.list_worker_stats().await.expect("应能读取执行统计");
    assert_eq!(stats.len(), 1);
    assert_eq!(stats[0].capability, "touch_customer");
    assert_eq!(stats[0].worker_id, "crm-worker");
    assert_eq!(stats[0].runs, 1);
    assert_eq!(stats[0].success, 0);
}
