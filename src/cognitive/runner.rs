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
use crate::compose::{Service, Task};
use crate::evolution2::EvolutionEngine;
use crate::kernel::{Kernel, KernelError};
use crate::session::run::RunStatus;
use crate::trace::TraceEvent;

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
    effect_defs: Option<Vec<crate::ontology::RuntimeDefinition>>,
    evolution: Option<&'k mut EvolutionEngine>,
    run_id: &str,
    goal: &str,
    trace: &'k mut Vec<TraceEvent>,
    approvals: &'k mut Vec<ApprovalRequest>,
    resolver: &mut dyn FnMut(&ApprovalRequest) -> ApprovalDecision,
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
                        // Rejected / Expired / AlreadyResolved → 终局（人工裁决失败）
                        return Ok(RunReport {
                            outcome: LoopOutcome::Denied("审批裁决未放行".to_string()),
                            status: RunStatus::Failed,
                            human_approval: true,
                        });
                    }
                }
            }
            out => {
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
