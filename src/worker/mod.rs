//! Worker —— Universal Worker & Plugin Runtime（Phase 10 · V10.1）。
//!
//! 核心命题：任何能完成某种 Capability 的外部系统都可以作为 Worker 被注册、
//! 发现、路由、授权、执行、记录、评价。
//!
//! 六层解耦：Skill → Capability → Worker Router → Worker → Plugin → Tool/AI/MCP/CLI
//! - Skill Provider Agnostic：绝不写 executor: codex——谁最适合由 Router 决定。

pub mod adapters;
pub mod exec;
pub mod http;
pub mod manifest;
pub mod mcp;
pub mod orchestrator;
pub mod policy;
pub mod process;
pub mod registry;
pub mod router;
pub mod sdk;
pub mod stats;
pub mod thinker;

pub use thinker::Thinker;
