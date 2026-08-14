//! runtime_exec —— 世界执行器（Phase 5 · D4：EffectExecutor / CompensationExecutor 分离）。
//!
//! - Effect：正向世界变化（Decision → Effect → Executor → World）
//! - Compensation：错误恢复语义（Failure → CompensationEngine → CompensationExecutor → World）
//!
//! 两者都修改 World，但语义不同，职责必须分开（避免 Executor 知道太多 Compensation）。
//! Phase 5 MVP：`RelationEffectExecutor` / `RelationCompensationExecutor`（V2-native，
//! 基于 Kernel relations 表达世界——State = Relation 投影，Effect 真实改变 World）。

pub mod compensation;
pub mod effect;

pub use compensation::{
    CompensationError, CompensationExecutor, CompensationResult, RelationCompensationExecutor,
};
pub use effect::{EffectError, EffectExecutor, EffectResult, RelationEffectExecutor};
