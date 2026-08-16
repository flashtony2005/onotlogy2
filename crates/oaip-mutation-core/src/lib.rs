//! oaip-mutation-core —— 变异提案核心类型（M4 拆分）：状态机 / 指标 / 配置 / 校验。
//! 基准裁决函数（benchmark/offline）在根 crate（依赖 storage），类型环由此解开。
#![cfg_attr(not(test), deny(clippy::unwrap_used))]

pub mod benchmark;
mod types;
mod validate;

pub use benchmark::benchmark;
pub use types::{
    BenchmarkConfig, BenchmarkMetric, BenchmarkVerdict, DEFAULT_MIN_EFFECT_SIZE, DEFAULT_MIN_RUNS,
    MutationError, MutationProposal, MutationStatus, OfflineConfig, OfflineVerdict,
};
pub use validate::validate_delta;
