//! Worker Selection Learning（Trust 轴）—— V2 经验影响下一次「把任务交给谁」。
//!
//! **严格边界**（与「自主改变自己」划清）：
//! - 学习对象只有 `(capability, worker)` 的执行统计（成功/失败/耗时）
//! - learned trust 只修正 Router 评分（Policy Filter 之后）
//! - 修正封顶 ±0.1（声明 trust 仍是主导；经验只是微调）
//! - Capability / Approval 两轴永不学习（I4/I5）
//!
//! 确定性：learned_trust(stat) 是纯函数——同一统计永远同一修正，可回放重建。

use crate::storage::LibSQLStorage;
use crate::storage::StorageError;

/// 学习生效的最小样本量（对齐仓库阈值模式：hits≥2 / min_runs=5）。
pub const TRUST_MIN_RUNS: u64 = 5;
/// 修正封顶（声明 trust 仍是主导；经验只是 ±0.1 的微调）。
pub const TRUST_MAX_CORRECTION: f64 = 0.1;

/// (capability, worker) 执行统计（派生层一行）。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WorkerStat {
    pub capability: String,
    pub worker_id: String,
    pub runs: u64,
    pub success: u64,
    pub total_duration_ms: u64,
    /// Policy 合规率（per-worker 评测数据接入后的扩展位；当前无数据源 → None）。
    pub policy_compliance: Option<f64>,
    /// 平均成本（per-worker cost 数据接入后的扩展位；当前无数据源 → None）。
    pub avg_cost: Option<f64>,
    /// 首次记录时间（first_seen；表列 created_at）。
    pub first_seen: Option<String>,
    /// 最近记录时间（last_seen；表列 updated_at）。
    pub last_seen: Option<String>,
}

impl WorkerStat {
    pub fn success_rate(&self) -> f64 {
        if self.runs == 0 {
            return 0.0;
        }
        self.success as f64 / self.runs as f64
    }

    /// 平均耗时（ms；0 runs → 0）。
    pub fn avg_duration_ms(&self) -> f64 {
        if self.runs == 0 {
            return 0.0;
        }
        self.total_duration_ms as f64 / self.runs as f64
    }

    /// learned 统计分（raw；0..1）——success 分量（历史语义，兼容保留）。
    ///
    /// 完整公式（数据接入后逐维度启用，遵循「不伪装」原则）：
    /// `raw = 0.4×success_rate + 0.2×policy_compliance + 0.1×efficiency`，
    /// 权重按可得维度归一化：
    /// - success_rate（✓ 有数据源）
    /// - efficiency（✓ 本轮激活：同 capability 相对表现）
    /// - policy_compliance（✗ 无数据源：deny 事件发生在 route 前，无 worker 归属，
    ///   归因即造假——保持 None）
    /// - quality（✗ 不入 learned：声明属性入学习会让「初始值伪装成学习结果」）
    pub fn raw_learned(&self) -> f64 {
        self.success_rate()
    }
}

/// 相对效率分量（同 capability worker 间比较；纯统计，无外部基准）：
/// 最快 worker 得 1.0，其余按 `min_avg / avg` 比例；无比较对象 → 1.0（不惩罚）。
pub fn efficiency_component(stat: &WorkerStat, siblings: &[WorkerStat]) -> f64 {
    let min_avg = siblings
        .iter()
        .filter(|s| s.capability == stat.capability && s.avg_duration_ms() > 0.0)
        .map(|s| s.avg_duration_ms())
        .fold(f64::INFINITY, f64::min);
    if !min_avg.is_finite() || min_avg <= 0.0 {
        return 1.0;
    }
    (min_avg / stat.avg_duration_ms().max(1.0)).clamp(0.0, 1.0)
}

/// learned 统计分（含效率维度）：`0.8×success_rate + 0.2×efficiency`（0.4/0.1 归一化；
/// policy_compliance 无数据源暂缺）。eff ∈ [0,1]。
pub fn raw_learned_with_efficiency(stat: &WorkerStat, efficiency: f64) -> f64 {
    0.8 * stat.success_rate() + 0.2 * efficiency.clamp(0.0, 1.0)
}

/// learned 修正（确定性纯函数；eff=1.0 时退化为 success-only 语义）：
/// - runs < TRUST_MIN_RUNS → None（样本不足，不学习）
/// - 否则：(raw − 0.5)，封顶 ±TRUST_MAX_CORRECTION（中性点 0.5：50% = 0 修正）
pub fn learned_trust_eff(stat: &WorkerStat, efficiency: f64) -> Option<f64> {
    if stat.runs < TRUST_MIN_RUNS {
        return None;
    }
    Some(
        (raw_learned_with_efficiency(stat, efficiency) - 0.5)
            .clamp(-TRUST_MAX_CORRECTION, TRUST_MAX_CORRECTION),
    )
}

/// learned 修正（success-only；中性点 50% 的冻结语义，兼容既有调用与测试）。
pub fn learned_trust(stat: &WorkerStat) -> Option<f64> {
    if stat.runs < TRUST_MIN_RUNS {
        return None;
    }
    Some((stat.raw_learned() - 0.5).clamp(-TRUST_MAX_CORRECTION, TRUST_MAX_CORRECTION))
}

/// effective trust = declared + learned 修正（三值模型：declared / learned / effective）。
pub fn effective_trust(declared: f64, adjustment: Option<f64>) -> f64 {
    (declared + adjustment.unwrap_or(0.0)).clamp(0.0, 1.0)
}

/// 记录一批执行结果（run 结束后调用；幂等累积）。
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
    use super::*;

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
