//! RuntimeDefinition —— Ontology Compiler → Runtime 的 ABI（Phase 5 · D1）。
//!
//! 中间对象：ODL / JSON / GraphQL / LLM-generated Ontology 都编译为
//! RuntimeDefinition，再统一进入 V2（Service/Capability/Policy）。
//! V1 的 Ontology Semantic Asset 被**编译**为 V2 Runtime 能理解的语义，
//! 而不是把 V1 ActionEngine 直接接进来。

use serde_json::Value;

/// 一份对象类型的运行时定义（ABI）。
#[derive(Debug, Clone, Default)]
pub struct RuntimeDefinition {
    pub object_type: String,
    pub capabilities: Vec<CapabilityDefinition>,
    pub policies: Vec<PolicyDefinition>,
    pub relations: Vec<RelationDefinition>,
}

/// 能力定义（→ compose::Capability）。
#[derive(Debug, Clone)]
pub struct CapabilityDefinition {
    /// 能力名（"Customer.retain"）。
    pub name: String,
    /// 动作语义名（Effect.action）。
    pub action: String,
    /// 必填参数（→ required_params）。
    pub params: Vec<String>,
    /// 可逆性（默认 Compensatable；creates 可撤销）。
    pub reversibility: crate::kernel::Reversibility,
    /// 策略（内联引用 policy 名；简化：直接内联）。
    pub policy: Option<PolicyDefinition>,
    /// 世界效应声明（executor 执行依据）。
    pub effects: Vec<EffectDefinition>,
}

/// 效应值来源（ABI 自足：不依赖 ir 内部类型，JSON/GraphQL/LLM ontology 同构）。
#[derive(Debug, Clone, PartialEq)]
pub enum EffectValue {
    /// 字面量。
    Literal(serde_json::Value),
    /// 引用动作请求参数（input args，按名）。
    Param(String),
    /// 目标对象自身 ID（`self`）。
    SelfRef,
    /// 目标对象字段值（`self.<field>`；MVP 求值为字面量字符串）。
    SelfField(String),
}

/// 效应声明（→ runtime_exec executor 的真实世界效果）。
#[derive(Debug, Clone, PartialEq)]
pub enum EffectDefinition {
    /// 状态更新：target --field--> 求值后的 value（State = Relation 投影）。
    Update { field: String, value: EffectValue },
    /// 创建：target --link_edge--> new_object（link_edge None 时仅创建）。
    Create {
        object_type: String,
        link_edge: Option<String>,
    },
}

/// 策略定义（→ compose::Policy）。
#[derive(Debug, Clone, Default)]
pub struct PolicyDefinition {
    pub require_approval: bool,
    pub preconditions: Vec<PreconditionDefinition>,
}

/// 前置条件定义。
#[derive(Debug, Clone)]
pub struct PreconditionDefinition {
    /// 参数名或 "self.<field>"。
    pub field: String,
    pub op: crate::compose::Cmp,
    pub value: Value,
}

/// 关系定义（世界结构声明）。
#[derive(Debug, Clone)]
pub struct RelationDefinition {
    pub predicate: String,
}
