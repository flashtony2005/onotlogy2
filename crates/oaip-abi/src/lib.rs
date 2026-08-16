//! oaip-abi —— 语言前端（M4 拆分）：`.odl` → OntologyIR → codegen。
//!
//! 确定性编译，无 LLM；产物（Rust struct / schema.sql / ontology.json）由 CLI `compile` 落盘。
#![cfg_attr(not(test), deny(clippy::unwrap_used))]

pub mod codegen;
pub mod ir;
pub mod odl;

pub use codegen::{CodegenOutput, ObjectDef, codegen};
pub use ir::{
    ActionDef, Clause, CompareOp, CreateEffect, FieldUpdate, ObjectType, OntologyIR, ParamDef,
    PolicyDef, PropertyDef, ScalarType, ValueExpr,
};
pub use odl::parse_odl;
