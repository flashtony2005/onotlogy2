//! 基准裁决（M5 拆分）：benchmark / offline_benchmark / run_offline_benchmark。
//! Benchmark = 纯函数（确定性、可回放）；离线批量裁决逐条持久化。

use super::types::{
    BenchmarkConfig, BenchmarkMetric, BenchmarkVerdict, DEFAULT_MIN_RUNS, MutationError,
    MutationProposal, MutationStatus,
};
use super::validate::validate_delta;
use crate::evaluation::EvaluationResult;
use crate::storage::StorageError;
use serde::{Deserialize, Serialize};

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
