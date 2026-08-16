//! 变异提案类型（M5 拆分）：状态机 / 指标 / 配置 / 裁决。
//! 铁律：Mutation 永远不原地修改——Proposal → Benchmark → Promotion 两阶段放行。

use super::validate::validate_delta;
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
///
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
