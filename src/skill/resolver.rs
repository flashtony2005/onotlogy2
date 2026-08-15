//! Resolver —— Capability / Ontology / Policy 语义解析到 V2 资产（P4：不凭空创造）。

use super::ir::SkillIR;

/// 解析状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolutionStatus {
    /// 已在能力注册表找到实现。
    Resolved,
    /// 未注册——只能成为 Candidate，绝不成为真实 Capability。
    Unresolved,
}

/// 单条能力解析结果。
#[derive(Debug, Clone)]
pub struct CapabilityResolution {
    pub capability: String,
    pub status: ResolutionStatus,
    pub confidence: f64,
}

/// 候选状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CandidateStatus {
    /// 待注册/待人工确认。
    Unresolved,
}

/// 候选信息。
#[derive(Debug, Clone)]
pub struct CandidateInfo {
    pub id: String,
    pub semantic_action: String,
    pub confidence: f64,
    pub status: CandidateStatus,
}

/// CapabilityCandidate —— 未注册能力的占位。
///
/// **类型层面与真实 Capability 分离**：`Resolved` 分支要求真实注册表条目，
/// 本模块永不构造它——候选只能是 `Unresolved`（P4）。
#[derive(Debug, Clone)]
pub enum CapabilityCandidate {
    Resolved(CapabilityResolution),
    Unresolved(CandidateInfo),
}

impl CapabilityCandidate {
    pub fn id(&self) -> &str {
        match self {
            CapabilityCandidate::Resolved(r) => &r.capability,
            CapabilityCandidate::Unresolved(i) => &i.id,
        }
    }

    pub fn semantic_action(&self) -> &str {
        match self {
            CapabilityCandidate::Resolved(r) => &r.capability,
            CapabilityCandidate::Unresolved(i) => &i.semantic_action,
        }
    }

    pub fn status(&self) -> CandidateStatus {
        match self {
            CapabilityCandidate::Resolved(_) => CandidateStatus::Unresolved, // 占位（本模块永不构造）
            CapabilityCandidate::Unresolved(i) => i.status,
        }
    }
}

/// Skill Resolver：已知能力注册表（可后续接 Phase 10 Plugin Registry）。
pub struct SkillResolver<'a> {
    known: Vec<String>,
    _marker: std::marker::PhantomData<&'a ()>,
}

impl<'a> SkillResolver<'a> {
    pub fn new<S: AsRef<str>>(known: &[S]) -> Self {
        Self {
            known: known.iter().map(|s| s.as_ref().to_string()).collect(),
            _marker: std::marker::PhantomData,
        }
    }

    /// 逐能力解析：known 命中 → Resolved；否则 Unresolved。
    pub fn resolve_capabilities(&self, ir: &SkillIR) -> Vec<CapabilityResolution> {
        ir.capabilities
            .iter()
            .map(|c| {
                let capability = c.value.capability.clone();
                let status = if self.known.contains(&capability) {
                    ResolutionStatus::Resolved
                } else {
                    ResolutionStatus::Unresolved
                };
                CapabilityResolution {
                    capability,
                    status,
                    confidence: c.confidence,
                }
            })
            .collect()
    }

    /// 未解析能力 → Candidate（UNRESOLVED；绝不产生真实 Capability）。
    pub fn unresolved_candidates(
        &self,
        resolutions: &[CapabilityResolution],
    ) -> Vec<CapabilityCandidate> {
        resolutions
            .iter()
            .filter(|r| r.status == ResolutionStatus::Unresolved)
            .map(|r| {
                CapabilityCandidate::Unresolved(CandidateInfo {
                    id: format!("candidate.{}", r.capability),
                    semantic_action: r.capability.clone(),
                    confidence: r.confidence,
                    status: CandidateStatus::Unresolved,
                })
            })
            .collect()
    }
}
