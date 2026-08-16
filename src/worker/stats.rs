//! Worker 统计持久化（M4 拆分）：类型与纯函数在 oaip-worker-core；本文件只留
//! LibSQLStorage 持久化实现（record_outcomes / rebuild / learned_trust_map）。

use crate::storage::{LibSQLStorage, StorageError};
use crate::worker_core::{efficiency_component, learned_trust_eff};

pub async fn record_outcomes(
    storage: &LibSQLStorage,
    outcomes: &[crate::evaluation::WorkerOutcome],
) -> Result<(), StorageError> {
    for o in outcomes {
        storage
            .record_worker_outcome(&o.capability, &o.worker_id, o.ok, o.duration_ms)
            .await?;
    }
    Ok(())
}

/// **权威重建**（可回放）：从 wb_evaluation.worker_outcomes 全量重建统计。
/// 与 record_outcomes（运行期增量）的关系：增量提供即时反馈，重建保证一致性——
/// 删表/跨重启后经 `WorkerStats = F(evaluations)` 可完全恢复（Trust 也是 Replayable 的）。
pub async fn rebuild_from_evals(storage: &LibSQLStorage) -> Result<(), StorageError> {
    storage.clear_worker_stats().await?;
    let evals = storage.list_evaluations(None, None, 1000).await?;
    for e in evals {
        record_outcomes(storage, &e.worker_outcomes).await?;
    }
    Ok(())
}

/// 加载全部 learned 修正 map（(capability, worker) → 修正值；供 RouterCriteria）。
pub async fn learned_trust_map(
    storage: &LibSQLStorage,
) -> Result<std::collections::HashMap<(String, String), f64>, StorageError> {
    let mut map = std::collections::HashMap::new();
    let all = storage.list_worker_stats().await?;
    for stat in &all {
        // 效率维度：同 capability 相对表现（最快 = 1.0）
        let eff = efficiency_component(stat, &all);
        if let Some(c) = learned_trust_eff(stat, eff) {
            map.insert((stat.capability.clone(), stat.worker_id.clone()), c);
        }
    }
    Ok(map)
}

#[cfg(test)]
mod tests {
    use crate::worker_core::{
        WorkerStat, effective_trust, efficiency_component, learned_trust, learned_trust_eff,
    };

    fn stat(runs: u64, success: u64) -> WorkerStat {
        WorkerStat {
            capability: "code.test".into(),
            worker_id: "w1".into(),
            runs,
            success,
            total_duration_ms: runs * 1000,
            policy_compliance: None,
            avg_cost: None,
            first_seen: None,
            last_seen: None,
        }
    }

    #[test]
    fn sample_size_gate() {
        assert_eq!(learned_trust(&stat(4, 4)), None, "4 runs 不学习");
        assert!(learned_trust(&stat(5, 5)).is_some(), "5 runs 生效");
        assert_eq!(learned_trust(&stat(0, 0)), None);
    }

    #[test]
    fn correction_is_neutral_at_50_percent() {
        assert!((learned_trust(&stat(10, 5)).unwrap() - 0.0).abs() < 1e-9);
    }

    #[test]
    fn correction_is_capped() {
        assert!(
            (learned_trust(&stat(10, 10)).unwrap() - 0.1).abs() < 1e-9,
            "100% → +0.1 封顶"
        );
        assert!(
            (learned_trust(&stat(10, 0)).unwrap() + 0.1).abs() < 1e-9,
            "0% → −0.1 封顶"
        );
        // 60% → +0.1（超过 0.5+0.1=0.6 边界？60%−50%=0.1 恰好封顶边界）
        assert!((learned_trust(&stat(10, 6)).unwrap() - 0.1).abs() < 1e-9);
        assert!(
            (learned_trust(&stat(10, 4)).unwrap() + 0.1).abs() < 1e-9,
            "40% → −0.1"
        );
    }

    #[test]
    fn avg_duration() {
        assert_eq!(stat(4, 2).avg_duration_ms(), 1000.0);
        assert_eq!(stat(0, 0).avg_duration_ms(), 0.0);
    }

    #[test]
    fn efficiency_component_relative_performance() {
        let fast = stat(10, 10);
        let mut fast = fast;
        fast.total_duration_ms = 10 * 100; // avg 100ms
        let mut slow = stat(10, 10);
        slow.total_duration_ms = 10 * 400; // avg 400ms
        let siblings = vec![fast.clone(), slow.clone()];
        assert!(
            (efficiency_component(&fast, &siblings) - 1.0).abs() < 1e-9,
            "最快 = 1.0"
        );
        assert!(
            (efficiency_component(&slow, &siblings) - 0.25).abs() < 1e-9,
            "100/400 = 0.25"
        );
        assert_eq!(efficiency_component(&fast, &[]), 1.0, "无比较对象不惩罚");
        // 不同 capability 不参与比较
        let mut other = stat(10, 10);
        other.capability = "other".into();
        other.total_duration_ms = 1;
        assert_eq!(efficiency_component(&fast, &[other]), 1.0);
    }

    #[test]
    fn efficiency_affects_learned_correction() {
        // 同成功率 100%，但 eff 0.25 vs 1.0 → raw: 0.8×1+0.2×0.25=0.85 vs 1.0
        // adjustment: 0.35 vs 0.5 → 均封顶 0.1（上限）；用 60% 成功率区分：
        let a = stat(10, 6); // success 0.6
        let mut a_slow = a.clone();
        a_slow.total_duration_ms = 10 * 400;
        let mut a_fast = a;
        a_fast.total_duration_ms = 10 * 100;
        let siblings = vec![a_fast.clone(), a_slow.clone()];
        let eff_fast = efficiency_component(&a_fast, &siblings);
        let eff_slow = efficiency_component(&a_slow, &siblings);
        let adj_fast = learned_trust_eff(&a_fast, eff_fast).unwrap();
        let adj_slow = learned_trust_eff(&a_slow, eff_slow).unwrap();
        assert!(
            adj_fast > adj_slow,
            "同样成功率下更快者修正更高: {adj_fast} vs {adj_slow}"
        );
        // success-only 语义独立（中性点 50%）
        assert!(
            (learned_trust(&a_fast).unwrap() - 0.1).abs() < 1e-9,
            "60% → +0.1"
        );
    }

    #[test]
    fn three_value_trust_model() {
        // declared 0.80、raw 1.0（10/10 成功）→ adjustment +0.1 → effective 0.90
        let s = stat(10, 10);
        assert!((s.raw_learned() - 1.0).abs() < 1e-9);
        assert!((learned_trust(&s).unwrap() - 0.1).abs() < 1e-9);
        assert!((effective_trust(0.80, learned_trust(&s)) - 0.90).abs() < 1e-9);
        // 无样本 → adjustment None → effective = declared
        assert_eq!(effective_trust(0.80, None), 0.80);
        // clamp：declared 0.95 + 0.1 → 1.0
        assert_eq!(effective_trust(0.95, Some(0.1)), 1.0);
    }
}
