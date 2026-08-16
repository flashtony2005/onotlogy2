//! 变异提案类型（M5 拆分）：状态机 / 指标 / 配置 / 裁决。
//! 铁律：Mutation 永远不原地修改——Proposal → Benchmark → Promotion 两阶段放行。

use serde::{Deserialize, Serialize};

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
