//! world_exec —— 世界执行层（Phase 5 · D4：EffectExecutor / CompensationExecutor 分离）。
//!
//! M3 改名：原名 runtime_exec 与 V2 Kernel（crate::kernel）概念撞车；本模块是
//! Kernel 之上的世界执行层——Effect 执行（真实改变 World 关系投影）与错误补偿
//! 两个语义分离的执行器。
//!
//! - Effect：正向世界变化（Decision → Effect → Executor → World）
//! - Compensation：错误恢复语义（Failure → CompensationEngine → CompensationExecutor → World）
//!
//! 两者都修改 World，但语义不同，职责必须分开（避免 Executor 知道太多 Compensation）。
//! 正向路径由 `RelationEffectExecutor` 唯一 apply；`EffectResult` 只作为审计和
//! 补偿输入返回。错误恢复路径由 `RelationCompensationExecutor` 负责闭合关系时间窗。
//! Phase 5 MVP：`RelationEffectExecutor` / `RelationCompensationExecutor`（V2-native，
//! 基于 Kernel relations 表达世界——State = Relation 投影，Effect 真实改变 World）。
//!
//! World execution boundary：正向 Effect mutation 与失败补偿的唯一组合入口。
//! Event replay 位于 `crate::projection`；它不能替代本模块执行 Effect。
#![cfg_attr(not(test), deny(clippy::unwrap_used))]

pub mod compensation;
pub mod effect;

pub use compensation::{
    CompensationError, CompensationExecutor, CompensationResult, RelationCompensationExecutor,
};
pub use effect::{EffectError, EffectExecutor, EffectResult, RelationEffectExecutor};
