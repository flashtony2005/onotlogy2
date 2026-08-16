//! Approval 适配器原语（M1b · G2）—— Human-in-the-loop 的交互层。
//!
//! Kernel 只提供挂起机制（WAITING + `Approval` 关系）与裁决原语
//! （`approve` / `reject_approval` / `expire_approval`）；本模块是组合层之上的
//! 交互契约：类型化请求（供 CLI stdin / HTTP 端点查询）、裁决应用、
//! 已裁决防重（二次裁决拒绝）与 CLI 决策缓存。
//!
//! 单一真相源仍是 Kernel：`resolve` / `expire` 以 Fiber 状态为准——
//! 非 WAITING 一律 AlreadyResolved（V1「已裁决请求拒绝二次裁决」语义）。

use serde_json::Value;
use std::collections::HashMap;

use crate::kernel::{ContextId, FiberId, FiberState, Kernel, KernelError};

/// 一条挂起的审批请求（AgentLoop 在 Waiting 时推入调用方 sink）。
#[derive(Debug, Clone, PartialEq)]
pub struct ApprovalRequest {
    /// 请求 ID（审计引用；与 TraceEvent 同源 ID）。
    pub id: String,
    pub run_id: String,
    /// Fiber 所在 Context（裁决 relate 的作用域）。
    pub context: ContextId,
    /// 挂起的 Task Fiber（裁决对象）。
    pub fiber: FiberId,
    /// 能力名（展示与审计）。
    pub capability: String,
    /// 动作名（DecisionCache key 的 action 部分）。
    pub action: String,
    /// 步骤参数（展示与审计；含 target）。
    pub args: Value,
    /// 请求创建时间（微秒；TTL 判定由适配器层做）。
    pub created_at: i64,
}

/// 裁决决策。
#[derive(Debug, Clone, PartialEq)]
pub enum ApprovalDecision {
    Approve { by: String },
    Reject { by: String },
}

/// 裁决结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalResolution {
    /// 已批准（fiber 恢复 RUNNING，调用方续跑）。
    Approved,
    /// 已拒绝（fiber → Cancelled）。
    Rejected,
    /// 已超时（fiber → Cancelled，审计事件 approval.expired 与 Rejected 区分）。
    Expired,
    /// 已裁决 / 非挂起状态（二次裁决拒绝，V1 语义）。
    AlreadyResolved,
}

/// 应用裁决（以 Kernel Fiber 状态为真相源；非 WAITING → AlreadyResolved）。
pub fn resolve(
    k: &mut Kernel,
    req: &ApprovalRequest,
    decision: &ApprovalDecision,
) -> Result<ApprovalResolution, KernelError> {
    let st = k
        .fiber(req.fiber)
        .map(|f| f.state)
        .ok_or(KernelError::FiberNotFound(req.fiber))?;
    if st != FiberState::Waiting {
        return Ok(ApprovalResolution::AlreadyResolved);
    }
    match decision {
        ApprovalDecision::Approve { by } => {
            k.approve(req.fiber, by)?;
            Ok(ApprovalResolution::Approved)
        }
        ApprovalDecision::Reject { by } => {
            k.reject_approval(req.fiber, by)?;
            Ok(ApprovalResolution::Rejected)
        }
    }
}

/// 超时裁决（Cancelled + approval.expired 审计，与 Rejected 区分；V1 tc023d 语义）。
pub fn expire(k: &mut Kernel, req: &ApprovalRequest) -> Result<ApprovalResolution, KernelError> {
    let st = k
        .fiber(req.fiber)
        .map(|f| f.state)
        .ok_or(KernelError::FiberNotFound(req.fiber))?;
    if st != FiberState::Waiting {
        return Ok(ApprovalResolution::AlreadyResolved);
    }
    k.expire_approval(req.fiber, "timeout")?;
    Ok(ApprovalResolution::Expired)
}

/// CLI 决策缓存（V1 ConsoleApprover 语义：同 `action@target` 只询问一次）。
#[derive(Debug, Default)]
pub struct DecisionCache {
    map: HashMap<String, ApprovalDecision>,
}

impl DecisionCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// 缓存 key：`action@target`（无 target 时退化为 action）。
    pub fn key(action: &str, args: &Value) -> String {
        let target = args
            .get("target")
            .and_then(|t| t.as_str())
            .unwrap_or(action);
        format!("{action}@{target}")
    }

    /// 命中缓存直接返回；未命中则询问一次并缓存。
    pub fn get_or_ask<F: FnMut() -> ApprovalDecision>(
        &mut self,
        key: &str,
        mut ask: F,
    ) -> ApprovalDecision {
        if let Some(d) = self.map.get(key) {
            return d.clone();
        }
        let d = ask();
        self.map.insert(key.to_string(), d.clone());
        d
    }
}
