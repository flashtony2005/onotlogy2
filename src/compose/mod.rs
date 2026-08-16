//! 组合层（L1/L2）—— Service / Capability / Policy / Task。
//!
//! 不是 Kernel Primitive：全部由 L0（Context/Fiber/Effect/Relation）组合「生长」：
//! - Policy：对 Capability/Effect 的约束（审批闸门/前置条件）
//! - Capability：能力声明（动作 + 可逆性 + 补偿 + 策略 + 关系投影）
//! - Service：能力集合（= Context + capabilities）
//! - Task：未来 Effect 序列的声明（执行 = spawn Fiber，TaskExecutor 驱动）

pub mod capability;
pub mod defs;
pub mod policy;
pub mod service;
pub mod task;

pub use capability::{Capability, CapabilityRegistry};
pub use policy::{Cmp, Policy, PolicyVerdict, Precondition};
pub use service::Service;
pub use task::{StepOutcome, Task, TaskExecutor};

pub use defs::{
    CapabilityDefinition, EffectDefinition, EffectValue, PolicyDefinition, PreconditionDefinition,
    RelationDefinition, RuntimeDefinition, ir_to_runtime, ir_to_services,
};
