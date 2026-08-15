//! Evolution —— 自进化（Phase 6）：Learning Layer 与 Authority Layer 的架构隔离。
//!
//! 核心不变量（置顶）：
//! > **Evolution learns from execution, but learning never becomes authority.**
//!
//! 三个不等式（代码与测试共同保证）：
//! ```text
//! Pattern ≠ Permission
//! Pattern ≠ Policy
//! Pattern ≠ Capability
//! ```
//!
//! 双通道架构（类型层面分离产物，绝不交叉）：
//! - `SuccessPattern` → Proposal → Validation → EvolutionGate → Permission 替代审批
//! - `FailurePattern` → RiskSignal → AgentView.evolution → Cognition → Decision
//!
//! 演化状态与 Memory 同为**认知状态**（P4：不进 fact 层；持久化并入 Memory 延期项）。

use std::collections::HashMap;

pub mod gate;

pub use gate::{GateVerdict, VALIDATE_THRESHOLD};

/// Fiber outcome 分类（埋点信号，E1 分账）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutcomeKind {
    /// Effect 已提交（执行成功）。
    Success,
    /// 执行失败（Fiber Failed；计入 Gate 的 FailureCount）。
    Failed,
    /// 发生补偿回滚（单独计数，进入 FailurePattern 风险信号，不参与 Gate 公式）。
    Compensated,
    /// 业务拒绝（Policy Deny；Effect 未提交）。
    Denied,
}

/// 经验分账（单动作；key 即 action，故无冗余字段）。
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Experience {
    pub success: u64,
    pub failed: u64,
    pub compensated: u64,
    pub denied: u64,
}

impl Experience {
    pub fn total(&self) -> u64 {
        self.success + self.failed + self.compensated + self.denied
    }

    /// 置信度（确定性）：`min(1, success / 5)`。
    pub fn confidence(&self) -> f64 {
        (self.success as f64 / gate::CONFIDENCE_DIVISOR).min(1.0)
    }
}

/// Pattern 双通道：**类型层面**分离产物。
#[derive(Debug, Clone, PartialEq)]
pub enum Pattern {
    /// 高频成功模式——唯一可产生 Proposal 的通道。
    Success(SuccessPattern),
    /// 高频失败/补偿模式——仅可产生 RiskSignal（认知警告，永不进入 Policy）。
    Failure(FailurePattern),
}

/// 成功模式（Permission Candidate 通道）。
#[derive(Debug, Clone, PartialEq)]
pub struct SuccessPattern {
    pub action: String,
    pub success: u64,
    pub confidence: f64,
}

/// 失败模式（Cognitive Warning 通道）。
#[derive(Debug, Clone, PartialEq)]
pub struct FailurePattern {
    pub action: String,
    pub failed: u64,
    pub compensated: u64,
    /// 风险分：失败+补偿 占总执行比例。
    pub risk_score: f64,
}

/// 认知风险信号（FailurePattern 的唯一产物）。
///
/// **永不进入 Policy / Deny / Capability / Permission**——只影响决策质量。
#[derive(Debug, Clone, PartialEq)]
pub struct RiskSignal {
    pub action: String,
    pub failed: u64,
    pub compensated: u64,
    pub risk_score: f64,
}

/// Proposal 校验状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ProposalStatus {
    Proposed,
    Validated,
    Rejected,
}

/// 演化提案（SuccessPattern 通道；幂等 id `proposal.<action>`）。
#[derive(Debug, Clone, PartialEq)]
pub struct Proposal {
    pub id: String,
    pub target: String,
    pub confidence: f64,
    pub status: ProposalStatus,
}

/// 演化权限状态（D5 语义；MVP 内部不做完整状态机转移，但保留语义）。
///
/// - `Disabled`：当前未开启（默认）
/// - `Candidate`：已提出 Proposal
/// - `Validated`：通过验证
/// - `Enabled`：已启用（可 Gate 放行）
/// - `Revoked`：曾获得 Evolution Permission 但已撤销——**此后即使重新满足
///   全部条件也永不 Gate 放行**（杜绝 `enabled = false → 重新满足 → 复活`）
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum EvolutionPermissionState {
    Disabled,
    Candidate,
    Validated,
    Enabled,
    Revoked,
}

/// 单动作演化权限记录。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LearnedPermission {
    pub action: String,
    pub state: EvolutionPermissionState,
    pub confidence: f64,
    pub proposal_id: String,
    /// `false` = Revoked（或从未启用）。
    pub auto_approve_enabled: bool,
}

/// 演化引擎（认知状态；持有经验 → 模式 → 提案 → 权限）。
#[derive(Debug, Clone, Default)]
pub struct EvolutionEngine {
    experiences: HashMap<String, Experience>,
    patterns: Vec<Pattern>,
    proposals: HashMap<String, Proposal>,
    permissions: HashMap<String, LearnedPermission>,
    /// 全局开关（默认关闭——任何 Gate 放行都必须显式开启）。
    gate_enabled: bool,
}

impl EvolutionEngine {
    pub fn new() -> Self {
        Self::default()
    }

    /// 显式开启 Gate（默认关闭；未开启 = 一切回退人工审批）。
    pub fn with_gate_enabled(mut self, enabled: bool) -> Self {
        self.gate_enabled = enabled;
        self
    }

    pub fn is_gate_enabled(&self) -> bool {
        self.gate_enabled
    }

    // ── 采集（E1：Fiber outcome 分账）──────────────────────────────────
    pub fn record(&mut self, action: impl Into<String>, kind: OutcomeKind) {
        let e = self.experiences.entry(action.into()).or_default();
        match kind {
            OutcomeKind::Success => e.success += 1,
            OutcomeKind::Failed => e.failed += 1,
            OutcomeKind::Compensated => e.compensated += 1,
            OutcomeKind::Denied => e.denied += 1,
        }
    }

    // ── 提炼（E2：双通道 Pattern）──────────────────────────────────────
    /// 从经验提炼双通道模式（幂等：每次全量重提炼）。
    pub fn mine(&mut self) {
        self.patterns.clear();
        for (action, exp) in &self.experiences {
            // Success 通道：成功 ≥ 阈值
            if exp.success >= gate::MIN_SUCCESS_PATTERN {
                self.patterns.push(Pattern::Success(SuccessPattern {
                    action: action.clone(),
                    success: exp.success,
                    confidence: exp.confidence(),
                }));
            }
            // Failure 通道：失败+补偿 ≥ 阈值（风险信号）
            let risk = exp.failed + exp.compensated;
            if risk >= gate::MIN_RISK_PATTERN {
                self.patterns.push(Pattern::Failure(FailurePattern {
                    action: action.clone(),
                    failed: exp.failed,
                    compensated: exp.compensated,
                    risk_score: risk as f64 / exp.total().max(1) as f64,
                }));
            }
        }
    }

    // ── 提案（E3：仅 SuccessPattern 通道）──────────────────────────────
    /// SuccessPattern → Proposal（幂等 id `proposal.<action>`；FailurePattern 不参与）。
    pub fn propose(&mut self) {
        for p in &self.patterns {
            if let Pattern::Success(sp) = p {
                let id = format!("proposal.{}", sp.action);
                self.proposals.entry(id.clone()).or_insert(Proposal {
                    id,
                    target: sp.action.clone(),
                    confidence: sp.confidence,
                    status: ProposalStatus::Proposed,
                });
            }
        }
    }

    // ── 验证（Validation：置信度 ≥ 0.8）────────────────────────────────
    /// Proposal → Validated/Rejected；Validated 同步提升权限状态。
    /// **Revoked 是吸收态**：撤销后即使再次 Validated 也不再恢复放行资格。
    pub fn validate(&mut self) {
        for prop in self.proposals.values_mut() {
            if prop.status == ProposalStatus::Proposed {
                prop.status = if prop.confidence >= gate::VALIDATE_THRESHOLD {
                    ProposalStatus::Validated
                } else {
                    ProposalStatus::Rejected
                };
            }
            if prop.status != ProposalStatus::Validated {
                continue;
            }
            // permissions 的 key = 动作语义名（prop.target），非 proposal id
            let perm = self
                .permissions
                .entry(prop.target.clone())
                .or_insert(LearnedPermission {
                    action: prop.target.clone(),
                    state: EvolutionPermissionState::Disabled,
                    confidence: prop.confidence,
                    proposal_id: prop.id.clone(),
                    auto_approve_enabled: false,
                });
            perm.confidence = prop.confidence;
            match perm.state {
                // 状态提升：Disabled/Candidate → Validated（语义上即可放行）
                EvolutionPermissionState::Disabled | EvolutionPermissionState::Candidate => {
                    perm.state = EvolutionPermissionState::Validated;
                    perm.auto_approve_enabled = true;
                }
                // Revoked 吸收态：绝不提升
                _ => {}
            }
        }
    }

    // ── Gate（D4：7 条件 AND；失败 = WAIT 非 DENY）─────────────────────
    /// EvolutionGate 判定（只读，不修改状态；审计由调用方落事实层）。
    /// 公式：Enabled ∧ Validated ∧ Confidence≥0.8 ∧ Success≥3 ∧ Failure=0
    ///       ∧ Compensable ∧ ¬Irreversible
    pub fn gate(&self, action: &str, reversibility: crate::kernel::Reversibility) -> bool {
        let Some(perm) = self.permissions.get(action) else {
            return false;
        };
        let exp = self.experiences.get(action);
        gate::check(
            self.gate_enabled,
            perm.state,
            perm.auto_approve_enabled,
            perm.confidence,
            exp.map(|e| e.success).unwrap_or(0),
            exp.map(|e| e.failed).unwrap_or(0),
            reversibility,
        ) == GateVerdict::Pass
    }

    // ── 撤销（D5：Revoked 永久禁止）────────────────────────────────────
    /// 撤销该动作的 Evolution Permission——此后永不 Gate 放行。
    pub fn revoke(&mut self, action: &str) {
        if let Some(p) = self.permissions.get_mut(action) {
            p.state = EvolutionPermissionState::Revoked;
            p.auto_approve_enabled = false;
        }
    }

    // ── 认知层读取（E4：RiskSignal → AgentView.evolution）──────────────
    /// FailurePattern → RiskSignal（唯一认知通道）。
    pub fn risk_signals(&self) -> Vec<RiskSignal> {
        self.patterns
            .iter()
            .filter_map(|p| match p {
                Pattern::Failure(fp) => Some(RiskSignal {
                    action: fp.action.clone(),
                    failed: fp.failed,
                    compensated: fp.compensated,
                    risk_score: fp.risk_score,
                }),
                _ => None,
            })
            .collect()
    }

    // ── 只读查询（测试/上层）───────────────────────────────────────────
    pub fn experience(&self, action: &str) -> Option<&Experience> {
        self.experiences.get(action)
    }

    pub fn patterns(&self) -> &[Pattern] {
        &self.patterns
    }

    pub fn proposal(&self, action: &str) -> Option<&Proposal> {
        // proposals 的 key 是稳定 id `proposal.<action>`
        self.proposals.get(&format!("proposal.{action}"))
    }

    pub fn proposals(&self) -> &HashMap<String, Proposal> {
        &self.proposals
    }

    pub fn permission(&self, action: &str) -> Option<&LearnedPermission> {
        self.permissions.get(action)
    }

    /// 持久化快照（patterns/proposals 可由 mine/propose/validate 重建——只存不可再生的状态）。
    pub fn snapshot(&self) -> EvolutionSnapshot {
        EvolutionSnapshot {
            experiences: self.experiences.clone(),
            permissions: self.permissions.clone(),
            gate_enabled: self.gate_enabled,
        }
    }

    pub fn restore(snap: EvolutionSnapshot) -> Self {
        Self {
            experiences: snap.experiences,
            patterns: Vec::new(),
            proposals: HashMap::new(),
            permissions: snap.permissions,
            gate_enabled: snap.gate_enabled,
        }
    }
}

/// EvolutionEngine 持久化快照（经验 + 权限 + 开关；模式/提案重建）。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EvolutionSnapshot {
    pub experiences: HashMap<String, Experience>,
    pub permissions: HashMap<String, LearnedPermission>,
    pub gate_enabled: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel::Reversibility;

    #[test]
    fn experience_confidence_is_deterministic() {
        let mut ev = EvolutionEngine::new();
        for _ in 0..10 {
            ev.record("RetainCustomer", OutcomeKind::Success);
        }
        let exp = ev.experience("RetainCustomer").expect("经验");
        assert_eq!(exp.success, 10);
        assert_eq!(exp.confidence(), 1.0);
        assert_eq!(Experience::default().confidence(), 0.0);
    }

    #[test]
    fn revoked_is_absorbing_after_validate() {
        let mut ev = EvolutionEngine::new();
        for _ in 0..5 {
            ev.record("RetainCustomer", OutcomeKind::Success);
        }
        ev.mine();
        ev.propose();
        ev.validate();
        assert_eq!(
            ev.permission("RetainCustomer").map(|p| p.state),
            Some(EvolutionPermissionState::Validated)
        );
        ev.revoke("RetainCustomer");
        assert_eq!(
            ev.permission("RetainCustomer").map(|p| p.state),
            Some(EvolutionPermissionState::Revoked)
        );
        // 吸收态：再 validate 也不复活
        ev.validate();
        assert_eq!(
            ev.permission("RetainCustomer").map(|p| p.state),
            Some(EvolutionPermissionState::Revoked)
        );
        assert!(!ev.gate("RetainCustomer", Reversibility::Compensatable));
    }

    #[test]
    fn failure_pattern_never_produces_proposal() {
        let mut ev = EvolutionEngine::new();
        ev.record("RefundCustomer", OutcomeKind::Failed);
        ev.record("RefundCustomer", OutcomeKind::Failed);
        ev.record("RefundCustomer", OutcomeKind::Compensated);
        ev.mine();
        assert!(
            ev.patterns()
                .iter()
                .any(|p| matches!(p, Pattern::Failure(_))),
            "应有 FailurePattern"
        );
        ev.propose();
        assert!(
            ev.proposal("RefundCustomer").is_none(),
            "FailurePattern 不能产生 Proposal（I1）"
        );
        assert!(
            ev.risk_signals()
                .iter()
                .any(|r| r.action == "RefundCustomer"),
            "FailurePattern 应产生 RiskSignal"
        );
    }
}
