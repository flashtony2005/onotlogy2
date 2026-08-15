//! SkillIR —— AI 对第三方 Skill 的标准语义理解（P3：先 IR 再 Asset）。
//!
//! 不是最终执行格式；每项推断携带 `{ confidence, evidence, source_span }`
//! （P5：AI 推断可追溯）。

/// 源跨度（行号区间，1-based）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceSpan {
    pub start: usize,
    pub end: usize,
}

impl SourceSpan {
    pub fn at(line: usize) -> Self {
        Self {
            start: line,
            end: line,
        }
    }
}

/// 带追溯的推断值（confidence + evidence + source_span）。
#[derive(Debug, Clone)]
pub struct Inferred<T> {
    pub value: T,
    pub confidence: f64,
    pub evidence: String,
    pub span: SourceSpan,
}

impl<T> Inferred<T> {
    pub fn new(value: T, span: SourceSpan) -> Self {
        Self {
            value,
            confidence: 1.0,
            evidence: String::new(),
            span,
        }
    }
}

/// Skill 类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillType {
    Atomic,
    Composite,
    Workflow,
    Strategy,
    Optimizer,
}

impl SkillType {
    pub fn as_str(&self) -> &'static str {
        match self {
            SkillType::Atomic => "Atomic",
            SkillType::Composite => "Composite",
            SkillType::Workflow => "Workflow",
            SkillType::Strategy => "Strategy",
            SkillType::Optimizer => "Optimizer",
        }
    }
}

/// 工作流步骤（阶段）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowStep {
    pub name: String,
    pub description: String,
}

/// 能力引用（`- capability: video.generate`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityRef {
    pub capability: String,
}

/// 工件引用（`- artifact: VideoScript`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactRef {
    pub name: String,
    pub artifact_type: String,
}

/// 关系引用（Ontology Discovery：`- relation: Storyboard contains Scene`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelationIR {
    pub source: String,
    pub predicate: String,
    pub target: String,
}

/// 策略推断（自然语言 → 结构化候选）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyIR {
    pub condition: String,
    pub requirement: String,
}

/// 指标。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetricIR {
    pub name: String,
    pub domain: Option<String>,
}

/// 反馈环（`- feedback: A -> B`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeedbackLoop {
    pub from: String,
    pub to: String,
}

/// 来源（可追溯至 RawSkill）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Provenance {
    pub raw_skill_id: String,
    pub parser: String,
}

/// SkillIR：AI 标准语义理解（MVP 14 段子集）。
#[derive(Debug, Clone)]
pub struct SkillIR {
    pub identity: SkillIdentity,
    pub skill_type: SkillType,
    pub goal: Inferred<String>,
    pub inputs: Vec<Inferred<String>>,
    pub outputs: Vec<Inferred<String>>,
    /// 工作流阶段（顺序）。
    pub workflow: Vec<Inferred<WorkflowStep>>,
    /// 子技能（Composite 判定依据）。
    pub sub_skills: Vec<Inferred<String>>,
    pub capabilities: Vec<Inferred<CapabilityRef>>,
    pub artifacts: Vec<Inferred<ArtifactRef>>,
    pub preconditions: Vec<Inferred<String>>,
    pub postconditions: Vec<Inferred<String>>,
    pub policies: Vec<Inferred<PolicyIR>>,
    pub metrics: Vec<Inferred<MetricIR>>,
    pub feedback_loops: Vec<Inferred<FeedbackLoop>>,
    pub constraints: Vec<Inferred<String>>,
    pub relations: Vec<Inferred<RelationIR>>,
    pub provenance: Provenance,
}

/// 身份段。
#[derive(Debug, Clone)]
pub struct SkillIdentity {
    pub name: String,
    pub description: String,
}

/// 解析错误。
#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("解析失败: {0}")]
    Failed(String),
}
