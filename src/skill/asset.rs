//! SkillAsset —— 通过 Validation 的能力资产（P3 的终点）+ 生命周期 + 治理验证。

use super::ir::{
    ArtifactRef, FeedbackLoop, MetricIR, PolicyIR, Provenance, SkillType, WorkflowStep,
};
use super::lineage::Lineage;
use super::raw::RawSkill;
use super::resolver::{CapabilityResolution, ResolutionStatus};
use crate::skill::ir::SkillIR;

/// 生命周期（MVP 链式推进 + 终态）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillStatus {
    Imported,
    Parsed,
    Normalized,
    Resolved,
    Validated,
    Registered,
    Active,
    Forked,
    Experimental,
    Evaluated,
    Promoted,
    Deprecated,
    Archived,
}

/// Skill Asset（canonical internal representation）。
#[derive(Debug, Clone)]
pub struct SkillAsset {
    pub id: String,
    pub name: String,
    pub version: String,
    pub skill_type: SkillType,
    pub goal: String,
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
    pub workflow: Vec<WorkflowStep>,
    /// 已解析能力（未注册能力绝不进入——P4）。
    pub capabilities: Vec<String>,
    pub artifacts: Vec<ArtifactRef>,
    pub preconditions: Vec<String>,
    pub postconditions: Vec<String>,
    pub policies: Vec<PolicyIR>,
    pub metrics: Vec<MetricIR>,
    pub feedback_loops: Vec<FeedbackLoop>,
    pub provenance: Provenance,
    pub status: SkillStatus,
    pub lineage: Lineage,
}

/// Asset 编译错误。
#[derive(Debug, thiserror::Error)]
pub enum AssetError {
    #[error("Asset 编译失败: {0}")]
    Failed(String),
}

/// Asset Compiler：SkillIR + Resolutions → SkillAsset。
///
/// 未解析能力不进入 `capabilities`（P4：Candidate 由 Resolver 提供，此处不创造）。
#[derive(Debug, Clone)]
pub struct AssetCompiler {
    pub min_confidence: f64,
}

impl Default for AssetCompiler {
    fn default() -> Self {
        Self {
            min_confidence: 0.5,
        }
    }
}

impl AssetCompiler {
    pub fn compile(
        &self,
        _raw: &RawSkill,
        ir: &SkillIR,
        resolutions: &[CapabilityResolution],
    ) -> Result<SkillAsset, AssetError> {
        let capabilities: Vec<String> = resolutions
            .iter()
            .filter(|r| {
                r.status == ResolutionStatus::Resolved && r.confidence >= self.min_confidence
            })
            .map(|r| r.capability.clone())
            .collect();
        Ok(SkillAsset {
            id: ir.provenance.raw_skill_id.clone(),
            name: ir.identity.name.clone(),
            version: "1.0".into(),
            skill_type: ir.skill_type,
            goal: ir.goal.value.clone(),
            inputs: ir.inputs.iter().map(|i| i.value.clone()).collect(),
            outputs: ir.outputs.iter().map(|o| o.value.clone()).collect(),
            workflow: ir.workflow.iter().map(|w| w.value.clone()).collect(),
            capabilities,
            artifacts: ir.artifacts.iter().map(|a| a.value.clone()).collect(),
            preconditions: ir.preconditions.iter().map(|p| p.value.clone()).collect(),
            postconditions: ir.postconditions.iter().map(|p| p.value.clone()).collect(),
            policies: ir.policies.iter().map(|p| p.value.clone()).collect(),
            metrics: ir.metrics.iter().map(|m| m.value.clone()).collect(),
            feedback_loops: ir.feedback_loops.iter().map(|f| f.value.clone()).collect(),
            provenance: ir.provenance.clone(),
            status: SkillStatus::Imported,
            lineage: Lineage::new("1.0"),
        })
    }
}

/// Validation 报告。
#[derive(Debug, Clone, Default)]
pub struct ValidationReport {
    pub ok: bool,
    pub errors: Vec<String>,
}

/// 治理验证（P2：AI 理解 ≠ 授权——能否进入系统由 Validator 决定）。
///
/// - Schema：必填字段（name/goal/workflow 非空）
/// - Semantic：每个 capability 在已知集中（Input → Capability → Output 连通）
pub struct Validator<'a> {
    known: Vec<String>,
    _marker: std::marker::PhantomData<&'a ()>,
}

impl<'a> Validator<'a> {
    pub fn new<S: AsRef<str>>(known: &[S]) -> Self {
        Self {
            known: known.iter().map(|s| s.as_ref().to_string()).collect(),
            _marker: std::marker::PhantomData,
        }
    }

    pub fn validate(&self, asset: &SkillAsset) -> ValidationReport {
        let mut errors: Vec<String> = Vec::new();
        // Schema
        if asset.name.is_empty() {
            errors.push("Schema: name 为空".into());
        }
        if asset.goal.is_empty() {
            errors.push("Schema: goal 为空".into());
        }
        if asset.workflow.is_empty() {
            errors.push("Schema: workflow 为空".into());
        }
        // Semantic：能力必须连通（已在已知集）
        for cap in &asset.capabilities {
            if !self.known.contains(cap) {
                errors.push(format!(
                    "Semantic: 能力 {cap} 不在已知集（Input→Capability→Output 不连通）"
                ));
            }
        }
        ValidationReport {
            ok: errors.is_empty(),
            errors,
        }
    }
}
