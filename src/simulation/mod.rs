//! Simulation（S1–S3）—— 双轨模型的假设轨。
//! - S1 Decision Replay（replay）：当前 Trust 重跑历史路由（Learning Counterfactual）
//! - S2 Projection（projection）：State(t) + 假设动作 → Predicted State(t+1)
//! - S3 Counterfactual（projection）：Action A/B → 对比 → Decision Support
//!
//! 铁律（I2）：一切产物是 Projection（假设轨），永不写入 EventLog。

pub mod projection;
pub mod replay;
