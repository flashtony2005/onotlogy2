//! Decision Replay（S1）—— Learning Counterfactual，不是 World Simulation。
//!
//! 用**当前 Trust 数据**重跑历史路由决策，与实际选择比较，回答：
//! 「如果当时已经拥有现在的经验，系统会不会做出不同选择？」
//!
//! 铁律（I2）：Replay **永不改变历史 EventLog**——产物是 Projection（假设轨，
//! hypothetical 恒真，basis/assumptions 可审计），只能作为决策参考。
//!
//! 数据源（全部持久化、可回放）：
//! - wb_worker：declared 属性（quality/trust/risk/cost/latency）
//! - wb_worker_stats：learned 修正（(capability, worker)）
//! - wb_evaluation.worker_outcomes：历史实际选择

use serde::Serialize;
use serde_json::json;

use crate::evaluation::WorkerOutcome;
use crate::kernel::projection::Projection;
use crate::storage::LibSQLStorage;
use crate::storage::StorageError;
use crate::worker_core::manifest::WorkerManifest;
use crate::worker_core::stats::WorkerStat;

/// 单条重放决策（实际 vs 重放，可审计）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplayDecision {
    pub run_id: String,
    pub capability: String,
    pub actual_worker: String,
    pub replay_worker: String,
    pub changed: bool,
    pub reason: String,
    /// 重跑时的全候选评分（Explainable Routing）。
    pub scores: Vec<(String, f64)>,
}

/// 重放报告（Learning Counterfactual 的产物）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplayReport {
    pub total_decisions: usize,
    pub changed: usize,
    pub change_rate: f64,
    pub decisions: Vec<ReplayDecision>,
}

/// 重放一条历史决策：用当前 (declared + learned) 重算该 capability 的最高分候选。
/// 纯函数（确定性）：同一输入永远同一重放。
pub fn replay_decision(
    workers: &[WorkerManifest],
    stats: &[WorkerStat],
    run_id: &str,
    outcome: &WorkerOutcome,
) -> ReplayDecision {
    let learned = |worker_id: &str| -> f64 {
        stats
            .iter()
            .find(|s| s.capability == outcome.capability && s.worker_id == worker_id)
            .and_then(crate::worker_core::stats::learned_trust)
            .unwrap_or(0.0)
    };
    // 候选 + 评分（quality × (declared + learned)，clamp 0..1；tie: cost/latency）
    let mut scored: Vec<(String, f64, u32, u32)> = workers
        .iter()
        .filter(|m| m.supports(&outcome.capability))
        .map(|m| {
            let effective = (m.trust + learned(&m.id)).clamp(0.0, 1.0);
            (m.id.clone(), m.quality * effective, m.cost, m.latency_ms)
        })
        .collect();
    scored.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.2.cmp(&b.2))
            .then_with(|| a.3.cmp(&b.3))
    });
    let scores: Vec<(String, f64)> = scored
        .iter()
        .map(|(id, s, _, _)| (id.clone(), *s))
        .collect();
    let replay_worker = scored
        .first()
        .map(|(id, _, _, _)| id.clone())
        .unwrap_or_else(|| "<no candidate>".to_string());
    let changed = replay_worker != outcome.worker_id;
    let reason = if changed {
        format!(
            "actual={} → replay={}（当前 learned trust 改变了选择）",
            outcome.worker_id, replay_worker
        )
    } else {
        format!("actual=replay={}（当前经验下选择不变）", outcome.worker_id)
    };
    ReplayDecision {
        run_id: run_id.to_string(),
        capability: outcome.capability.clone(),
        actual_worker: outcome.worker_id.clone(),
        replay_worker,
        changed,
        reason,
        scores,
    }
}

/// 全量重放：遍历历史 per-worker 结果，逐条重放。
pub fn replay_all(
    workers: &[WorkerManifest],
    stats: &[WorkerStat],
    history: &[(String, WorkerOutcome)],
) -> ReplayReport {
    let decisions: Vec<ReplayDecision> = history
        .iter()
        .map(|(run_id, o)| replay_decision(workers, stats, run_id, o))
        .collect();
    let changed = decisions.iter().filter(|d| d.changed).count();
    let total = decisions.len();
    ReplayReport {
        total_decisions: total,
        changed,
        change_rate: if total == 0 {
            0.0
        } else {
            changed as f64 / total as f64
        },
        decisions,
    }
}

/// 编排：读持久化（worker manifest + stats + 历史选择）→ 重放 → 报告。
pub async fn run_decision_replay(storage: &LibSQLStorage) -> Result<ReplayReport, StorageError> {
    let workers = storage.list_workers().await?;
    let stats = storage.list_worker_stats().await?;
    let evals = storage.list_evaluations(None, None, 1000).await?;
    let history: Vec<(String, WorkerOutcome)> = evals
        .iter()
        .flat_map(|e| {
            e.worker_outcomes
                .iter()
                .map(|o| (e.run_id.clone(), o.clone()))
                .collect::<Vec<_>>()
        })
        .collect();
    Ok(replay_all(&workers, &stats, &history))
}

/// 把重放报告承载为 Projection（假设轨；I2：永不入 EventLog，只能作决策参考）。
pub fn report_projection(report: &ReplayReport) -> Projection {
    Projection::new(
        format!("replay.{}", report.total_decisions),
        vec![], // 历史事件引用（当前按 run 聚合；精确事件 ref 为扩展位）
        vec![
            "Decision Replay：当前 learned trust 应用于历史选择".to_string(),
            "Replay 不改变历史 EventLog（I2）".to_string(),
        ],
        json!({ "kind": "decision_replay" }),
        serde_json::to_value(report).unwrap_or(json!({})),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn worker(id: &str, caps: &[&str], quality: f64, trust: f64) -> WorkerManifest {
        let mut m = WorkerManifest::new(id, "agent").with_capabilities(caps);
        m.quality = quality;
        m.trust = trust;
        m
    }

    fn stat(capability: &str, worker_id: &str, runs: u64, success: u64) -> WorkerStat {
        WorkerStat {
            capability: capability.into(),
            worker_id: worker_id.into(),
            runs,
            success,
            total_duration_ms: runs * 100,
            policy_compliance: None,
            avg_cost: None,
            first_seen: None,
            last_seen: None,
        }
    }

    fn outcome(capability: &str, worker: &str) -> WorkerOutcome {
        WorkerOutcome {
            capability: capability.into(),
            worker_id: worker.into(),
            ok: true,
            duration_ms: 100,
        }
    }

    #[test]
    fn replay_changes_when_learned_flips_ranking() {
        // 两个同质 worker（quality/trust 相同）→ 历史选了 w-a
        let workers = vec![
            worker("w-a", &["code.read"], 0.9, 0.8),
            worker("w-b", &["code.read"], 0.9, 0.8),
        ];
        // 当前经验：w-a 差（−0.1）、w-b 好（+0.1）→ 重放应选 w-b
        let stats = vec![
            stat("code.read", "w-a", 10, 2),
            stat("code.read", "w-b", 10, 10),
        ];
        let d = replay_decision(&workers, &stats, "run_1", &outcome("code.read", "w-a"));
        assert!(d.changed, "learned trust 翻转选择");
        assert_eq!(d.replay_worker, "w-b");
        assert!(
            d.scores.iter().any(|(id, s)| id == "w-a" && *s < 0.9),
            "评分可审计"
        );
        assert!(d.reason.contains("learned"), "理由可解释");
    }

    #[test]
    fn replay_unchanged_without_experience() {
        let workers = vec![worker("w-a", &["code.read"], 0.9, 0.8)];
        let d = replay_decision(&workers, &[], "run_1", &outcome("code.read", "w-a"));
        assert!(!d.changed);
        assert_eq!(d.replay_worker, "w-a");
    }

    #[test]
    fn no_candidate_reports_honestly() {
        let d = replay_decision(&[], &[], "run_1", &outcome("code.read", "w-a"));
        assert_eq!(d.replay_worker, "<no candidate>");
        assert!(d.changed);
    }

    #[test]
    fn projection_never_enters_event_log() {
        let report = replay_all(
            &[worker("w-a", &["code.read"], 0.9, 0.8)],
            &[],
            &[("run_1".into(), outcome("code.read", "w-a"))],
        );
        let p = report_projection(&report);
        assert!(p.hypothetical(), "I2：重放产物是假设轨");
        assert!(!p.assumptions.is_empty(), "假设清单可审计");
    }
}
