//! Projection Simulation（S2/S3）—— 双轨模型的假设轨核心。
//!
//! S2 — Projection：Current State(t) + Hypothetical Action → Predicted State(t+1)
//! S3 — Counterfactual：Action A → Projection A vs Action B → Projection B → Decision Support
//!
//! 铁律（I2）：产物全部是 Projection（hypothetical 恒真、assumptions 可审计），
//! **永不写入 EventLog、永不参与 State=Reduce**——Simulation 永远是 Decision Support，
//! 不是 Autonomous Authority。真正改变世界的只有：Authorized Action → Event → State。

use serde::Serialize;
use serde_json::json;

use crate::kernel::effect::Reversibility;
use crate::kernel::projection::Projection;
use crate::worker::policy::{WorkerPolicy, WorkerVerdict};

/// 假设动作（S2/S3 输入；未执行、永不执行）。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct HypotheticalAction {
    /// 语义名（如 "DeployProduction" / "customer.retain"）。
    pub action: String,
    /// 若绑定 worker 执行：capability（Policy 判定 + worker 信任维度）。
    pub capability: Option<String>,
    /// 世界投影 (predicate, target)（V2 kernel 关系语义）。
    pub predicate: Option<String>,
    pub target: Option<String>,
    /// 可逆性（序列化为字符串，与 kernel 枚举区分）。
    pub reversibility: String,
    /// S3：假设派给谁（worker 信任维度）。
    pub worker_id: Option<String>,
}

impl HypotheticalAction {
    pub fn new(
        action: impl Into<String>,
        predicate: Option<&str>,
        target: Option<&str>,
        reversibility: Reversibility,
    ) -> Self {
        Self {
            action: action.into(),
            capability: None,
            predicate: predicate.map(String::from),
            target: target.map(String::from),
            reversibility: format!("{reversibility:?}"),
            worker_id: None,
        }
    }
}

/// 预测状态（当前 + 假设投影）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PredictedState {
    pub relations: Vec<(String, String, String)>,
    /// 相对当前状态新增的关系（Outcome 维度）。
    pub added: Vec<(String, String, String)>,
}

/// 风险等级（可逆性排序：Reversible < Compensatable < Idempotent < Irreversible）。
pub fn risk_level(r: Reversibility) -> u8 {
    match r {
        Reversibility::Reversible => 1,
        Reversibility::Compensatable => 2,
        Reversibility::Idempotent => 3,
        Reversibility::Irreversible => 4,
    }
}

/// 从可逆性字符串解析风险等级（HypotheticalAction.reversibility）。
pub fn risk_level_str(s: &str) -> u8 {
    match s {
        "Reversible" => 1,
        "Compensatable" => 2,
        "Idempotent" => 3,
        "Irreversible" => 4,
        _ => 0,
    }
}

/// S2：单动作投影（纯函数；确定性）。
/// 预测 = 当前关系集 + 假设动作的投影关系（source 取 action 名，与 kernel 投影语义一致）。
pub fn project(
    state_relations: &[(String, String, String)],
    action: &HypotheticalAction,
) -> Projection {
    let mut relations = state_relations.to_vec();
    let mut added = Vec::new();
    if let (Some(predicate), Some(target)) = (&action.predicate, &action.target) {
        let edge = (action.action.clone(), predicate.clone(), target.clone());
        if !relations.contains(&edge) {
            relations.push(edge.clone());
            added.push(edge);
        }
    }
    let predicted = PredictedState { relations, added };
    Projection::new(
        format!("projection.{}", action.action),
        vec![],
        vec![
            format!(
                "假设动作：{}（可逆性 {}）",
                action.action, action.reversibility
            ),
            "Projection 永不写入 EventLog（I2）".to_string(),
        ],
        serde_json::to_value(action).unwrap_or(json!({})),
        serde_json::to_value(&predicted).unwrap_or(json!({})),
    )
}

/// S3 对比维度。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CounterfactualDim {
    pub action: String,
    pub outcome_added: usize,
    pub risk_level: u8,
    pub policy: String,
    pub worker_trust: Option<f64>,
    pub recommended: bool,
    pub reason: String,
}

/// S3 反事实报告（Decision Support）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CounterfactualReport {
    pub projection_a: Projection,
    pub projection_b: Projection,
    pub dimensions: Vec<CounterfactualDim>,
    /// 确定性推荐（低风险 → 高 worker 信任；deny 一方永不推荐）。
    pub recommendation: Option<String>,
}

/// S3：双动作反事实对比（纯函数；确定性）。
/// `worker_scores`：(worker_id, effective_trust)（调用方从 learned_trust_map + manifest 计算）。
pub fn counterfactual(
    state_relations: &[(String, String, String)],
    action_a: &HypotheticalAction,
    action_b: &HypotheticalAction,
    policy: &WorkerPolicy,
    worker_scores: &[(String, f64)],
) -> CounterfactualReport {
    let pa = project(state_relations, action_a);
    let pb = project(state_relations, action_b);
    let dim = |a: &HypotheticalAction, proj: &Projection| {
        let outcome_added = proj
            .predicted_state
            .get("added")
            .and_then(|v| v.as_array())
            .map(|arr| arr.len())
            .unwrap_or(0);
        let policy_verdict = match &a.capability {
            Some(cap) => match policy.check(cap, &json!({})) {
                WorkerVerdict::Allow => "allow",
                WorkerVerdict::RequireApproval => "require_approval",
                WorkerVerdict::Deny(_) => "deny",
            }
            .to_string(),
            None => "n/a".to_string(),
        };
        let worker_trust = a.worker_id.as_ref().and_then(|wid| {
            worker_scores
                .iter()
                .find(|(id, _)| id == wid)
                .map(|(_, s)| *s)
        });
        let (recommended, reason) = match policy_verdict.as_str() {
            "deny" => (false, "Policy deny：不可执行".to_string()),
            "require_approval" => (false, "需人工审批（L4 治理环）：不可自动推荐".to_string()),
            _ => (true, "Policy allow".to_string()),
        };
        CounterfactualDim {
            action: a.action.clone(),
            outcome_added,
            risk_level: risk_level_str(&a.reversibility),
            policy: policy_verdict,
            worker_trust,
            recommended,
            reason,
        }
    };
    let da = dim(action_a, &pa);
    let db = dim(action_b, &pb);

    // 确定性推荐：deny/需审批者出局；低风险优先；平 → worker 信任高者
    let recommendation = match (da.recommended, db.recommended) {
        (false, false) => None,
        (true, false) => Some(action_a.action.clone()),
        (false, true) => Some(action_b.action.clone()),
        (true, true) => Some(match da.risk_level.cmp(&db.risk_level) {
            std::cmp::Ordering::Less => action_a.action.clone(),
            std::cmp::Ordering::Greater => action_b.action.clone(),
            std::cmp::Ordering::Equal => match (
                da.worker_trust.unwrap_or(0.0),
                db.worker_trust.unwrap_or(0.0),
            ) {
                (ta, tb) if tb > ta => action_b.action.clone(),
                _ => action_a.action.clone(),
            },
        }),
    };
    CounterfactualReport {
        projection_a: pa,
        projection_b: pb,
        dimensions: vec![da, db],
        recommendation,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn state() -> Vec<(String, String, String)> {
        vec![
            ("order_1".into(), "owns".into(), "customer_1".into()),
            ("customer_1".into(), "status".into(), "active".into()),
        ]
    }

    fn deploy(worker: Option<&str>) -> HypotheticalAction {
        let mut a = HypotheticalAction::new(
            "DeployProduction",
            Some("version"),
            Some("V2"),
            Reversibility::Irreversible,
        );
        a.capability = Some("deploy.production".into());
        a.worker_id = worker.map(String::from);
        a
    }

    #[test]
    fn s2_projection_predicts_state() {
        let p = project(&state(), &deploy(None));
        assert!(p.hypothetical(), "I2：投影是假设");
        assert_eq!(
            p.predicted_state["added"][0],
            json!(["DeployProduction", "version", "V2"]),
            "预测新增关系"
        );
        assert_eq!(
            p.predicted_state["relations"].as_array().unwrap().len(),
            3,
            "当前 + 假设投影"
        );
        assert!(p.assumptions[0].contains("Irreversible"), "风险入假设清单");
        // 幂等：同状态同动作 → 同投影（added 不重复）
        let p2 = project(&state(), &deploy(None));
        assert_eq!(p.predicted_state, p2.predicted_state);
        assert_eq!(p.assumptions, p2.assumptions);
    }

    #[test]
    fn s3_counterfactual_recommends_low_risk() {
        let policy = WorkerPolicy::new().allow("deploy.production");
        let mut low_risk = HypotheticalAction::new(
            "CanaryDeploy",
            Some("version"),
            Some("V2-canary"),
            Reversibility::Compensatable,
        );
        low_risk.capability = Some("deploy.production".into());
        let report = counterfactual(&state(), &deploy(None), &low_risk, &policy, &[]);
        assert_eq!(
            report.recommendation.as_deref(),
            Some("CanaryDeploy"),
            "低风险优先"
        );
        assert_eq!(report.dimensions.len(), 2);
        assert!(report.dimensions[0].risk_level > report.dimensions[1].risk_level);
    }

    #[test]
    fn s3_policy_deny_never_recommended() {
        // 两个候选都被 deny → 无推荐（Governance 边界：Simulation 不能绕过 Policy）
        let policy = WorkerPolicy::new().deny("deploy.production");
        let report = counterfactual(
            &state(),
            &deploy(None),
            &deploy(Some("w-a")),
            &policy,
            &[("w-a".into(), 0.9)],
        );
        assert_eq!(report.recommendation, None, "deny 一方永不推荐");
        assert!(report.dimensions.iter().all(|d| !d.recommended));
    }

    #[test]
    fn s3_worker_trust_breaks_risk_tie() {
        let policy = WorkerPolicy::new().allow("deploy.production");
        let mut a = HypotheticalAction::new("A", None, None, Reversibility::Compensatable);
        a.capability = Some("deploy.production".into());
        a.worker_id = Some("w-low".into());
        let mut b = HypotheticalAction::new("B", None, None, Reversibility::Compensatable);
        b.capability = Some("deploy.production".into());
        b.worker_id = Some("w-high".into());
        let report = counterfactual(
            &state(),
            &a,
            &b,
            &policy,
            &[("w-low".into(), 0.7), ("w-high".into(), 0.95)],
        );
        assert_eq!(
            report.recommendation.as_deref(),
            Some("B"),
            "同风险 → worker 信任高者"
        );
        assert_eq!(
            report.dimensions[1].worker_trust,
            Some(0.95),
            "worker 维度可审计"
        );
    }
}
