//! Planner 编排闭环（对应 REQ-011 / prd-agent-integration.md 4.3）
//!
//! OpenAIP 自有核心：**Goal → LLMGoalParser → Planner → PlanStep → Capability → Execute**。
//! Pi 仅作 Cognitive Engine（经 `AgentEngine::reason` 提供 NLU/推理），本模块拥有编排与决策。
//! 每步产出 cognitive `TraceEvent` 并绑定 `object_refs`（可解释性根基）；Policy 拒 → WB2005；
//! 认知引擎不可用 → WB2004。
//!
//! 模块拆分（长文件模块化）：核心类型在 `mod`，审批闸门在 `approval`，策略校验在 `policy`，
//! 意图过滤/参数抽取在 `intent`，主流程在 `run`，工具函数在 `util`。

pub mod approval;
pub mod intent;
pub mod policy;
pub mod run;
pub mod util;

pub use approval::{
    ApprovalAuditor, ApprovalRequest, ApprovalState, ApprovalStore, AutoApprover, ConsoleApprover,
    PolicyApprover, RunAuditor, ServerApprover,
};
pub use policy::may_auto_approve;
pub use run::{LLMGoalParser, Planner};
pub(crate) use util::lock_mutex;
pub use util::{APPROVAL_TIMEOUT, generate_run_id};

use crate::action::ActionError;
use crate::storage::StorageError;
use serde_json::Value;

/// REQ-018 运行时配置：Evolution 第二阶段 AutoApprove 的全局开关。
/// **默认关闭**——不显式开启，行为等同 MVP（require_approval 动作仍走 Human Approval）。
#[derive(Debug, Clone, Copy, Default)]
pub struct RuntimeConfig {
    /// 允许基于已验证 LearnedKnowledge 对 require_approval 动作自动放行（替代一次 Human Approval）。
    pub auto_approve_from_evolution: bool,
}

/// Planner 运行错误，映射到 WB 错误码。
#[derive(Debug, thiserror::Error)]
pub enum PlannerError {
    #[error(transparent)]
    Storage(#[from] StorageError),
    /// 认知引擎不可用 → WB2004（TC-010-b）。
    #[error("认知引擎不可用（Pi 未就绪/崩溃/鉴权失败）: {0}")]
    EngineUnavailable(String),
    /// Policy 拒绝 → WB2005（TC-011-b），等待 Human Approval。
    #[error("Policy 拒绝（需 Human Approval）: {0}")]
    PolicyDenied(String),
    /// 动作执行失败（V0.4 Action Engine）。
    #[error("动作执行失败: {0}")]
    Action(#[from] ActionError),
}

/// 运行状态码（与 timeline.status 对齐）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunStatus {
    Running,
    Completed,
    Failed,
    /// 进程崩溃未正常结束（重启恢复时由 Running 标记，区别于 Failed）。
    Interrupted,
}

impl RunStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            RunStatus::Running => "Running",
            RunStatus::Completed => "Completed",
            RunStatus::Failed => "Failed",
            RunStatus::Interrupted => "Interrupted",
        }
    }

    /// 持久化字符串 → 状态（未知 → Interrupted，安全兜底，避免回退成 Running）。
    pub fn from_db_str(s: &str) -> RunStatus {
        match s {
            "Running" => RunStatus::Running,
            "Completed" => RunStatus::Completed,
            "Failed" => RunStatus::Failed,
            _ => RunStatus::Interrupted,
        }
    }
}

/// 一次 Agent 运行的结果。
#[derive(Debug, Clone)]
pub struct AgentRunResult {
    pub run_id: String,
    pub agent_id: String,
    pub status: RunStatus,
    pub goal: String,
    pub error: Option<String>,
}

/// 能力（Capability Registry）：对世界模型的**受控**操作；Pi 必须经此路由，禁止直写。
#[derive(Debug, Clone)]
pub enum Capability {
    /// 查询某类型对象。
    QueryObjects { object_type: String },
    /// 创建对象（最小 Action 闭环的核心写操作）。
    CreateObject {
        object_type: String,
        id: String,
        data: Value,
    },
    /// 建立关系边。
    LinkRelation {
        source: String,
        edge: String,
        target: String,
    },
    /// 发出领域事件。
    EmitEvent {
        event_type: String,
        refs: Vec<String>,
    },
    /// 删除对象（破坏性，MVP 默认需 Human Approval → Policy 拒绝，见 TC-011-b）。
    DeleteObject { id: String },
    /// 声明式动作执行（V0.4 完整 Action Engine）：经 ActionRegistry 路由到对象自有动作。
    Action {
        object_type: String,
        action: String,
        target_id: String,
        args: Value,
    },
}

/// 计划步：一个能力 + 绑定对象 + 描述。
#[derive(Debug, Clone)]
pub struct PlanStep {
    pub capability: Capability,
    pub object_refs: Vec<String>,
    pub description: String,
}

/// Policy 引擎：对 Action（PlanStep）做约束与审批。破坏性动作默认拒；声明式动作经
/// 自带 `PolicyDef`（前置条件 + 审批闸门）校验；拒 → WB2005，等待 Human Approval。
pub struct Policy;

/// Policy 决策。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyDecision {
    Allow,
    Deny { reason: String },
}
