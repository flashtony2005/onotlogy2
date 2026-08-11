//! Ontology IR —— 语言无关的中间表示（对应 REQ-001 / REQ-002）

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// 标量类型。V0.1 仅支持基础标量；Relation / Event 类型在 V0.2+。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScalarType {
    String,
    Int,
    Float,
    Bool,
}

impl ScalarType {
    /// 对应 Rust 类型名（用于 codegen）。
    pub fn rust_type(&self) -> &'static str {
        match self {
            ScalarType::String => "String",
            ScalarType::Int => "i64",
            ScalarType::Float => "f64",
            ScalarType::Bool => "bool",
        }
    }

    /// 对应 SQLite 列类型（对应 sql-script.md 的 TEXT/INTEGER/REAL）。
    pub fn sqlite_type(&self) -> &'static str {
        match self {
            ScalarType::String => "TEXT",
            ScalarType::Int => "INTEGER",
            ScalarType::Float => "REAL",
            ScalarType::Bool => "INTEGER",
        }
    }
}

impl FromStr for ScalarType {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim() {
            "string" => Ok(ScalarType::String),
            "int" => Ok(ScalarType::Int),
            "float" => Ok(ScalarType::Float),
            "bool" => Ok(ScalarType::Bool),
            other => Err(format!("未知标量类型: {other}")),
        }
    }
}

impl fmt::Display for ScalarType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            ScalarType::String => "string",
            ScalarType::Int => "int",
            ScalarType::Float => "float",
            ScalarType::Bool => "bool",
        };
        f.write_str(s)
    }
}

/// 单个属性定义。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PropertyDef {
    pub name: String,
    pub ty: ScalarType,
}

/// 对象类型（对应 ODL `object X:`）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectType {
    pub name: String,
    pub properties: Vec<PropertyDef>,
}

/// 整份 ODL 解析出的 IR。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct OntologyIR {
    pub object_types: Vec<ObjectType>,
}

impl OntologyIR {
    /// 按名称查找对象类型（codegen / query 复用）。
    pub fn find(&self, name: &str) -> Option<&ObjectType> {
        self.object_types.iter().find(|o| o.name == name)
    }
}
