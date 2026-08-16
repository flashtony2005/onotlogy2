//! Loop Health —— V2 多速率环的元评估（Loop Health，不是普通系统监控）。
//!
//! 六指标四平面，全部从持久化数据**派生**（确定性、可回放——与 Evaluation 同一原则）：
//!
//! - EXECUTION：closure_rate（有评测的 run 比例）、action_success（Action 成功率）、
//!   world_change_rate（真正改变 World State 的 run 比例——「Agent 说了很多话但世界没变」的度量）
//! - OBSERVATION：evaluation_coverage（六维非未知均值——「越用越清楚」的可度量形态）
//! - EVOLUTION：mutation_consumption（候选 Skill 被新 run 消费的比例——§26 环是否真的转）、
//!   learning_effectiveness（候选 Skill 表现 − 基线 Skill 表现）、regression_rate（候选退化比例）
//! - GOVERNANCE：violation_rate（Policy 拒绝尝试 / 全部策略检查）
//!
//! 诚实边界：Trace 查询按 limit 截断（MVP 1000 条）；world_change 用 kind='world' 事件判定。

use serde::Serialize;

use crate::evaluation::EvaluationResult;
use crate::mutation::MutationProposal;
use crate::storage::LibSQLStorage;
use crate::storage::TraceQuery;
use crate::trace::TraceEvent;

/// 执行环健康。
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionHealth {
    pub total_runs: usize,
    pub closure_rate: f64,
    pub action_success_rate: f64,
    pub world_change_rate: f64,
}

/// 观察环健康。
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ObservationHealth {
    pub evaluated_runs: usize,
    pub evaluation_coverage: f64,
}

/// 进化环健康。
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EvolutionHealth {
    pub experimental_or_active: usize,
    pub mutation_consumption: f64,
    pub learning_effectiveness: f64,
    pub regression_rate: f64,
}

/// 治理环健康。
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GovernanceHealth {
    pub policy_checks: usize,
    pub violation_rate: f64,
}

/// Loop Health 报告（确定性派生；无时间戳——可回放重算）。
#[derive(Debug, Clone, Default, Serialize)]
pub struct LoopHealth {
    pub execution: ExecutionHealth,
    pub observation: ObservationHealth,
    pub evolution: EvolutionHealth,
    pub governance: GovernanceHealth,
}

// ── 纯函数（单测友好；确定性）───────────────────────────────────────

/// closure_rate：有评测的 run / 总 run。
pub fn closure_rate(eval_run_ids: &std::collections::HashSet<String>, total_runs: usize) -> f64 {
    if total_runs == 0 {
        return 0.0;
    }
    eval_run_ids.len() as f64 / total_runs as f64
}

/// evaluation_coverage：六维中非未知维度的均值（0=全未知，1=全可测）。
pub fn evaluation_coverage(evals: &[EvaluationResult]) -> f64 {
    if evals.is_empty() {
        return 0.0;
    }
    evals
        .iter()
        .map(|e| (6 - e.unknowns().len()) as f64 / 6.0)
        .sum::<f64>()
        / evals.len() as f64
}

/// action_success_rate：Action 事件 output 含 Success 的比例。
pub fn action_success_rate(actions: &[TraceEvent]) -> f64 {
    if actions.is_empty() {
        return 0.0;
    }
    let ok = actions
        .iter()
        .filter(|e| e.output.as_ref().and_then(|v| v.as_str()) == Some("Success"))
        .count();
    ok as f64 / actions.len() as f64
}

/// world_change_rate：产生 world kind 事件的 run / 总 run。
pub fn world_change_rate(world_runs: usize, total_runs: usize) -> f64 {
    if total_runs == 0 {
        return 0.0;
    }
    world_runs as f64 / total_runs as f64
}

/// mutation_consumption：被新 run 消费（skill_version 包含 candidate_version）的候选比例。
pub fn mutation_consumption(mutations: &[MutationProposal], evals: &[EvaluationResult]) -> f64 {
    let candidates: Vec<&MutationProposal> = mutations
        .iter()
        .filter(|m| {
            matches!(
                m.status,
                crate::mutation::MutationStatus::Experimental
                    | crate::mutation::MutationStatus::Active
            )
        })
        .collect();
    if candidates.is_empty() {
        return 0.0;
    }
    let consumed = candidates
        .iter()
        .filter(|m| {
            evals.iter().any(|e| {
                e.skill_version
                    .as_deref()
                    .is_some_and(|sv| sv.contains(&m.candidate_version))
            })
        })
        .count();
    consumed as f64 / candidates.len() as f64
}

/// 版本形态：候选 Skill 版本（含 '+'，如 "v1+missing-test-evidence"）vs 基线版本。
fn is_candidate_version(skill_version: &str) -> bool {
    skill_version.contains('+')
}

/// learning_effectiveness：候选版本 eval 均值 − 基线版本 eval 均值。
pub fn learning_effectiveness(evals: &[EvaluationResult]) -> f64 {
    let mut base: Vec<f64> = Vec::new();
    let mut cand: Vec<f64> = Vec::new();
    for e in evals {
        let Some(sv) = e.skill_version.as_deref() else {
            continue;
        };
        if is_candidate_version(sv) {
            cand.push(e.aggregate);
        } else {
            base.push(e.aggregate);
        }
    }
    if base.is_empty() || cand.is_empty() {
        return 0.0;
    }
    let base_mean = base.iter().sum::<f64>() / base.len() as f64;
    let cand_mean = cand.iter().sum::<f64>() / cand.len() as f64;
    cand_mean - base_mean
}

/// regression_rate：候选版本 run 中 aggregate 低于基线均值的比例。
pub fn regression_rate(evals: &[EvaluationResult]) -> f64 {
    let base_vals: Vec<f64> = evals
        .iter()
        .filter_map(|e| {
            e.skill_version
                .as_deref()
                .filter(|v| !is_candidate_version(v))
                .map(|_| e.aggregate)
        })
        .collect();
    let cand: Vec<&EvaluationResult> = evals
        .iter()
        .filter(|e| e.skill_version.as_deref().is_some_and(is_candidate_version))
        .collect();
    if cand.is_empty() {
        return 0.0;
    }
    let base_mean = if base_vals.is_empty() {
        return 0.0;
    } else {
        base_vals.iter().sum::<f64>() / base_vals.len() as f64
    };
    let regressed = cand.iter().filter(|e| e.aggregate < base_mean).count();
    regressed as f64 / cand.len() as f64
}

/// violation_rate：PolicyCheck output=Deny / 全部 PolicyCheck。
pub fn violation_rate(policy_checks: &[TraceEvent]) -> f64 {
    if policy_checks.is_empty() {
        return 0.0;
    }
    let denied = policy_checks
        .iter()
        .filter(|e| e.output.as_ref().and_then(|v| v.as_str()) == Some("Deny"))
        .count();
    denied as f64 / policy_checks.len() as f64
}

// ── 编排：从持久化数据收集全部指标 ─────────────────────────────────

/// 收集 Loop Health（确定性派生；Trace 查询 limit=1000，MVP 截断边界）。
pub async fn collect(storage: &LibSQLStorage) -> Result<LoopHealth, crate::storage::StorageError> {
    let runs = storage.list_runs().await?;
    let evals = storage.list_evaluations(None, None, 1000).await?;
    let mutations = storage.list_mutations(None).await?;

    let eval_run_ids: std::collections::HashSet<String> =
        evals.iter().map(|e| e.run_id.clone()).collect();

    // Trace 派生：Action 成功率 / Policy 违规率 / 世界变化
    let actions = storage
        .list_events(TraceQuery {
            event_type: Some("Action".into()),
            limit: 1000,
            ..Default::default()
        })
        .await?;
    let policy_checks = storage
        .list_events(TraceQuery {
            event_type: Some("PolicyCheck".into()),
            limit: 1000,
            ..Default::default()
        })
        .await?;
    let all_trace = storage
        .list_events(TraceQuery {
            limit: 1000,
            ..Default::default()
        })
        .await?;
    let world_runs: std::collections::HashSet<String> = all_trace
        .iter()
        .filter(|e| e.kind == crate::trace::TraceKind::World)
        .filter_map(|e| e.run_id.clone())
        .collect();

    let total_runs = runs.len();

    Ok(LoopHealth {
        execution: ExecutionHealth {
            total_runs,
            closure_rate: closure_rate(&eval_run_ids, total_runs),
            action_success_rate: action_success_rate(&actions),
            world_change_rate: world_change_rate(world_runs.len(), total_runs),
        },
        observation: ObservationHealth {
            evaluated_runs: eval_run_ids.len(),
            evaluation_coverage: evaluation_coverage(&evals),
        },
        evolution: EvolutionHealth {
            experimental_or_active: mutations
                .iter()
                .filter(|m| {
                    matches!(
                        m.status,
                        crate::mutation::MutationStatus::Experimental
                            | crate::mutation::MutationStatus::Active
                    )
                })
                .count(),
            mutation_consumption: mutation_consumption(&mutations, &evals),
            learning_effectiveness: learning_effectiveness(&evals),
            regression_rate: regression_rate(&evals),
        },
        governance: GovernanceHealth {
            policy_checks: policy_checks.len(),
            violation_rate: violation_rate(&policy_checks),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::evaluation::{Score, TaskDefinition};
    use crate::mutation::{MutationProposal, MutationStatus};
    use crate::session::event::{EventType, SessionId};
    use serde_json::json;

    fn eval(run_id: &str, skill: Option<&str>, aggregate: f64) -> EvaluationResult {
        EvaluationResult {
            run_id: run_id.into(),
            task_id: "t1".into(),
            skill_version: skill.map(String::from),
            worker_outcomes: vec![],
            correctness: Score::known(aggregate, vec![]),
            completeness: Score::known(aggregate, vec![]),
            efficiency: Score::known(aggregate, vec![]),
            reliability: Score::known(aggregate, vec![]),
            policy_compliance: Score::known(aggregate, vec![]),
            output_quality: Score::unknown(),
            aggregate,
            failure_modes: vec![],
            evaluator_version: crate::EVALUATOR_VERSION.into(),
        }
    }

    fn trace(event_type: &str, kind: crate::trace::TraceKind, output: Option<&str>) -> TraceEvent {
        TraceEvent {
            id: format!("tr_{event_type}"),
            kind,
            event_type: event_type.into(),
            object_refs: vec!["o1".into()],
            project_id: None,
            run_id: Some("run_1".into()),
            agent_id: None,
            reasoning: None,
            input: None,
            output: output.map(|s| json!(s)),
            confidence: None,
            timestamp: "1".into(),
        }
    }

    #[test]
    fn closure_and_coverage() {
        let runs: std::collections::HashSet<String> =
            ["r1".into(), "r2".into()].into_iter().collect();
        assert_eq!(closure_rate(&runs, 4), 0.5);
        assert_eq!(closure_rate(&runs, 0), 0.0);

        let evals = vec![eval("r1", None, 0.8), eval("r2", None, 0.9)];
        // 各 5/6 已知（output_quality 未知）
        let cov = evaluation_coverage(&evals);
        assert!((cov - 5.0 / 6.0).abs() < 1e-9, "{cov}");
        assert_eq!(evaluation_coverage(&[]), 0.0);
    }

    #[test]
    fn action_and_world_and_violation() {
        let actions = vec![
            trace(
                "Action",
                crate::trace::TraceKind::Cognitive,
                Some("Success"),
            ),
            trace(
                "Action",
                crate::trace::TraceKind::Cognitive,
                Some("Failure"),
            ),
        ];
        assert_eq!(action_success_rate(&actions), 0.5);
        assert_eq!(action_success_rate(&[]), 0.0);

        assert_eq!(world_change_rate(2, 4), 0.5);

        let checks = vec![
            trace(
                "PolicyCheck",
                crate::trace::TraceKind::Cognitive,
                Some("Deny"),
            ),
            trace(
                "PolicyCheck",
                crate::trace::TraceKind::Cognitive,
                Some("ALLOW"),
            ),
        ];
        assert_eq!(violation_rate(&checks), 0.5);
    }

    #[tokio::test]
    async fn evolution_metrics() {
        let evals = vec![
            eval("b1", Some("fix@v1"), 0.80),
            eval("b2", Some("fix@v1"), 0.82),
            eval("c1", Some("fix@v1+missing-test-evidence"), 0.90),
        ];
        assert!(
            (learning_effectiveness(&evals) - 0.09).abs() < 1e-9,
            "cand 0.90 − base 0.81"
        );
        assert_eq!(regression_rate(&evals), 0.0);

        // 退化：候选低于基线均值
        let regressed = vec![
            eval("b1", Some("fix@v1"), 0.90),
            eval("c1", Some("fix@v1+missing-test-evidence"), 0.80),
        ];
        assert_eq!(regression_rate(&regressed), 1.0);
        assert_eq!(regression_rate(&[]), 0.0);

        // mutation_consumption
        let mut m1 = MutationProposal::propose(
            "fix",
            "v1",
            "v1+missing-test-evidence",
            "rc",
            vec![],
            json!({ "insert_step": { "step": "run_tests" } }),
        )
        .unwrap();
        m1.status = MutationStatus::Experimental;
        let mut m2 = MutationProposal::propose(
            "fix",
            "v1",
            "v1+unused",
            "rc",
            vec![],
            json!({ "insert_step": { "step": "x" } }),
        )
        .unwrap();
        m2.status = MutationStatus::Rejected;
        let consumed_evals = vec![eval("c1", Some("fix@v1+missing-test-evidence"), 0.9)];
        assert_eq!(
            mutation_consumption(&[m1.clone(), m2.clone()], &consumed_evals),
            1.0,
            "1/1 候选被消费"
        );
        assert_eq!(mutation_consumption(&[m2], &consumed_evals), 0.0);
        assert_eq!(mutation_consumption(&[], &consumed_evals), 0.0);
    }

    #[tokio::test]
    async fn collect_end_to_end() {
        let storage = crate::storage::LibSQLStorage::open(":memory:")
            .await
            .unwrap();
        storage.insert_run("run_1", "agent", "goal").await.unwrap();
        let reducer = crate::evaluation::EvaluationReducer::default();
        let task = TaskDefinition {
            task_id: "t1".into(),
            expected_relations: vec![],
            expected_artifacts: vec![],
            time_budget_secs: 10.0,
            skill_version: Some("fix@v1".into()),
        };
        let mut log = crate::session::log::SessionLog::new(SessionId::new("run_1"));
        log.append(
            EventType::WorkerStarted,
            json!({ "worker": "w1", "attempt": 1 }),
        );
        log.append(
            EventType::WorkerCompleted,
            json!({ "worker": "w1", "ok": true, "duration_ms": 1000.0 }),
        );
        let r = reducer.evaluate(log.events(), "run_1", &task);
        storage.save_evaluation(&r).await.unwrap();

        let health = collect(&storage).await.unwrap();
        assert_eq!(health.execution.total_runs, 1);
        assert_eq!(health.execution.closure_rate, 1.0, "1 run 全评测");
        assert!(health.observation.evaluation_coverage > 0.0);
        assert!(
            health.evolution.learning_effectiveness.abs() < 1e-9,
            "仅基线版本 → 0"
        );
    }
}
