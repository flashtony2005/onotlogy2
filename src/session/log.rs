//! SessionLog —— Append-only 事件日志（源；Model Context / UI Timeline / Audit 是投影）。

use serde::{Deserialize, Serialize};

use super::event::{EventType, SessionEvent, SessionId, now_secs};

/// Session 事件日志（不可变事实源）。
///
/// 只允许 `append`；**禁止 update / delete**（API 层面不存在）。
/// 修正 = 追加新事件（data 中带 `correction_of: seq`），历史事实永不变更。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionLog {
    pub session_id: SessionId,
    events: Vec<SessionEvent>,
    next_seq: u64,
}

impl SessionLog {
    pub fn new(session_id: SessionId) -> Self {
        Self {
            session_id,
            events: Vec::new(),
            next_seq: 0,
        }
    }

    /// Append（返回分配的 seq；seq 单调递增）。
    pub fn append(&mut self, event_type: EventType, data: serde_json::Value) -> u64 {
        let seq = self.next_seq;
        self.next_seq += 1;
        self.events.push(SessionEvent {
            session_id: self.session_id.clone(),
            seq,
            event_type,
            timestamp: now_secs(),
            data,
        });
        seq
    }

    /// 全部事件（只读；按 seq 有序）。
    pub fn events(&self) -> &[SessionEvent] {
        &self.events
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Replay：Event[0..N] 完整时间线（重建历史）。
    pub fn replay(&self) -> Vec<&SessionEvent> {
        self.events.iter().collect()
    }

    /// Fork：在稳定边界复制到新 Session（实验基础：GPT vs Claude vs Qwen）。
    /// 共享历史保留原 seq；新 Session 从 next_seq 延续。
    pub fn fork(&self, new_session: SessionId) -> SessionLog {
        SessionLog {
            session_id: new_session,
            events: self.events.clone(),
            next_seq: self.next_seq,
        }
    }

    /// Model Context 投影：进入模型上下文的信息来源（context.injected 等）。
    pub fn context_events(&self) -> Vec<&SessionEvent> {
        self.events
            .iter()
            .filter(|e| {
                matches!(
                    e.event_type,
                    EventType::ContextInjected
                        | EventType::UserMessage
                        | EventType::AssistantMessage
                )
            })
            .collect()
    }
}
