//! Mutation —— Skill 变异（Phase 11 · D6）：Proposal → Benchmark → Promotion。
//!
//! 模块（M5 拆分）：
//! - `types`：提案状态机 / 指标 / 配置 / 裁决
//! - `benchmark`：基准与离线裁决（纯函数）
//! - `validate`：Delta 白名单校验

mod benchmark;
mod types;
mod validate;

pub use benchmark::{OfflineConfig, OfflineVerdict};
pub use benchmark::{benchmark, offline_benchmark, run_offline_benchmark};
pub use types::{
    BenchmarkConfig, BenchmarkMetric, BenchmarkVerdict, DEFAULT_MIN_EFFECT_SIZE, DEFAULT_MIN_RUNS,
    MutationError, MutationProposal, MutationStatus,
};
pub use validate::validate_delta;
