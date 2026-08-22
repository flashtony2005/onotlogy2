//! Skill —— AI Skill Asset Compiler & Registry（Phase 9）。
//!
//! 核心命题：把第三方非标准 Skill 知识转换为标准化、可执行、可组合、可演化的 Skill Asset。
//!
//! 五原则（与既有架构不变量对齐）：
//! - P1 Raw 不修改：第三方 Skill 永远保留原始快照；AI 转换皆派生资产
//! - P2 AI 理解 ≠ 授权：AI 只做 Semantic Interpretation；Validator/Policy/Registry 决定
//! - P3 先 IR 再 Asset：禁止 Text → Executable；必须 Text → SkillIR → Validation → SkillAsset
//! - P4 Capability 不凭空创造：未注册能力只产生 CapabilityCandidate（UNRESOLVED）
//! - P5 所有 AI 推断可追溯：原始 Skill + Source Span + Interpretation + Confidence

pub mod apply;
pub mod asset;
pub mod builtin;
pub mod compose;
pub mod export;
pub mod graph;
pub mod ir;
pub mod lineage;
pub mod parser;
pub mod raw;
pub mod registry;
pub mod resolver;
