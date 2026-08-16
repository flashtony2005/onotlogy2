//! 基准判定（纯函数，M4）：Benchmark = EvaluationResult 的确定性 reducer。
//! 与持久化（run_offline_benchmark，根 crate）分离——类型环解开。

use crate::types::{
    BenchmarkConfig, BenchmarkMetric, BenchmarkVerdict, MutationProposal, MutationStatus,
};
use oaip_eval_core::EvaluationResult;

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

pub fn mean_of(results: &[EvaluationResult], metric: BenchmarkMetric) -> f64 {
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
pub fn failure_rate(results: &[EvaluationResult]) -> f64 {
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
pub fn new_modes(base: &[EvaluationResult], cand: &[EvaluationResult]) -> Vec<String> {
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

impl MutationProposal {
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
}
