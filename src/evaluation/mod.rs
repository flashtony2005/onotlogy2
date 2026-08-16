//! Evaluation —— Agent Evolution（Phase 11 · Domain Agent Evolution Runtime）。
//!
//! 第一原则：**Evaluation = Pure Function(Event Log)**。
//! - Deterministic：同一事件集永远得到同一分数
//! - Idempotent：重复调用结果完全一致
//! - Replayable：删除评测结果后可经 Reducer 完整重建
//!
//! 因此：**Evaluation Result 是派生数据，不是事实**——Event Log 永远是 Source of Truth。
//!
//! 证据系统而非分数机器（11A 升级）：每个维度是 `Score { value, evidence, confidence }`——
//! 不仅知道"多少分"，还知道"哪类证据导致了分数"（Mutation 归因的前提）。
//! **Unknown ≠ Success**：无证据的维度是 `None`（我不知道），绝不是满分。
//!
//! Fact 与 Judgment 彻底分离：
//! - 客观维度（efficiency / reliability / policy_compliance）从事件直接计算，绝不用 LLM 推断；
//! - 半客观维度（correctness / completeness）优先使用一等证据（TestResult / 产物事件），
//!   其次 relation/effect 提及，最后才允许 judge 修正；
//! - 主观维度（output_quality）仅 judge（judge.submitted 事件，携带 rubric_version + confidence）。

use serde::{Deserialize, Serialize};

use crate::session::event::{EventType, SessionEvent};

pub mod hook;

/// Evaluator 版本（随实现演进递增；wb_evaluation 记录以区分口径）。
pub const EVALUATOR_VERSION: &str = "11.1.0";

/// 证据引用：指向贡献该维度分数的事件（seq = 该 run 事件流中的 append 序号）。
pub use crate::kernel::EventRef;

/// 单维度评分：值 + 证据 + 置信度。
///
/// - `value = None` 表示**未知**（Unknown ≠ Success），aggregate 剔除该维度；
/// - `evidence` 是得出该分数的全部事件（确定性、可追溯）；
/// - `confidence`：客观维度 = 1.0（事实即事实）；judge 维度 = judge 事件自带置信度。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Score {
    pub value: Option<f64>,
    pub evidence: Vec<EventRef>,
    pub confidence: f64,
}

impl Score {
    pub fn unknown() -> Self {
        Self {
            value: None,
            evidence: Vec::new(),
            confidence: 0.0,
        }
    }

    pub fn known(value: f64, evidence: Vec<EventRef>) -> Self {
        Self {
            value: Some(value.clamp(0.0, 1.0)),
            evidence,
            confidence: 1.0,
        }
    }

    /// judge 来源的分数：置信度来自 judge 事件本身（可被 EvaluationPolicy 过滤）。
    pub fn judged(value: f64, evidence: Vec<EventRef>, judge_confidence: f64) -> Self {
        Self {
            value: Some(value.clamp(0.0, 1.0)),
            evidence,
            confidence: judge_confidence.clamp(0.0, 1.0),
        }
    }

    pub fn is_unknown(&self) -> bool {
        self.value.is_none()
    }
}

/// 任务定义（评测输入之一；来自 Planner 产物，不是事件）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskDefinition {
    pub task_id: String,
    /// 预期达成的世界关系（correctness 兜底证据；`relation` / `effect` 事件字段命中）。
    pub expected_relations: Vec<String>,
    /// 预期产物（completeness 客观基准；`artifact.created` 事件字段命中）。
    pub expected_artifacts: Vec<String>,
    /// 时间预算（秒；efficiency 客观基准，对照 worker.completed.duration_ms）。
    pub time_budget_secs: f64,
    /// 执行所用 Skill 版本（learning_effectiveness 的观测前提；None = 未指定）。
    pub skill_version: Option<String>,
}

/// 评测策略（纯数据；不改变 reducer 的确定性）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvaluationPolicy {
    /// judge 分数生效的最低置信度；低于则回退客观证据（默认 0.0 = 全部生效）。
    pub judge_min_confidence: f64,
    /// 综合分数是否纳入 output_quality（默认 true；未知维度自动剔除）。
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

/// 单次 worker 执行结果（统计重建的数据源；WorkerStats = F(evaluations) 可回放）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkerOutcome {
    pub worker_id: String,
    pub capability: String,
    pub ok: bool,
    pub duration_ms: u64,
}

/// 六维评测结果（**底层六维保留**；aggregate 只是派生视图）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvaluationResult {
    pub run_id: String,
    pub task_id: String,
    /// 执行所用 Skill 版本（I7 观测：EveryRun→Evaluation 携带 skill 身份）。
    pub skill_version: Option<String>,
    /// 本 run 的 per-worker 执行结果（从事件推导；WorkerStats 可回放重建的数据源）。
    pub worker_outcomes: Vec<WorkerOutcome>,
    pub correctness: Score,
    pub completeness: Score,
    pub efficiency: Score,
    pub reliability: Score,
    pub policy_compliance: Score,
    pub output_quality: Score,
    /// 已出现维度的均值（未知维度剔除）。
    pub aggregate: f64,
    /// 本次 run 出现的失败模式（worker.failed 的 error 去重排序；供 Benchmark 判「无新增失败模式」）。
    pub failure_modes: Vec<String>,
    pub evaluator_version: String,
}

impl EvaluationResult {
    /// 未知维度清单（证据系统必须能回答"我不知道什么"）。
    pub fn unknowns(&self) -> Vec<&'static str> {
        [
            ("correctness", &self.correctness),
            ("completeness", &self.completeness),
            ("efficiency", &self.efficiency),
            ("reliability", &self.reliability),
            ("policy_compliance", &self.policy_compliance),
            ("output_quality", &self.output_quality),
        ]
        .iter()
        .filter(|(_, s)| s.is_unknown())
        .map(|(name, _)| *name)
        .collect()
    }
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
            correctness.value,
            completeness.value,
            efficiency.value,
            reliability.value,
            policy_compliance.value,
            if self.policy.include_output_quality {
                output_quality.value
            } else {
                None
            },
        );
        EvaluationResult {
            run_id: run_id.to_string(),
            task_id: task.task_id.clone(),
            skill_version: task.skill_version.clone(),
            worker_outcomes: collect_worker_outcomes(events),
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

    // ── CorrectnessReducer（半客观）：judge 修正 → TestResult 一等证据 → relation 提及 ──
    fn correctness(&self, events: &[SessionEvent], task: &TaskDefinition) -> Score {
        // 1) judge 修正（半客观：人/LLM 可修正客观证据的误判）
        if let Some((s, ev, conf)) =
            judge_score(events, "correctness", self.policy.judge_min_confidence)
        {
            return Score::judged(s, vec![EventRef::new(ev.seq, ev.event_type.as_str())], conf);
        }
        // 2) 一等证据：测试通过率（TestResult 事件；代码类任务的真实 correctness）
        if let Some((rate, ev)) = test_pass_rate(events) {
            return Score::known(rate, vec![EventRef::new(ev.seq, ev.event_type.as_str())]);
        }
        // 3) 客观兜底：expected_relations 被事件提及
        if !task.expected_relations.is_empty() {
            let hit_events: Vec<&SessionEvent> = events
                .iter()
                .filter(|e| events_mention(e, &task.expected_relations))
                .collect();
            if !hit_events.is_empty() {
                let rate = hit_events.len() as f64 / task.expected_relations.len() as f64;
                return Score::known(
                    rate,
                    hit_events
                        .iter()
                        .map(|e| EventRef::new(e.seq, e.event_type.as_str()))
                        .collect(),
                );
            }
        }
        Score::unknown()
    }

    // ── CompletenessReducer（半客观）：产物事件 → judge 修正 ──
    fn completeness(&self, events: &[SessionEvent], task: &TaskDefinition) -> Score {
        if !task.expected_artifacts.is_empty() {
            let hit_events: Vec<&SessionEvent> = events
                .iter()
                .filter(|e| artifact_created(e, &task.expected_artifacts))
                .collect();
            if !hit_events.is_empty() {
                let rate = hit_events.len() as f64 / task.expected_artifacts.len() as f64;
                return Score::known(
                    rate,
                    hit_events
                        .iter()
                        .map(|e| EventRef::new(e.seq, e.event_type.as_str()))
                        .collect(),
                );
            }
        }
        if let Some((s, ev, conf)) =
            judge_score(events, "completeness", self.policy.judge_min_confidence)
        {
            return Score::judged(s, vec![EventRef::new(ev.seq, ev.event_type.as_str())], conf);
        }
        Score::unknown()
    }

    // ── EfficiencyReducer（客观）：worker.completed.duration_ms vs 时间预算 ──
    fn efficiency(&self, events: &[SessionEvent], task: &TaskDefinition) -> Score {
        // 无预算 → 未知（Unknown ≠ 0：0 分是"差"，未知是"不知道"）
        if task.time_budget_secs <= 0.0 {
            return Score::unknown();
        }
        let completed: Vec<&SessionEvent> = events
            .iter()
            .filter(|e| e.event_type == EventType::WorkerCompleted)
            .collect();
        let total_ms: f64 = completed
            .iter()
            .filter_map(|e| e.data.get("duration_ms").and_then(|v| v.as_f64()))
            .sum();
        if total_ms <= 0.0 {
            return Score::unknown();
        }
        let budget_ms = task.time_budget_secs * 1000.0;
        Score::known(
            budget_ms / total_ms,
            completed
                .iter()
                .map(|e| EventRef::new(e.seq, e.event_type.as_str()))
                .collect(),
        )
    }

    // ── ReliabilityReducer（客观）：failed / started（attempt 口径，重试计入失败）──
    fn reliability(&self, events: &[SessionEvent]) -> Score {
        let started: Vec<&SessionEvent> = events
            .iter()
            .filter(|e| e.event_type == EventType::WorkerStarted)
            .collect();
        if started.is_empty() {
            return Score::unknown();
        }
        let fails = events
            .iter()
            .filter(|e| e.event_type == EventType::WorkerFailed)
            .count();
        let mut evidence: Vec<EventRef> = started
            .iter()
            .map(|e| EventRef::new(e.seq, e.event_type.as_str()))
            .collect();
        evidence.extend(
            events
                .iter()
                .filter(|e| e.event_type == EventType::WorkerFailed)
                .map(|e| EventRef::new(e.seq, e.event_type.as_str())),
        );
        Score::known(1.0 - fails as f64 / started.len() as f64, evidence)
    }

    // ── PolicyComplianceReducer（客观）：有证据才算；无证据 = 未知（Unknown ≠ Success）──
    fn policy_compliance(&self, events: &[SessionEvent]) -> Score {
        let checks: Vec<&SessionEvent> = events
            .iter()
            .filter(|e| {
                matches!(
                    e.event_type,
                    EventType::PolicyChecked | EventType::ApprovalResolved
                )
            })
            .collect();
        if checks.is_empty() {
            return Score::unknown();
        }
        let violated = checks.iter().any(|e| match e.event_type {
            EventType::PolicyChecked => {
                e.data.get("verdict").and_then(|v| v.as_str()) == Some("deny")
            }
            EventType::ApprovalResolved => {
                e.data.get("decision").and_then(|v| v.as_str()) == Some("rejected")
            }
            _ => false,
        });
        Score::known(
            if violated { 0.0 } else { 1.0 },
            checks
                .iter()
                .map(|e| EventRef::new(e.seq, e.event_type.as_str()))
                .collect(),
        )
    }

    // ── OutputQualityReducer（主观）：仅 judge ──
    fn output_quality(&self, events: &[SessionEvent]) -> Score {
        match judge_score(events, "output_quality", self.policy.judge_min_confidence) {
            Some((s, ev, conf)) => {
                Score::judged(s, vec![EventRef::new(ev.seq, ev.event_type.as_str())], conf)
            }
            None => Score::unknown(),
        }
    }
}

/// 综合分数（派生视图）：已出现维度的均值；全未知 → 0.0。
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

/// 从事件推导 per-worker 执行结果（确定性；worker.completed/failed 各记一条，
/// 重试 attempt 计入——与 reliability 口径一致）。WorkerStats 重建的数据源。
fn collect_worker_outcomes(events: &[SessionEvent]) -> Vec<WorkerOutcome> {
    events
        .iter()
        .filter_map(|e| match e.event_type {
            EventType::WorkerCompleted => {
                let worker_id = e.data.get("worker").and_then(|v| v.as_str())?;
                let capability = e
                    .data
                    .get("capability")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                Some(WorkerOutcome {
                    worker_id: worker_id.to_string(),
                    capability: capability.to_string(),
                    ok: e.data.get("ok").and_then(|v| v.as_bool()).unwrap_or(true),
                    duration_ms: e
                        .data
                        .get("duration_ms")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0),
                })
            }
            EventType::WorkerFailed => {
                let worker_id = e.data.get("worker").and_then(|v| v.as_str())?;
                let capability = e
                    .data
                    .get("capability")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                Some(WorkerOutcome {
                    worker_id: worker_id.to_string(),
                    capability: capability.to_string(),
                    ok: false,
                    duration_ms: 0,
                })
            }
            _ => None,
        })
        .collect()
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

/// 测试通过率（一等证据）：取**最后一条** TestResult（append 顺序，确定性）。
/// TestResult 事件 data 契约：`{ "tests_total": n, "tests_passed": m }`。
fn test_pass_rate(events: &[SessionEvent]) -> Option<(f64, &SessionEvent)> {
    events
        .iter()
        .filter(|e| e.event_type == EventType::TestResult)
        .filter_map(|e| {
            let total = e.data.get("tests_total").and_then(|v| v.as_u64())?;
            let passed = e.data.get("tests_passed").and_then(|v| v.as_u64())?;
            if total == 0 {
                return None;
            }
            Some((passed as f64 / total as f64, e))
        })
        .next_back()
}

/// 事件数据是否提及任一关系/效果（`relation` / `effect` 字段精确命中）。
fn events_mention(e: &SessionEvent, names: &[String]) -> bool {
    ["relation", "effect"]
        .iter()
        .any(|k| matches!(e.data.get(k).and_then(|v| v.as_str()), Some(v) if names.iter().any(|n| n == v)))
}

/// artifact.created 事件是否产出任一预期产物（`artifact` / `artifact_id` 字段命中）。
fn artifact_created(e: &SessionEvent, names: &[String]) -> bool {
    e.event_type == EventType::ArtifactCreated
        && ["artifact", "artifact_id"]
            .iter()
            .any(|k| matches!(e.data.get(k).and_then(|v| v.as_str()), Some(v) if names.iter().any(|n| n == v)))
}

/// 最近一次 `judge.submitted`（append 顺序 → 后者生效，确定性）中该维度的分数。
/// judge 事件 data：{ judge, dimension, score, rubric_version, confidence }。
fn judge_score<'a>(
    events: &'a [SessionEvent],
    dimension: &str,
    min_confidence: f64,
) -> Option<(f64, &'a SessionEvent, f64)> {
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
        .filter_map(|e| {
            let score = e.data.get("score").and_then(|v| v.as_f64())?;
            let conf = e
                .data
                .get("confidence")
                .and_then(|v| v.as_f64())
                .unwrap_or(1.0);
            Some((score.clamp(0.0, 1.0), e, conf))
        })
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
            skill_version: None,
        }
    }

    #[test]
    fn unknown_never_scores_full_without_evidence() {
        // 一个什么都没做的 run：任何维度都不该有分
        let r = EvaluationReducer::default().evaluate(&[], "run_1", &task());
        assert!(r.correctness.is_unknown());
        assert!(r.completeness.is_unknown());
        assert!(r.efficiency.is_unknown());
        assert!(r.reliability.is_unknown());
        assert!(
            r.policy_compliance.is_unknown(),
            "无 PolicyChecked 证据 ≠ 合规满分"
        );
        assert!(r.output_quality.is_unknown());
        assert_eq!(r.aggregate, 0.0);
        assert_eq!(r.unknowns().len(), 6);
    }

    #[test]
    fn test_result_is_first_class_correctness_evidence() {
        let es = vec![
            ev(
                EventType::TestResult,
                json!({ "tests_total": 10, "tests_passed": 8 }),
                0,
            ),
            ev(
                EventType::SessionCompleted,
                json!({ "relation": "order.active" }),
                1,
            ),
        ];
        let r = EvaluationReducer::default().evaluate(&es, "run_1", &task());
        assert_eq!(
            r.correctness.value,
            Some(0.8),
            "测试通过率优先于 relation 提及"
        );
        assert_eq!(r.correctness.evidence.len(), 1);
        assert_eq!(r.correctness.evidence[0].event_type, "test.result");
        assert_eq!(r.correctness.confidence, 1.0);
    }

    #[test]
    fn relation_mention_is_fallback_evidence() {
        let es = vec![ev(
            EventType::SessionCompleted,
            json!({ "relation": "order.active" }),
            0,
        )];
        let r = EvaluationReducer::default().evaluate(&es, "run_1", &task());
        assert_eq!(r.correctness.value, Some(1.0));
        assert_eq!(r.correctness.evidence[0].seq, 0);
    }

    #[test]
    fn policy_compliance_requires_evidence() {
        let no_check = EvaluationReducer::default().evaluate(&[], "run_1", &task());
        assert!(no_check.policy_compliance.is_unknown());

        let allowed = vec![ev(
            EventType::PolicyChecked,
            json!({ "capability": "code.test", "verdict": "allow" }),
            0,
        )];
        let r = EvaluationReducer::default().evaluate(&allowed, "run_1", &task());
        assert_eq!(r.policy_compliance.value, Some(1.0));
        assert_eq!(r.policy_compliance.evidence.len(), 1);

        let denied = vec![ev(
            EventType::PolicyChecked,
            json!({ "capability": "database.delete", "verdict": "deny" }),
            0,
        )];
        let r2 = EvaluationReducer::default().evaluate(&denied, "run_1", &task());
        assert_eq!(r2.policy_compliance.value, Some(0.0));
    }

    #[test]
    fn judge_supplies_confidence_and_evidence() {
        let es = vec![ev(
            EventType::JudgeSubmitted,
            json!({ "judge": "human", "dimension": "output_quality", "score": 0.86, "rubric_version": "rubric.v1", "confidence": 0.9 }),
            0,
        )];
        let r = EvaluationReducer::default().evaluate(&es, "run_1", &task());
        assert_eq!(r.output_quality.value, Some(0.86));
        assert_eq!(r.output_quality.confidence, 0.9);
        assert_eq!(r.output_quality.evidence[0].event_type, "judge.submitted");
        assert_eq!(r.output_quality.confidence, 0.9);
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
        // 无测试、无 relation 证据 → 未知（回退后仍然 Unknown ≠ 0）
        assert!(r.correctness.is_unknown());
    }

    #[test]
    fn evidence_survives_deterministic_replay() {
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
                json!({ "worker": "w1", "ok": true, "duration_ms": 20000.0 }),
                3,
            ),
            ev(
                EventType::TestResult,
                json!({ "tests_total": 4, "tests_passed": 2 }),
                4,
            ),
        ];
        let a = EvaluationReducer::default().evaluate(&es, "run_1", &task());
        let b = EvaluationReducer::default().evaluate(&es, "run_1", &task());
        assert_eq!(a, b, "同一事件集（含证据）永远同一结果");
        assert_eq!(a.reliability.value, Some(0.5));
        assert_eq!(a.efficiency.value, Some(0.5));
        assert_eq!(a.correctness.value, Some(0.5));
        assert_eq!(a.failure_modes, vec!["timeout".to_string()]);
        assert_eq!(
            a.unknowns(),
            vec!["completeness", "policy_compliance", "output_quality"]
        );
    }
}
