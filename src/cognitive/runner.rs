//! AgentRunner（M1c）—— V2 认知闭环的入口编排（CLI / server 共用真相源）。
//!
//! 职责：装配 Kernel + Agent + Services + CognitiveEngine → 循环驱动
//! `run_until_done`；审批挂起时经调用方提供的 resolver 裁决（CLI=stdin+缓存、
//! server=HTTP 等待）；trace 事件与挂起请求进调用方 sink（由入口层落库）。
//!
//! 每个迭代重建 AgentLoop（Kernel 借用释放后再裁决，裁决经 `approval::resolve`
//! 以 Fiber 状态为真相源）。终局映射：`LoopOutcome::to_run_status`（G6）。

use serde_json::json;

use super::approval::{ApprovalDecision, ApprovalRequest, ApprovalResolution, resolve};
use super::engine::CognitiveEngine;
use super::loop_impl::{Agent, AgentLoop, LoopOutcome};
use crate::compose::task::WorldEventSink;
use crate::compose::{Service, Task};
use crate::evolution2::EvolutionEngine;
use crate::kernel::{Kernel, KernelError};
use crate::session::run::RunStatus;
use crate::trace::{TraceEvent, TraceKind};

/// P6：同步落库的世界事件 sink（`Handle::block_on` 驱动 storage append）。
///
/// 用法约束：`append` 在**阻塞上下文**调用（server=spawn_blocking、
/// CLI=`tokio::task::block_in_place`）——async 上下文内 block_on 会 panic。
/// 提交点同步落库 → run_agent 返回前事件已在 Event Log（P6 步骤级一致）。
pub struct StorageWorldSink {
    storage: std::sync::Arc<crate::storage::LibSQLStorage>,
    handle: tokio::runtime::Handle,
}

impl StorageWorldSink {
    pub fn new(
        storage: std::sync::Arc<crate::storage::LibSQLStorage>,
        handle: tokio::runtime::Handle,
    ) -> Self {
        Self { storage, handle }
    }
}

impl WorldEventSink for StorageWorldSink {
    fn append(&self, ev: &TraceEvent) -> Result<(), String> {
        self.handle
            .block_on(self.storage.append_trace(ev))
            .map_err(|e| e.to_string())
    }
}

/// P5.6：审计事件产生时同步落库（Kernel `record_event` → wb_trace_event）。
///
/// 载荷与旧 run 终局桥接（bridge_audit）一致：event_type=审计类型、
/// kind=Cognitive、refs=["world"]、run_id/agent_id 绑定——/audit 端点
/// 直读事件表，语义不变；区别是**不依赖 run 生命周期**（进程崩溃时
/// 已产生的审计事件依然在场）。用法约束同 StorageWorldSink：阻塞上下文。
pub struct StorageAuditSink {
    storage: std::sync::Arc<crate::storage::LibSQLStorage>,
    handle: tokio::runtime::Handle,
    run_id: String,
    agent_id: String,
}

impl StorageAuditSink {
    pub fn new(
        storage: std::sync::Arc<crate::storage::LibSQLStorage>,
        handle: tokio::runtime::Handle,
        run_id: impl Into<String>,
        agent_id: impl Into<String>,
    ) -> Self {
        Self {
            storage,
            handle,
            run_id: run_id.into(),
            agent_id: agent_id.into(),
        }
    }
}

impl crate::kernel::AuditSink for StorageAuditSink {
    fn append(&self, ev: &crate::kernel::KernelEvent) -> Result<(), String> {
        let tev = TraceEvent {
            id: crate::trace::new_trace_id(),
            kind: TraceKind::Cognitive,
            event_type: ev.ty.clone(),
            object_refs: vec!["world".to_string()],
            run_id: Some(self.run_id.clone()),
            agent_id: Some(self.agent_id.clone()),
            reasoning: None,
            input: Some(ev.payload.clone()),
            output: None,
            confidence: None,
            timestamp: ev.timestamp.to_string(),
        };
        self.handle
            .block_on(self.storage.append_trace(&tev))
            .map_err(|e| e.to_string())
    }
}

/// 一次 run 的编排结果。
#[derive(Debug, Clone)]
pub struct RunReport {
    pub outcome: LoopOutcome,
    pub status: RunStatus,
    pub human_approval: bool,
}

/// 运行一个目标：V2 认知闭环 + 审批裁决循环。
///
/// - `evolution`：可选演化引擎（Gate 放行路径；None = 不采集不 Gate）。
/// - `resolver`：挂起裁决回调（每请求恰好调用一次；Gate 放行时零调用）。
/// - `world_sink`：P6 世界事件回写（Effect 提交点同步落库；None = 不回写，
///   仅无存储/测试场景——生产路径（CLI/server）必须注入）。
///
/// 参数较多是编排入口的固有形状（与 cmd_agent_run 同例）；
/// M5 拆文件时若参数继续增长再收敛为配置结构体。
#[allow(clippy::too_many_arguments)]
pub fn run_agent<'k>(
    kernel: &'k mut Kernel,
    agent: &'k mut Agent,
    services: &'k [Service],
    engine: &'k dyn CognitiveEngine,
    // 世界效应声明（Phase 5：Effect 真实改变 World；None = 回退 projection 模拟）。
    effect_defs: Option<Vec<crate::compose::defs::RuntimeDefinition>>,
    evolution: Option<&'k mut EvolutionEngine>,
    run_id: &str,
    goal: &str,
    trace: &'k mut Vec<TraceEvent>,
    approvals: &'k mut Vec<ApprovalRequest>,
    resolver: &mut dyn FnMut(&ApprovalRequest) -> ApprovalDecision,
    world_sink: Option<std::sync::Arc<dyn WorldEventSink>>,
) -> Result<RunReport, KernelError> {
    let ctx = agent.context;
    // task id 用 run_id：单次 run 唯一（重复 run 不串扰 fiber 查找）。
    let task = Task::new(run_id, goal);
    let mut fiber: Option<crate::kernel::FiberId> = None;
    let mut evolution = evolution;

    loop {
        let outcome = {
            let mut lp = AgentLoop::new(kernel, agent, services, engine)
                .with_trace(run_id, trace)
                .with_approvals(run_id, approvals);
            if let Some(defs) = effect_defs.clone() {
                lp = lp.with_effect_defs(defs);
            }
            if let Some(ev) = evolution.as_deref_mut() {
                lp = lp.with_evolution(ev);
            }
            if let Some(sink) = &world_sink {
                lp = lp.with_world_sink(sink.clone());
            }
            match fiber {
                None => lp.run_until_done(ctx, &task, json!({})),
                Some(f) => lp.resume_until_done(f, &task),
            }?
        };
        match outcome {
            LoopOutcome::Waiting => {
                let req = approvals
                    .last()
                    .cloned()
                    .expect("挂起必注册 ApprovalRequest（with_approvals 保证）");
                fiber = Some(req.fiber);
                let decision = resolver(&req);
                let resolution = match &decision {
                    // 超时裁决走 expire（审计与 Reject 区分，G2 契约）
                    ApprovalDecision::Expire { .. } => {
                        crate::cognitive::approval::expire(kernel, &req)?
                    }
                    _ => resolve(kernel, &req, &decision)?,
                };
                match resolution {
                    ApprovalResolution::Approved => continue,
                    _ => {
                        // Rejected / Expired / AlreadyResolved → 终局（人工裁决失败）。
                        // P5.6：审计已产生时落库（kernel audit sink），此处不再桥接。
                        return Ok(RunReport {
                            outcome: LoopOutcome::Denied("审批裁决未放行".to_string()),
                            status: RunStatus::Failed,
                            human_approval: true,
                        });
                    }
                }
            }
            out => {
                // P5.6：审计已产生时落库（kernel audit sink），终局不再桥接——
                // 审计事件与 Run 生命周期解耦（崩溃窗口消除）。
                let (status, human_approval) = out.to_run_status();
                return Ok(RunReport {
                    outcome: out,
                    status,
                    human_approval,
                });
            }
        }
    }
}
