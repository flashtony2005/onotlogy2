//! Ontology 语义集成（Phase 5）：V1 Ontology 语义资产 → V2 Runtime。
//!
//! 三层分离的 ① ONTOLOGY：世界是什么。
//! `RuntimeDefinition` 是 Ontology Compiler → Runtime 的 ABI：
//! ODL / JSON / GraphQL / LLM-generated Ontology 都编译为同一中间对象，
//! 再统一进入 V2（Service/Capability/Policy）。

pub mod adapter;
pub mod runtime_def;

pub use adapter::ir_to_runtime;
pub use runtime_def::{
    CapabilityDefinition, EffectDefinition, EffectValue, PolicyDefinition, PreconditionDefinition,
    RelationDefinition, RuntimeDefinition,
};

/// RuntimeDefinition → compose::Service（ABI 落 V2 组合层）。
pub mod compose {
    use super::runtime_def::{CapabilityDefinition, PolicyDefinition, RuntimeDefinition};
    use crate::compose::policy::{Policy, Precondition};
    use crate::compose::{Capability, Service};

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
}

/// 便捷：IR → 服务列表（adapter + compose 两步）。
pub fn ir_to_services(ir: &crate::ir::OntologyIR) -> Vec<crate::compose::Service> {
    adapter::ir_to_runtime(ir)
        .iter()
        .map(compose::service_from)
        .collect()
}
