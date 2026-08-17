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
pub mod adapters;
pub mod router;
pub mod session;

use std::collections::{HashMap, HashSet};
use std::str::FromStr;
use std::sync::{Arc, Mutex};

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
    /// 取消标记集：cancel(correlation_id) 写入，finish 据此把终局事件选为
    /// `fabric.task_cancelled`（而非 failed）。Bridge 内 Mutex<HashSet>，
    /// 无需新增 correlation→adapter 注册表（cancel 遍历所有 adapter 调 cancel，幂等）。
    cancelled: Mutex<HashSet<String>>,
}

impl Bridge {
    pub fn new(router: router::FabricRouter, sink: Option<Arc<dyn WorldEventSink>>) -> Self {
        Self {
            router,
            adapters: HashMap::new(),
            sink,
            cancelled: Mutex::new(HashSet::new()),
        }
    }

    /// T2：取消任务——按 correlation_id kill 运行中的子进程（遍历所有 adapter，
    /// 幂等：未在跑的 adapter 返回 Ok）。同时写入取消标记集，供 finish 选择
    /// `fabric.task_cancelled` 终局事件类型（区分正常 failed）。
    pub fn cancel(&self, correlation_id: &str) -> Result<(), FabricError> {
        self.cancelled
            .lock()
            .map_err(|_| FabricError::Adapter("取消标记锁中毒".into()))?
            .insert(correlation_id.to_string());
        for adapter in self.adapters.values() {
            adapter
                .cancel(correlation_id)
                .map_err(|e| FabricError::Adapter(e.to_string()))?;
        }
        Ok(())
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
        self.event("fabric.agent_connected", vec![id.clone()], json!({}), None);
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
                None,
            );
        }
    }

    /// 能力发现：返回已接入的 Agent（能力提供者）id 列表。
    pub fn discover(&self) -> Vec<String> {
        self.adapters.keys().cloned().collect()
    }

    /// 提交任务（OAB v0.1 主链路）：
    /// `Intent → Router（Capability → Policy → Selection）→ Adapter.send →
    /// task.result → 事件 → Session 记录`。
    ///
    /// Policy 在 Router 前面：RequiresApproval → fabric.approval_requested 事件 +
    /// 返回错误（复用现有 Approval/Gate/Audit 机制裁决；v0.1 只留判定接口）。
    pub fn submit(
        &self,
        sessions: &mut session::SessionManager,
        session_id: &str,
        from: &str,
        intent: &str,
        payload: Value,
        project_id: Option<String>,
    ) -> Result<AgentMessage, FabricError> {
        // 1) 路由：意图 → 能力 → 策略 → 选择（fail-closed）
        let plan = self.router.route(intent)?;
        // T1：RequiresApproval = 挂起（请求照常入会话；裁决后续跑），Denied = 拒绝
        let needs_approval = matches!(&plan.verdict, router::RouteVerdict::RequiresApproval);
        if let router::RouteVerdict::Denied(reason) = &plan.verdict {
            return Err(FabricError::Denied(reason.clone()));
        }
        // 2) 规范化 task.request（统一消息语义）。
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
            project_id.clone(),
        );
        self.event(
            "fabric.task_requested",
            vec![from.to_string()],
            json!({ "capability": plan.capability, "intent": intent, "correlation_id": correlation }),
            project_id.clone(),
        );
        self.event(
            "fabric.message_sent",
            vec![from.to_string(), plan.agent_id.clone()],
            json!({ "kind": "task.request", "correlation_id": correlation }),
            project_id.clone(),
        );

        // 4) Session 记录请求（会话是消息真相源——挂起也入会话，resume 据此找回）
        let session = sessions
            .get_mut(session_id)
            .ok_or_else(|| FabricError::SessionNotFound(session_id.to_string()))?;
        session.append(req.clone())?;

        // T1：需审批 → 发 approval_requested + 返回挂起态（请求已入会话，裁决后续跑）
        if needs_approval {
            self.event(
                "fabric.approval_requested",
                vec![plan.agent_id.clone()],
                json!({ "capability": plan.capability, "intent": intent, "run_id": correlation }),
                project_id,
            );
            return Ok(pending_message(
                session_id,
                from,
                &plan.agent_id,
                intent,
                &plan.capability,
                &correlation,
            ));
        }

        // 5/6/7) Adapter 执行 + 终局事件 + Session 记录（resume 复用同一后半段）
        self.finish(sessions, session_id, req, project_id)
    }

    /// T1：裁决后续跑——按 run_id(correlation) 找回挂起的 task.request，执行后半段。
    pub fn resume(
        &self,
        sessions: &mut session::SessionManager,
        session_id: &str,
        run_id: &str,
    ) -> Result<AgentMessage, FabricError> {
        // 从会话消息找回对应 task.request（correlation_id == run_id）
        let session = sessions
            .get(session_id)
            .ok_or_else(|| FabricError::SessionNotFound(session_id.to_string()))?;
        let req = session
            .messages()
            .iter()
            .rev()
            .find(|m| {
                m.kind == MessageKind::TaskRequest && m.correlation_id.as_deref() == Some(run_id)
            })
            .cloned()
            .ok_or_else(|| FabricError::SessionNotFound(format!("{session_id}#{run_id}")))?;
        // 裁决通过 = 续跑执行（后半段从 req 提取上下文，与 submit 一致）
        self.finish(sessions, session_id, req, None)
    }

    /// T1：审批拒绝——发 fabric.approval_rejected 审计事件（run_id 回指挂起）。
    pub fn reject_event(
        &self,
        sessions: &mut session::SessionManager,
        session_id: &str,
        run_id: &str,
    ) -> Result<(), FabricError> {
        let session = sessions
            .get(session_id)
            .ok_or_else(|| FabricError::SessionNotFound(session_id.to_string()))?;
        let req = session
            .messages()
            .iter()
            .rev()
            .find(|m| {
                m.kind == MessageKind::TaskRequest && m.correlation_id.as_deref() == Some(run_id)
            })
            .ok_or_else(|| FabricError::SessionNotFound(format!("{session_id}#{run_id}")))?;
        self.event(
            "fabric.approval_rejected",
            vec![req.receiver.clone()],
            json!({ "run_id": run_id, "capability": req.capability }),
            None,
        );
        Ok(())
    }

    /// 提交/续跑的公共后半段：adapter.send → 终局事件（completed/failed）→ Session 记录。
    /// 上下文（correlation / agent / obj_ref）从 `req` 提取，调用点只传 `project_id`，
    /// 以压低参数数量（clippy too_many_arguments）。
    fn finish(
        &self,
        sessions: &mut session::SessionManager,
        session_id: &str,
        req: AgentMessage,
        project_id: Option<String>,
    ) -> Result<AgentMessage, FabricError> {
        let correlation = req.correlation_id.clone().unwrap_or_default();
        let agent_id = &req.receiver;
        let obj_ref = req
            .payload
            .get("object")
            .and_then(|v| v.as_str())
            .map(String::from);
        let adapter = self
            .adapters
            .get(agent_id)
            .ok_or_else(|| FabricError::NoAdapter(agent_id.to_string()))?;
        // T2：执行走 stream（逐行进度 + 子进程进 running 被追踪，cancel 才有目标）
        let msgs = adapter
            .stream(&req)
            .map_err(|e| FabricError::Adapter(e.to_string()))?;
        let result = msgs
            .into_iter()
            .last()
            .ok_or_else(|| FabricError::Adapter("stream 未返回终局结果".into()))?;

        let status = result
            .payload
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("failed");
        // T2：取消标记集命中 → 终局事件选 fabric.task_cancelled（区分正常 failed）
        let cancelled = self
            .cancelled
            .lock()
            .map_err(|_| FabricError::Adapter("取消标记锁中毒".into()))?
            .remove(&correlation);
        let ty = if cancelled {
            "fabric.task_cancelled"
        } else if status == "completed" {
            "fabric.task_completed"
        } else {
            "fabric.task_failed"
        };
        let obj = obj_ref
            .as_deref()
            .or_else(|| result.payload.get("object").and_then(|v| v.as_str()))
            .unwrap_or(&result.sender);
        self.event(
            ty,
            vec![obj.to_string()],
            json!({ "to": status, "correlation_id": correlation, "agent": agent_id }),
            project_id,
        );

        let session = sessions
            .get_mut(session_id)
            .ok_or_else(|| FabricError::SessionNotFound(session_id.to_string()))?;
        // 先 append（会话仍活跃，append 拒绝终态会话）；命中取消标记集再同源标记终态，
        // 与 fabric.task_cancelled 审计事件一致。
        session.append(result.clone())?;
        if cancelled {
            session.cancel();
        }
        Ok(result)
    }

    /// 事件入日志（同步；sink 失败不阻断——旁路扇出，与 P5.6 审计同语义）。
    fn event(&self, ty: &str, refs: Vec<String>, input: Value, project_id: Option<String>) {
        if let Some(sink) = &self.sink {
            let ev = TraceEvent {
                id: crate::trace::new_trace_id(),
                kind: crate::trace::TraceKind::Cognitive,
                event_type: ty.to_string(),
                object_refs: refs,
                project_id,
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

/// 挂起态消息（T1：RequiresApproval 时返回；run_id 供审批查询/裁决续跑）。
fn pending_message(
    session_id: &str,
    from: &str,
    agent_id: &str,
    intent: &str,
    capability: &str,
    run_id: &str,
) -> AgentMessage {
    AgentMessage {
        id: crate::trace::new_trace_id(),
        session_id: session_id.to_string(),
        sender: from.to_string(),
        receiver: agent_id.to_string(),
        kind: MessageKind::TaskResult,
        intent: Some(intent.to_string()),
        payload: json!({
            "status": "pending",
            "run_id": run_id,
            "capability": capability,
        }),
        context: Value::Null,
        capability: Some(capability.to_string()),
        correlation_id: Some(run_id.to_string()),
        causation_id: None,
        timestamp: crate::kernel::now_secs(),
    }
}
