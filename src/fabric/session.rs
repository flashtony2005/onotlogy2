//! ABI-2：Session —— 通信的第二个核心（不是 request/response，是会话）。
//!
//! Session 保存 participants / messages / state / lifecycle；支撑 streaming、
//! cancel、retry、resume、timeout、human approval。Effective Context 原则：
//! 消息不携带全部历史——`Message + Session + World State → Effective Context`。

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::{AgentMessage, FabricError};

/// 会话状态机：Created → Active ⇄ Paused → Completed / Failed / Cancelled。
/// v0.1 简化：create 即 Active（创建即活动）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionState {
    Created,
    Active,
    Paused,
    Completed,
    Failed,
    Cancelled,
}

/// 会话（通信层核心对象）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    /// 参与方 agent id。
    pub participants: Vec<String>,
    pub state: SessionState,
    pub created_at: i64,
    /// 会话级策略引用（v0.1 保留；接 compose::Policy）。
    pub policy_ref: Option<String>,
    /// 会话消息（真相源：messages 是事件的投影依据）。
    messages: Vec<AgentMessage>,
}

impl Session {
    pub fn new(id: impl Into<String>, participants: Vec<String>) -> Self {
        Self {
            id: id.into(),
            participants,
            state: SessionState::Active,
            created_at: crate::kernel::now_secs(),
            policy_ref: None,
            messages: Vec::new(),
        }
    }

    pub fn messages(&self) -> &[AgentMessage] {
        &self.messages
    }

    /// 追加消息（终态拒绝：Completed/Failed/Cancelled 不可再写入）。
    pub fn append(&mut self, m: AgentMessage) -> Result<(), FabricError> {
        if matches!(
            self.state,
            SessionState::Completed | SessionState::Failed | SessionState::Cancelled
        ) {
            return Err(FabricError::SessionClosed(self.id.clone()));
        }
        self.messages.push(m);
        Ok(())
    }

    pub fn pause(&mut self) {
        if self.state == SessionState::Active {
            self.state = SessionState::Paused;
        }
    }

    pub fn resume(&mut self) {
        if self.state == SessionState::Paused {
            self.state = SessionState::Active;
        }
    }

    pub fn complete(&mut self) {
        if !matches!(
            self.state,
            SessionState::Completed | SessionState::Failed | SessionState::Cancelled
        ) {
            self.state = SessionState::Completed;
        }
    }

    pub fn fail(&mut self) {
        self.state = SessionState::Failed;
    }

    pub fn cancel(&mut self) {
        self.state = SessionState::Cancelled;
    }
}

/// 会话管理器（id → Session；单点创建/查询）。
#[derive(Debug, Default)]
pub struct SessionManager {
    sessions: HashMap<String, Session>,
    seq: u64,
}

impl SessionManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// 创建会话（返回副本；id 单调）。
    pub fn create(&mut self, participants: Vec<String>) -> Session {
        self.seq += 1;
        self.create_with_id(format!("sess_{:04}", self.seq), participants)
    }

    /// 以调用方指定 id 创建（Facade FabricService 用：session_id 由请求方持有）。
    pub fn create_with_id(&mut self, id: impl Into<String>, participants: Vec<String>) -> Session {
        let s = Session::new(id, participants);
        self.sessions.insert(s.id.clone(), s.clone());
        s
    }

    pub fn get(&self, id: &str) -> Option<&Session> {
        self.sessions.get(id)
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut Session> {
        self.sessions.get_mut(id)
    }
}
