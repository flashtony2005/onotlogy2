//! OntologyAdapter —— V1 ODL IR → RuntimeDefinition（Phase 5 · D1）。
//!
//! 映射规则：
//! - object X { actions: [...] } → RuntimeDefinition { object_type: X }
//! - 每个 action → CapabilityDefinition（name = "X.action"，action = 语义名，
//!   params → required，policy 直映，creates/updates → EffectDefinition）
//! - ODL `link:`（creates 的 link_edge）→ RelationDefinition
//!
//! 不执行任何行为：只编译语义。

use crate::ir::{CreateEffect, FieldUpdate, OntologyIR};
use crate::kernel::Reversibility;
use crate::ontology::runtime_def::{
    CapabilityDefinition, EffectDefinition, EffectValue, PolicyDefinition, PreconditionDefinition,
    RelationDefinition, RuntimeDefinition,
};

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
        crate::ir::ValueExpr::Literal(l) => EffectValue::Literal(l.clone()),
        crate::ir::ValueExpr::Param(p) => EffectValue::Param(p.clone()),
        crate::ir::ValueExpr::SelfId => EffectValue::SelfRef,
        crate::ir::ValueExpr::SelfField(f) => EffectValue::SelfField(f.clone()),
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
                                        crate::ir::CompareOp::Eq => crate::compose::Cmp::Eq,
                                        crate::ir::CompareOp::Ne => crate::compose::Cmp::Ne,
                                        crate::ir::CompareOp::Lt => crate::compose::Cmp::Lt,
                                        crate::ir::CompareOp::Le => crate::compose::Cmp::Le,
                                        crate::ir::CompareOp::Gt => crate::compose::Cmp::Gt,
                                        crate::ir::CompareOp::Ge => crate::compose::Cmp::Ge,
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
fn resolve_literal(v: &crate::ir::ValueExpr) -> serde_json::Value {
    match v {
        crate::ir::ValueExpr::Literal(l) => l.clone(),
        crate::ir::ValueExpr::Param(p) => serde_json::Value::String(p.clone()),
        crate::ir::ValueExpr::SelfId => serde_json::Value::String("self".into()),
        crate::ir::ValueExpr::SelfField(f) => serde_json::Value::String(format!("self.{f}")),
    }
}
