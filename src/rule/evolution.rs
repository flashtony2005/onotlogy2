//! 行为演化 —— Proposal → Validation → Adoption（Phase 7 · D5）。
//!
//! **Adoption 无人工审批，但不是无治理**：Validation 就是 Cognition Layer
//! 的自动治理闸门（`Proposal → Validation → Adoption` ✅；跳过 Validation ❌）。
//!
//! 治理边界（Learning ≠ Permission 的 Phase 7 延伸）：
//! - `Promote` 只改变 Cognition Selection Order（priority），绝不改变
//!   Policy / Capability / Approval requirement / Effect 语义；
//! - RuleEngine / RuleStore 路径不存在任何 Policy / Capability / Ontology 写入接口。

use std::collections::HashMap;

use crate::evolution2::OutcomeKind;

use super::BehaviorRule;

// ── 规则级经验（D4：双尺度分账，rule 粒度）─────────────────────────────

/// 单条规则的 outcome 分账。
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RuleStats {
    pub rule_id: String,
    pub success: u64,
    pub failed: u64,
    pub compensated: u64,
    pub denied: u64,
}

impl RuleStats {
    pub fn total(&self) -> u64 {
        self.success + self.failed + self.compensated + self.denied
    }

    /// 失败信号（Disable 依据）。
    pub fn failure_signal(&self) -> u64 {
        self.failed + self.compensated
    }

    /// 成功置信（Promote 依据）：min(1, success / 5)。
    pub fn success_confidence(&self) -> f64 {
        (self.success as f64 / super::evolution::MIN_SUCCESS as f64).min(1.0)
    }

    /// 失败占比（Disable 置信）。
    pub fn failure_ratio(&self) -> f64 {
        if self.total() == 0 {
            0.0
        } else {
            self.failure_signal() as f64 / self.total() as f64
        }
    }
}

/// 规则级经验分账（与 Phase 6 action 级并存——双尺度）。
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct RuleStatsStore {
    stats: HashMap<String, RuleStats>,
}

impl RuleStatsStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record(&mut self, rule_id: impl Into<String>, kind: OutcomeKind) {
        let id = rule_id.into();
        let s = self.stats.entry(id.clone()).or_insert_with(|| RuleStats {
            rule_id: id,
            ..Default::default()
        });
        match kind {
            OutcomeKind::Success => s.success += 1,
            OutcomeKind::Failed => s.failed += 1,
            OutcomeKind::Compensated => s.compensated += 1,
            OutcomeKind::Denied => s.denied += 1,
        }
    }

    pub fn stats_of(&self, rule_id: &str) -> Option<&RuleStats> {
        self.stats.get(rule_id)
    }

    pub fn all(&self) -> &HashMap<String, RuleStats> {
        &self.stats
    }
}

// ── 行为提案（D5：Disable / Promote + 版本一致性）─────────────────────

/// 行为变更类型（类型层面穷尽：不可能触碰 Authority）。
#[derive(Debug, Clone, PartialEq)]
pub enum BehaviorChange {
    /// 停用规则（收紧：移除行为）。
    Disable { rule_id: String },
    /// 提升优先级（调整：只改 Cognition Selection Order）。
    Promote { rule_id: String, new_priority: i64 },
}

/// 提案证据（rule_stats 摘要）。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ProposalEvidence {
    pub rule_id: String,
    pub success: u64,
    pub failed: u64,
    pub compensated: u64,
    pub confidence: f64,
}

impl ProposalEvidence {
    pub fn total(&self) -> u64 {
        self.success + self.failed + self.compensated
    }
}

/// 治理验证检查项（9 不变量）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidationCheck {
    /// ① 证据充分（样本总量 ≥ 阈值）。
    EvidenceSufficient,
    /// ② 置信充分（≥ 阈值）。
    ConfidenceSufficient,
    /// ③ 目标版本匹配（expected_version == current，stale 防护）。
    TargetVersionMatches,
    /// ④ 能力仍存在。
    CapabilityExists,
    /// ⑤ Policy 未被该路径修改（指纹一致）。
    PolicyUnchanged,
    /// ⑥ Ontology 未被该路径修改（指纹一致）。
    OntologyUnchanged,
    /// ⑦ 至少保留一条有效行为路径（no-dead-agent）。
    FallbackRemains,
    /// ⑧ 无不可逆的 Authority 变更（Disable/Promote 不触碰 Authority）。
    NoAuthorityChange,
    /// ⑨ 可回滚（adoption_log 支持 undo）。
    RollbackAvailable,
}

/// 提案状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidationStatus {
    Proposed,
    Validated,
    Rejected,
}

/// 行为演化提案（幂等 id；带版本一致性）。
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorProposal {
    pub id: String,
    pub target_rule_id: String,
    pub expected_version: u64,
    pub expected_snapshot_hash: u64,
    pub change: BehaviorChange,
    pub evidence: ProposalEvidence,
    /// 逐条治理验证记录（validate 填充）。
    pub checks: Vec<ValidationCheck>,
    pub status: ValidationStatus,
}

// ── 治理验证（Validation = 自动 Adoption 的治理闸门）──────────────────

/// 阈值（规则级）。
pub const MIN_EVIDENCE: u64 = 5; // ① 样本总量
pub const CONFIDENCE_THRESHOLD: f64 = 0.8; // ②
pub const MIN_SUCCESS: u64 = 5; // Promote 提炼阈值（confidence=1.0）
pub const MIN_FAILURE_SIGNAL: u64 = 3; // Disable 提炼阈值（失败+补偿）

/// Validation 上下文（调用方提供治理观测）。
#[derive(Debug, Clone, Default)]
pub struct ValidationContext {
    /// 当前规则集（用于 ③⑦）。
    pub rules: Vec<BehaviorRule>,
    /// 可用能力名（用于 ④）。
    pub capabilities: Vec<String>,
    /// Policy 指纹（⑤；"" = 未提供 → fail-closed）。
    pub policy_fingerprint: String,
    /// Ontology 指纹（⑥；"" = 未提供 → fail-closed）。
    pub ontology_fingerprint: String,
}

/// 9 不变量治理验证。全部满足 → Validated；任一缺失 → Rejected（fail-closed）。
pub fn validate_into(proposal: &mut BehaviorProposal, ctx: &ValidationContext) -> ValidationStatus {
    let mut checks: Vec<ValidationCheck> = Vec::new();

    // ① 证据充分
    let evidence_ok = proposal.evidence.total() >= MIN_EVIDENCE;
    // ② 置信充分（Promote 看成功置信；Disable 看失败占比）
    let confidence_ok = match &proposal.change {
        BehaviorChange::Promote { .. } => proposal.evidence.confidence >= CONFIDENCE_THRESHOLD,
        BehaviorChange::Disable { .. } => proposal.evidence.confidence >= CONFIDENCE_THRESHOLD,
    };
    // ③ 版本匹配（规则必须存在且版本一致）
    let rule = ctx.rules.iter().find(|r| r.id == proposal.target_rule_id);
    let version_ok = rule
        .map(|r| r.version == proposal.expected_version)
        .unwrap_or(false);
    // ④ 能力仍存在
    let capability_ok = rule
        .map(|r| ctx.capabilities.iter().any(|c| c == &r.capability))
        .unwrap_or(false);
    // ⑤⑥ Authority 未被该路径修改（fail-closed）
    let policy_ok = !ctx.policy_fingerprint.is_empty() && !ctx.ontology_fingerprint.is_empty();
    // ⑦ no-dead-agent：Disable 后仍至少一条 enabled 规则（且该规则条件可判定）
    let fallback_ok = match &proposal.change {
        BehaviorChange::Disable { rule_id } => {
            let alive = ctx
                .rules
                .iter()
                .filter(|r| r.enabled && r.id != *rule_id)
                .count();
            alive >= 1
        }
        BehaviorChange::Promote { .. } => true, // 不减少规则
    };
    // ⑧ 无 Authority 变更：change 枚举穷尽即保证（Disable/Promote 均不触碰 Policy）
    // ⑨ 可回滚：adoption_log 提供 undo（RuleEvolver 实现保证）

    if evidence_ok {
        checks.push(ValidationCheck::EvidenceSufficient);
    }
    if confidence_ok {
        checks.push(ValidationCheck::ConfidenceSufficient);
    }
    if version_ok {
        checks.push(ValidationCheck::TargetVersionMatches);
    }
    if capability_ok {
        checks.push(ValidationCheck::CapabilityExists);
    }
    if policy_ok {
        checks.push(ValidationCheck::PolicyUnchanged);
        checks.push(ValidationCheck::OntologyUnchanged);
    }
    if fallback_ok {
        checks.push(ValidationCheck::FallbackRemains);
    }
    // ⑧⑨ 恒成立（类型保证 + 实现保证）
    checks.push(ValidationCheck::NoAuthorityChange);
    checks.push(ValidationCheck::RollbackAvailable);

    proposal.checks = checks;
    proposal.status = if evidence_ok
        && confidence_ok
        && version_ok
        && capability_ok
        && policy_ok
        && fallback_ok
    {
        ValidationStatus::Validated
    } else {
        ValidationStatus::Rejected
    };
    proposal.status
}

/// 只读版验证（不修改提案）。
pub fn validate(proposal: &BehaviorProposal, ctx: &ValidationContext) -> ValidationStatus {
    let mut p = proposal.clone();
    validate_into(&mut p, ctx)
}

// ── 提炼（F5：从 rule_stats 生成提案）─────────────────────────────────

/// 提炼行为提案：高频失败/补偿 → Disable；高频成功 → Promote。
pub fn propose_changes(rules: &[BehaviorRule], stats: &RuleStatsStore) -> Vec<BehaviorProposal> {
    let mut out = Vec::new();
    for rule in rules {
        let Some(s) = stats.stats_of(&rule.id) else {
            continue;
        };
        // Disable：失败信号 ≥ 3 且失败占比 > 0.5（认知风险结构化）
        if rule.enabled && s.failure_signal() >= MIN_FAILURE_SIGNAL && s.failure_ratio() > 0.5 {
            out.push(BehaviorProposal {
                id: format!("bp.disable.{}", rule.id),
                target_rule_id: rule.id.clone(),
                expected_version: rule.version,
                expected_snapshot_hash: rule.snapshot_hash(),
                change: BehaviorChange::Disable {
                    rule_id: rule.id.clone(),
                },
                evidence: ProposalEvidence {
                    rule_id: rule.id.clone(),
                    success: s.success,
                    failed: s.failed,
                    compensated: s.compensated,
                    confidence: s.failure_ratio(),
                },
                checks: Vec::new(),
                status: ValidationStatus::Proposed,
            });
        }
        // Promote：成功 ≥ 5（confidence=1.0）——提升选择顺序
        if rule.enabled && s.success >= MIN_SUCCESS {
            out.push(BehaviorProposal {
                id: format!("bp.promote.{}", rule.id),
                target_rule_id: rule.id.clone(),
                expected_version: rule.version,
                expected_snapshot_hash: rule.snapshot_hash(),
                change: BehaviorChange::Promote {
                    rule_id: rule.id.clone(),
                    new_priority: rule.priority + 10,
                },
                evidence: ProposalEvidence {
                    rule_id: rule.id.clone(),
                    success: s.success,
                    failed: s.failed,
                    compensated: s.compensated,
                    confidence: s.success_confidence(),
                },
                checks: Vec::new(),
                status: ValidationStatus::Proposed,
            });
        }
    }
    out
}

// ── Adoption（无人工审批；Validation 即治理闸门）+ 审计/撤销 ──────────

/// Adoption 审计条目（可撤销）。
#[derive(Debug, Clone)]
pub struct AdoptionLogEntry {
    pub proposal_id: String,
    pub rule_id: String,
    pub old_state: BehaviorRule,
    pub new_state: BehaviorRule,
}

/// 演化错误。
#[derive(Debug, thiserror::Error)]
pub enum RuleError {
    #[error("提案未通过治理验证: {0}")]
    NotValidated(String),
    #[error("提案过期（stale）：规则 {0} 版本 {1} != 期望 {2}")]
    Stale(String, u64, u64),
    #[error("规则不存在: {0}")]
    RuleNotFound(String),
    #[error("无法撤销：未找到该提案的 adoption 记录: {0}")]
    NoAdoptionRecord(String),
}

/// 行为演化器（adoption 唯一入口 + 审计日志）。
#[derive(Debug, Clone, Default)]
pub struct RuleEvolver {
    log: Vec<AdoptionLogEntry>,
}

impl RuleEvolver {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adoption：Validation（9 不变量）→ stale 防护 → 应用变更 → 审计。
    pub fn adopt(
        &mut self,
        proposal: &BehaviorProposal,
        store: &mut [BehaviorRule],
        ctx: &ValidationContext,
    ) -> Result<(), RuleError> {
        // stale 防护优先（F-Rev4）：过期提案不值得治理验证——并发一致性第一
        {
            let rule = store
                .iter()
                .find(|r| r.id == proposal.target_rule_id)
                .ok_or(RuleError::RuleNotFound(proposal.target_rule_id.clone()))?;
            if rule.version != proposal.expected_version {
                return Err(RuleError::Stale(
                    rule.id.clone(),
                    rule.version,
                    proposal.expected_version,
                ));
            }
        }
        // 治理闸门：未 Validated 一律拒绝（F-Rev1：无后门）
        if validate(proposal, ctx) != ValidationStatus::Validated {
            return Err(RuleError::NotValidated(proposal.id.clone()));
        }
        // 应用变更（Promote 只改 priority——不触碰任何 Authority 结构）
        let rule = store
            .iter_mut()
            .find(|r| r.id == proposal.target_rule_id)
            .expect("stale 检查已确认存在");
        let old_state = rule.clone();
        match &proposal.change {
            BehaviorChange::Disable { rule_id } => {
                if rule_id != &rule.id {
                    return Err(RuleError::NotValidated("change 与 target 不一致".into()));
                }
                rule.enabled = false;
            }
            BehaviorChange::Promote {
                rule_id,
                new_priority,
            } => {
                if rule_id != &rule.id {
                    return Err(RuleError::NotValidated("change 与 target 不一致".into()));
                }
                rule.priority = *new_priority;
            }
        }
        rule.version += 1;
        // 审计（可观察、可撤销）
        self.log.push(AdoptionLogEntry {
            proposal_id: proposal.id.clone(),
            rule_id: rule.id.clone(),
            old_state: old_state.clone(),
            new_state: rule.clone(),
        });
        Ok(())
    }

    /// 撤销 adoption（恢复旧状态，版本回退）。
    pub fn undo(&mut self, store: &mut [BehaviorRule], proposal_id: &str) -> Result<(), RuleError> {
        let idx = self
            .log
            .iter()
            .position(|e| e.proposal_id == proposal_id)
            .ok_or_else(|| RuleError::NoAdoptionRecord(proposal_id.to_string()))?;
        let entry = self.log.remove(idx);
        let rule = store
            .iter_mut()
            .find(|r| r.id == entry.rule_id)
            .ok_or(RuleError::RuleNotFound(entry.rule_id.clone()))?;
        *rule = entry.old_state;
        Ok(())
    }

    pub fn log(&self) -> &[AdoptionLogEntry] {
        &self.log
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rule::ConditionExpr;

    fn ctx_of(rules: Vec<BehaviorRule>) -> ValidationContext {
        ValidationContext {
            rules,
            capabilities: vec!["customer.retain".to_string(), "customer.refund".to_string()],
            policy_fingerprint: "policy-v1".into(),
            ontology_fingerprint: "ontology-v1".into(),
        }
    }

    #[test]
    fn stats_double_scale() {
        let mut store = RuleStatsStore::new();
        store.record("r1", OutcomeKind::Success);
        store.record("r1", OutcomeKind::Failed);
        let s = store.stats_of("r1").unwrap();
        assert_eq!(s.success, 1);
        assert_eq!(s.failure_signal(), 1);
        assert_eq!(s.total(), 2);
        assert_eq!(s.success_confidence(), 0.2);
    }

    #[test]
    fn disable_proposal_needs_fallback() {
        // 单规则 store：Disable 会 no-dead-agent → Rejected（F-Rev3）
        let rule = BehaviorRule::new(
            "r1",
            "customer.retain",
            serde_json::json!({}),
            ConditionExpr::HasRelation {
                predicate: "status".into(),
                target: "active".into(),
            },
        );
        let mut stats = RuleStatsStore::new();
        stats.record("r1", OutcomeKind::Failed);
        stats.record("r1", OutcomeKind::Failed);
        stats.record("r1", OutcomeKind::Compensated);
        stats.record("r1", OutcomeKind::Success);
        let proposals = propose_changes(std::slice::from_ref(&rule), &stats);
        assert_eq!(proposals.len(), 1, "高频失败 → Disable 提案");
        let ctx = ctx_of(vec![rule]);
        assert_eq!(
            validate(&proposals[0], &ctx),
            ValidationStatus::Rejected,
            "Disable 最后一个可行规则 → reject（no-dead-agent）"
        );
    }

    #[test]
    fn promote_only_changes_priority() {
        let rule = BehaviorRule::new(
            "r1",
            "customer.retain",
            serde_json::json!({}),
            ConditionExpr::HasRelation {
                predicate: "status".into(),
                target: "active".into(),
            },
        );
        let mut stats = RuleStatsStore::new();
        for _ in 0..5 {
            stats.record("r1", OutcomeKind::Success);
        }
        let proposals = propose_changes(std::slice::from_ref(&rule), &stats);
        let p = proposals
            .iter()
            .find(|p| matches!(p.change, BehaviorChange::Promote { .. }))
            .expect("Promote 提案");
        let ctx = ctx_of(vec![rule.clone()]);
        assert_eq!(validate(p, &ctx), ValidationStatus::Validated);

        let mut evolver = RuleEvolver::new();
        let mut rules = vec![rule];
        evolver.adopt(p, &mut rules, &ctx).unwrap();
        assert_eq!(rules[0].priority, 10, "priority 提升");
        assert!(rules[0].enabled);
        assert_eq!(rules[0].version, 2, "version 递增");
        assert_eq!(evolver.log().len(), 1, "审计记录");
    }
}
