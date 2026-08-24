//! Business Loop v4：企业状态 → 机会 → 候选动作比较 → 执行证据 → 业务结果反馈与再规划。
//!
//! 闭环对象都是普通世界对象；所有事实写入都经 `WorldCommand`，因此天然获得
//! 项目归属、事件溯源和审计链。机会规则集中在本模块，Kernel 不感知具体业务。

use crate::abi::ir::OntologyIR;
use crate::abi::{DataValidationError, parse_odl, validate_object_data};
use crate::storage::{LibSQLStorage, ObjectEngine, ProjectRecord, StorageError, WorldCommand};
use serde::Serialize;
use serde_json::{Value, json};
use thiserror::Error;

const OPPORTUNITY_OPEN: &str = "Open";
const OPPORTUNITY_DECIDED: &str = "Decided";
const OPPORTUNITY_EXECUTED: &str = "Executed";
const DECISION_SELECTED: &str = "Selected";
const DECISION_EXECUTED: &str = "Executed";
const OUTCOME_ACTION_EXECUTED: &str = "ActionExecuted";
const OUTCOME_BUSINESS_RESULT: &str = "BusinessResult";
const TASK_PENDING: &str = "Pending";
const TASK_IN_PROGRESS: &str = "InProgress";
const TASK_SETTLED_SUCCESS: &str = "Completed";
const TASK_SETTLED_FAILURE: &str = "Failed";

/// 可比较的业务动作目录。新增动作先扩展这张表，再由引擎和规划器解释执行，
/// 避免在 handler 或多个业务函数中散落 `match object_type` 规则。
#[derive(Debug, Clone, Copy)]
enum ActionEffect {
    Update { field: &'static str, value: i64 },
    CreateRetentionTask,
}

#[derive(Debug, Clone, Copy)]
struct BusinessAction {
    name: &'static str,
    prior_confidence: f64,
    cost: f64,
    effect: ActionEffect,
}

impl BusinessAction {
    fn expected_benefit(&self, estimated_value: f64, customer: &Value) -> f64 {
        match self.effect {
            ActionEffect::Update { .. } => estimated_value,
            // 挽留的价值主要来自可挽回的存量余额；低余额客户自然不适合支付挽留成本。
            ActionEffect::CreateRetentionTask => {
                customer
                    .get("balance")
                    .and_then(Value::as_f64)
                    .unwrap_or_default()
                    .max(0.0)
                    * 0.35
            }
        }
    }
}

const ACTIONS: &[BusinessAction] = &[
    BusinessAction {
        name: "ContactCustomer",
        prior_confidence: 0.7,
        cost: 5_000.0,
        effect: ActionEffect::Update {
            field: "days_since_last_contact",
            value: 0,
        },
    },
    BusinessAction {
        name: "RetainCustomer",
        prior_confidence: 0.6,
        cost: 20_000.0,
        effect: ActionEffect::CreateRetentionTask,
    },
];
const GOAL_METRIC: &str = "monthly_reactivations";

#[derive(Debug, Error)]
pub enum BusinessLoopError {
    #[error("不存在：{0}")]
    NotFound(String),
    #[error("{0}")]
    Invalid(String),
    #[error(transparent)]
    Odl(#[from] crate::abi::odl::OdlError),
    #[error("业务对象数据无效：{0}")]
    Data(#[from] DataValidationError),
    #[error(transparent)]
    Storage(#[from] StorageError),
}

impl BusinessLoopError {
    fn invalid(message: impl Into<String>) -> Self {
        Self::Invalid(message.into())
    }
}

#[derive(Debug, Serialize)]
pub struct GoalGap {
    pub id: String,
    #[serde(rename = "targetMetric")]
    pub target_metric: String,
    #[serde(rename = "targetValue")]
    pub target_value: f64,
    #[serde(rename = "currentValue")]
    pub current_value: f64,
    pub gap: f64,
    pub status: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ActionRecommendation {
    #[serde(rename = "projectId")]
    pub project_id: String,
    #[serde(rename = "opportunityId")]
    pub opportunity_id: String,
    pub subject: String,
    pub action: String,
    pub score: f64,
    pub confidence: f64,
    pub reason: String,
    pub candidates: Vec<ActionCandidate>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ActionCandidate {
    pub action: String,
    #[serde(rename = "expectedBenefit")]
    pub expected_benefit: f64,
    pub cost: f64,
    pub confidence: f64,
    #[serde(rename = "netValue")]
    pub net_value: f64,
}

#[derive(Debug, Serialize)]
pub struct BusinessLoopRun {
    pub executed: bool,
    #[serde(rename = "projectId")]
    pub project_id: String,
    pub recommendation: Option<ActionRecommendation>,
    pub decision: Option<Value>,
    pub execution: Option<Value>,
    pub goals: Vec<GoalGap>,
}

fn text<'a>(data: &'a Value, key: &str) -> Result<&'a str, BusinessLoopError> {
    data.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| BusinessLoopError::invalid(format!("对象缺少字符串字段 {key}")))
}

fn number(data: &Value, key: &str) -> Result<f64, BusinessLoopError> {
    data.get(key)
        .and_then(Value::as_f64)
        .ok_or_else(|| BusinessLoopError::invalid(format!("对象缺少数值字段 {key}")))
}

async fn project_ontology(
    storage: &LibSQLStorage,
    project_id: &str,
) -> Result<(ProjectRecord, OntologyIR), BusinessLoopError> {
    let project = storage
        .project(project_id)
        .await?
        .ok_or_else(|| BusinessLoopError::NotFound(project_id.to_string()))?;
    let ir = parse_odl(&project.ontology)?;
    Ok((project, ir))
}

fn validate_data(
    ontology: &OntologyIR,
    object_type: &str,
    data: &Value,
) -> Result<(), BusinessLoopError> {
    let definition = ontology
        .find(object_type)
        .ok_or_else(|| BusinessLoopError::invalid(format!("本体未定义对象类型 {object_type}")))?;
    validate_object_data(definition, data)?;
    Ok(())
}

fn set_status(mut data: Value, status: &str) -> Value {
    if let Some(object) = data.as_object_mut() {
        object.insert("status".into(), json!(status));
    }
    data
}

fn opportunity_id(project_id: &str, customer_id: &str) -> String {
    let key = customer_id.replace([':', '/'], "-");
    format!("opportunity:{project_id}:reactivation:{key}")
}

fn generated_id(prefix: &str) -> String {
    format!("{prefix}:{}", crate::trace::new_trace_id())
}

async fn create_world_object(
    storage: &LibSQLStorage,
    project_id: &str,
    object_type: &str,
    id: String,
    data: Value,
) -> Result<(), BusinessLoopError> {
    storage
        .apply_world(WorldCommand::CreateObject {
            object_type: object_type.to_string(),
            id: id.clone(),
            data,
            project_id: Some(project_id.to_string()),
        })
        .await?;

    Ok(())
}

async fn link_world(
    storage: &LibSQLStorage,
    project_id: &str,
    source_id: String,
    edge: &str,
    target_id: String,
) -> Result<(), BusinessLoopError> {
    storage
        .apply_world(WorldCommand::LinkRelation {
            source: source_id,
            edge: edge.to_string(),
            target: target_id,
            project_id: Some(project_id.to_string()),
        })
        .await?;
    Ok(())
}

fn is_reactivation(customer: &Value) -> bool {
    customer.get("level").and_then(Value::as_str) == Some("VIP")
        && customer
            .get("days_since_last_contact")
            .and_then(Value::as_i64)
            .is_some_and(|days| days > 45)
}

/// 扫描项目内 Customer 状态，幂等创建 Open Opportunity。
pub async fn discover_opportunities(
    storage: &LibSQLStorage,
    project_id: &str,
) -> Result<Vec<Value>, BusinessLoopError> {
    let (_, ontology) = project_ontology(storage, project_id).await?;
    validate_data(
        &ontology,
        "Opportunity",
        &json!({
            "subject": "",
            "opportunity_type": "",
            "status": "",
            "estimated_value": 0.0,
            "evidence": ""
        }),
    )?;

    let customers = storage
        .list_objects_for_project(project_id, "Customer")
        .await?;
    let opportunities = storage
        .list_objects_for_project(project_id, "Opportunity")
        .await?;
    let mut created = Vec::new();
    for customer in customers {
        if !is_reactivation(&customer.data) {
            continue;
        }
        let already_discovered = opportunities.iter().any(|existing| {
            existing.data.get("subject").and_then(Value::as_str) == Some(customer.id.as_str())
                && existing
                    .data
                    .get("opportunity_type")
                    .and_then(Value::as_str)
                    == Some("Reactivation")
        });
        if already_discovered {
            continue;
        }
        let id = opportunity_id(project_id, &customer.id);
        let days = customer
            .data
            .get("days_since_last_contact")
            .and_then(Value::as_i64)
            .unwrap_or_default();
        let data = json!({
            "subject": customer.id,
            "opportunity_type": "Reactivation",
            "status": OPPORTUNITY_OPEN,
            "estimated_value": 180_000.0,
            "evidence": format!("VIP 客户已 {days} 天未联系，超过 45 天复购预警阈值")
        });
        validate_data(&ontology, "Opportunity", &data)?;
        create_world_object(storage, project_id, "Opportunity", id.clone(), data.clone()).await?;
        link_world(
            storage,
            project_id,
            customer.id.clone(),
            "targets",
            id.clone(),
        )
        .await?;
        created.push(json!({ "id": id, "type": "Opportunity", "data": data }));
    }
    Ok(created)
}

pub async fn goal_gaps(
    storage: &LibSQLStorage,
    project_id: &str,
) -> Result<Vec<GoalGap>, BusinessLoopError> {
    let goals = storage
        .list_objects_for_project(project_id, "BusinessGoal")
        .await?;
    let mut gaps = Vec::new();
    for goal in goals {
        let target = number(&goal.data, "target_value")?;
        let current = number(&goal.data, "current_value")?;
        gaps.push(GoalGap {
            id: goal.id,
            target_metric: text(&goal.data, "target_metric")?.to_string(),
            target_value: target,
            current_value: current,
            gap: target - current,
            status: text(&goal.data, "status")?.to_string(),
        });
    }
    Ok(gaps)
}

/// 从不可变的 Outcome 证据重建目标进度，避免执行链中断后重复累加指标。
pub async fn refresh_goal_progress(
    storage: &LibSQLStorage,
    project_id: &str,
) -> Result<Vec<GoalGap>, BusinessLoopError> {
    let goals = storage
        .list_objects_for_project(project_id, "BusinessGoal")
        .await?;
    let outcomes = storage
        .list_objects_for_project(project_id, "Outcome")
        .await?;
    let completed_reactivations = outcomes
        .iter()
        .filter(|outcome| is_counted_reactivation(&outcome.data))
        .count() as f64;

    for goal in goals {
        let baseline = number(&goal.data, "baseline_value")?;
        let current = baseline + completed_reactivations;
        if number(&goal.data, "current_value")? == current {
            continue;
        }
        let mut data = goal.data.clone();
        if let Some(object) = data.as_object_mut() {
            object.insert("current_value".into(), json!(current));
        }
        storage
            .apply_world(WorldCommand::UpdateObject {
                id: goal.id.clone(),
                data,
                project_id: Some(project_id.to_string()),
            })
            .await?;
    }

    goal_gaps(storage, project_id).await
}

fn outcome_snapshot_days(outcome: &Value, field: &str) -> Option<i64> {
    let snapshot = outcome.get(field)?.as_str()?;
    let snapshot: Value = serde_json::from_str(snapshot).ok()?;
    snapshot.get("days_since_last_contact")?.as_i64()
}

fn is_counted_reactivation(outcome: &Value) -> bool {
    outcome.get("success").and_then(Value::as_bool) == Some(true)
        && outcome.get("action").and_then(Value::as_str) == Some("ContactCustomer")
        && outcome.get("stage").and_then(Value::as_str) == Some(OUTCOME_BUSINESS_RESULT)
        && outcome_snapshot_days(outcome, "before").is_some_and(|days| days > 45)
        && outcome_snapshot_days(outcome, "after") == Some(0)
}

/// 按目标相关性、状态紧急度、估计价值和历史成功率对 Open Opportunity 排序。
pub async fn recommend_next_actions(
    storage: &LibSQLStorage,
    project_id: &str,
) -> Result<Vec<ActionRecommendation>, BusinessLoopError> {
    project_ontology(storage, project_id).await?;
    let goals = storage
        .list_objects_for_project(project_id, "BusinessGoal")
        .await?;
    let opportunities = storage
        .list_objects_for_project(project_id, "Opportunity")
        .await?;
    let customers = storage
        .list_objects_for_project(project_id, "Customer")
        .await?;
    let outcomes = storage
        .list_objects_for_project(project_id, "Outcome")
        .await?;

    let active_goal = goals.iter().find(|goal| {
        text(&goal.data, "target_metric").is_ok_and(|metric| metric == GOAL_METRIC)
            && text(&goal.data, "status").is_ok_and(|status| status == "active")
    });
    let remaining_gap = match active_goal {
        Some(goal) => {
            (number(&goal.data, "target_value")? - number(&goal.data, "current_value")?).max(0.0)
        }
        None => 0.0,
    };

    let mut recommendations = Vec::new();
    for opportunity in opportunities {
        if text(&opportunity.data, "status")? != OPPORTUNITY_OPEN {
            continue;
        }
        let subject = text(&opportunity.data, "subject")?.to_string();
        let customer = find_record(&customers, &subject, "Customer")
            .ok_or_else(|| BusinessLoopError::NotFound(subject.clone()))?;
        let days = customer
            .data
            .get("days_since_last_contact")
            .and_then(Value::as_i64)
            .unwrap_or_default();
        let estimated_value = number(&opportunity.data, "estimated_value")?;
        let opportunity_type = text(&opportunity.data, "opportunity_type")?.to_string();

        let mut candidates = evaluate_candidates(&customer.data, estimated_value, &outcomes);
        let urgency = (days.min(90) as f64 / 90.0).max(0.1);
        candidates.sort_by(|left, right| {
            right
                .net_value
                .total_cmp(&left.net_value)
                .then_with(|| left.action.cmp(&right.action))
        });
        let selected = &candidates[0];
        let score = selected.net_value
            * goal_alignment(
                selected.action.as_str(),
                opportunity_type.as_str(),
                remaining_gap,
            )
            * (0.6 + 0.4 * urgency);
        let reason = if remaining_gap > 0.0 {
            format!(
                "{GOAL_METRIC} 还差 {remaining_gap:.0}；客户已 {days} 天未联系；{}",
                candidate_reason(candidates.as_slice())
            )
        } else {
            format!(
                "客户已 {days} 天未联系；{}",
                candidate_reason(candidates.as_slice())
            )
        };
        recommendations.push(ActionRecommendation {
            project_id: project_id.to_string(),
            opportunity_id: opportunity.id.clone(),
            subject,
            action: selected.action.clone(),
            score,
            confidence: selected.confidence,
            reason,
            candidates,
        });
    }

    recommendations.sort_by(|left, right| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| left.opportunity_id.cmp(&right.opportunity_id))
    });
    Ok(recommendations)
}

fn action_confidence(action: &str, outcomes: &[crate::storage::ObjectRecord]) -> f64 {
    let definition = ACTIONS
        .iter()
        .find(|candidate| candidate.name == action)
        .expect("注册表中的动作必然存在");
    let relevant = outcomes
        .iter()
        .filter(|item| {
            text(&item.data, "action").is_ok_and(|name| name == action)
                && text(&item.data, "stage").is_ok_and(|stage| stage == OUTCOME_BUSINESS_RESULT)
        })
        .collect::<Vec<_>>();
    if relevant.is_empty() {
        return definition.prior_confidence;
    }

    let success = relevant
        .iter()
        .filter(|item| item.data.get("success").and_then(Value::as_bool) == Some(true))
        .count();
    success as f64 / relevant.len() as f64
}

fn evaluate_candidates(
    customer: &Value,
    estimated_value: f64,
    outcomes: &[crate::storage::ObjectRecord],
) -> Vec<ActionCandidate> {
    ACTIONS
        .iter()
        .map(|definition| {
            let confidence = action_confidence(definition.name, outcomes);
            let expected_benefit = definition.expected_benefit(estimated_value, customer);
            ActionCandidate {
                action: definition.name.to_string(),
                expected_benefit,
                cost: definition.cost,
                confidence,
                net_value: expected_benefit * confidence - definition.cost,
            }
        })
        .collect()
}

fn goal_alignment(action: &str, opportunity_type: &str, remaining_gap: f64) -> f64 {
    if remaining_gap > 0.0 && opportunity_type == "Reactivation" && action == "ContactCustomer" {
        return 1.0;
    }
    if remaining_gap > 0.0 && opportunity_type == "Reactivation" {
        return 0.85;
    }
    0.5
}

fn candidate_reason(candidates: &[ActionCandidate]) -> String {
    let parts = candidates.iter().map(|candidate| {
        format!(
            "{} 净价值 {:.0}（置信度 {:.2}）",
            candidate.action, candidate.net_value, candidate.confidence
        )
    });
    format!("候选比较：{}", parts.collect::<Vec<_>>().join("；"))
}

fn find_record<'a>(
    records: &'a [crate::storage::ObjectRecord],
    id: &str,
    object_type: &str,
) -> Option<&'a crate::storage::ObjectRecord> {
    records
        .iter()
        .find(|record| record.id == id && record.r#type == object_type)
}

/// 为一个 Opportunity 建立显式 Decision；不直接执行动作。
pub async fn decide_opportunity(
    storage: &LibSQLStorage,
    project_id: &str,
    opportunity_id: &str,
    requested_action: Option<&str>,
    decided_by: &str,
    reason: Option<&str>,
    alternatives: Option<Value>,
) -> Result<Value, BusinessLoopError> {
    let default_action = ACTIONS[0].name;
    let action = requested_action.unwrap_or(default_action);
    if !ACTIONS.iter().any(|definition| definition.name == action) {
        return Err(BusinessLoopError::invalid(format!(
            "不支持的业务动作 {action}"
        )));
    }
    let (_, ontology) = project_ontology(storage, project_id).await?;
    let opportunities = storage
        .list_objects_for_project(project_id, "Opportunity")
        .await?;
    let opportunity = find_record(&opportunities, opportunity_id, "Opportunity")
        .ok_or_else(|| BusinessLoopError::NotFound(opportunity_id.to_string()))?;
    if text(&opportunity.data, "status")? != OPPORTUNITY_OPEN {
        return Err(BusinessLoopError::invalid("只有 Open 状态的机会可以决策"));
    }

    let subject = text(&opportunity.data, "subject")?.to_string();
    let customers = storage
        .list_objects_for_project(project_id, "Customer")
        .await?;
    if find_record(&customers, &subject, "Customer").is_none() {
        return Err(BusinessLoopError::NotFound(subject));
    }

    let decision_id = generated_id("decision");
    let alternatives = alternatives
        .map(|value| serde_json::to_string(&value))
        .transpose()
        .map_err(|error| BusinessLoopError::invalid(error.to_string()))?;
    let decision_data = json!({
        "opportunity": opportunity.id,
        "action": action,
        "reason": reason.unwrap_or("选择客户接触动作以关闭复购风险差距"),
        "alternatives": alternatives.unwrap_or_else(|| json!([]).to_string()),
        "decided_by": decided_by,
        "status": DECISION_SELECTED
    });
    validate_data(&ontology, "Decision", &decision_data)?;
    create_world_object(
        storage,
        project_id,
        "Decision",
        decision_id.clone(),
        decision_data.clone(),
    )
    .await?;
    link_world(
        storage,
        project_id,
        decision_id.clone(),
        "decided_from",
        opportunity.id.clone(),
    )
    .await?;
    storage
        .apply_world(WorldCommand::UpdateObject {
            id: opportunity.id.clone(),
            data: set_status(opportunity.data.clone(), OPPORTUNITY_DECIDED),
            project_id: Some(project_id.to_string()),
        })
        .await?;

    Ok(json!({
        "id": decision_id,
        "type": "Decision",
        "data": decision_data
    }))
}

/// 执行已选动作，记录 Outcome 与 StateTransition，并推进 Opportunity/Decision 状态。
pub async fn execute_opportunity(
    storage: &LibSQLStorage,
    project_id: &str,
    opportunity_id: &str,
) -> Result<Value, BusinessLoopError> {
    let (_, ontology) = project_ontology(storage, project_id).await?;
    let opportunities = storage
        .list_objects_for_project(project_id, "Opportunity")
        .await?;
    let opportunity = find_record(&opportunities, opportunity_id, "Opportunity")
        .ok_or_else(|| BusinessLoopError::NotFound(opportunity_id.to_string()))?
        .clone();
    if text(&opportunity.data, "status")? != OPPORTUNITY_DECIDED {
        return Err(BusinessLoopError::invalid(
            "只有 Decided 状态的机会可以执行",
        ));
    }

    let decisions = storage
        .list_objects_for_project(project_id, "Decision")
        .await?;
    let decision = decisions
        .iter()
        .filter(|item| {
            item.data.get("opportunity").and_then(Value::as_str) == Some(opportunity.id.as_str())
                && item.data.get("status").and_then(Value::as_str) == Some(DECISION_SELECTED)
        })
        .max_by(|a, b| a.id.cmp(&b.id))
        .ok_or_else(|| BusinessLoopError::NotFound("机会缺少 Selected 决策".into()))?
        .clone();
    let action = text(&decision.data, "action")?.to_string();
    let definition = ACTIONS
        .iter()
        .find(|candidate| candidate.name == action)
        .ok_or_else(|| BusinessLoopError::invalid(format!("不支持的业务动作 {action}")))?;

    let subject = text(&opportunity.data, "subject")?.to_string();
    let before_record = storage
        .get(&subject)
        .await?
        .ok_or_else(|| BusinessLoopError::NotFound(subject.clone()))?;
    let mut after_data = before_record.data.clone();
    let mut retention_task_id = None;
    match definition.effect {
        ActionEffect::Update { field, value } => {
            let after_object = after_data
                .as_object_mut()
                .ok_or_else(|| BusinessLoopError::invalid("Customer 数据必须是 JSON object"))?;
            after_object.insert(field.to_string(), json!(value));
        }
        ActionEffect::CreateRetentionTask => {
            let task_id = generated_id("retention-task");
            let task_data = json!({
                "title": "客户挽留计划",
                "customer": subject,
                "status": "Pending",
                "priority": "HIGH"
            });
            validate_data(&ontology, "RetentionTask", &task_data)?;
            create_world_object(
                storage,
                project_id,
                "RetentionTask",
                task_id.clone(),
                task_data,
            )
            .await?;
            link_world(
                storage,
                project_id,
                subject.clone(),
                "needs_retention",
                task_id.clone(),
            )
            .await?;
            link_world(
                storage,
                project_id,
                decision.id.clone(),
                "produced_task",
                task_id.clone(),
            )
            .await?;
            retention_task_id = Some(task_id);
        }
    }

    let before = serde_json::to_string(&before_record.data)
        .map_err(|error| BusinessLoopError::invalid(error.to_string()))?;
    let after = serde_json::to_string(&after_data)
        .map_err(|error| BusinessLoopError::invalid(error.to_string()))?;
    let action_outcome_id = generated_id("outcome");
    let transition_id = generated_id("state-transition");
    let mut action_outcome_data = json!({
        "action": action,
        "before": before,
        "after": after,
        "success": true,
        "stage": OUTCOME_ACTION_EXECUTED,
    });
    if let Some(task_id) = &retention_task_id {
        action_outcome_data["before"] = json!(r#"{"RetentionTask":null}"#);
        action_outcome_data["after"] = json!(format!(r#"{{"RetentionTask":"{task_id}"}}"#));
    }
    let transition_data = json!({
        "subject": subject,
        "before_data": before,
        "after_data": after,
        "at_action": action
    });
    validate_data(&ontology, "Outcome", &action_outcome_data)?;
    validate_data(&ontology, "StateTransition", &transition_data)?;

    // 先提交事实变更，再写闭环证据和状态推进；任一后续命令失败时审计链能定位中断点。
    if matches!(definition.effect, ActionEffect::Update { .. }) {
        storage
            .apply_world(WorldCommand::UpdateObject {
                id: subject.clone(),
                data: after_data.clone(),
                project_id: Some(project_id.to_string()),
            })
            .await?;
    }

    create_world_object(
        storage,
        project_id,
        "Outcome",
        action_outcome_id.clone(),
        action_outcome_data.clone(),
    )
    .await?;
    link_world(
        storage,
        project_id,
        decision.id.clone(),
        "produced_outcome",
        action_outcome_id.clone(),
    )
    .await?;
    let business_result = if action == "ContactCustomer" {
        let result_id = generated_id("outcome");
        let result_data = json!({
            "action": action,
            "before": before,
            "after": after,
            "success": true,
            "stage": OUTCOME_BUSINESS_RESULT,
        });
        validate_data(&ontology, "Outcome", &result_data)?;
        create_world_object(
            storage,
            project_id,
            "Outcome",
            result_id.clone(),
            result_data.clone(),
        )
        .await?;
        link_world(
            storage,
            project_id,
            decision.id.clone(),
            "produced_outcome",
            result_id.clone(),
        )
        .await?;
        Some(json!({ "id": result_id, "data": result_data }))
    } else {
        None
    };
    create_world_object(
        storage,
        project_id,
        "StateTransition",
        transition_id.clone(),
        transition_data.clone(),
    )
    .await?;
    link_world(
        storage,
        project_id,
        transition_id.clone(),
        "changed_state",
        subject.clone(),
    )
    .await?;
    for (id, status) in [
        (decision.id.clone(), DECISION_EXECUTED),
        (opportunity.id.clone(), OPPORTUNITY_EXECUTED),
    ] {
        let records = storage
            .list_objects_for_project(
                project_id,
                if id == decision.id {
                    "Decision"
                } else {
                    "Opportunity"
                },
            )
            .await?;
        let record = find_record(
            &records,
            &id,
            if id == decision.id {
                "Decision"
            } else {
                "Opportunity"
            },
        )
        .ok_or_else(|| BusinessLoopError::NotFound(id.clone()))?;
        storage
            .apply_world(WorldCommand::UpdateObject {
                id,
                data: set_status(record.data.clone(), status),
                project_id: Some(project_id.to_string()),
            })
            .await?;
    }

    let goals = refresh_goal_progress(storage, project_id).await?;

    let mut result = json!({
        "opportunity": { "id": opportunity.id, "data": set_status(opportunity.data, OPPORTUNITY_EXECUTED) },
        "decision": { "id": decision.id, "data": set_status(decision.data, DECISION_EXECUTED) },
        "actionOutcome": { "id": action_outcome_id, "data": action_outcome_data },
        "businessResult": business_result,
        "stateTransition": { "id": transition_id, "data": transition_data },
        "goals": goals,
    });
    if let Some(task_id) = retention_task_id {
        result["createdObject"] = json!({ "id": task_id, "type": "RetentionTask" });
    }
    Ok(result)
}

async fn retention_task(
    storage: &LibSQLStorage,
    project_id: &str,
    task_id: &str,
) -> Result<crate::storage::ObjectRecord, BusinessLoopError> {
    let tasks = storage
        .list_objects_for_project(project_id, "RetentionTask")
        .await?;
    find_record(&tasks, task_id, "RetentionTask")
        .ok_or_else(|| BusinessLoopError::NotFound(task_id.to_string()))
        .cloned()
}

/// 启动挽留任务。业务结果必须等到终局状态，执行动作本身不等于客户被留住。
pub async fn start_retention_task(
    storage: &LibSQLStorage,
    project_id: &str,
    task_id: &str,
) -> Result<Value, BusinessLoopError> {
    let (_, ontology) = project_ontology(storage, project_id).await?;
    let mut task = retention_task(storage, project_id, task_id).await?;
    if text(&task.data, "status")? != TASK_PENDING {
        return Err(BusinessLoopError::invalid(
            "只有 Pending 状态的挽留任务可以启动",
        ));
    }

    task.data = set_status(task.data.clone(), TASK_IN_PROGRESS);
    validate_data(&ontology, "RetentionTask", &task.data)?;
    storage
        .apply_world(WorldCommand::UpdateObject {
            id: task.id.clone(),
            data: task.data.clone(),
            project_id: Some(project_id.to_string()),
        })
        .await?;

    Ok(json!({ "id": task.id, "type": task.r#type, "data": task.data }))
}

/// 记录挽留终局。成功与失败都会成为 BusinessResult，避免只拿成功样本学习。
pub async fn settle_retention_task(
    storage: &LibSQLStorage,
    project_id: &str,
    task_id: &str,
    success: bool,
) -> Result<Value, BusinessLoopError> {
    let (_, ontology) = project_ontology(storage, project_id).await?;
    let task = retention_task(storage, project_id, task_id).await?;
    if text(&task.data, "status")? != TASK_IN_PROGRESS {
        return Err(BusinessLoopError::invalid(
            "只有 InProgress 状态的挽留任务可以记录终局",
        ));
    }

    let relations = storage.list_all_relations().await?;
    let subject = relations
        .iter()
        .find(|relation| relation.relation == "needs_retention" && relation.target == task.id)
        .map(|relation| relation.source.clone())
        .ok_or_else(|| BusinessLoopError::invalid("挽留任务缺少 Customer 关系"))?;
    if text(&task.data, "customer")? != subject {
        return Err(BusinessLoopError::invalid(
            "挽留任务的 Customer 关系与任务数据不一致",
        ));
    }
    let decision_links = relations
        .iter()
        .filter(|relation| relation.relation == "produced_task" && relation.target == task.id)
        .map(|relation| relation.source.as_str())
        .collect::<Vec<_>>();
    let decisions = storage
        .list_objects_for_project(project_id, "Decision")
        .await?;
    let decision = decisions
        .iter()
        .filter(|item| {
            decision_links.contains(&item.id.as_str())
                && text(&item.data, "action").is_ok_and(|action| action == "RetainCustomer")
        })
        .max_by(|left, right| left.id.cmp(&right.id))
        .ok_or_else(|| BusinessLoopError::invalid("挽留任务缺少 RetainCustomer 决策"))?
        .clone();

    let settled_status = if success {
        TASK_SETTLED_SUCCESS
    } else {
        TASK_SETTLED_FAILURE
    };
    let before = serde_json::to_string(&task.data)
        .map_err(|error| BusinessLoopError::invalid(error.to_string()))?;
    let settled_data = set_status(task.data.clone(), settled_status);
    validate_data(&ontology, "RetentionTask", &settled_data)?;
    let after = serde_json::to_string(&settled_data)
        .map_err(|error| BusinessLoopError::invalid(error.to_string()))?;
    storage
        .apply_world(WorldCommand::UpdateObject {
            id: task.id.clone(),
            data: settled_data.clone(),
            project_id: Some(project_id.to_string()),
        })
        .await?;

    let outcome_id = generated_id("outcome");
    let outcome_data = json!({
        "action": "RetainCustomer",
        "before": before,
        "after": after,
        "success": success,
        "stage": OUTCOME_BUSINESS_RESULT,
    });
    validate_data(&ontology, "Outcome", &outcome_data)?;
    create_world_object(
        storage,
        project_id,
        "Outcome",
        outcome_id.clone(),
        outcome_data.clone(),
    )
    .await?;
    link_world(
        storage,
        project_id,
        decision.id.clone(),
        "produced_outcome",
        outcome_id.clone(),
    )
    .await?;
    let goals = refresh_goal_progress(storage, project_id).await?;

    Ok(json!({
        "task": { "id": task.id, "data": settled_data },
        "decision": { "id": decision.id, "data": decision.data },
        "outcome": { "id": outcome_id, "data": outcome_data },
        "goals": goals,
    }))
}

/// 执行一轮完整再规划：发现 → 选择最优动作 → 决策 → 执行 → 目标反馈。
pub async fn run_next_best_action(
    storage: &LibSQLStorage,
    project_id: &str,
    decided_by: &str,
) -> Result<BusinessLoopRun, BusinessLoopError> {
    discover_opportunities(storage, project_id).await?;
    let goals = refresh_goal_progress(storage, project_id).await?;
    let recommendation = recommend_next_actions(storage, project_id)
        .await?
        .into_iter()
        .next();

    let Some(recommendation) = recommendation else {
        return Ok(BusinessLoopRun {
            executed: false,
            project_id: project_id.to_string(),
            recommendation: None,
            decision: None,
            execution: None,
            goals,
        });
    };

    let decision = decide_opportunity(
        storage,
        project_id,
        &recommendation.opportunity_id,
        Some(&recommendation.action),
        decided_by,
        Some(&recommendation.reason),
        serde_json::to_value(recommendation.candidates.clone()).ok(),
    )
    .await?;
    let execution =
        execute_opportunity(storage, project_id, &recommendation.opportunity_id).await?;
    let goals = refresh_goal_progress(storage, project_id).await?;

    Ok(BusinessLoopRun {
        executed: true,
        project_id: project_id.to_string(),
        recommendation: Some(recommendation),
        decision: Some(decision),
        execution: Some(execution),
        goals,
    })
}
