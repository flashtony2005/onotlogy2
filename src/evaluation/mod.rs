//! Evaluation —— Agent Evolution（Phase 11 · Domain Agent Evolution Runtime）。
//!
//! 第一原则：**Evaluation = Pure Function(Event Log)**。
//! - Deterministic：同一事件集永远得到同一分数
//! - Idempotent：重复调用结果完全一致
//! - Replayable：删除评测结果后可经 Reducer 完整重建
//!
//! 因此：**Evaluation Result 是派生数据，不是事实**——Event Log 永远是 Source of Truth。
//!
//! Fact 与 Judgment 彻底分离：
//! - 客观维度（efficiency / reliability / policy_compliance）从事件直接计算，绝不用 LLM 推断；
//! - 主观/半客观维度（correctness / completeness / output_quality）的 judge 修正
//!   一律经 `judge.submitted` 事件进入（携带 rubric_version + confidence，可审计）。

use serde::{Deserialize, Serialize};

use crate::session::event::{EventType, SessionEvent};

/// Evaluator 版本（随实现演进递增；wb_evaluation 记录以区分口径）。
pub const EVALUATOR_VERSION: &str = "11.0.0";

/// 任务定义（评测输入之一；来自 Planner 产物，不是事件）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskDefinition {
    pub task_id: String,
    /// 预期达成的世界关系（correctness 客观基准；`relation` / `effect` 事件字段命中）。
    pub expected_relations: Vec<String>,
    /// 预期产物（completeness 客观基准；`artifact.created` 事件字段命中）。
    pub expected_artifacts: Vec<String>,
    /// 时间预算（秒；efficiency 客观基准，对照 worker.completed.duration_ms）。
    pub time_budget_secs: f64,
}

/// 评测策略（纯数据；不改变 reducer 的确定性）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvaluationPolicy {
    /// judge 分数生效的最低置信度；低于则回退客观基准（默认 0.0 = 全部生效）。
    pub judge_min_confidence: f64,
    /// 综合分数是否纳入 output_quality（默认 true；未评分维度自动剔除）。
    pub include_output_quality: bool,
}

impl Default for EvaluationPolicy {
    fn default() -> Self {
        Self {
            judge_min_confidence: 0.0,
            include_output_quality: true,
        }
    }
}

/// 六维评测结果（**底层六维保留**；aggregate 只是派生视图）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct EvaluationResult {
    pub run_id: String,
    pub task_id: String,
    pub correctness: Option<f64>,
    pub completeness: Option<f64>,
    pub efficiency: Option<f64>,
    pub reliability: Option<f64>,
    pub policy_compliance: Option<f64>,
    pub output_quality: Option<f64>,
    /// 已出现维度的均值（未评分维度剔除）。
    pub aggregate: f64,
    /// 本次 run 出现的失败模式（worker.failed 的 error 去重排序；供 Benchmark 判「无新增失败模式」）。
    pub failure_modes: Vec<String>,
    pub evaluator_version: String,
}

/// Evaluation Reducer（六子 reducer 组合；新增维度只加子 reducer，不破坏系统）。
#[derive(Debug, Clone, Default)]
pub struct EvaluationReducer {
    pub policy: EvaluationPolicy,
}

impl EvaluationReducer {
    pub fn new(policy: EvaluationPolicy) -> Self {
        Self { policy }
    }

    /// 评测（纯函数）：`events` 必须是该 run 的事件切片（按 append 顺序）。
    ///
    /// 确定性保证：只依赖 events + task + policy；不读时钟、不随机、无外部状态。
    pub fn evaluate(
        &self,
        events: &[SessionEvent],
        run_id: &str,
        task: &TaskDefinition,
    ) -> EvaluationResult {
        let correctness = self.correctness(events, task);
        let completeness = self.completeness(events, task);
        let efficiency = self.efficiency(events, task);
        let reliability = self.reliability(events);
        let policy_compliance = self.policy_compliance(events);
        let output_quality = self.output_quality(events);
        let aggregate = aggregate_score(
            correctness,
            completeness,
            efficiency,
            reliability,
            policy_compliance,
            if self.policy.include_output_quality {
                output_quality
            } else {
                None
            },
        );
        EvaluationResult {
            run_id: run_id.to_string(),
            task_id: task.task_id.clone(),
            correctness,
            completeness,
            efficiency,
            reliability,
            policy_compliance,
            output_quality,
            aggregate,
            failure_modes: collect_failure_modes(events),
            evaluator_version: EVALUATOR_VERSION.to_string(),
        }
    }

    // ── CorrectnessReducer（半客观）：expected_relations 命中 + judge 修正 ──
    fn correctness(&self, events: &[SessionEvent], task: &TaskDefinition) -> Option<f64> {
        if let Some(s) = judge_score(events, "correctness", self.policy.judge_min_confidence) {
            return Some(s);
        }
        if task.expected_relations.is_empty() {
            return None;
        }
        let hit = task
            .expected_relations
            .iter()
            .filter(|r| events_mention(events, r))
            .count();
        Some(hit as f64 / task.expected_relations.len() as f64)
    }

    // ── CompletenessReducer（半客观）：预期产物命中 + judge 修正 ──
    fn completeness(&self, events: &[SessionEvent], task: &TaskDefinition) -> Option<f64> {
        if let Some(s) = judge_score(events, "completeness", self.policy.judge_min_confidence) {
            return Some(s);
        }
        if task.expected_artifacts.is_empty() {
            return None;
        }
        let hit = task
            .expected_artifacts
            .iter()
            .filter(|a| artifact_created(events, a))
            .count();
        Some(hit as f64 / task.expected_artifacts.len() as f64)
    }

    // ── EfficiencyReducer（客观）：worker.completed.duration_ms vs 时间预算 ──
    fn efficiency(&self, events: &[SessionEvent], task: &TaskDefinition) -> Option<f64> {
        let total_ms: f64 = events
            .iter()
            .filter(|e| e.event_type == EventType::WorkerCompleted)
            .filter_map(|e| e.data.get("duration_ms").and_then(|v| v.as_f64()))
            .sum();
        if total_ms <= 0.0 {
            return None;
        }
        let budget_ms = task.time_budget_secs * 1000.0;
        Some((budget_ms / total_ms).clamp(0.0, 1.0))
    }

    // ── ReliabilityReducer（客观）：failed / started（attempt 口径，重试计入失败）──
    fn reliability(&self, events: &[SessionEvent]) -> Option<f64> {
        let starts = events
            .iter()
            .filter(|e| e.event_type == EventType::WorkerStarted)
            .count();
        if starts == 0 {
            return None;
        }
        let fails = events
            .iter()
            .filter(|e| e.event_type == EventType::WorkerFailed)
            .count();
        Some((1.0 - fails as f64 / starts as f64).clamp(0.0, 1.0))
    }

    // ── PolicyComplianceReducer（客观）：deny / rejected 即违规 ──
    fn policy_compliance(&self, events: &[SessionEvent]) -> Option<f64> {
        let violated = events.iter().any(|e| {
            match e.event_type {
                // 策略拒绝（fail-closed）：worker 层 PolicyChecked verdict=deny
                EventType::PolicyChecked => {
                    e.data.get("verdict").and_then(|v| v.as_str()) == Some("deny")
                }
                // 审批拒绝：ApprovalResolved decision=rejected
                EventType::ApprovalResolved => {
                    e.data.get("decision").and_then(|v| v.as_str()) == Some("rejected")
                }
                _ => false,
            }
        });
        Some(if violated { 0.0 } else { 1.0 })
    }

    // ── OutputQualityReducer（主观）：仅 judge ──
    fn output_quality(&self, events: &[SessionEvent]) -> Option<f64> {
        judge_score(events, "output_quality", self.policy.judge_min_confidence)
    }
}

/// 综合分数（派生视图）：已出现维度的均值；全无 → 0.0。
fn aggregate_score(
    correctness: Option<f64>,
    completeness: Option<f64>,
    efficiency: Option<f64>,
    reliability: Option<f64>,
    policy_compliance: Option<f64>,
    output_quality: Option<f64>,
) -> f64 {
    let dims = [
        correctness,
        completeness,
        efficiency,
        reliability,
        policy_compliance,
        output_quality,
    ];
    let present: Vec<f64> = dims.into_iter().flatten().collect();
    if present.is_empty() {
        0.0
    } else {
        present.iter().sum::<f64>() / present.len() as f64
    }
}

/// 该 run 的失败模式（worker.failed.error 去重 + 排序——确定性）。
fn collect_failure_modes(events: &[SessionEvent]) -> Vec<String> {
    let mut modes: Vec<String> = events
        .iter()
        .filter(|e| e.event_type == EventType::WorkerFailed)
        .filter_map(|e| {
            e.data
                .get("error")
                .and_then(|v| v.as_str())
                .map(String::from)
        })
        .collect();
    modes.sort();
    modes.dedup();
    modes
}

/// 事件数据是否提及某关系/效果（`relation` / `effect` 字段精确命中）。
fn events_mention(events: &[SessionEvent], name: &str) -> bool {
    events.iter().any(|e| {
        ["relation", "effect"]
            .iter()
            .any(|k| e.data.get(k).and_then(|v| v.as_str()) == Some(name))
    })
}

/// artifact.created 事件是否产出该产物（`artifact` / `artifact_id` 字段命中）。
fn artifact_created(events: &[SessionEvent], name: &str) -> bool {
    events.iter().any(|e| {
        e.event_type == EventType::ArtifactCreated
            && ["artifact", "artifact_id"]
                .iter()
                .any(|k| e.data.get(k).and_then(|v| v.as_str()) == Some(name))
    })
}

/// 最近一次 `judge.submitted`（append 顺序 → 后者生效，确定性）中该维度的分数。
/// judge 事件 data：{ judge, dimension, score, rubric_version, confidence }。
fn judge_score(events: &[SessionEvent], dimension: &str, min_confidence: f64) -> Option<f64> {
    events
        .iter()
        .filter(|e| e.event_type == EventType::JudgeSubmitted)
        .filter(|e| e.data.get("dimension").and_then(|v| v.as_str()) == Some(dimension))
        .filter(|e| {
            let conf = e
                .data
                .get("confidence")
                .and_then(|v| v.as_f64())
                .unwrap_or(1.0);
            conf >= min_confidence
        })
        .filter_map(|e| e.data.get("score").and_then(|v| v.as_f64()))
        .map(|s| s.clamp(0.0, 1.0))
        .next_back()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::event::SessionId;
    use serde_json::json;

    fn ev(t: EventType, data: serde_json::Value, seq: u64) -> SessionEvent {
        SessionEvent {
            session_id: SessionId::new("s1"),
            seq,
            event_type: t,
            timestamp: 1000 + seq as i64,
            data,
        }
    }

    fn task() -> TaskDefinition {
        TaskDefinition {
            task_id: "t1".into(),
            expected_relations: vec!["order.active".into()],
            expected_artifacts: vec!["report.md".into()],
            time_budget_secs: 10.0,
        }
    }

    fn happy_events() -> Vec<SessionEvent> {
        vec![
            ev(
                EventType::WorkerStarted,
                json!({ "worker": "w1", "capability": "code.test", "attempt": 1 }),
                0,
            ),
            ev(
                EventType::WorkerCompleted,
                json!({ "worker": "w1", "capability": "code.test", "ok": true, "duration_ms": 2000.0 }),
                1,
            ),
            ev(
                EventType::ArtifactCreated,
                json!({ "artifact": "report.md" }),
                2,
            ),
            ev(
                EventType::ContextAssembled,
                json!({ "assets": [{ "asset_id": "java.architecture", "relevance": 0.9 }] }),
                3,
            ),
        ]
    }

    #[test]
    fn six_dimensions_on_happy_path() {
        let mut es = happy_events();
        // 补一个 relation 命中（correctness 客观基准）
        es.push(ev(
            EventType::SessionCompleted,
            json!({ "relation": "order.active" }),
            4,
        ));
        let r = EvaluationReducer::default().evaluate(&es, "run_1", &task());
        assert_eq!(r.correctness, Some(1.0));
        assert_eq!(r.completeness, Some(1.0));
        assert_eq!(r.efficiency, Some(1.0));
        assert_eq!(r.reliability, Some(1.0));
        assert_eq!(r.policy_compliance, Some(1.0));
        assert_eq!(r.output_quality, None, "无 judge 时主观维度不猜测");
        assert_eq!(r.aggregate, 1.0);
        assert_eq!(r.evaluator_version, EVALUATOR_VERSION);
    }

    #[test]
    fn deterministic_idempotent_replayable() {
        let es = happy_events();
        let a = EvaluationReducer::default().evaluate(&es, "run_1", &task());
        let b = EvaluationReducer::default().evaluate(&es, "run_1", &task());
        assert_eq!(
            a, b,
            "同一事件集永远得到同一分数（Deterministic/Idempotent）"
        );
    }

    #[test]
    fn efficiency_uses_duration_budget() {
        let es = vec![ev(
            EventType::WorkerCompleted,
            json!({ "worker": "w1", "ok": true, "duration_ms": 20000.0 }),
            0,
        )];
        let mut t = task();
        t.time_budget_secs = 10.0; // 20s 用时 vs 10s 预算 → 0.5
        let r = EvaluationReducer::default().evaluate(&es, "run_1", &t);
        assert_eq!(r.efficiency, Some(0.5));
    }

    #[test]
    fn reliability_counts_failed_attempts() {
        let es = vec![
            ev(
                EventType::WorkerStarted,
                json!({ "worker": "w1", "attempt": 1 }),
                0,
            ),
            ev(
                EventType::WorkerFailed,
                json!({ "worker": "w1", "error": "timeout" }),
                1,
            ),
            ev(
                EventType::WorkerStarted,
                json!({ "worker": "w1", "attempt": 2 }),
                2,
            ),
            ev(
                EventType::WorkerCompleted,
                json!({ "worker": "w1", "ok": true, "duration_ms": 100.0 }),
                3,
            ),
        ];
        let r = EvaluationReducer::default().evaluate(&es, "run_1", &task());
        assert_eq!(r.reliability, Some(0.5));
        assert_eq!(r.failure_modes, vec!["timeout".to_string()]);
    }

    #[test]
    fn policy_deny_breaks_compliance() {
        let es = vec![ev(
            EventType::PolicyChecked,
            json!({ "capability": "database.delete", "verdict": "deny" }),
            0,
        )];
        let r = EvaluationReducer::default().evaluate(&es, "run_1", &task());
        assert_eq!(r.policy_compliance, Some(0.0));
    }

    #[test]
    fn judge_overrides_semi_objective_and_supplies_quality() {
        let es = vec![
            ev(
                EventType::JudgeSubmitted,
                json!({ "judge": "human", "dimension": "correctness", "score": 0.91, "rubric_version": "rubric.v1", "confidence": 0.9 }),
                0,
            ),
            ev(
                EventType::JudgeSubmitted,
                json!({ "judge": "llm", "dimension": "output_quality", "score": 0.86, "confidence": 0.8 }),
                1,
            ),
        ];
        let r = EvaluationReducer::default().evaluate(&es, "run_1", &task());
        assert_eq!(r.correctness, Some(0.91));
        assert_eq!(r.output_quality, Some(0.86));
        // correctness=0.91 + output_quality=0.86 + completeness=0.0（无产物）+ policy=1.0
        assert_eq!(r.aggregate, (0.91 + 0.86 + 0.0 + 1.0) / 4.0);
    }

    #[test]
    fn judge_below_confidence_is_ignored() {
        let es = vec![ev(
            EventType::JudgeSubmitted,
            json!({ "judge": "llm", "dimension": "correctness", "score": 0.99, "confidence": 0.2 }),
            0,
        )];
        let policy = EvaluationPolicy {
            judge_min_confidence: 0.5,
            ..Default::default()
        };
        let r = EvaluationReducer::new(policy).evaluate(&es, "run_1", &task());
        // 回退客观基准：无 relation 命中 → 0.0
        assert_eq!(r.correctness, Some(0.0));
    }
}
