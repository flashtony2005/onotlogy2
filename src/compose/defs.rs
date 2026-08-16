//! defs —— 定义与适配层（M3：ontology/ 并入 compose）。
//!
//! 组合层不只是运行时类型（capability/policy/service/task），还持有**定义**：
//! `RuntimeDefinition` 是 Compiler → Runtime 的 ABI（ODL / JSON / GraphQL / LLM 生成的
//! Ontology 都编译为同一中间对象），`ir_to_runtime` / `ir_to_services` 是 ODL 编译器
//! 到组合层的两步适配。V1 时期这组概念住在独立的 `ontology/` 包；M3 并入 compose——
//! 「ontology」一词留给编译器前端（ir/odl/codegen），定义与映射归组合层。

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
use super::policy::{Policy, Precondition};
use crate::abi::ir::{CreateEffect, FieldUpdate, OntologyIR};
use crate::compose::{Capability, Service};
use crate::kernel::Reversibility;

/// 动作语义名：`retain` → `RetainCustomer`（首字母大写 + 类型前缀）。
fn action_semantic_name(object_type: &str, action: &str) -> String {
    let mut chars = action.chars();
    let cap = chars.next().map(|c| c.to_ascii_uppercase()).unwrap_or('A');
    format!("{cap}{}{object_type}", chars.as_str())
}

/// ODL `creates:` → EffectDefinition（含 link_edge）。
fn create_effect(c: &CreateEffect) -> EffectDefinition {
    EffectDefinition::Create {
        object_type: c.object_type.clone(),
        link_edge: c.link_edge.clone(),
    }
}

/// ODL `updates:` → EffectDefinition（value 表达式编译为 EffectValue，执行期求值）。
fn update_effect(u: &FieldUpdate) -> EffectDefinition {
    let value = match &u.value {
        crate::abi::ir::ValueExpr::Literal(l) => EffectValue::Literal(l.clone()),
        crate::abi::ir::ValueExpr::Param(p) => EffectValue::Param(p.clone()),
        crate::abi::ir::ValueExpr::SelfId => EffectValue::SelfRef,
        crate::abi::ir::ValueExpr::SelfField(f) => EffectValue::SelfField(f.clone()),
    };
    EffectDefinition::Update {
        field: u.field.clone(),
        value,
    }
}

/// IR → RuntimeDefinition 列表（每个 object type 一份）。
pub fn ir_to_runtime(ir: &OntologyIR) -> Vec<RuntimeDefinition> {
    ir.object_types
        .iter()
        .map(|ot| {
            let mut policies = Vec::new();
            let mut relations = Vec::new();
            let capabilities = ot
                .actions
                .iter()
                .map(|a| {
                    let mut effects: Vec<EffectDefinition> = Vec::new();
                    effects.extend(a.creates.iter().map(create_effect));
                    effects.extend(a.updates.iter().map(update_effect));
                    for c in &a.creates {
                        if let Some(edge) = &c.link_edge {
                            relations.push(RelationDefinition {
                                predicate: edge.clone(),
                            });
                        }
                    }
                    let policy = if a.policy.require_approval || !a.policy.precondition.is_empty() {
                        let p = PolicyDefinition {
                            require_approval: a.policy.require_approval,
                            preconditions: a
                                .policy
                                .precondition
                                .iter()
                                .map(|cl| PreconditionDefinition {
                                    field: cl.field.clone(),
                                    op: match cl.op {
                                        crate::abi::ir::CompareOp::Eq => crate::compose::Cmp::Eq,
                                        crate::abi::ir::CompareOp::Ne => crate::compose::Cmp::Ne,
                                        crate::abi::ir::CompareOp::Lt => crate::compose::Cmp::Lt,
                                        crate::abi::ir::CompareOp::Le => crate::compose::Cmp::Le,
                                        crate::abi::ir::CompareOp::Gt => crate::compose::Cmp::Gt,
                                        crate::abi::ir::CompareOp::Ge => crate::compose::Cmp::Ge,
                                    },
                                    value: resolve_literal(&cl.value),
                                })
                                .collect(),
                        };
                        policies.push(p.clone());
                        Some(p)
                    } else {
                        None
                    };
                    CapabilityDefinition {
                        name: format!("{}.{}", ot.name, a.name),
                        action: action_semantic_name(&ot.name, &a.name),
                        params: a.params.iter().map(|p| p.name.clone()).collect(),
                        // creates/updates 均可撤销（V2-native 语义）
                        reversibility: Reversibility::Compensatable,
                        policy,
                        effects,
                    }
                })
                .collect();
            RuntimeDefinition {
                object_type: ot.name.clone(),
                capabilities,
                policies,
                relations,
            }
        })
        .collect()
}

/// ValueExpr 字面量 → Value（引用参数/self 在策略求值时解析，此处仅取字面量）。
fn resolve_literal(v: &crate::abi::ir::ValueExpr) -> serde_json::Value {
    match v {
        crate::abi::ir::ValueExpr::Literal(l) => l.clone(),
        crate::abi::ir::ValueExpr::Param(p) => serde_json::Value::String(p.clone()),
        crate::abi::ir::ValueExpr::SelfId => serde_json::Value::String("self".into()),
        crate::abi::ir::ValueExpr::SelfField(f) => serde_json::Value::String(format!("self.{f}")),
    }
}

/// 策略定义 → compose::Policy。
pub fn policy_from(def: &PolicyDefinition) -> Policy {
    Policy {
        require_approval: def.require_approval,
        preconditions: def
            .preconditions
            .iter()
            .map(|p| Precondition {
                param: p.field.clone(),
                op: p.op,
                value: p.value.clone(),
            })
            .collect(),
    }
}

/// 能力定义 → compose::Capability（投影由 runtime_exec 执行后产生——Effect≠Projection）。
fn capability_from(def: &CapabilityDefinition) -> Capability {
    let mut cap = Capability::new(&def.name, &def.action)
        .with_reversibility(def.reversibility)
        .requiring(&def.params.iter().map(|s| s.as_str()).collect::<Vec<_>>());
    // D4：可补偿能力生成逆动作——补偿执行器以 inverse.payload 撤销清单闭合关系时间窗
    if def.reversibility == crate::kernel::Reversibility::Compensatable {
        cap = cap.with_inverse(format!("Undo{}", def.action));
    }
    if let Some(p) = &def.policy {
        cap = cap.with_policy(policy_from(p));
    }
    cap
}

/// RuntimeDefinition → Service（capabilities 挂载）。
pub fn service_from(def: &RuntimeDefinition) -> Service {
    let mut svc = Service::new(&def.object_type);
    for cap in &def.capabilities {
        svc = svc.with_capability(capability_from(cap));
    }
    svc
}

/// 便捷：IR → 服务列表（ir_to_runtime + service_from 两步）。
pub fn ir_to_services(ir: &crate::abi::ir::OntologyIR) -> Vec<crate::compose::Service> {
    ir_to_runtime(ir).iter().map(service_from).collect()
}
