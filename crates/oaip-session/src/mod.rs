//! Session —— Session Event Log（Phase 10 · 第五核心：不可变事实源）。
//!
//! Session Event Log 是 Agent Session 的 Source of Truth（不是 Chat History）。
//! 核心原则：**Model-visible ⇒ Logged**——凡进入模型上下文的信息，必能在日志中找到来源。
#![cfg_attr(not(test), deny(clippy::unwrap_used))]

pub mod event;
pub mod log;
pub mod run;
