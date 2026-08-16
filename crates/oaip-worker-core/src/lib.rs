//! oaip-worker-core —— Worker 核心类型（M4 拆分）：Manifest / SDK trait / Registry / 信任统计。
//!
//! 与 worker 运行时（process/http/mcp/adapters/router/orchestrator）分离：
//! storage 持久化层依赖本 crate 的类型，运行时依赖 storage——类型环由此解开。
#![cfg_attr(not(test), deny(clippy::unwrap_used))]

pub mod manifest;
pub mod registry;
pub mod sdk;
pub mod stats;

pub use manifest::{ExecutionProtocol, WorkerManifest};
pub use registry::WorkerRegistry;
pub use sdk::{Worker, WorkerError, WorkerResult};
pub use stats::{
    WorkerStat, effective_trust, efficiency_component, learned_trust, learned_trust_eff,
};
