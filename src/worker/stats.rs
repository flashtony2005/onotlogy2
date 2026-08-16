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

    /// learned 统计分（raw；0..1）。
    ///
    /// MVP 公式：`success_rate`（唯一有数据维度）。
    /// 扩展位（数据接入后启用，遵循「不伪装」原则）：
    /// `raw = 0.4×success_rate + 0.2×policy_compliance + 0.1×efficiency`
    /// （quality 不入 learned——它是声明属性，入 learned 会让「初始值伪装成学习结果」；
    ///   efficiency 从 avg_duration 与声明 latency 之比推导）
    pub fn raw_learned(&self) -> f64 {
        self.success_rate()
    }
}

/// learned 修正（确定性纯函数）：
/// - runs < TRUST_MIN_RUNS → None（样本不足，不学习）
/// - 否则：(raw_learned − 0.5)，封顶 ±TRUST_MAX_CORRECTION
///   中性点 0.5（50% 成功率 = 0 修正；100% = +0.1；0% = −0.1）
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
    for stat in storage.list_worker_stats().await? {
        if let Some(c) = learned_trust(&stat) {
            map.insert((stat.capability, stat.worker_id), c);
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
