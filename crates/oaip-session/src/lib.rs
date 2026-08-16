//! oaip-session —— Session 层（M4 拆分）：Session Event Log（不可变事实源）+ run 生命周期。
#![cfg_attr(not(test), deny(clippy::unwrap_used))]

pub mod event;
pub mod log;
pub mod run;

pub use event::{EventType, SessionEvent, SessionId};
pub use log::SessionLog;
pub use run::{RunStatus, fiber_to_run_status, generate_run_id};
