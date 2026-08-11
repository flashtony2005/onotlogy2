//! OpenAIP-OS —— 可执行本体平台内核（V0.1: Object Kernel + 观察基建；V0.2: World Model）
//!
//! 模块划分（对应 docs/03_架构设计/module-split.md）：
//! - `ir`：语言无关的 Ontology IR（ObjectType / PropertyDef / ScalarType）
//! - `odl`：ODL 文本解析为 IR（确定性，无 LLM）
//! - `codegen`：IR → Rust struct / schema.sql / ontology.json
//! - `storage`：ObjectEngine / RelationEngine 抽象 + LibSQLStorage（Turso 落地）
//! - `runtime`：TraceEvent + EventBus（可解释性根基）

pub mod codegen;
pub mod ir;
pub mod odl;
pub mod runtime;
pub mod storage;

pub use codegen::{CodegenOutput, ObjectDef, codegen};
pub use ir::{ObjectType, OntologyIR, PropertyDef, ScalarType};
pub use odl::parse_odl;
pub use runtime::{EventBus, RuntimeKernel, TraceEvent, TraceKind};
pub use storage::{LibSQLStorage, ObjectEngine, ObjectRecord, RelationEngine, RelationRecord};
