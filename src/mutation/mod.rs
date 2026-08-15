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
use crate::storage::StorageError;

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

impl std::str::FromStr for MutationStatus {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "PROPOSED" => Ok(MutationStatus::Proposed),
            "VALIDATED" => Ok(MutationStatus::Validated),
            "BENCHMARKING" => Ok(MutationStatus::Benchmarking),
            "EXPERIMENTAL" => Ok(MutationStatus::Experimental),
            "ACTIVE" => Ok(MutationStatus::Active),
            "REJECTED" => Ok(MutationStatus::Rejected),
            "ARCHIVED" => Ok(MutationStatus::Archived),
            other => Err(format!("未知状态：{other}")),
        }
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

    // ── Phase 11B 诊断环：Human Review（Proposal ≠ Mutation）────────────
    /// 人工评审通过：PROPOSED → VALIDATED（评审记录进 promotion_reason；可审计）。
    pub fn review_approve(&mut self, reviewer: &str) -> Result<(), MutationError> {
        if self.status != MutationStatus::Proposed {
            return Err(MutationError::IllegalTransition {
                from: self.status,
                to: MutationStatus::Validated,
            });
        }
        validate_delta(&self.delta)?;
        self.status = MutationStatus::Validated;
        self.promotion_reason = Some(format!("human approved by {reviewer}"));
        Ok(())
    }

    /// 人工评审拒绝：PROPOSED → REJECTED（失败/否决保留，不删除）。
    pub fn review_reject(&mut self, reviewer: &str, reason: &str) -> Result<(), MutationError> {
        if self.status != MutationStatus::Proposed {
            return Err(MutationError::IllegalTransition {
                from: self.status,
                to: MutationStatus::Rejected,
            });
        }
        self.status = MutationStatus::Rejected;
        self.promotion_reason = Some(format!("human rejected by {reviewer}: {reason}"));
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

    // ── Phase 11C 离线 Benchmark（覆盖率语义；Validated → EXPERIMENTAL / REJECTED）──
    /// 应用离线 Benchmark 判定（仅 VALIDATED 可进入；REJECTED 是持久态，不可复活）。
    ///
    /// pass → EXPERIMENTAL（可路由小流量）；fail → REJECTED（失败保留，不删除）。
    pub fn apply_offline_verdict(&mut self, verdict: &OfflineVerdict) -> Result<(), MutationError> {
        if self.status != MutationStatus::Validated {
            return Err(MutationError::IllegalTransition {
                from: self.status,
                to: if verdict.pass {
                    MutationStatus::Experimental
                } else {
                    MutationStatus::Rejected
                },
            });
        }
        self.status = if verdict.pass {
            MutationStatus::Experimental
        } else {
            MutationStatus::Rejected
        };
        self.promotion_reason = Some(verdict.reason.clone());
        Ok(())
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

// ── Phase 11C：离线 Benchmark（覆盖率语义）───────────────────────────

/// 离线 Benchmark 配置。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OfflineConfig {
    /// 历史任务集最少 run 数（默认 5，对齐 T10 样本下限）。
    pub min_task_runs: usize,
    /// 目标模式覆盖率下限（默认 0.5）。
    pub min_coverage: f64,
}

impl Default for OfflineConfig {
    fn default() -> Self {
        Self {
            min_task_runs: DEFAULT_MIN_RUNS,
            min_coverage: 0.5,
        }
    }
}

/// 离线 Benchmark 判定（确定性）。
///
/// 诚实语义：**Evaluation = f(EventLog)，Skill 版本不进入评测**——候选 v2 没有
/// 真实执行时，"重放旧事件流"只会恒等于基线（Δ=0 平凡成立）。因此离线 Δ 衡量的是
/// **覆盖率**：候选 Delta 针对的失败模式，在历史任务集中真实出现的比例。
/// 结果性 Δ（score_new − score_old）只能在候选真实执行后测得（在线 Shadow）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OfflineVerdict {
    /// 历史任务集 run 数。
    pub baseline_runs: usize,
    /// 历史任务集 aggregate 均值（基线画像）。
    pub baseline_mean: f64,
    /// 历史任务集失败率（1 - reliability 均值）。
    pub baseline_failure_rate: f64,
    /// 命中候选目标模式的 run 数。
    pub targeted_runs: usize,
    /// 覆盖率 = targeted / baseline。
    pub coverage: f64,
    pub pass: bool,
    pub reason: String,
}

/// 离线 Benchmark 判定（纯函数）：
/// 1. 历史任务集样本量 ≥ min_task_runs（不足 = 数据不足，不判定）
/// 2. 覆盖率 ≥ min_coverage（候选确实针对历史失败）
pub fn offline_benchmark(
    base: &[EvaluationResult],
    targeted_runs: &[String],
    config: &OfflineConfig,
) -> OfflineVerdict {
    let baseline_runs = base.len();
    let targeted = base
        .iter()
        .filter(|r| targeted_runs.contains(&r.run_id))
        .count();
    let coverage = if baseline_runs == 0 {
        0.0
    } else {
        targeted as f64 / baseline_runs as f64
    };
    let mut reasons = Vec::new();
    if baseline_runs < config.min_task_runs {
        reasons.push(format!(
            "历史任务集 runs {baseline_runs} < {}（数据不足）",
            config.min_task_runs
        ));
    }
    if coverage < config.min_coverage {
        reasons.push(format!(
            "覆盖率 {coverage:.3} < {}（候选未针对历史失败）",
            config.min_coverage
        ));
    }
    let pass = reasons.is_empty();
    OfflineVerdict {
        baseline_runs,
        baseline_mean: mean_of(base, BenchmarkMetric::Aggregate),
        baseline_failure_rate: failure_rate(base),
        targeted_runs: targeted,
        coverage,
        pass,
        reason: if pass {
            format!(
                "offline pass: coverage {coverage:.3} ≥ {} over {baseline_runs} historical runs（结果性 Δ 需候选真实执行后测得；新增失败模式离线不可观测，需 Shadow 确认）",
                config.min_coverage
            )
        } else {
            reasons.join("; ")
        },
    }
}

/// 离线 Benchmark 编排：读提案 → 任务集评测 → 重诊断定位目标模式 → 判定 → 落库。
///
/// - 提案须为 VALIDATED（人工评审通过后）或 EXPERIMENTAL（复评）
/// - `task_ids = None` → 全部历史任务集；否则仅指定任务
/// - 判定后状态：pass → EXPERIMENTAL；fail → REJECTED（持久，不复活）
pub async fn run_offline_benchmark(
    storage: &crate::storage::LibSQLStorage,
    proposal_id: &str,
    task_ids: Option<&[String]>,
    config: &OfflineConfig,
) -> Result<(MutationProposal, OfflineVerdict), StorageError> {
    use crate::diagnosis::Diagnoser;

    let mut p = storage
        .get_mutation(proposal_id)
        .await?
        .ok_or_else(|| StorageError::NotFound(proposal_id.to_string()))?;
    if !matches!(
        p.status,
        MutationStatus::Validated | MutationStatus::Experimental
    ) {
        return Err(StorageError::Constraint {
            code: "WB3002",
            message: format!("提案状态 {} 不可离线 Benchmark（需 VALIDATED）", p.status),
        });
    }
    let mut results = storage.list_evaluations(None, None, 500).await?;
    if let Some(ids) = task_ids {
        results.retain(|r| ids.contains(&r.task_id));
    }
    // 重诊断：候选针对的失败模式是否仍在任务集出现（确定性；与提案诞生同源）
    let diags = Diagnoser::default().diagnose(&results, &p.target_asset);
    let mut targeted = Vec::new();
    for d in &diags {
        if crate::diagnosis::proposal_id_for(&p.target_asset, &p.source_version, d) == p.id {
            targeted = d.evidence_runs.clone();
            break;
        }
    }
    let verdict = offline_benchmark(&results, &targeted, config);
    if p.status == MutationStatus::Validated {
        p.apply_offline_verdict(&verdict)
            .map_err(|e| StorageError::Constraint {
                code: "WB3003",
                message: format!("离线 Benchmark 状态迁移失败：{e}"),
            })?;
    } else {
        // EXPERIMENTAL 复评：不降级，仅刷新判定理由
        p.promotion_reason = Some(verdict.reason.clone());
    }
    storage.save_mutation(&p).await?;
    Ok((p, verdict))
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
/// `insert_step.after` 可选（缺省 = 追加到工作流末尾）——供诊断环自动产出的草稿使用。
pub fn validate_delta(delta: &serde_json::Value) -> Result<(), MutationError> {
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
                require_str(spec_obj, "step", op)?;
                if spec_obj
                    .get("after")
                    .is_some_and(|a| !a.is_string() || a.as_str().unwrap_or("").is_empty())
                {
                    return Err(MutationError::MissingField(format!("{op}.after")));
                }
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

    // ── Phase 11C：离线 Benchmark ──────────────────────────────────────
    #[test]
    fn offline_benchmark_rules() {
        let config = OfflineConfig::default();
        // 样本不足（< 5）
        let small = vec![result("r0", 0.8, Some(1.0), &[])];
        let v = offline_benchmark(&small, &["r0".into()], &config);
        assert!(!v.pass);
        assert!(v.reason.contains("数据不足"));

        // 覆盖率不足（1/5 = 0.2 < 0.5）
        let base: Vec<_> = (0..5)
            .map(|i| result(&format!("r{i}"), 0.8, Some(1.0), &[]))
            .collect();
        let v2 = offline_benchmark(&base, &["r0".into()], &config);
        assert!(!v2.pass);
        assert!(v2.reason.contains("覆盖率"));

        // 通过：全覆盖
        let all: Vec<String> = base.iter().map(|r| r.run_id.clone()).collect();
        let v3 = offline_benchmark(&base, &all, &config);
        assert!(v3.pass, "{}", v3.reason);
        assert_eq!(v3.baseline_runs, 5);
        assert_eq!(v3.coverage, 1.0);
        assert_eq!(v3.targeted_runs, 5);
        // 诚实语义：reason 明确结果性 Δ 需真实执行
        assert!(v3.reason.contains("结果性 Δ"));
    }

    #[test]
    fn apply_offline_verdict_transitions() {
        let pass_v = OfflineVerdict {
            baseline_runs: 5,
            baseline_mean: 0.8,
            baseline_failure_rate: 0.0,
            targeted_runs: 5,
            coverage: 1.0,
            pass: true,
            reason: "offline pass".into(),
        };
        let fail_v = OfflineVerdict {
            baseline_runs: 5,
            baseline_mean: 0.8,
            baseline_failure_rate: 0.0,
            targeted_runs: 1,
            coverage: 0.2,
            pass: false,
            reason: "coverage low".into(),
        };
        // Validated + pass → EXPERIMENTAL
        let mut p = MutationProposal::propose("s", "v1", "v2", "rc", vec![], delta()).unwrap();
        p.validate().unwrap();
        p.apply_offline_verdict(&pass_v).unwrap();
        assert_eq!(p.status, MutationStatus::Experimental);
        // EXPERIMENTAL 复评 → 非法迁移（不降级）
        assert!(p.apply_offline_verdict(&fail_v).is_err());
        // Validated + fail → REJECTED（持久）
        let mut p2 = MutationProposal::propose("s", "v1", "v2", "rc", vec![], delta()).unwrap();
        p2.validate().unwrap();
        p2.apply_offline_verdict(&fail_v).unwrap();
        assert_eq!(p2.status, MutationStatus::Rejected);
        assert!(
            p2.promotion_reason
                .as_deref()
                .unwrap()
                .contains("coverage low")
        );
        // REJECTED 不可复活
        assert!(p2.apply_offline_verdict(&pass_v).is_err());
        // PROPOSED（未评审）不可直接 Benchmark
        let mut p3 = MutationProposal::propose("s", "v1", "v2", "rc", vec![], delta()).unwrap();
        assert!(p3.apply_offline_verdict(&pass_v).is_err());
    }

    #[tokio::test]
    async fn run_offline_benchmark_lifecycle() {
        use crate::DiagnosisConfig;
        use crate::diagnosis::run_diagnosis;
        use crate::session::event::{EventType, SessionId};
        use crate::session::log::SessionLog;
        use serde_json::json;

        let storage = crate::storage::LibSQLStorage::open(":memory:")
            .await
            .unwrap();
        let reducer = EvaluationReducer::default();
        let task = TaskDefinition {
            task_id: "fix-java-bug".into(),
            expected_relations: vec![],
            expected_artifacts: vec![],
            time_budget_secs: 10.0,
        };
        // 5 个历史 run：执行但无正确性证据（T1 模式）
        for i in 0..5 {
            let mut log = SessionLog::new(SessionId::new(&format!("run_{i}")));
            log.append(
                EventType::WorkerStarted,
                json!({ "worker": "w1", "attempt": 1 }),
            );
            log.append(
                EventType::WorkerCompleted,
                json!({ "worker": "w1", "ok": true, "duration_ms": 5000.0 }),
            );
            let r = reducer.evaluate(log.events(), &format!("run_{i}"), &task);
            storage.save_evaluation(&r).await.unwrap();
        }
        // 诊断 → 提案（PROPOSED）→ 人工评审 → VALIDATED
        let proposals = run_diagnosis(
            &storage,
            "fix-java-bug",
            "v1",
            None,
            &DiagnosisConfig::default(),
        )
        .await
        .unwrap();
        assert_eq!(proposals.len(), 1, "T1 命中");
        let mut p = proposals.into_iter().next().unwrap();
        p.review_approve("developer-1").unwrap();
        storage.save_mutation(&p).await.unwrap();
        assert_eq!(p.status, MutationStatus::Validated);

        // 离线 Benchmark：历史任务集 5 runs、覆盖率 1.0 → pass → EXPERIMENTAL
        let (after, verdict) =
            run_offline_benchmark(&storage, &p.id, None, &OfflineConfig::default())
                .await
                .unwrap();
        assert!(verdict.pass, "{}", verdict.reason);
        assert_eq!(verdict.baseline_runs, 5);
        assert_eq!(verdict.coverage, 1.0);
        assert_eq!(after.status, MutationStatus::Experimental);

        // 已评审提案不被诊断环重新生成
        let rerun = run_diagnosis(
            &storage,
            "fix-java-bug",
            "v1",
            None,
            &DiagnosisConfig::default(),
        )
        .await
        .unwrap();
        assert!(rerun.is_empty());

        // 失败路径：基准任务集为空（task_ids 过滤）→ 数据不足 → REJECTED（持久）
        let p2 = run_offline_benchmark(&storage, &p.id, None, &OfflineConfig::default())
            .await
            .unwrap()
            .0;
        assert_eq!(
            p2.status,
            MutationStatus::Experimental,
            "EXPERIMENTAL 复评不降级"
        );
        let _ = p2;
        // 新提案（同任务集但不同 task 过滤）→ 数据不足 → REJECTED
        let mut p3 = MutationProposal::propose(
            "fix-java-bug",
            "v1",
            "v2",
            "rc",
            vec!["run_0".into()],
            delta(),
        )
        .unwrap();
        p3.validate().unwrap();
        storage.save_mutation(&p3).await.unwrap();
        let (p3_after, v3) = run_offline_benchmark(
            &storage,
            &p3.id,
            Some(&["other-task".to_string()]),
            &OfflineConfig::default(),
        )
        .await
        .unwrap();
        assert!(!v3.pass);
        assert_eq!(p3_after.status, MutationStatus::Rejected);
        // REJECTED 不可复活
        let res = run_offline_benchmark(&storage, &p3.id, None, &OfflineConfig::default()).await;
        assert!(res.is_err(), "REJECTED 不可再 Benchmark");
    }
}
