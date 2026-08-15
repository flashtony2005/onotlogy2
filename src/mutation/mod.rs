//! Mutation —— Agent Evolution（Phase 11 · Domain Agent Evolution Runtime）。
//!
//! Skill 永不原地修改；Mutation 是 **Candidate Asset**：
//! `Skill v1 → Mutation Proposal → v2 Candidate → Validation → Benchmark → EXPERIMENTAL
//!  → Human Approval / REQ-018 → ACTIVE`；失败路径 `→ REJECTED`（失败本身就是 Experience，必须保留）。
//!
//! 晋升规则（T11，确定性纯函数）：候选必须同时满足
//! 1. 效果量 mean(v2) ≥ mean(v1) + min_effect_size（默认 0.05）
//! 2. 失败率不劣 failure_rate(v2) ≤ failure_rate(v1)
//! 3. 无新增失败模式 new_failure_modes(v2) = ∅
//!
//! EXPERIMENTAL ≠ ACTIVE：晋升 ACTIVE 需要 Human Approval 或 REQ-018 闸门（默认 OFF），
//! 本模块只负责状态机与判定，不持有权限（Learn ≠ Permission）。

use serde::{Deserialize, Serialize};

use crate::evaluation::EvaluationResult;

/// 配对实验基线 runs 下限（T10）。
pub const DEFAULT_MIN_RUNS: usize = 5;
/// 晋升效果量下限（T11）。
pub const DEFAULT_MIN_EFFECT_SIZE: f64 = 0.05;

/// Mutation 提案状态机。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum MutationStatus {
    Proposed,
    Validated,
    Benchmarking,
    Experimental,
    Active,
    Rejected,
    Archived,
}

impl MutationStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            MutationStatus::Proposed => "PROPOSED",
            MutationStatus::Validated => "VALIDATED",
            MutationStatus::Benchmarking => "BENCHMARKING",
            MutationStatus::Experimental => "EXPERIMENTAL",
            MutationStatus::Active => "ACTIVE",
            MutationStatus::Rejected => "REJECTED",
            MutationStatus::Archived => "ARCHIVED",
        }
    }
}

impl std::fmt::Display for MutationStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Mutation 错误。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MutationError {
    #[error("Delta 必须是 JSON 对象")]
    DeltaNotObject,
    #[error("Delta 操作不在白名单内：{0}")]
    UnknownOp(String),
    #[error("Delta 缺少必需字段：{0}")]
    MissingField(String),
    #[error("非法状态迁移：{from} → {to}")]
    IllegalTransition {
        from: MutationStatus,
        to: MutationStatus,
    },
}

/// 结构化 Delta 允许的操作（SkillIR 步骤图变更）。
const DELTA_OPS: &[&str] = &["insert_step", "modify", "delete_step", "replace"];

/// Mutation 提案（Candidate Asset；id 幂等 `mutation.<target>.<candidate_version>`）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MutationProposal {
    pub id: String,
    pub target_asset: String,
    pub source_version: String,
    pub candidate_version: String,
    pub root_cause: String,
    /// 触发本次 Mutation 的 run 证据（可追溯）。
    pub evidence: Vec<String>,
    /// 结构化 Delta（whitelist 操作）。
    pub delta: serde_json::Value,
    pub status: MutationStatus,
    pub promotion_reason: Option<String>,
}

/// Benchmark 指标选择（扩展位；MVP 仅 aggregate）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BenchmarkMetric {
    Aggregate,
}

/// 配对实验配置。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkConfig {
    pub min_runs_per_side: usize,
    pub min_effect_size: f64,
    pub metric: BenchmarkMetric,
}

impl Default for BenchmarkConfig {
    fn default() -> Self {
        Self {
            min_runs_per_side: DEFAULT_MIN_RUNS,
            min_effect_size: DEFAULT_MIN_EFFECT_SIZE,
            metric: BenchmarkMetric::Aggregate,
        }
    }
}

/// 配对 Benchmark 判定结果（确定性；同一输入永远同一判定）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkVerdict {
    pub baseline_runs: usize,
    pub candidate_runs: usize,
    pub baseline_mean: f64,
    pub candidate_mean: f64,
    /// candidate_mean - baseline_mean（效果量）。
    pub effect_size: f64,
    pub baseline_failure_rate: f64,
    pub candidate_failure_rate: f64,
    /// 候选新增的失败模式（set 差集，排序）。
    pub new_failure_modes: Vec<String>,
    pub pass: bool,
    /// 未通过原因（human-readable；通过时为规则满足说明）。
    pub reason: String,
}

impl MutationProposal {
    /// 创建提案（Proposed）；Delta 立即做白名单校验。
    pub fn propose(
        target_asset: &str,
        source_version: &str,
        candidate_version: &str,
        root_cause: &str,
        evidence: Vec<String>,
        delta: serde_json::Value,
    ) -> Result<Self, MutationError> {
        validate_delta(&delta)?;
        Ok(Self {
            id: format!("mutation.{target_asset}.{candidate_version}"),
            target_asset: target_asset.to_string(),
            source_version: source_version.to_string(),
            candidate_version: candidate_version.to_string(),
            root_cause: root_cause.to_string(),
            evidence,
            delta,
            status: MutationStatus::Proposed,
            promotion_reason: None,
        })
    }

    /// 结构校验通过 → VALIDATED（幂等：已是后续状态则不变）。
    pub fn validate(&mut self) -> Result<(), MutationError> {
        if self.status != MutationStatus::Proposed {
            return Ok(());
        }
        validate_delta(&self.delta)?;
        self.status = MutationStatus::Validated;
        Ok(())
    }

    /// 配对 Benchmark：Validated → BENCHMARKING → 判定 → EXPERIMENTAL / REJECTED。
    ///
    /// `base` / `cand` 必须是同一任务集上各自 ≥ min_runs 的评测结果（T10）。
    pub fn benchmark(
        &mut self,
        base: &[EvaluationResult],
        cand: &[EvaluationResult],
        config: &BenchmarkConfig,
    ) -> BenchmarkVerdict {
        let verdict = benchmark(base, cand, config);
        match self.status {
            MutationStatus::Validated | MutationStatus::Benchmarking => {
                self.status = MutationStatus::Benchmarking;
                self.status = if verdict.pass {
                    MutationStatus::Experimental
                } else {
                    MutationStatus::Rejected
                };
                self.promotion_reason = Some(verdict.reason.clone());
            }
            _ => {}
        }
        verdict
    }

    /// EXPERIMENTAL → ACTIVE。**本函数不检查权限**：调用方必须保证 Human Approval
    /// 或 REQ-018 闸门（默认 OFF）已通过（Learn ≠ Permission）。
    pub fn activate(&mut self) -> Result<(), MutationError> {
        if self.status != MutationStatus::Experimental {
            return Err(MutationError::IllegalTransition {
                from: self.status,
                to: MutationStatus::Active,
            });
        }
        self.status = MutationStatus::Active;
        self.promotion_reason = Some("human approval / REQ-018 gate passed".into());
        Ok(())
    }

    /// 终止态（任何状态 → ARCHIVED；失败保留不被删除，归档即保留）。
    pub fn archive(&mut self) {
        self.status = MutationStatus::Archived;
    }
}

/// 配对 Benchmark 判定（纯函数，T11）。
///
/// - 效果量：candidate_mean ≥ baseline_mean + min_effect_size
/// - 失败率不劣：candidate_failure_rate ≤ baseline_failure_rate（reliability 缺失视为 0 失败）
/// - 无新增失败模式：new_failure_modes 为空
pub fn benchmark(
    base: &[EvaluationResult],
    cand: &[EvaluationResult],
    config: &BenchmarkConfig,
) -> BenchmarkVerdict {
    let baseline_mean = mean_of(base, config.metric);
    let candidate_mean = mean_of(cand, config.metric);
    let effect_size = candidate_mean - baseline_mean;
    let baseline_failure_rate = failure_rate(base);
    let candidate_failure_rate = failure_rate(cand);
    let new_failure_modes = new_modes(base, cand);

    let mut reasons = Vec::new();
    if base.len() < config.min_runs_per_side {
        reasons.push(format!(
            "基线 runs {} < {}",
            base.len(),
            config.min_runs_per_side
        ));
    }
    if cand.len() < config.min_runs_per_side {
        reasons.push(format!(
            "候选 runs {} < {}",
            cand.len(),
            config.min_runs_per_side
        ));
    }
    if effect_size < config.min_effect_size {
        reasons.push(format!(
            "效果量 {effect_size:.4} < {}",
            config.min_effect_size
        ));
    }
    if candidate_failure_rate > baseline_failure_rate {
        reasons.push(format!(
            "失败率不劣：{candidate_failure_rate:.4} > {baseline_failure_rate:.4}"
        ));
    }
    if !new_failure_modes.is_empty() {
        reasons.push(format!("新增失败模式：{new_failure_modes:?}"));
    }

    let pass = reasons.is_empty();
    BenchmarkVerdict {
        baseline_runs: base.len(),
        candidate_runs: cand.len(),
        baseline_mean,
        candidate_mean,
        effect_size,
        baseline_failure_rate,
        candidate_failure_rate,
        new_failure_modes,
        pass,
        reason: if pass {
            format!(
                "pass: effect {effect_size:.4} ≥ {}, failure rate {candidate_failure_rate:.4} ≤ {baseline_failure_rate:.4}, no new failure modes",
                config.min_effect_size
            )
        } else {
            reasons.join("; ")
        },
    }
}

/// 按指标取均值（空集 → 0.0；确定性）。
fn mean_of(results: &[EvaluationResult], metric: BenchmarkMetric) -> f64 {
    if results.is_empty() {
        return 0.0;
    }
    match metric {
        BenchmarkMetric::Aggregate => {
            results.iter().map(|r| r.aggregate).sum::<f64>() / results.len() as f64
        }
    }
}

/// 失败率 = 1 - reliability（reliability 缺失视为无失败）。
fn failure_rate(results: &[EvaluationResult]) -> f64 {
    if results.is_empty() {
        return 0.0;
    }
    let fails: f64 = results
        .iter()
        .map(|r| 1.0 - r.reliability.value.unwrap_or(1.0))
        .sum();
    fails / results.len() as f64
}

/// 候选相对基线的新增失败模式（set 差集，排序）。
fn new_modes(base: &[EvaluationResult], cand: &[EvaluationResult]) -> Vec<String> {
    let mut base_modes: Vec<String> = base
        .iter()
        .flat_map(|r| r.failure_modes.iter().cloned())
        .collect();
    base_modes.sort();
    base_modes.dedup();
    let mut cand_modes: Vec<String> = cand
        .iter()
        .flat_map(|r| r.failure_modes.iter().cloned())
        .collect();
    cand_modes.sort();
    cand_modes.dedup();
    cand_modes
        .into_iter()
        .filter(|m| !base_modes.contains(m))
        .collect()
}

/// Delta 白名单校验（结构化变更；AI 自由文本改 Skill 一律拒绝）。
fn validate_delta(delta: &serde_json::Value) -> Result<(), MutationError> {
    let obj = delta.as_object().ok_or(MutationError::DeltaNotObject)?;
    if obj.is_empty() {
        return Err(MutationError::DeltaNotObject);
    }
    for (op, spec) in obj {
        if !DELTA_OPS.contains(&op.as_str()) {
            return Err(MutationError::UnknownOp(op.clone()));
        }
        let spec_obj = spec
            .as_object()
            .ok_or_else(|| MutationError::MissingField(format!("{op}: 操作体必须是对象")))?;
        match op.as_str() {
            "insert_step" => {
                require_str(spec_obj, "after", op)?;
                require_str(spec_obj, "step", op)?;
            }
            "modify" => require_str(spec_obj, "step", op)?,
            "delete_step" => require_str(spec_obj, "step", op)?,
            "replace" => {
                require_str(spec_obj, "step", op)?;
                require_str(spec_obj, "with", op)?;
            }
            _ => unreachable!("白名单已过滤"),
        }
    }
    Ok(())
}

fn require_str(
    spec: &serde_json::Map<String, serde_json::Value>,
    key: &str,
    op: &str,
) -> Result<(), MutationError> {
    match spec.get(key).and_then(|v| v.as_str()) {
        Some(s) if !s.is_empty() => Ok(()),
        _ => Err(MutationError::MissingField(format!("{op}.{key}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::evaluation::{EvaluationReducer, TaskDefinition};
    use crate::session::event::{EventType, SessionEvent, SessionId};
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

    /// 构造一个 aggregate = score 的评测结果（reliability 可指定）。
    fn result(
        run_id: &str,
        aggregate: f64,
        reliability: Option<f64>,
        modes: &[&str],
    ) -> EvaluationResult {
        EvaluationResult {
            run_id: run_id.into(),
            task_id: "t1".into(),
            correctness: crate::evaluation::Score::known(aggregate, vec![]),
            completeness: crate::evaluation::Score::known(aggregate, vec![]),
            efficiency: crate::evaluation::Score::known(aggregate, vec![]),
            reliability: match reliability {
                Some(v) => crate::evaluation::Score::known(v, vec![]),
                None => crate::evaluation::Score::unknown(),
            },
            policy_compliance: crate::evaluation::Score::known(aggregate, vec![]),
            output_quality: crate::evaluation::Score::unknown(),
            aggregate,
            failure_modes: modes.iter().map(|s| s.to_string()).collect(),
            evaluator_version: crate::evaluation::EVALUATOR_VERSION.into(),
        }
    }

    fn delta() -> serde_json::Value {
        json!({
            "insert_step": { "after": "unit_test", "step": "integration_test" },
            "modify": { "step": "verify", "precondition": "regression_check" }
        })
    }

    #[test]
    fn propose_validates_delta_whitelist() {
        let ok = MutationProposal::propose(
            "fix-java-bug",
            "v1",
            "v2",
            "integration test omitted",
            vec!["run_1".into()],
            delta(),
        );
        assert!(ok.is_ok());
        assert_eq!(ok.unwrap().id, "mutation.fix-java-bug.v2");

        let bad = MutationProposal::propose(
            "fix-java-bug",
            "v1",
            "v2",
            "x",
            vec![],
            json!({ "rewrite_step": { "step": "a" } }),
        );
        assert_eq!(bad, Err(MutationError::UnknownOp("rewrite_step".into())));

        let free_text = MutationProposal::propose(
            "fix-java-bug",
            "v1",
            "v2",
            "x",
            vec![],
            json!("AI 自由文本修改被拒绝"),
        );
        assert_eq!(free_text, Err(MutationError::DeltaNotObject));
    }

    #[test]
    fn benchmark_passes_when_rules_met() {
        let config = BenchmarkConfig {
            min_runs_per_side: 2, // 测试用小样本（生产默认 5）
            ..Default::default()
        };
        let base: Vec<EvaluationResult> = (0..3)
            .map(|i| result(&format!("b{i}"), 0.70, Some(1.0), &[]))
            .collect();
        let cand: Vec<EvaluationResult> = (0..3)
            .map(|i| result(&format!("c{i}"), 0.85, Some(1.0), &[]))
            .collect();
        let v = benchmark(&base, &cand, &config);
        assert!(v.pass, "{}", v.reason);
        assert!((v.effect_size - 0.15).abs() < 1e-9);
    }

    #[test]
    fn benchmark_rejects_insufficient_runs() {
        let config = BenchmarkConfig::default();
        let base = vec![result("b0", 0.70, Some(1.0), &[])];
        let cand = vec![result("c0", 0.90, Some(1.0), &[])];
        let v = benchmark(&base, &cand, &config);
        assert!(!v.pass);
        assert!(v.reason.contains("runs"));
    }

    #[test]
    fn benchmark_rejects_no_effect_or_worse_failure() {
        let config = BenchmarkConfig {
            min_runs_per_side: 2,
            ..Default::default()
        };
        let base: Vec<EvaluationResult> = (0..2)
            .map(|i| result(&format!("b{i}"), 0.80, Some(1.0), &[]))
            .collect();
        // 效果量不足
        let cand1: Vec<EvaluationResult> = (0..2)
            .map(|i| result(&format!("c{i}"), 0.82, Some(1.0), &[]))
            .collect();
        assert!(!benchmark(&base, &cand1, &config).pass);
        // 失败率劣化
        let cand2: Vec<EvaluationResult> = (0..2)
            .map(|i| result(&format!("c{i}"), 0.95, Some(0.5), &[]))
            .collect();
        let v = benchmark(&base, &cand2, &config);
        assert!(!v.pass);
        assert!(v.reason.contains("失败率"));
    }

    #[test]
    fn benchmark_rejects_new_failure_modes() {
        let config = BenchmarkConfig {
            min_runs_per_side: 2,
            ..Default::default()
        };
        let base: Vec<EvaluationResult> = (0..2)
            .map(|i| result(&format!("b{i}"), 0.70, Some(1.0), &[]))
            .collect();
        let cand: Vec<EvaluationResult> = (0..2)
            .map(|i| result(&format!("c{i}"), 0.90, Some(1.0), &["integration timeout"]))
            .collect();
        let v = benchmark(&base, &cand, &config);
        assert!(!v.pass);
        assert_eq!(v.new_failure_modes, vec!["integration timeout"]);
    }

    #[test]
    fn lifecycle_proposed_to_active_with_rejected_kept() {
        let mut p = MutationProposal::propose(
            "fix-java-bug",
            "v1",
            "v2",
            "integration test omitted",
            vec!["run_1".into()],
            delta(),
        )
        .unwrap();
        p.validate().unwrap();
        assert_eq!(p.status, MutationStatus::Validated);

        // 不达标 → REJECTED（失败保留，不删除）
        let config = BenchmarkConfig {
            min_runs_per_side: 2,
            ..Default::default()
        };
        let base: Vec<EvaluationResult> = (0..2)
            .map(|i| result(&format!("b{i}"), 0.80, Some(1.0), &[]))
            .collect();
        let cand: Vec<EvaluationResult> = (0..2)
            .map(|i| result(&format!("c{i}"), 0.81, Some(1.0), &[]))
            .collect();
        p.benchmark(&base, &cand, &config);
        assert_eq!(p.status, MutationStatus::Rejected);
        assert!(p.promotion_reason.is_some());
        // REJECTED 不能直接 ACTIVE
        assert!(p.activate().is_err());
    }

    #[test]
    fn experimental_requires_explicit_activation() {
        let mut p = MutationProposal::propose("s", "v1", "v2", "rc", vec![], delta()).unwrap();
        p.validate().unwrap();
        let config = BenchmarkConfig {
            min_runs_per_side: 2,
            ..Default::default()
        };
        let base: Vec<EvaluationResult> = (0..2)
            .map(|i| result(&format!("b{i}"), 0.70, Some(1.0), &[]))
            .collect();
        let cand: Vec<EvaluationResult> = (0..2)
            .map(|i| result(&format!("c{i}"), 0.85, Some(1.0), &[]))
            .collect();
        let v = p.benchmark(&base, &cand, &config);
        assert!(v.pass);
        assert_eq!(
            p.status,
            MutationStatus::Experimental,
            "自动晋升只到 EXPERIMENTAL"
        );
        // 无权限上下文时不能进入 ACTIVE（Learn ≠ Permission 由调用方保证；此处模拟通过）
        p.activate().unwrap();
        assert_eq!(p.status, MutationStatus::Active);
        // 从 ACTIVE 再 activate 是非法迁移
        assert!(p.activate().is_err());
    }

    #[test]
    fn reducer_feeds_benchmark_end_to_end() {
        // 端到端：SessionEvent → EvaluationResult → benchmark
        let r = EvaluationReducer::default();
        let t = task();
        let base_events = vec![
            ev(EventType::WorkerStarted, json!({ "worker": "w1" }), 0),
            ev(
                EventType::WorkerCompleted,
                json!({ "worker": "w1", "duration_ms": 20000.0 }),
                1,
            ),
            ev(
                EventType::SessionCompleted,
                json!({ "relation": "order.active" }),
                2,
            ),
        ];
        let cand_events = vec![
            ev(EventType::WorkerStarted, json!({ "worker": "w1" }), 0),
            ev(
                EventType::WorkerCompleted,
                json!({ "worker": "w1", "duration_ms": 2000.0 }),
                1,
            ),
            ev(
                EventType::SessionCompleted,
                json!({ "relation": "order.active" }),
                2,
            ),
        ];
        let base = vec![
            r.evaluate(&base_events, "b0", &t),
            r.evaluate(&base_events, "b1", &t),
        ];
        let cand = vec![
            r.evaluate(&cand_events, "c0", &t),
            r.evaluate(&cand_events, "c1", &t),
        ];
        let config = BenchmarkConfig {
            min_runs_per_side: 2,
            ..Default::default()
        };
        let v = benchmark(&base, &cand, &config);
        assert!(v.pass, "{}", v.reason);
        assert!(v.effect_size > 0.05);
    }
}
