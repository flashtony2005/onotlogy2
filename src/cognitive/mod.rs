//! 认知层（L3）—— Agent / AgentLoop / Memory / CognitiveEngine。
//!
//! P4 Cognitive Separation：本层全部产物（Observation/Hypothesis/Decision）不进 fact 层；
//! 只有经 `TaskExecutor::step` 提交的 Effect 改变 World。
//! 设计定稿：docs/03_架构设计/cognitive-layer-v3.md

pub mod engine;
pub mod episode;
pub mod loop_impl;
pub mod memory;
pub mod observe;

pub use engine::{
    AgentView, CognitiveEngine, CognitiveError, Decision, FakeEngine, NoOpEngine, PlanStep,
};
pub use loop_impl::{Agent, AgentLoop, LoopOutcome};
pub use memory::{Memory, MemoryItem};
