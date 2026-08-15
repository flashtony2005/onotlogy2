//! SessionEvent —— 不可变事实单元。

use serde::{Deserialize, Serialize};

/// Session 标识。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionId(pub String);

impl SessionId {
    pub fn new(id: &str) -> Self {
        Self(id.to_string())
    }
}

/// 第一版 Event Type（控制范围，不追求几十上百种）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventType {
    SessionCreated,
    SessionCompleted,
    TurnStarted,
    TurnCompleted,
    UserMessage,
    AssistantChunk,
    AssistantMessage,
    ContextInjected,
    /// Phase 11（Domain Agent Evolution）：本次运行选择了哪些 Context Asset。
    ContextAssembled,
    SkillSelected,
    SkillStarted,
    SkillCompleted,
    CapabilitySelected,
    CapabilityStarted,
    CapabilityCompleted,
    /// Worker 层（V10.1）：策略检查 / Worker 选择与生命周期。
    PolicyChecked,
    WorkerSelected,
    WorkerStarted,
    WorkerCompleted,
    WorkerFailed,
    PluginCall,
    PluginResult,
    ToolCall,
    ToolResult,
    AiRequest,
    AiResponse,
    ApprovalRequested,
    ApprovalResolved,
    /// Phase 11：Human / LLM 判断信号（Fact/Judgment 分离——judge 本身成为可审计事实）。
    JudgeSubmitted,
    ArtifactCreated,
    CodeChanged,
    TestStarted,
    TestResult,
    Error,
}

impl EventType {
    pub fn as_str(&self) -> &'static str {
        match self {
            EventType::SessionCreated => "session.created",
            EventType::SessionCompleted => "session.completed",
            EventType::TurnStarted => "turn.started",
            EventType::TurnCompleted => "turn.completed",
            EventType::UserMessage => "user.message",
            EventType::AssistantChunk => "assistant.chunk",
            EventType::AssistantMessage => "assistant.message",
            EventType::ContextInjected => "context.injected",
            EventType::ContextAssembled => "context.assembled",
            EventType::SkillSelected => "skill.selected",
            EventType::SkillStarted => "skill.started",
            EventType::SkillCompleted => "skill.completed",
            EventType::CapabilitySelected => "capability.selected",
            EventType::CapabilityStarted => "capability.started",
            EventType::CapabilityCompleted => "capability.completed",
            EventType::PolicyChecked => "policy.checked",
            EventType::WorkerSelected => "worker.selected",
            EventType::WorkerStarted => "worker.started",
            EventType::WorkerCompleted => "worker.completed",
            EventType::WorkerFailed => "worker.failed",
            EventType::PluginCall => "plugin.call",
            EventType::PluginResult => "plugin.result",
            EventType::ToolCall => "tool.call",
            EventType::ToolResult => "tool.result",
            EventType::AiRequest => "ai.request",
            EventType::AiResponse => "ai.response",
            EventType::ApprovalRequested => "approval.requested",
            EventType::ApprovalResolved => "approval.resolved",
            EventType::JudgeSubmitted => "judge.submitted",
            EventType::ArtifactCreated => "artifact.created",
            EventType::CodeChanged => "code.changed",
            EventType::TestStarted => "test.started",
            EventType::TestResult => "test.result",
            EventType::Error => "error",
        }
    }
}

/// 不可变事件（append-only；修正 = 追加 Correction Event）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionEvent {
    pub session_id: SessionId,
    /// 单调递增（append-only 顺序）。
    pub seq: u64,
    pub event_type: EventType,
    /// 秒级时间戳。
    pub timestamp: i64,
    pub data: serde_json::Value,
}

/// 当前时间（秒）。
pub fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
