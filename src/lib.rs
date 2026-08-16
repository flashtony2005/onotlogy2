//! OpenAIP-OS —— 可执行本体平台内核（V0.1~V0.6：Object Kernel → World Model → Agent 闭环 → Action/Policy → Memory/Evolution → AutoApprove）
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

pub mod auth;
pub mod cognitive;
pub mod compose;
pub mod diagnosis;
pub mod evaluation;
pub mod evolution2;
pub mod health;
pub use oaip_abi as abi;
pub use oaip_kernel as kernel;
pub mod mutation;
pub mod plugin;
pub mod rule;
pub mod server;
pub mod session;
pub mod simulation;
pub mod skill;
pub mod storage;
pub mod trace;
pub mod worker;
pub mod world;
pub mod world_exec;

pub use auth::{
    AuthError, AuthUser, IdentityProvider, LocalIdentityProvider, Role, SessionStore, clamp_view,
};
pub use diagnosis::{Diagnoser, Diagnosis, DiagnosisConfig, propose_from_diagnosis, run_diagnosis};
pub use evaluation::hook::on_run_end;
pub use evaluation::{
    EVALUATOR_VERSION, EvaluationPolicy, EvaluationReducer, EvaluationResult, EventRef, Score,
    TaskDefinition,
};
pub use health::{LoopHealth, collect};
pub use mutation::{
    BenchmarkConfig, BenchmarkMetric, BenchmarkVerdict, DEFAULT_MIN_EFFECT_SIZE, DEFAULT_MIN_RUNS,
    MutationError, MutationProposal, MutationStatus, OfflineConfig, OfflineVerdict, benchmark,
    offline_benchmark, run_offline_benchmark,
};
pub use session::run::{RunStatus, fiber_to_run_status, generate_run_id};
pub use simulation::replay::{
    ReplayDecision, ReplayReport, replay_all, replay_decision, run_decision_replay,
};
pub use storage::{
    LibSQLStorage, ObjectEngine, ObjectRecord, RelationEngine, RelationRecord, SessionRecord,
    TraceQuery, UserRecord,
};
pub use trace::{RunTimeline, TraceEvent, TraceEventDto, TraceKind, View};
pub use worker::orchestrator::{
    TaskOutcome, TaskStatus, WorkerOrchestrator, WorkerRunResult, WorkerTask,
};
pub use worker::stats::{WorkerStat, learned_trust, learned_trust_map, record_outcomes};
