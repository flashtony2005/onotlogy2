//! OpenAIP-OS —— 可执行本体平台内核（V0.1: Object Kernel + 观察基建；V0.2: World Model；V0.3: Agent 闭环 MVP；V0.4: Action & Policy 引擎；V0.5: Memory 沉淀闭环）
//!
//! 模块划分（对应 docs/03_架构设计/module-split.md）：
//! - `ir`：语言无关的 Ontology IR（ObjectType / PropertyDef / ScalarType）
//! - `odl`：ODL 文本解析为 IR（确定性，无 LLM）
//! - `codegen`：IR → Rust struct / schema.sql / ontology.json
//! - `storage`：ObjectEngine / RelationEngine 抽象 + LibSQLStorage（Turso 落地）
//! - `runtime`：TraceEvent + EventBus（可解释性根基）
//! - `agent`：AgentEngine trait + PiAgentEngine（Pi 作 Cognitive Engine）
//! - `planner`：Goal→LLMGoalParser→Planner→PlanStep→Capability→Execute（OpenAIP 自有核心）
//! - `action`：完整 Action Engine（V0.4）—— ActionRegistry + ActionEngine，解释执行对象自有动作
//! - `trace`：三层观察时间线 + TraceEventDto（Executive/Operator/Developer）
//! - `evolution`：Evolution Engine（BL-003 · REQ-017，运行在 Kernel 之上的 Extension）—— Experience→Pattern→Proposal→Validation→LearnedKnowledge，可观察/可验证/可撤销，**不修改 Policy、不绕过审批**
//! - `server`：axum REST + WebSocket 流 + 三层观察 UI 静态托管

pub mod action;
pub mod agent;
pub mod auth;
pub mod codegen;
pub mod evolution;
pub mod ir;
pub mod odl;
pub mod planner;
pub mod runtime;
pub mod server;
pub mod storage;
pub mod trace;

pub use action::{ActionEffect, ActionEngine, ActionError, ActionRegistry};
pub use agent::{AgentEngine, AgentError, Context, Decision, PiAgentEngine, WorldSnapshot};
pub use auth::{
    AuthError, AuthUser, IdentityProvider, LocalIdentityProvider, Role, SessionStore, clamp_view,
};
pub use codegen::{CodegenOutput, ObjectDef, codegen};
pub use evolution::{
    EvolutionState, Experience, LearnedKnowledge, PATTERN_MIN_HITS, Pattern, Proposal,
    ProposalKind, ProposalStatus, VALIDATE_THRESHOLD,
};
pub use ir::{
    ActionDef, Clause, CompareOp, CreateEffect, FieldUpdate, ObjectType, OntologyIR, ParamDef,
    PolicyDef, PropertyDef, ScalarType, ValueExpr,
};
pub use odl::parse_odl;
pub use planner::{
    AgentRunResult, ApprovalAuditor, ApprovalRequest, ApprovalState, ApprovalStore, AutoApprover,
    Capability, ConsoleApprover, LLMGoalParser, PlanStep, Planner, PlannerError, Policy,
    PolicyApprover, PolicyDecision, RunAuditor, RunStatus, RuntimeConfig, ServerApprover,
    may_auto_approve,
};
pub use runtime::{EventBus, RuntimeKernel, TraceEvent, TraceKind};
pub use storage::{
    ApprovalAudit, LibSQLStorage, MemoryRecord, ObjectEngine, ObjectRecord, RelationEngine,
    RelationRecord, SessionRecord, TraceQuery, UserRecord,
};
pub use trace::{RunTimeline, TraceEventDto, View};
