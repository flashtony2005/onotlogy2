//! Agent Communication Fabric（OAB）—— 四 ABI 之核心：AgentMessage + Bridge。
//!
//! 契约：docs/03_架构设计/agent-fabric-v1.md
//!
//! 定位：OpenAIP 不是 DSH ↔ Codex 转发器，而是统一语义（Session / Context /
//! Capability / Policy / Event）下的通信层。**Adapter 管协议，Bridge 管语义**：
//! 跨 Agent 消息必须经 `AgentMessage` 规范化（禁止协议直通），
//! Router 按 Intent → Capability → Policy → Agent Selection 路由，
//! 所有通信事件全量入 Event Log（kind=Cognitive, run_id=None——与 P5 一致，
//! Projection → World State 原样接续）。
//!
//! OAB v0.1 边界：4 类消息（task.request/accepted/progress/result）+
//! 5 接口（connect/discover/send/stream/cancel）+ correlation_id/session_id。

pub mod adapter;
pub mod router;
pub mod session;

use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::compose::task::WorldEventSink;
use crate::trace::TraceEvent;

/// 消息类型（OAB v0.1 只实现 4 类；扩展类型在 AsStr/FromStr 处收敛）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageKind {
    #[serde(rename = "task.request")]
    TaskRequest,
    #[serde(rename = "task.accepted")]
    TaskAccepted,
    #[serde(rename = "task.progress")]
    TaskProgress,
    #[serde(rename = "task.result")]
    TaskResult,
}

impl MessageKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            MessageKind::TaskRequest => "task.request",
            MessageKind::TaskAccepted => "task.accepted",
            MessageKind::TaskProgress => "task.progress",
            MessageKind::TaskResult => "task.result",
        }
    }
}

impl FromStr for MessageKind {
    type Err = FabricError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "task.request" => Ok(MessageKind::TaskRequest),
            "task.accepted" => Ok(MessageKind::TaskAccepted),
            "task.progress" => Ok(MessageKind::TaskProgress),
            "task.result" => Ok(MessageKind::TaskResult),
            other => Err(FabricError::UnknownMessageKind(other.to_string())),
        }
    }
}

/// ABI-1：统一消息（通信层唯一真相；协议差异止于 Adapter）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentMessage {
    /// 消息唯一 id（new_trace_id 同源，单调）。
    pub id: String,
    /// 所属会话（v0.1 必填）。
    pub session_id: String,
    /// 发送方 agent id（"dsh" / "codex" / ...）。
    pub sender: String,
    /// 接收方 agent id；"*" = 广播。
    pub receiver: String,
    /// 消息类型。
    pub kind: MessageKind,
    /// 语义意图（"modify_code"）；kind=task.* 时必填。
    pub intent: Option<String>,
    /// 协议无关载荷（任务描述/结果）。
    pub payload: Value,
    /// 轻量上下文引用（session 内增量；全量上下文走 Session+World）。
    pub context: Value,
    /// 请求的能力（"coding.modify"）。
    pub capability: Option<String>,
    /// 关联（task.request ↔ task.result）。
    pub correlation_id: Option<String>,
    /// 因果链（前驱消息 id；事件溯源用）。
    pub causation_id: Option<String>,
    /// Kernel 秒轴（与 Projection normalize_ts 一致）。
    pub timestamp: i64,
}

/// Fabric 错误（fail-closed：未知意图/无提供方/策略拒绝/审批挂起）。
#[derive(Debug, thiserror::Error)]
pub enum FabricError {
    #[error("未知意图: {0}")]
    UnknownIntent(String),
    #[error("未知消息类型: {0}")]
    UnknownMessageKind(String),
    #[error("无能力提供方: {0}")]
    NoProvider(String),
    #[error("无适配器: {0}")]
    NoAdapter(String),
    #[error("策略拒绝: {0}")]
    Denied(String),
    #[error("需要人工审批: {0}")]
    RequiresApproval(String),
    #[error("会话不存在: {0}")]
    SessionNotFound(String),
    #[error("会话已终态: {0}")]
    SessionClosed(String),
    #[error("适配器错误: {0}")]
    Adapter(String),
}

/// Bridge（语义连接层）：Router 路由 + Adapter 执行 + 事件全量入日志。
///
/// 事件（前缀 fabric.，kind=Cognitive，run_id=None）经 sink 同步落库——
/// 复用 P5.6 的同步落库通道；投影侧以领域规则接续（见契约 §5）。
pub struct Bridge {
    router: router::FabricRouter,
    adapters: HashMap<String, Arc<dyn adapter::AgentAdapter>>,
    sink: Option<Arc<dyn WorldEventSink>>,
}

impl Bridge {
    pub fn new(router: router::FabricRouter, sink: Option<Arc<dyn WorldEventSink>>) -> Self {
        Self {
            router,
            adapters: HashMap::new(),
            sink,
        }
    }

    /// 接入 Agent（Adapter）：connect + discover（能力市场身份证由调用方注册进
    /// Router）→ fabric.agent_connected 事件。Agent 是 Capability Provider，
    /// 不是特殊公民。
    pub fn connect_agent(
        &mut self,
        adapter: Arc<dyn adapter::AgentAdapter>,
    ) -> Result<(), FabricError> {
        let id = adapter.id().to_string();
        adapter
            .connect()
            .map_err(|e| FabricError::Adapter(e.to_string()))?;
        self.event("fabric.agent_connected", vec![id.clone()], json!({}));
        self.adapters.insert(id.clone(), adapter);
        Ok(())
    }

    /// 断开 Agent（下线）：fabric.agent_disconnected 事件 + 移除。
    pub fn disconnect_agent(&mut self, agent_id: &str) {
        if self.adapters.remove(agent_id).is_some() {
            self.event(
                "fabric.agent_disconnected",
                vec![agent_id.to_string()],
                json!({}),
            );
        }
    }

    /// 提交任务（OAB v0.1 主链路）：
    /// `Intent → Router（Capability → Policy → Selection）→ Adapter.send →
    /// task.result → 事件 → Session 记录`。
    ///
    /// Policy 在 Router 前面：RequiresApproval → fabric.approval_requested 事件 +
    /// 返回错误（复用现有 Approval/Gate/Audit 机制裁决；v0.1 只留判定接口）。
    pub fn submit(
        &mut self,
        sessions: &mut session::SessionManager,
        session_id: &str,
        from: &str,
        intent: &str,
        payload: Value,
    ) -> Result<AgentMessage, FabricError> {
        // 1) 路由：意图 → 能力 → 策略 → 选择（fail-closed）
        let plan = self.router.route(intent)?;
        match &plan.verdict {
            router::RouteVerdict::RequiresApproval => {
                self.event(
                    "fabric.approval_requested",
                    vec![plan.agent_id.clone()],
                    json!({ "capability": plan.capability, "intent": intent }),
                );
                return Err(FabricError::RequiresApproval(plan.capability.clone()));
            }
            router::RouteVerdict::Denied(reason) => {
                return Err(FabricError::Denied(reason.clone()));
            }
            router::RouteVerdict::Allow => {}
        }
        let adapter = self
            .adapters
            .get(&plan.agent_id)
            .ok_or_else(|| FabricError::NoAdapter(plan.agent_id.clone()))?;

        // 2) 规范化 task.request（统一消息语义）
        let correlation = crate::trace::new_trace_id();
        let req = AgentMessage {
            id: crate::trace::new_trace_id(),
            session_id: session_id.to_string(),
            sender: from.to_string(),
            receiver: plan.agent_id.clone(),
            kind: MessageKind::TaskRequest,
            intent: Some(intent.to_string()),
            payload,
            context: Value::Null,
            capability: Some(plan.capability.clone()),
            correlation_id: Some(correlation.clone()),
            causation_id: None,
            timestamp: crate::kernel::now_secs(),
        };

        // 3) 事件（通信黑匣子）：能力调用 + 请求 + 发出
        self.event(
            "fabric.capability_invoked",
            vec![plan.agent_id.clone()],
            json!({ "capability": plan.capability, "intent": intent }),
        );
        self.event(
            "fabric.task_requested",
            vec![from.to_string()],
            json!({ "capability": plan.capability, "intent": intent, "correlation_id": correlation }),
        );
        self.event(
            "fabric.message_sent",
            vec![from.to_string(), plan.agent_id.clone()],
            json!({ "kind": "task.request", "correlation_id": correlation }),
        );

        // 4) Session 记录请求（会话是消息真相源）
        let session = sessions
            .get_mut(session_id)
            .ok_or_else(|| FabricError::SessionNotFound(session_id.to_string()))?;
        session.append(req.clone())?;

        // 5) Adapter 执行（协议差异止于此）
        let result = adapter
            .send(&req)
            .map_err(|e| FabricError::Adapter(e.to_string()))?;

        // 6) 终局事件：task_completed / task_failed（refs=[payload.object]，
        //    投影领域规则 (refs[0], fabric.status, input.to) → World State）
        let status = result
            .payload
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("failed");
        let ty = if status == "completed" {
            "fabric.task_completed"
        } else {
            "fabric.task_failed"
        };
        let obj = result
            .payload
            .get("object")
            .and_then(|v| v.as_str())
            .unwrap_or(&result.sender);
        self.event(
            ty,
            vec![obj.to_string()],
            json!({ "to": status, "correlation_id": correlation, "agent": plan.agent_id }),
        );

        // 7) Session 记录结果
        let session = sessions
            .get_mut(session_id)
            .ok_or_else(|| FabricError::SessionNotFound(session_id.to_string()))?;
        session.append(result.clone())?;

        Ok(result)
    }

    /// 事件入日志（同步；sink 失败不阻断——旁路扇出，与 P5.6 审计同语义）。
    fn event(&self, ty: &str, refs: Vec<String>, input: Value) {
        if let Some(sink) = &self.sink {
            let ev = TraceEvent {
                id: crate::trace::new_trace_id(),
                kind: crate::trace::TraceKind::Cognitive,
                event_type: ty.to_string(),
                object_refs: refs,
                run_id: None, // 事实不依赖 Run（P5 约定）
                agent_id: None,
                reasoning: None,
                input: Some(input),
                output: None,
                confidence: None,
                timestamp: crate::kernel::now_secs().to_string(),
            };
            let _ = sink.append(&ev);
        }
    }
}
