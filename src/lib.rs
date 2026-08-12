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
//! - `server`：axum REST + WebSocket 流 + 三层观察 UI 静态托管

pub mod action;
pub mod agent;
pub mod codegen;
pub mod ir;
pub mod odl;
pub mod planner;
pub mod runtime;
pub mod server;
pub mod storage;
pub mod trace;

pub use action::{ActionEffect, ActionEngine, ActionError, ActionRegistry};
pub use agent::{AgentEngine, AgentError, Context, Decision, PiAgentEngine, WorldSnapshot};
pub use codegen::{CodegenOutput, ObjectDef, codegen};
pub use ir::{
    ActionDef, Clause, CompareOp, CreateEffect, FieldUpdate, ObjectType, OntologyIR, ParamDef,
    PolicyDef, PropertyDef, ScalarType, ValueExpr,
};
pub use odl::parse_odl;
pub use planner::{
    AgentRunResult, ApprovalRequest, ApprovalState, ApprovalStore, AutoApprover, Capability,
    ConsoleApprover, LLMGoalParser, PlanStep, Planner, PlannerError, Policy, PolicyApprover,
    PolicyDecision, RunStatus, ServerApprover,
};
pub use runtime::{EventBus, RuntimeKernel, TraceEvent, TraceKind};
pub use storage::{
    LibSQLStorage, MemoryRecord, ObjectEngine, ObjectRecord, RelationEngine, RelationRecord,
    TraceQuery,
};
pub use trace::{RunTimeline, TraceEventDto, View};
