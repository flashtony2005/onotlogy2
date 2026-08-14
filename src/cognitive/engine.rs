//! CognitiveEngine —— 认知引擎抽象（D4：MVP 确定性实现，LLM 仅 Adapter）。
//!
//! 职责（MVP）：Intent Recognition / Parameter Extraction / Capability Selection /
//! Decision Generation。换引擎（Pi/Claude）不修改 Agent Loop——只实现同一 trait。
//!
//! P4：决策输出（Decision）是唯一进入 fact 层的认知产物；
//! Observation/Hypothesis 由上层显式管理（不在此 trait）。

use serde_json::Value;

use super::memory::MemoryItem;

/// 认知引擎错误。
#[derive(Debug, thiserror::Error)]
pub enum CognitiveError {
    #[error("认知引擎错误: {0}")]
    Engine(String),
    #[error("决策引用了未知能力: {0}")]
    UnknownCapability(String),
}

/// 决策：执行哪个能力（+ 参数 + 置信度）。
#[derive(Debug, Clone)]
pub struct Decision {
    pub capability: String,
    pub args: Value,
    pub confidence: f64,
}

/// 观察视图（决策输入）：世界快照 + 相关关系 + 召回记忆 + 可用能力。
#[derive(Debug, Clone)]
pub struct AgentView {
    pub goal: String,
    /// 世界快照（观察构建；fact 层 relations 的只读投影）。
    pub world: Value,
    /// 相关关系 (source, predicate, target)。
    pub related: Vec<(String, String, String)>,
    /// 召回记忆（D3：认知状态注入）。
    pub memory: Vec<MemoryItem>,
    /// 可用能力名（Service 提供）。
    pub capabilities: Vec<String>,
}

impl AgentView {
    /// 世界是否已达成某关系（D5：Agent 不信任自己的执行，重新观察确认）。
    pub fn has_relation(&self, predicate: &str, target: &str) -> bool {
        self.related
            .iter()
            .any(|(_, p, t)| p == predicate && t == target)
    }
}

/// 认知引擎（可替换）。
pub trait CognitiveEngine: Send + Sync {
    /// 基于观察视图决策；`None` = 无更多动作（配合 expected_relations 判定完成）。
    fn decide(&self, view: &AgentView) -> Result<Option<Decision>, CognitiveError>;
}

/// 决策条件谓词（view 检查；用于防重/断言观察注入）。
pub type WhenPredicate = Box<dyn Fn(&AgentView) -> bool + Send + Sync>;

/// 决策计划步（FakeEngine 用）。
pub struct PlanStep {
    pub capability: String,
    pub args: Value,
    /// 执行条件（view 谓词）；None = 无条件。可断言观察/记忆注入。
    pub when: Option<WhenPredicate>,
}

/// 确定性决策引擎（Phase 3 闭环）。
///
/// **无状态条件引擎**：每轮 poll 独立评估（与真实 LLM poll 语义一致）——
/// 第一个 `when` 满足的步骤触发；全部不满足 → None。
/// 调用方须在 `when` 中用世界状态防重（如 `!view.has_relation(...)`），
/// 即「目标达成前持续行动」的目标函数语义（D5）。
pub struct FakeEngine {
    plan: Vec<PlanStep>,
}

impl FakeEngine {
    pub fn new(plan: Vec<PlanStep>) -> Self {
        Self { plan }
    }
}

impl CognitiveEngine for FakeEngine {
    fn decide(&self, view: &AgentView) -> Result<Option<Decision>, CognitiveError> {
        for step in &self.plan {
            let gate = step.when.as_ref().map(|f| f(view)).unwrap_or(true);
            if gate {
                return Ok(Some(Decision {
                    capability: step.capability.clone(),
                    args: step.args.clone(),
                    confidence: 0.9,
                }));
            }
        }
        Ok(None)
    }
}

/// 无决策引擎（A5 用：直接判定完成/失败）。
pub struct NoOpEngine;

impl CognitiveEngine for NoOpEngine {
    fn decide(&self, _view: &AgentView) -> Result<Option<Decision>, CognitiveError> {
        Ok(None)
    }
}
