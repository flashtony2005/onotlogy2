//! ABI-3：Adapter —— 协议差异隔离（DSH/Codex/... 的语义翻译层）。
//!
//! Adapter 管协议（AgentMessage ↔ 各 Agent 原语），Worker 是执行连接器：
//! `CodexAdapter → CodexWorker（ProcessWorker）→ Codex`。每个 Agent 是
//! **Capability Provider**（discover 声明能力市场身份证），不是特殊公民。

use super::{AgentMessage, FabricError};

/// 能力市场身份证（discover 的产出；Router 注册依据）。
#[derive(Debug, Clone, PartialEq)]
pub struct AgentCapability {
    /// 能力名（"coding.modify" / "code.test" / "shell.execute"）。
    pub name: String,
    /// 提供方 agent id。
    pub agent_id: String,
    /// 声明质量（0-1；评分参考，非硬约束）。
    pub quality: f64,
    /// 声明风险（1-10；Policy 判定依据）。
    pub risk: u32,
}

/// Agent Adapter（OAB v0.1 五个核心接口）。
pub trait AgentAdapter: Send + Sync {
    fn id(&self) -> &str;

    /// 建立连接（协议握手）。
    fn connect(&self) -> Result<(), FabricError>;

    /// 能力声明（能力市场；注册进 Router 的 Capability Registry）。
    fn discover(&self) -> Vec<AgentCapability>;

    /// 同步往返：send(task.request) → task.result（v0.1 主链路）。
    fn send(&self, msg: &AgentMessage) -> Result<AgentMessage, FabricError>;

    /// 流式：send → 进度序列（TaskProgress...）→ TaskResult。
    /// 默认实现 = 同步往返退化（单元素序列）。
    fn stream(&self, msg: &AgentMessage) -> Result<Vec<AgentMessage>, FabricError> {
        Ok(vec![self.send(msg)?])
    }

    /// 取消任务（按 correlation_id）。
    fn cancel(&self, correlation_id: &str) -> Result<(), FabricError>;
}
