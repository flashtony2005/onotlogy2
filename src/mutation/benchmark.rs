//! 基准裁决（M5 拆分）：benchmark / offline_benchmark / run_offline_benchmark。
//! Benchmark = 纯函数（确定性、可回放）；离线批量裁决逐条持久化。

use crate::evaluation::EvaluationResult;
use crate::mutation::{
    BenchmarkMetric, MutationProposal, MutationStatus, OfflineConfig, OfflineVerdict,
};
use crate::mutation_core::benchmark::{failure_rate, mean_of};
use crate::storage::StorageError;

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
