//! Business Loop v4：企业状态 → 机会 → 候选动作比较 → 执行证据 → 业务结果反馈与再规划。
//!
//! 闭环对象都是普通世界对象；所有事实写入都经 `WorldCommand`，因此天然获得
//! 项目归属、事件溯源和审计链。机会规则集中在本模块，Kernel 不感知具体业务。

use crate::abi::ir::{
    ActionDef, ActionPlanning, CompareOp, OntologyIR, OpportunityRule, OutcomeTiming, ValueExpr,
};
use crate::abi::{DataValidationError, parse_odl, validate_object_data};
use crate::storage::{LibSQLStorage, ProjectRecord, StorageError, WorldCommand};
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
const PATH_MAX_STEPS: usize = 3;

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

#[derive(Debug, Clone, Serialize)]
pub struct OpportunityPath {
    #[serde(rename = "projectId")]
    pub project_id: String,
    #[serde(rename = "opportunityId")]
    pub opportunity_id: String,
    pub subject: String,
    pub actions: Vec<String>,
    #[serde(rename = "projectedNetValue")]
    pub projected_net_value: f64,
    pub confidence: f64,
    pub score: f64,
    pub reason: String,
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

/// 从项目当前 ODL 收集参与商业规划的动作；动作目录不再属于 Rust。
fn planning_actions_for<'a>(
    ontology: &'a OntologyIR,
    subject_type: &str,
) -> Result<Vec<&'a ActionDef>, BusinessLoopError> {
    let definition = ontology.find(subject_type).ok_or_else(|| {
        BusinessLoopError::invalid(format!("本体未定义业务规划对象类型 {subject_type}"))
    })?;
    let actions = definition
        .actions
        .iter()
        .filter(|action| action.planning.is_some())
        .collect::<Vec<_>>();
    if actions.is_empty() {
        return Err(BusinessLoopError::invalid(format!(
            "{subject_type} 本体未声明可规划的 action"
        )));
    }
    Ok(actions)
}

fn find_action<'a>(actions: &'a [&'a ActionDef], name: &str) -> Option<&'a ActionDef> {
    actions.iter().copied().find(|action| action.name == name)
}

fn action_planning(action: &ActionDef) -> Result<&ActionPlanning, BusinessLoopError> {
    action.planning.as_ref().ok_or_else(|| {
        BusinessLoopError::invalid(format!("业务动作 {} 缺少 planning 元数据", action.name))
    })
}

fn all_planning_actions(ontology: &OntologyIR) -> Vec<&ActionDef> {
    ontology
        .object_types
        .iter()
        .flat_map(|object| object.actions.iter())
        .filter(|action| action.planning.is_some())
        .collect()
}

fn expected_benefit(
    action: &ActionDef,
    estimated_value: f64,
    subject: &Value,
) -> Result<f64, BusinessLoopError> {
    let planning = action_planning(action)?;
    let Some((field, factor)) = planning
        .benefit_field
        .as_deref()
        .zip(planning.benefit_factor)
    else {
        if planning.outcome_timing != OutcomeTiming::Immediate {
            return Ok(0.0);
        }
        return Ok(estimated_value);
    };
    Ok(number(subject, field)?.max(0.0) * factor)
}

fn action_available(action: &ActionDef, subject: &Value) -> Result<bool, BusinessLoopError> {
    let Some(_) = action.planning.as_ref() else {
        return Ok(false);
    };
    if action.policy.require_approval || !action.params.is_empty() {
        return Ok(false);
    }
    path_precondition_satisfied(action, subject)
}

fn evaluate_value(
    expression: &ValueExpr,
    subject: &str,
    subject_data: &Value,
) -> Result<Value, BusinessLoopError> {
    match expression {
        ValueExpr::Literal(value) => Ok(value.clone()),
        ValueExpr::SelfId => Ok(json!(subject)),
        ValueExpr::SelfField(field) => Ok(subject_data.get(field).cloned().unwrap_or(Value::Null)),
        ValueExpr::Param(name) => Err(BusinessLoopError::invalid(format!(
            "Business Loop 尚未提供动作参数 {name}"
        ))),
    }
}

fn set_status(mut data: Value, status: &str) -> Value {
    if let Some(object) = data.as_object_mut() {
        object.insert("status".into(), json!(status));
    }
    data
}

fn opportunity_id(project_id: &str, opportunity_type: &str, customer_id: &str) -> String {
    let key = customer_id.replace([':', '/'], "-");
    let rule = opportunity_type.to_lowercase().replace([':', '/'], "-");
    format!("opportunity:{project_id}:{rule}:{key}")
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

type SubjectRule<'a> = (&'a crate::abi::ir::ObjectType, &'a OpportunityRule);

fn subject_rule<'a>(
    ontology: &'a OntologyIR,
    name: &str,
) -> Result<SubjectRule<'a>, BusinessLoopError> {
    ontology
        .object_types
        .iter()
        .find_map(|object| {
            object
                .opportunity_rules
                .iter()
                .find(|rule| rule.name == name)
                .map(|rule| (object, rule))
        })
        .ok_or_else(|| BusinessLoopError::NotFound(format!("opportunity 规则 {name}")))
}

fn action_goal_metric(action: &ActionDef) -> Option<String> {
    action
        .planning
        .as_ref()
        .and_then(|planning| planning.goal_metric.clone())
}

fn shared_goal_metric(
    ontology: &OntologyIR,
    opportunity_type: &str,
    action: &ActionDef,
) -> Option<String> {
    let opportunity_metric = subject_rule(ontology, opportunity_type)
        .ok()
        .and_then(|(_, rule)| rule.goal_metric.clone());
    let action_metric = action_goal_metric(action);
    if opportunity_metric.is_some() && opportunity_metric == action_metric {
        return opportunity_metric;
    }
    None
}

fn condition_literal(expression: &ValueExpr) -> Result<&Value, BusinessLoopError> {
    match expression {
        ValueExpr::Literal(value) => Ok(value),
        _ => Err(BusinessLoopError::invalid("机会条件值必须是字面量")),
    }
}

fn compare_values(left: &Value, op: CompareOp, right: &Value) -> Result<bool, BusinessLoopError> {
    let invalid = || BusinessLoopError::invalid(format!("机会条件不能比较 {left} 和 {right}"));
    let ordered = |ordering: Option<std::cmp::Ordering>| {
        ordering.ok_or_else(invalid).map(|ordering| match op {
            CompareOp::Lt => ordering.is_lt(),
            CompareOp::Le => ordering.is_le(),
            CompareOp::Gt => ordering.is_gt(),
            CompareOp::Ge => ordering.is_ge(),
            CompareOp::Eq | CompareOp::Ne => false,
        })
    };
    let equality = |equal: bool| match op {
        CompareOp::Eq => Ok(equal),
        CompareOp::Ne => Ok(!equal),
        _ => Err(invalid()),
    };
    if let (Some(left), Some(right)) = (left.as_f64(), right.as_f64()) {
        if matches!(op, CompareOp::Eq | CompareOp::Ne) {
            return equality((left - right).abs() < f64::EPSILON);
        }
        return ordered(left.partial_cmp(&right));
    }
    if let (Some(left), Some(right)) = (left.as_str(), right.as_str()) {
        if matches!(op, CompareOp::Eq | CompareOp::Ne) {
            return equality(left == right);
        }
        return ordered(Some(left.cmp(right)));
    }
    if left.is_boolean() && right.is_boolean() {
        return equality(left == right);
    }
    if left.is_null() && right.is_null() {
        return equality(true);
    }
    Err(invalid())
}

fn rule_matches(rule: &OpportunityRule, customer: &Value) -> Result<bool, BusinessLoopError> {
    for clause in &rule.when {
        let field = clause.field.strip_prefix("self.").ok_or_else(|| {
            BusinessLoopError::invalid(format!(
                "机会条件字段必须引用 self.<property>：{}",
                clause.field
            ))
        })?;
        let Some(left) = customer.get(field) else {
            return Ok(false);
        };
        let matched = compare_values(left, clause.op, condition_literal(&clause.value)?)?;
        if !matched {
            return Ok(false);
        }
    }
    Ok(true)
}

fn evidence_text(template: &str, owner: &crate::abi::ir::ObjectType, customer: &Value) -> String {
    let mut text = template.to_owned();
    for property in &owner.properties {
        if let Some(value) = customer.get(&property.name) {
            text = text.replace(
                format!("{{{}}}", property.name).as_str(),
                value.as_str().unwrap_or(&value.to_string()),
            );
        }
    }
    text
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

    let opportunities = storage
        .list_objects_for_project(project_id, "Opportunity")
        .await?;
    let mut created = Vec::new();
    for owner in &ontology.object_types {
        for rule in &owner.opportunity_rules {
            let subjects = storage
                .list_objects_for_project(project_id, &owner.name)
                .await?;
            for subject in subjects {
                if !rule_matches(rule, &subject.data)? {
                    continue;
                }
                let already_discovered = opportunities.iter().any(|existing| {
                    existing.data.get("subject").and_then(Value::as_str)
                        == Some(subject.id.as_str())
                        && existing
                            .data
                            .get("opportunity_type")
                            .and_then(Value::as_str)
                            == Some(rule.name.as_str())
                });
                if already_discovered {
                    continue;
                }
                let id = opportunity_id(project_id, &rule.name, &subject.id);
                let data = json!({
                    "subject": subject.id,
                    "opportunity_type": rule.name,
                    "status": OPPORTUNITY_OPEN,
                    "estimated_value": rule.estimated_value,
                    "evidence": evidence_text(&rule.evidence, owner, &subject.data),
                });
                validate_data(&ontology, "Opportunity", &data)?;
                create_world_object(storage, project_id, "Opportunity", id.clone(), data.clone())
                    .await?;
                link_world(
                    storage,
                    project_id,
                    subject.id.clone(),
                    &rule.link_edge,
                    id.clone(),
                )
                .await?;
                created.push(json!({ "id": id, "type": "Opportunity", "data": data }));
            }
        }
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
    let (_, ontology) = project_ontology(storage, project_id).await?;
    let actions = all_planning_actions(&ontology);
    let goals = storage
        .list_objects_for_project(project_id, "BusinessGoal")
        .await?;
    let outcomes = storage
        .list_objects_for_project(project_id, "Outcome")
        .await?;

    // 目标进度只信任图谱证据：Outcome 必须由 Decision 产生，且 Decision 指向
    // 与动作声明同一 goal_metric 的 Opportunity Rule。
    let relations = storage.list_all_relations().await?;
    let decisions = storage
        .list_objects_for_project(project_id, "Decision")
        .await?;
    let opportunities = storage
        .list_objects_for_project(project_id, "Opportunity")
        .await?;
    let mut completed_by_metric = std::collections::HashMap::<String, u64>::new();
    for outcome in outcomes {
        if outcome.data.get("success").and_then(Value::as_bool) != Some(true)
            || text(&outcome.data, "stage")? != OUTCOME_BUSINESS_RESULT
        {
            continue;
        }
        let decision_id = relations
            .iter()
            .filter(|relation| {
                relation.relation == "produced_outcome" && relation.target == outcome.id
            })
            .map(|relation| relation.source.as_str())
            .next();
        let Some(decision) = decision_id.and_then(|id| find_record(&decisions, id, "Decision"))
        else {
            continue;
        };
        let action_name = text(&decision.data, "action")?;
        let Some(action) = find_action(&actions, action_name) else {
            continue;
        };
        let opportunity_id = text(&decision.data, "opportunity")?;
        let Some(opportunity) = find_record(&opportunities, opportunity_id, "Opportunity") else {
            continue;
        };
        let metric = shared_goal_metric(
            &ontology,
            text(&opportunity.data, "opportunity_type")?,
            action,
        );
        if let Some(metric) = metric {
            *completed_by_metric.entry(metric).or_default() += 1;
        }
    }

    for goal in goals {
        let baseline = number(&goal.data, "baseline_value")?;
        let metric = text(&goal.data, "target_metric")?;
        let current =
            baseline + completed_by_metric.get(metric).copied().unwrap_or_default() as f64;
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

/// 按目标相关性、状态紧急度、估计价值和历史成功率对 Open Opportunity 排序。
pub async fn recommend_next_actions(
    storage: &LibSQLStorage,
    project_id: &str,
) -> Result<Vec<ActionRecommendation>, BusinessLoopError> {
    let (_, ontology) = project_ontology(storage, project_id).await?;
    let goals = storage
        .list_objects_for_project(project_id, "BusinessGoal")
        .await?;
    let opportunities = storage
        .list_objects_for_project(project_id, "Opportunity")
        .await?;
    let outcomes = storage
        .list_objects_for_project(project_id, "Outcome")
        .await?;

    let mut recommendations = Vec::new();
    for opportunity in opportunities {
        if text(&opportunity.data, "status")? != OPPORTUNITY_OPEN {
            continue;
        }
        let subject = text(&opportunity.data, "subject")?.to_string();
        let opportunity_type = text(&opportunity.data, "opportunity_type")?;
        let (owner, rule) = subject_rule(&ontology, opportunity_type)?;
        let actions = planning_actions_for(&ontology, &owner.name)?;
        let subjects = storage
            .list_objects_for_project(project_id, &owner.name)
            .await?;
        let subject_record = find_record(&subjects, &subject, &owner.name)
            .ok_or_else(|| BusinessLoopError::NotFound(subject.clone()))?;
        let urgency_value = number(&subject_record.data, &rule.urgency_field)?;
        let estimated_value = number(&opportunity.data, "estimated_value")?;
        let urgency_value = urgency_value.max(0.0);
        let urgency = (urgency_value.min(rule.urgency_horizon) / rule.urgency_horizon).max(0.1);
        let mut candidates =
            evaluate_candidates(&actions, &subject_record.data, estimated_value, &outcomes);
        candidates.sort_by(|left, right| {
            right
                .net_value
                .total_cmp(&left.net_value)
                .then_with(|| left.action.cmp(&right.action))
        });

        // 每个候选先按目标对齐加权，再选总分最高者；净价值排序只保留为候选明细。
        let mut scored = Vec::with_capacity(candidates.len());
        for candidate in &candidates {
            let active_goal = active_goal_gap(
                &goals,
                &actions,
                &ontology,
                opportunity_type,
                candidate.action.as_str(),
            )?;
            let remaining_gap = active_goal.as_ref().map_or(0.0, |(_, gap)| *gap);
            let score = candidate.net_value
                * goal_alignment(&actions, candidate.action.as_str(), remaining_gap)
                * (0.6 + 0.4 * urgency);
            scored.push((candidate.action.clone(), score, active_goal));
        }
        let (selected_action, score, active_goal) = scored
            .into_iter()
            .max_by(|left, right| {
                left.1
                    .total_cmp(&right.1)
                    .then_with(|| right.0.cmp(&left.0))
            })
            .ok_or_else(|| BusinessLoopError::invalid("Open Opportunity 没有候选动作"))?;
        let selected = candidates
            .iter()
            .find(|candidate| candidate.action == selected_action)
            .ok_or_else(|| BusinessLoopError::invalid("选中动作缺少候选明细"))?;
        let urgency_reason = format!(
            "{}={urgency_value:.0}，紧迫度 {urgency:.2}",
            rule.urgency_field
        );
        let reason = if let Some((metric, gap)) = active_goal {
            format!(
                "{metric} 还差 {gap:.0}；{urgency_reason}；{}",
                candidate_reason(candidates.as_slice())
            )
        } else {
            format!(
                "{urgency_reason}；{}",
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

/// 有界路径搜索只解释可直接落地的状态更新；创建效应和参数化动作留给执行层扩展。
// ponytail: 只搜 3 步直接更新；需要资源约束/组合优化时再换显式规划器。
#[derive(Debug, Clone)]
struct ScoredPath {
    actions: Vec<String>,
    benefit: f64,
    cost: f64,
    net_value: f64,
    confidence: f64,
    score: f64,
}

fn path_precondition_satisfied(
    action: &ActionDef,
    state: &Value,
) -> Result<bool, BusinessLoopError> {
    for clause in &action.policy.precondition {
        let Some(field) = clause.field.strip_prefix("self.") else {
            return Ok(false);
        };
        let Some(left) = state.get(field) else {
            return Ok(false);
        };
        if !compare_values(left, clause.op, condition_literal(&clause.value)?)? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn apply_path_action(
    ontology: &OntologyIR,
    owner_type: &str,
    action: &ActionDef,
    state: &mut Value,
) -> Result<(), BusinessLoopError> {
    let object = state.as_object_mut().ok_or_else(|| {
        BusinessLoopError::invalid(format!("{} 主体数据必须是 JSON object", owner_type))
    })?;
    for update in &action.updates {
        let ValueExpr::Literal(value) = &update.value else {
            return Err(BusinessLoopError::invalid(format!(
                "动作 {} 的路径效应必须是字面量",
                action.name
            )));
        };
        object.insert(update.field.clone(), value.clone());
    }
    validate_data(ontology, owner_type, state)
}

struct PathContext<'a> {
    ontology: &'a OntologyIR,
    owner_type: &'a str,
    subject_id: &'a str,
    actions: &'a [&'a ActionDef],
    opportunity_type: &'a str,
    estimated_value: f64,
    urgency: f64,
    goals: &'a [crate::storage::ObjectRecord],
    outcomes: &'a [crate::storage::ObjectRecord],
}

fn validate_path_creates(
    context: &PathContext,
    action: &ActionDef,
    subject_data: &Value,
) -> Result<(), BusinessLoopError> {
    if action.creates.len() != 1 {
        return Err(BusinessLoopError::invalid(
            "路径中的动作最多支持一个创建效应",
        ));
    }
    let effect = &action.creates[0];
    let mut object_data = serde_json::Map::new();
    for (field, expression) in &effect.data {
        object_data.insert(
            field.clone(),
            evaluate_value(expression, context.subject_id, subject_data)?,
        );
    }
    validate_data(
        context.ontology,
        &effect.object_type,
        &Value::Object(object_data),
    )
}

fn search_paths<'a>(
    context: &PathContext<'a>,
    state: &Value,
    used: &[String],
    visited: &mut std::collections::BTreeSet<String>,
    depth: usize,
) -> Result<Vec<ScoredPath>, BusinessLoopError> {
    let PathContext {
        ontology,
        owner_type,
        subject_id: _,
        actions,
        opportunity_type,
        estimated_value,
        urgency,
        goals,
        outcomes,
    } = *context;
    if depth >= PATH_MAX_STEPS {
        return Ok(Vec::new());
    }

    let mut paths = Vec::new();
    for action in actions {
        let Some(planning) = action.planning.as_ref() else {
            continue;
        };
        if !action.params.is_empty()
            || action.policy.require_approval
            || used.iter().any(|name| name == &action.name)
            || action
                .updates
                .iter()
                .any(|update| !matches!(update.value, ValueExpr::Literal(_)))
            || !path_precondition_satisfied(action, state)?
        {
            continue;
        }

        let mut next_state = state.clone();
        apply_path_action(ontology, owner_type, action, &mut next_state)?;
        let mut next_used = used.to_vec();
        next_used.push(action.name.clone());
        let step_confidence = action_confidence(action, outcomes)?;
        let benefit = if let Some((field, factor)) = planning
            .benefit_field
            .as_deref()
            .zip(planning.benefit_factor)
        {
            number(&next_state, field)?.max(0.0) * factor
        } else if planning.outcome_timing == OutcomeTiming::Immediate
            && shared_goal_metric(ontology, opportunity_type, action).is_some()
        {
            estimated_value
        } else {
            0.0
        };
        let contributes_to_goal = planning.outcome_timing == OutcomeTiming::Immediate
            && shared_goal_metric(ontology, opportunity_type, action).is_some();

        // 创建效应是流程终点：其后续结果必须等真实终局证据，不能虚构对象内部状态。
        if !action.creates.is_empty() {
            validate_path_creates(context, action, &next_state)?;
            paths.push(ScoredPath {
                actions: next_used.clone(),
                benefit,
                cost: planning.cost,
                net_value: benefit * step_confidence - planning.cost,
                confidence: step_confidence,
                score: f64::NAN,
            });
            if let Some(path) = paths.last_mut() {
                path.score = path.net_value.max(0.0)
                    * (0.6 + 0.4 * urgency)
                    * goal_alignment(
                        actions,
                        &action.name,
                        active_goal_gap(goals, actions, ontology, opportunity_type, &action.name)?
                            .map_or(0.0, |(_, gap)| gap),
                    );
            }
            continue;
        }
        // 一个机会只消费一次；能直接产生目标结果的动作是互斥路线终点，不能继续叠加同机会奖励。
        if contributes_to_goal {
            let net_value = benefit * step_confidence - planning.cost;
            paths.push(ScoredPath {
                actions: next_used.clone(),
                benefit,
                cost: planning.cost,
                net_value,
                confidence: step_confidence,
                score: net_value.max(0.0)
                    * (0.6 + 0.4 * urgency)
                    * goal_alignment(
                        actions,
                        &action.name,
                        active_goal_gap(goals, actions, ontology, opportunity_type, &action.name)?
                            .map_or(0.0, |(_, gap)| gap),
                    ),
            });
            continue;
        }
        let state_key = serde_json::to_string(&next_state)
            .map_err(|error| BusinessLoopError::invalid(error.to_string()))?;
        if !visited.insert(state_key.clone()) {
            continue;
        }
        let mut candidate = ScoredPath {
            actions: next_used.clone(),
            benefit: f64::NAN,
            cost: f64::NAN,
            net_value: f64::NAN,
            confidence: step_confidence,
            score: f64::NAN,
        };
        // 每个可执行前缀本身也是完整路径；继续递归只用于比较更长的替代路线。
        candidate.benefit = benefit;
        candidate.cost = planning.cost;
        candidate.net_value = benefit * step_confidence - planning.cost;
        candidate.score = candidate.net_value.max(0.0)
            * (0.6 + 0.4 * urgency)
            * goal_alignment(
                actions,
                &action.name,
                active_goal_gap(goals, actions, ontology, opportunity_type, &action.name)?
                    .map_or(0.0, |(_, gap)| gap),
            );
        paths.push(candidate);
        let children = search_paths(context, &next_state, &next_used, visited, depth + 1)?;
        for mut child in children {
            child.confidence *= step_confidence;
            child.benefit += benefit;
            child.cost += planning.cost;
            child.net_value = child.benefit * child.confidence - child.cost;
            child.score = child.net_value.max(0.0)
                * (0.6 + 0.4 * urgency)
                * goal_alignment(
                    actions,
                    &action.name,
                    active_goal_gap(goals, actions, ontology, opportunity_type, &action.name)?
                        .map_or(0.0, |(_, gap)| gap),
                );
            paths.push(child);
        }
        visited.remove(&state_key);
    }

    Ok(paths)
}

pub async fn plan_opportunity_paths(
    storage: &LibSQLStorage,
    project_id: &str,
) -> Result<Vec<OpportunityPath>, BusinessLoopError> {
    let (_, ontology) = project_ontology(storage, project_id).await?;
    let goals = storage
        .list_objects_for_project(project_id, "BusinessGoal")
        .await?;
    let opportunities = storage
        .list_objects_for_project(project_id, "Opportunity")
        .await?;
    let outcomes = storage
        .list_objects_for_project(project_id, "Outcome")
        .await?;

    let mut plans = Vec::new();
    for opportunity in opportunities {
        if text(&opportunity.data, "status")? != OPPORTUNITY_OPEN {
            continue;
        }
        let subject = text(&opportunity.data, "subject")?.to_string();
        let opportunity_type = text(&opportunity.data, "opportunity_type")?;
        let (owner, rule) = subject_rule(&ontology, opportunity_type)?;
        let actions = planning_actions_for(&ontology, &owner.name)?;
        let subjects = storage
            .list_objects_for_project(project_id, &owner.name)
            .await?;
        let subject_record = find_record(&subjects, &subject, &owner.name)
            .ok_or_else(|| BusinessLoopError::NotFound(subject.clone()))?;
        let urgency_value = number(&subject_record.data, &rule.urgency_field)?.max(0.0);
        let urgency = (urgency_value.min(rule.urgency_horizon) / rule.urgency_horizon).max(0.1);
        let initial_key = serde_json::to_string(&subject_record.data)
            .map_err(|error| BusinessLoopError::invalid(error.to_string()))?;
        let mut visited = std::collections::BTreeSet::from([initial_key]);
        let context = PathContext {
            ontology: &ontology,
            owner_type: &owner.name,
            subject_id: &subject,
            actions: &actions,
            opportunity_type,
            estimated_value: number(&opportunity.data, "estimated_value")?,
            urgency,
            goals: &goals,
            outcomes: &outcomes,
        };
        let mut scored = search_paths(&context, &subject_record.data, &[], &mut visited, 0)?;
        scored.retain(|path| path.net_value > 0.0);
        scored.sort_by(|left, right| {
            right
                .score
                .total_cmp(&left.score)
                .then_with(|| left.actions.cmp(&right.actions))
        });
        plans.extend(scored.into_iter().take(5).map(|path| {
            let reason = format!(
                "{}；净值 {:.0}；置信度 {:.2}",
                path.actions.join(" → "),
                path.net_value,
                path.confidence
            );
            OpportunityPath {
                project_id: project_id.to_string(),
                opportunity_id: opportunity.id.clone(),
                subject: subject.clone(),
                actions: path.actions,
                projected_net_value: path.net_value,
                confidence: path.confidence,
                score: path.score,
                reason,
            }
        }));
    }

    plans.sort_by(|left, right| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| left.opportunity_id.cmp(&right.opportunity_id))
    });
    Ok(plans)
}

fn action_confidence(
    definition: &ActionDef,
    outcomes: &[crate::storage::ObjectRecord],
) -> Result<f64, BusinessLoopError> {
    let prior_confidence = action_planning(definition)?.prior_confidence;
    let relevant = outcomes
        .iter()
        .filter(|item| {
            text(&item.data, "action").is_ok_and(|name| name == definition.name)
                && text(&item.data, "stage").is_ok_and(|stage| stage == OUTCOME_BUSINESS_RESULT)
        })
        .collect::<Vec<_>>();
    if relevant.is_empty() {
        return Ok(prior_confidence);
    }

    let success = relevant
        .iter()
        .filter(|item| item.data.get("success").and_then(Value::as_bool) == Some(true))
        .count();
    Ok(success as f64 / relevant.len() as f64)
}

fn evaluate_candidates(
    actions: &[&ActionDef],
    customer: &Value,
    estimated_value: f64,
    outcomes: &[crate::storage::ObjectRecord],
) -> Vec<ActionCandidate> {
    actions
        .iter()
        .filter_map(|definition| {
            if !action_available(definition, customer).ok()? {
                return None;
            }
            let confidence = action_confidence(definition, outcomes).ok()?;
            let benefit = expected_benefit(definition, estimated_value, customer).ok()?;
            let cost = action_planning(definition).ok()?.cost;
            Some(ActionCandidate {
                action: definition.name.to_string(),
                expected_benefit: benefit,
                cost,
                confidence,
                net_value: benefit * confidence - cost,
            })
        })
        .collect()
}

fn goal_alignment(actions: &[&ActionDef], action: &str, remaining_gap: f64) -> f64 {
    let immediate = find_action(actions, action).is_some_and(|definition| {
        action_planning(definition)
            .is_ok_and(|planning| planning.outcome_timing == OutcomeTiming::Immediate)
    });
    if remaining_gap > 0.0 && immediate {
        return 1.0;
    }
    if remaining_gap > 0.0 {
        return 0.85;
    }
    0.5
}

fn active_goal_gap(
    goals: &[crate::storage::ObjectRecord],
    actions: &[&ActionDef],
    ontology: &OntologyIR,
    opportunity_type: &str,
    action_name: &str,
) -> Result<Option<(String, f64)>, BusinessLoopError> {
    let Some(action) = find_action(actions, action_name) else {
        return Ok(None);
    };
    let Some(metric) = shared_goal_metric(ontology, opportunity_type, action) else {
        return Ok(None);
    };

    let mut remaining_gap = 0.0_f64;
    for goal in goals {
        if text(&goal.data, "target_metric")? != metric || text(&goal.data, "status")? != "active" {
            continue;
        }
        let gap =
            (number(&goal.data, "target_value")? - number(&goal.data, "current_value")?).max(0.0);
        remaining_gap = remaining_gap.max(gap);
    }

    if remaining_gap > 0.0 {
        return Ok(Some((metric, remaining_gap)));
    }
    Ok(None)
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
    let (_, ontology) = project_ontology(storage, project_id).await?;
    let opportunities = storage
        .list_objects_for_project(project_id, "Opportunity")
        .await?;
    let opportunity = find_record(&opportunities, opportunity_id, "Opportunity")
        .ok_or_else(|| BusinessLoopError::NotFound(opportunity_id.to_string()))?;
    if text(&opportunity.data, "status")? != OPPORTUNITY_OPEN {
        return Err(BusinessLoopError::invalid("只有 Open 状态的机会可以决策"));
    }
    let opportunity_type = text(&opportunity.data, "opportunity_type")?;
    let (owner, _) = subject_rule(&ontology, opportunity_type)?;
    let actions = planning_actions_for(&ontology, &owner.name)?;
    let action = requested_action.unwrap_or(actions[0].name.as_str());
    let Some(definition) = find_action(&actions, action) else {
        return Err(BusinessLoopError::invalid(format!(
            "不支持的业务动作 {action}"
        )));
    };

    let subject = text(&opportunity.data, "subject")?.to_string();
    let subjects = storage
        .list_objects_for_project(project_id, &owner.name)
        .await?;
    let Some(subject_record) = find_record(&subjects, &subject, &owner.name) else {
        return Err(BusinessLoopError::NotFound(subject));
    };
    if !action_available(definition, &subject_record.data)? {
        return Err(BusinessLoopError::invalid(format!(
            "业务动作 {action} 当前不可用"
        )));
    }

    let decision_id = generated_id("decision");
    let alternatives = alternatives
        .map(|value| serde_json::to_string(&value))
        .transpose()
        .map_err(|error| BusinessLoopError::invalid(error.to_string()))?;
    let decision_data = json!({
        "opportunity": opportunity.id,
        "action": action,
        "reason": reason.unwrap_or("选择净价值最优业务动作以关闭目标差距"),
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
    let (owner, _) = subject_rule(&ontology, text(&opportunity.data, "opportunity_type")?)?;
    let actions = planning_actions_for(&ontology, &owner.name)?;
    let definition = find_action(&actions, &action)
        .cloned()
        .ok_or_else(|| BusinessLoopError::invalid(format!("不支持的业务动作 {action}")))?;
    let planning = action_planning(&definition)?;

    let subject = text(&opportunity.data, "subject")?.to_string();
    let subjects = storage
        .list_objects_for_project(project_id, &owner.name)
        .await?;
    let before_record = find_record(&subjects, &subject, &owner.name)
        .ok_or_else(|| BusinessLoopError::NotFound(subject.clone()))?
        .clone();
    if !action_available(&definition, &before_record.data)? {
        return Err(BusinessLoopError::invalid(format!(
            "业务动作 {action} 当前不可用"
        )));
    }
    let mut after_data = before_record.data.clone();
    let mut retention_task_id = None;
    for update in &definition.updates {
        {
            let after_object = after_data
                .as_object_mut()
                .ok_or_else(|| BusinessLoopError::invalid("Customer 数据必须是 JSON object"))?;
            let value = evaluate_value(&update.value, &subject, &before_record.data)?;
            after_object.insert(update.field.clone(), value);
        }
    }
    if !definition.creates.is_empty() {
        if definition.creates.len() != 1 {
            return Err(BusinessLoopError::invalid(
                "Business Loop 每个动作只支持一个创建效应",
            ));
        }
        let effect = &definition.creates[0];
        if ontology.find(&effect.object_type).is_none() {
            return Err(BusinessLoopError::invalid(format!(
                "本体未定义创建对象类型 {}",
                effect.object_type
            )));
        }
        let mut object_data = serde_json::Map::new();
        for (field, expression) in &effect.data {
            let value = evaluate_value(expression, &subject, &before_record.data)?;
            object_data.insert(field.clone(), value);
        }
        let task_data = Value::Object(object_data);
        validate_data(&ontology, &effect.object_type, &task_data)?;
        let task_id = effect
            .id
            .clone()
            .unwrap_or_else(|| generated_id("retention-task"));
        create_world_object(
            storage,
            project_id,
            &effect.object_type,
            task_id.clone(),
            task_data,
        )
        .await?;
        if let Some(edge) = &effect.link_edge {
            link_world(storage, project_id, subject.clone(), edge, task_id.clone()).await?;
        }
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
    if !definition.updates.is_empty() {
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
    let business_result = if planning.outcome_timing == OutcomeTiming::Immediate {
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
    let retention_actions = all_planning_actions(&ontology)
        .into_iter()
        .filter(|action| {
            action
                .creates
                .iter()
                .any(|effect| effect.object_type == "RetentionTask")
        })
        .map(|action| action.name.clone())
        .collect::<Vec<_>>();
    let retention_edges = all_planning_actions(&ontology)
        .into_iter()
        .flat_map(|action| action.creates.iter())
        .filter(|effect| effect.object_type == "RetentionTask")
        .filter_map(|effect| effect.link_edge.clone())
        .collect::<Vec<_>>();
    let task = retention_task(storage, project_id, task_id).await?;
    if text(&task.data, "status")? != TASK_IN_PROGRESS {
        return Err(BusinessLoopError::invalid(
            "只有 InProgress 状态的挽留任务可以记录终局",
        ));
    }

    let relations = storage.list_all_relations().await?;
    let subject = relations
        .iter()
        .find(|relation| retention_edges.contains(&relation.relation) && relation.target == task.id)
        .map(|relation| relation.source.clone())
        .ok_or_else(|| BusinessLoopError::invalid("挽留任务缺少 Customer 创建关系"))?;
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
                && text(&item.data, "action")
                    .is_ok_and(|action| retention_actions.iter().any(|name| name == action))
        })
        .max_by(|left, right| left.id.cmp(&right.id))
        .ok_or_else(|| BusinessLoopError::invalid("挽留任务缺少创建动作决策"))?
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
        "action": decision.data["action"],
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
    let recommendations = recommend_next_actions(storage, project_id).await?;
    let paths = plan_opportunity_paths(storage, project_id).await?;
    let path_choice = paths.first().and_then(|path| {
        let first_action = path.actions.first()?;
        recommendations.iter().find_map(|recommendation| {
            if recommendation.opportunity_id != path.opportunity_id {
                return None;
            }
            let candidate = recommendation
                .candidates
                .iter()
                .find(|candidate| candidate.action.as_str() == first_action.as_str())?;
            let mut selected = recommendation.clone();
            selected.action = candidate.action.clone();
            selected.confidence = candidate.confidence;
            Some(selected)
        })
    });
    let mut recommendation = path_choice.or_else(|| recommendations.into_iter().next());
    if let Some((recommendation, path)) = recommendation.as_mut().zip(paths.first())
        && recommendation.opportunity_id == path.opportunity_id
    {
        recommendation.reason = format!("{}；路径：{}", recommendation.reason, path.reason);
    }

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
