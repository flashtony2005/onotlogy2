//! Worker Selection Learning（Trust 轴）—— V2 经验影响下一次「把任务交给谁」。
//!
//! **严格边界**（与「自主改变自己」划清）：
//! - 学习对象只有 `(capability, worker)` 的执行统计（成功/失败/耗时）
//! - learned trust 只修正 Router 评分（Policy Filter 之后）
//! - 修正封顶 ±0.1（声明 trust 仍是主导；经验只是微调）
//! - Capability / Approval 两轴永不学习（I4/I5）
//!
//! 确定性：learned_trust(stat) 是纯函数——同一统计永远同一修正，可回放重建。

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
