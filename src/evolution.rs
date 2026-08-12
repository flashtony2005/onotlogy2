//! Evolution Engine（BL-003 · REQ-017）—— Agent Experience → Knowledge 闭环。
//!
//! # 第一原则：Learn ≠ Permission
//!
//! Evolution 只负责把 `Experience`（来自 V0.5 的 `wb_memory`）提炼成可观察、可验证、
//! 可撤销的 `Knowledge`。它**不修改 Policy**、**不自动绕过 Human Approval / WB2005**。
//! 任何「Knowledge → Permission」的升级（如 AutoApprove 绕过审批）属于第二阶段能力，
//! 需额外闸门（Policy 显式允许学习 + 高置信 + 多次成功 + 无失败 + 低风险），不在 MVP。
//!
//! # 闭环（5 阶段，确定性，无 LLM 参与判断）
//!
//! ```text
//! Experience (wb_memory: label="action.retain", hits=N)
//!   → Pattern   (label="pattern.action.retain", support=N, confidence=min(1, N/5))
//!   → Proposal  (kind=ActionPattern, target="retain", status=Proposed)
//!   → Validation(threshold/schema/consistency; <0.6→Rejected, ≥0.6→Validated)
//!   → LearnedKnowledge (kind=ActionPattern, action="retain", proposal_id, run_ids[])
//! ```
//!
//! 每个产物都带可追溯父引用：Proposal→Pattern、LearnedKnowledge→Proposal、各阶段→来源 run。
//! `run_evolution` 幂等（按稳定 id upsert），可重复运行不重复产出知识。

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 成为 Pattern 所需的最小 support（来自 `wb_memory` 的 hits）。
pub const PATTERN_MIN_HITS: i64 = 2;
/// 置信度除数：`confidence = min(1, hits / CONFIDENCE_DIVISOR)`。
pub const CONFIDENCE_DIVISOR: f64 = 5.0;
/// 校验通过阈值：`confidence >= VALIDATE_THRESHOLD` 才 Validated。
pub const VALIDATE_THRESHOLD: f64 = 0.6;

/// 从 `wb_memory` 的一条记忆推导 Pattern 的唯一标签。
/// `action.retain` → `pattern.action.retain`。
pub fn pattern_label_of(memory_label: &str) -> String {
    format!("pattern.{memory_label}")
}

/// 由 action 名推导 Proposal 稳定 id（保证幂等 upsert）。
pub fn proposal_id_of(action: &str) -> String {
    format!("proposal.{action}")
}

/// 由 action 名推导 LearnedKnowledge 稳定 id（保证幂等 upsert）。
pub fn learned_id_of(action: &str) -> String {
    format!("learned.{action}")
}

/// 经验（来自 `wb_memory` 的记忆条目），Evolution 的输入。
/// 仅取 label / hits，run 关联在 `run_evolution` 时由调用方注入。
#[derive(Debug, Clone)]
pub struct Experience {
    pub label: String,
    pub hits: i64,
}

impl Experience {
    /// 把记忆标签解析为 action 名（`action.retain` → `retain`）。
    pub fn action(&self) -> String {
        self.label
            .strip_prefix("action.")
            .unwrap_or(&self.label)
            .to_string()
    }
}

/// Pattern：从多条 Experience 提炼出的高频模式。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Pattern {
    /// 唯一标签，如 `pattern.action.retain`。
    pub label: String,
    /// 关联的动作名（从记忆标签解析）。
    pub action: String,
    /// 支持度 = 来源记忆的 hits。
    pub support: i64,
    /// 置信度 = min(1, hits / 5)。
    pub confidence: f64,
    /// 贡献来源 run 列表（可追溯至 AgentRun）。
    pub run_ids: Vec<String>,
}

impl Pattern {
    /// 由一条 Experience 计算置信度（确定性）。
    ///
    /// MVP 已知边界：`hits` 只增不减且无时间衰减，高频动作置信度最终趋满 1.0；
    /// 后续需引入时间窗口、失败次数等信号，避免“学习”退化为执行次数计数。
    pub fn confidence_of(hits: i64) -> f64 {
        (hits as f64 / CONFIDENCE_DIVISOR).min(1.0)
    }
}

/// Proposal 种类。MVP 仅 `ActionPattern`（动作高频模式）。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ProposalKind {
    ActionPattern,
}

/// Proposal 校验状态。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ProposalStatus {
    Proposed,
    Validated,
    Rejected,
}

/// Proposal：对某个 Pattern 提出的可验证知识建议。
/// **MVP 中 Proposal 不改变 Agent 行为**——它只是等待 Validation 的知识候选。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Proposal {
    /// 稳定 id（`proposal.<action>`），保证幂等 upsert。
    pub id: String,
    pub kind: ProposalKind,
    /// 目标动作名。
    pub target: String,
    pub confidence: f64,
    pub status: ProposalStatus,
    /// 来源 Pattern 标签（可追溯）。
    pub pattern_label: String,
    /// 贡献来源 run 列表（可追溯）。
    pub run_ids: Vec<String>,
}

/// LearnedKnowledge：通过 Validation 后沉淀的知识。
/// MVP 中它**只进入 Evolution Trace / 可被下一轮 Run 观察**，不修改 Policy、不绕过审批。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LearnedKnowledge {
    /// 稳定 id（`learned.<action>`），保证幂等 upsert。
    pub id: String,
    pub kind: ProposalKind,
    pub action: String,
    pub confidence: f64,
    /// 来源 Proposal id（可追溯至 Proposal → Pattern）。
    pub proposal_id: String,
    /// 贡献来源 run 列表（可追溯至 AgentRun）。
    pub run_ids: Vec<String>,
    /// REQ-018：自动放行资格（可撤销）。默认 true；撤销即置 false，
    /// 此后该 action 即使满足其余 4 道闸门也回退 Human Approval（决不静默放行）。
    #[serde(default = "default_true")]
    pub auto_approve_enabled: bool,
    pub created_at: String,
}

fn default_true() -> bool {
    true
}

/// Evolution 的聚合状态（供 HTTP API / UI 读取）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EvolutionState {
    pub patterns: Vec<Pattern>,
    pub proposals: Vec<Proposal>,
    pub learned: Vec<LearnedKnowledge>,
}

/// 经验挖掘：把 `hits >= PATTERN_MIN_HITS` 的记忆转成 Pattern。
///
/// - 非 `action.*` 前缀的记忆不参与（MVP 只学动作经验）；
/// - 置信度确定性：`min(1, hits / 5)`。
pub fn mine_patterns(experiences: &[Experience]) -> Vec<Pattern> {
    let mut out = Vec::new();
    for exp in experiences {
        if exp.hits < PATTERN_MIN_HITS {
            continue;
        }
        let action = exp.action();
        if action == exp.label {
            // 非 action.* 前缀，跳过（避免把无关记忆当 Pattern）
            continue;
        }
        out.push(Pattern {
            label: pattern_label_of(&exp.label),
            action: action.clone(),
            support: exp.hits,
            confidence: Pattern::confidence_of(exp.hits),
            run_ids: Vec::new(),
        });
    }
    out
}

/// 由 Pattern 提出 Proposal（默认 `Proposed`）。不改变 Agent 行为。
pub fn propose(pattern: &Pattern) -> Proposal {
    Proposal {
        id: proposal_id_of(&pattern.action),
        kind: ProposalKind::ActionPattern,
        target: pattern.action.clone(),
        confidence: pattern.confidence,
        status: ProposalStatus::Proposed,
        pattern_label: pattern.label.clone(),
        run_ids: pattern.run_ids.clone(),
    }
}

/// 结构 / 阈值校验：返回该 Proposal 应处的状态。
/// - 结构校验：`confidence` 必须落在 [0,1]；
/// - 阈值校验：`confidence >= VALIDATE_THRESHOLD` → Validated，否则 Rejected。
///
/// 注意：Validated 仅表示「该知识通过验证」，**绝不表示可修改安全策略**。
pub fn validate(proposal: &Proposal) -> ProposalStatus {
    let in_range = proposal.confidence >= 0.0 && proposal.confidence <= 1.0;
    if !in_range {
        return ProposalStatus::Rejected;
    }
    if proposal.confidence >= VALIDATE_THRESHOLD {
        ProposalStatus::Validated
    } else {
        ProposalStatus::Rejected
    }
}

/// 把存储层读出的 `run_ids` JSON 字符串解析为 Vec（容错）。
pub(crate) fn parse_run_ids(s: &str) -> Vec<String> {
    serde_json::from_str::<Vec<String>>(s).unwrap_or_default()
}

/// 把 Vec 序列化为存储用 JSON 字符串。
pub(crate) fn encode_run_ids(ids: &[String]) -> String {
    Value::from(
        ids.iter()
            .map(|s| Value::from(s.clone()))
            .collect::<Vec<_>>(),
    )
    .to_string()
}

/// 合并两个 run_ids 列表并去重（保留首次出现顺序）。
pub(crate) fn merge_run_ids(mut base: Vec<String>, extra: &[String]) -> Vec<String> {
    for r in extra {
        if !base.iter().any(|x| x == r) {
            base.push(r.to_string());
        }
    }
    base
}
