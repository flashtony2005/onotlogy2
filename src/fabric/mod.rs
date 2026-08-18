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
    /// 取消请求标记集（H2，异步化新洞修复）：cancel(correlation_id) 写入；
    /// execute 在 spawn 子进程前检查，命中则直接发 `fabric.task_cancelled` 不执行——
    /// 消除「submit 返 running 与子进程进 running map 之间的窗口内 cancel 命中 0
    /// 但任务照跑完成」的 early-cancel 竞态。与 late-cancel（adapter killed mutex）
    /// 互补：early 在 spawn 前判，late 在 spawn 后判。
    cancel_requested: Mutex<HashSet<String>>,
}

impl Bridge {
    pub fn new(router: router::FabricRouter, sink: Option<Arc<dyn WorldEventSink>>) -> Self {
        Self {
            router,
            adapters: HashMap::new(),
            sink,
            cancel_requested: Mutex::new(HashSet::new()),
        }
    }

    /// T2：取消任务——按 correlation_id kill 运行中的子进程（遍历所有 adapter，
    /// 幂等：未在跑的 adapter 返回 false）。返回命中并 kill 的 adapter 数。
    /// 同时发 `fabric.task_cancel_requested` 审计事件（含 correlation_id），与 finish
    /// 发出的 `fabric.task_cancelled` 终局事件形成完整审计链（治理闭环可追：谁/何时取消）。
    pub fn cancel(&self, correlation_id: &str) -> Result<usize, FabricError> {
        self.event(
            "fabric.task_cancel_requested",
            vec![correlation_id.to_string()],
            json!({}),
            None,
        );
        // H2：写入取消请求标记集，execute 在 spawn 子进程前检查——early-cancel 窗口
        // （submit 返 running 与子进程进 running map 之间）内到达的 cancel 也能生效。
        self.cancel_requested
            .lock()
            .map_err(|_| FabricError::Adapter("取消请求标记锁中毒".into()))?
            .insert(correlation_id.to_string());
        let mut hits = 0usize;
        for adapter in self.adapters.values() {
            if adapter
                .cancel(correlation_id)
                .map_err(|e| FabricError::Adapter(e.to_string()))?
            {
                hits += 1;
            }
        }
        Ok(hits)
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

    /// S2：服务关闭时回收所有 adapter 运行中的孤儿后台 job（透传 adapter.shutdown）。
    /// 由 FabricService::shutdown → Facade::shutdown 在服务 SIGINT/SIGTERM 优雅关闭时调用。
    pub fn shutdown(&self) {
        for adapter in self.adapters.values() {
            adapter.shutdown();
        }
    }

    /// 提交任务（OAB v0.1 主链路，T2b-2 异步契约）：
    /// 路由 + 事件 + 会话记录（同步、立即返回），执行部分（adapter.stream 子进程）
    /// 交给后台 spawn_blocking——立即返回 running 态 + correlation_id，不再阻塞到
    /// 子进程结束。返回 `(立即回复, Option<RunJob>)`；RunJob=None 表示挂起/错误，
    /// 不后台执行（审批挂起由调用方注册、HTTP 层据此返回 pending）。
    pub fn submit(
        &self,
        sessions: Arc<Mutex<session::SessionManager>>,
        session_id: &str,
        from: &str,
        intent: &str,
        payload: Value,
        project_id: Option<String>,
    ) -> Result<(AgentMessage, Option<RunJob>), FabricError> {
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
        {
            let mut g = sessions
                .lock()
                .map_err(|_| FabricError::Adapter("会话表锁中毒".into()))?;
            let session = g
                .get_mut(session_id)
                .ok_or_else(|| FabricError::SessionNotFound(session_id.to_string()))?;
            session.append(req.clone())?;
        }

        // T1：需审批 → 发 approval_requested + 返回挂起态（RunJob=None，不后台执行）
        if needs_approval {
            self.event(
                "fabric.approval_requested",
                vec![plan.agent_id.clone()],
                json!({ "capability": plan.capability, "intent": intent, "run_id": correlation }),
                project_id,
            );
            return Ok((
                pending_message(
                    session_id,
                    from,
                    &plan.agent_id,
                    intent,
                    &plan.capability,
                    &correlation,
                ),
                None,
            ));
        }

        // 5) 执行：返回 running 态 + RunJob（由调用方 spawn 后台执行）
        let running = running_message(
            session_id,
            from,
            &plan.agent_id,
            intent,
            &plan.capability,
            &correlation,
        );
        Ok((
            running,
            Some(RunJob {
                req,
                correlation,
                project_id,
            }),
        ))
    }

    /// T1：裁决后续跑——按 run_id(correlation) 找回挂起的 task.request，执行后半段。
    pub fn resume(
        &self,
        sessions: Arc<Mutex<session::SessionManager>>,
        session_id: &str,
        run_id: &str,
    ) -> Result<AgentMessage, FabricError> {
        // 从会话消息找回对应 task.request（correlation_id == run_id）
        let req = {
            let g = sessions
                .lock()
                .map_err(|_| FabricError::Adapter("会话表锁中毒".into()))?;
            let session = g
                .get(session_id)
                .ok_or_else(|| FabricError::SessionNotFound(session_id.to_string()))?;
            session
                .messages()
                .iter()
                .rev()
                .find(|m| {
                    m.kind == MessageKind::TaskRequest
                        && m.correlation_id.as_deref() == Some(run_id)
                })
                .cloned()
                .ok_or_else(|| FabricError::SessionNotFound(format!("{session_id}#{run_id}")))?
        };
        // 裁决通过 = 续跑执行（后半段从 req 提取上下文，与 submit 一致）
        let correlation = req.correlation_id.clone().unwrap_or_default();
        self.execute(
            sessions,
            RunJob {
                req,
                correlation,
                project_id: None,
            },
        )
    }

    /// T1：审批拒绝——发 fabric.approval_rejected 审计事件（run_id 回指挂起）。
    pub fn reject_event(
        &self,
        sessions: Arc<Mutex<session::SessionManager>>,
        session_id: &str,
        run_id: &str,
    ) -> Result<(), FabricError> {
        let req = {
            let g = sessions
                .lock()
                .map_err(|_| FabricError::Adapter("会话表锁中毒".into()))?;
            let session = g
                .get(session_id)
                .ok_or_else(|| FabricError::SessionNotFound(session_id.to_string()))?;
            session
                .messages()
                .iter()
                .rev()
                .find(|m| {
                    m.kind == MessageKind::TaskRequest
                        && m.correlation_id.as_deref() == Some(run_id)
                })
                .cloned()
                .ok_or_else(|| FabricError::SessionNotFound(format!("{session_id}#{run_id}")))?
        };
        self.event(
            "fabric.approval_rejected",
            vec![req.receiver.clone()],
            json!({ "run_id": run_id, "capability": req.capability }),
            None,
        );
        Ok(())
    }

    /// 后台执行入口（T2b-2）：由 FabricService 在 spawn_blocking 中调用。
    /// 执行 adapter.stream → 终局事件 → 会话记录；失败仅记录（旁路扇出，不 panic）。
    pub(crate) fn run_in_background(
        &self,
        sessions: Arc<Mutex<session::SessionManager>>,
        job: RunJob,
    ) {
        if let Err(e) = self.execute(sessions, job) {
            // 执行失败：终局事件已由 execute 选 failed 发出；此处仅吞错，不阻断。
            let _ = e;
        }
    }

    /// 同步执行任务包（T2b-2）：adapter.stream → 终局事件（completed/failed/cancelled）
    /// → Session 记录。返回终局 result 消息（含 status/files_changed/exit_code 等）。
    /// 调用方：`run_in_background`（spawn_blocking 后台侧）、resume（审批续跑）、测试。
    /// 上下文（correlation / agent / obj_ref）从 `job.req` 提取。
    pub fn execute(
        &self,
        sessions: Arc<Mutex<session::SessionManager>>,
        job: RunJob,
    ) -> Result<AgentMessage, FabricError> {
        let req = job.req;
        let project_id = job.project_id;
        let correlation = req.correlation_id.clone().unwrap_or_default();
        let agent_id = &req.receiver;

        // H2：early-cancel 检查（spawn 子进程前）。cancel 在 submit 返 running 与
        // 子进程进 running map 的窗口内写入 cancel_requested；此处命中则直接发
        // fabric.task_cancelled，不执行——消除「cancel 命中 0 但任务照跑完成」。
        // （late-cancel 由 adapter killed mutex 在 spawn 后处理，二者互补。）
        if self
            .cancel_requested
            .lock()
            .map_err(|_| FabricError::Adapter("取消请求标记锁中毒".into()))?
            .remove(&correlation)
        {
            let msg = terminal_result_message(&req, &correlation, "cancelled", None);
            self.event(
                "fabric.task_cancelled",
                vec![agent_id.to_string()],
                json!({ "to": "cancelled", "correlation_id": correlation, "agent": agent_id }),
                project_id,
            );
            {
                let mut g = sessions
                    .lock()
                    .map_err(|_| FabricError::Adapter("会话表锁中毒".into()))?;
                if let Some(session) = g.get_mut(&req.session_id) {
                    session.append(msg.clone())?;
                    session.cancel();
                }
            }
            return Ok(msg);
        }

        let obj_ref = req
            .payload
            .get("object")
            .and_then(|v| v.as_str())
            .map(String::from);
        let adapter = self
            .adapters
            .get(agent_id)
            .ok_or_else(|| FabricError::NoAdapter(agent_id.to_string()))?;
        // T2：执行走 stream（逐行进度 + 子进程进 running 被追踪，cancel 才有目标）。
        // H1：stream 失败（如运行时命令不可用）必须发 fabric.task_failed + 会话终态，
        // 否则 run_in_background 吞错后 correlation 永久悬挂、CLI 误导超时。
        let msgs = match adapter.stream(&req) {
            Ok(m) => m,
            Err(e) => {
                let err_text = e.to_string();
                let msg = terminal_result_message(&req, &correlation, "failed", Some(&err_text));
                self.event(
                    "fabric.task_failed",
                    vec![agent_id.to_string()],
                    json!({ "to": "failed", "correlation_id": correlation, "agent": agent_id, "error": err_text }),
                    project_id.clone(),
                );
                {
                    let mut g = sessions
                        .lock()
                        .map_err(|_| FabricError::Adapter("会话表锁中毒".into()))?;
                    if let Some(session) = g.get_mut(&req.session_id) {
                        session.append(msg.clone())?;
                    }
                }
                return Err(FabricError::Adapter(err_text));
            }
        };
        let result = msgs
            .into_iter()
            .last()
            .ok_or_else(|| FabricError::Adapter("stream 未返回终局结果".into()))?;

        let status = result
            .payload
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("failed");
        // T2：终态由 adapter 反馈（killed=被 cancel kill / completed / failed），
        // 不再依赖 Bridge 全局标记集——见 adapters.rs 的 killed 互斥设计，复用 running
        // map 存在性判据，消除「延迟 cancel 误标已完成任务」竞态。
        let ty = if status == "killed" {
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

        {
            let mut g = sessions
                .lock()
                .map_err(|_| FabricError::Adapter("会话表锁中毒".into()))?;
            let session = g
                .get_mut(&req.session_id)
                .ok_or_else(|| FabricError::SessionNotFound(req.session_id.clone()))?;
            // 先 append（会话仍活跃，append 拒绝终态会话）；终态为 killed 再同源标记终态，
            // 与 fabric.task_cancelled 审计事件一致。
            session.append(result.clone())?;
            if status == "killed" {
                session.cancel();
            }
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

/// 后台执行任务包（T2b-2）：submit 同步返回后立即由调用方 spawn 后台执行。
pub struct RunJob {
    pub req: AgentMessage,
    pub correlation: String,
    pub project_id: Option<String>,
}

/// 运行中消息（T2b-2 异步契约：submit 立即返回 running 态 + correlation_id，
/// 子进程在后台执行；HTTP/CLI 据此轮询终局事件）。
fn running_message(
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
            "status": "running",
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

/// 合成终态 task.result 消息（early-cancel / stream 失败用）：status 固定，
/// error 可选（failed 时带错误文本；cancelled 为 None）。与 stream 正常终态
/// 同构（均含 correlation_id），下游（审计/会话/WS）统一消费。
fn terminal_result_message(
    req: &AgentMessage,
    correlation: &str,
    status: &str,
    error: Option<&str>,
) -> AgentMessage {
    AgentMessage {
        id: crate::trace::new_trace_id(),
        session_id: req.session_id.clone(),
        sender: req.sender.clone(),
        receiver: req.receiver.clone(),
        kind: MessageKind::TaskResult,
        intent: req.intent.clone(),
        payload: json!({
            "status": status,
            "correlation_id": correlation,
            "error": error,
        }),
        context: Value::Null,
        capability: req.capability.clone(),
        correlation_id: Some(correlation.to_string()),
        causation_id: Some(req.id.clone()),
        timestamp: crate::kernel::now_secs(),
    }
}
