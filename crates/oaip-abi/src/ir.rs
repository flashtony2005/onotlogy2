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
    Date,
    DateTime,
    Decimal,
}

impl ScalarType {
    /// 对应 Rust 类型名（用于 codegen）。
    pub fn rust_type(&self) -> &'static str {
        match self {
            ScalarType::String => "String",
            ScalarType::Int => "i64",
            ScalarType::Float => "f64",
            ScalarType::Bool => "bool",
            ScalarType::Date => "String",
            ScalarType::DateTime => "String",
            ScalarType::Decimal => "String",
        }
    }

    /// 对应 SQLite 列类型（对应 sql-script.md 的 TEXT/INTEGER/REAL）。
    pub fn sqlite_type(&self) -> &'static str {
        match self {
            ScalarType::String => "TEXT",
            ScalarType::Int => "INTEGER",
            ScalarType::Float => "REAL",
            ScalarType::Bool => "INTEGER",
            ScalarType::Date => "TEXT",
            ScalarType::DateTime => "TEXT",
            ScalarType::Decimal => "TEXT",
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
            "date" => Ok(ScalarType::Date),
            "decimal" => Ok(ScalarType::Decimal),
            "datetime" => Ok(ScalarType::DateTime),
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
            ScalarType::Date => "date",
            ScalarType::DateTime => "datetime",
            ScalarType::Decimal => "decimal",
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

/// 动作参数定义（对应 ODL `actions.<name>.params`）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParamDef {
    pub name: String,
    pub ty: ScalarType,
}

/// 值表达式：Action 的 effects 中引用参数 / 自身 / 字面量（V0.4 完整 Action Engine）。
///
/// 纯数据、可序列化，由 `ActionEngine` 解释执行，杜绝每动作写专属 Rust 分支。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ValueExpr {
    /// 字面量（字符串/数字/布尔/null）。
    Literal(serde_json::Value),
    /// 引用动作参数（按名）。
    Param(String),
    /// 目标对象自身 ID（`self`）。
    SelfId,
    /// 目标对象的某字段值（`self.<field>`）。
    SelfField(String),
}

/// 字段更新：把目标对象的 `field` 改写为求值后的 `value`（对应 `updates:`）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FieldUpdate {
    pub field: String,
    pub value: ValueExpr,
}

/// 创建效应：新建一个对象，可选地经 `link_edge` 与目标对象连边（对应 `creates:`）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CreateEffect {
    /// 新对象类型。
    pub object_type: String,
    /// 显式 ID；缺省自动生成 `<target>_<action>`（幂等 upsert）。
    #[serde(default)]
    pub id: Option<String>,
    /// 新对象字段 → 值表达式。
    #[serde(default)]
    pub data: Vec<(String, ValueExpr)>,
    /// 若设置，建立 `target --edge--> new` 关系边。
    #[serde(default)]
    pub link_edge: Option<String>,
}

/// 比较运算符（Policy precondition 子句使用）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompareOp {
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
}

/// Policy 前置条件子句：把目标字段或参数与值表达式比较（多条子句为 AND）。
///
/// `field` 支持 `self.<字段>`（目标对象字段）或直接参数名（动作参数）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Clause {
    pub field: String,
    pub op: CompareOp,
    pub value: ValueExpr,
}

/// 动作的 Policy 约束（对应 ODL `actions.<name>.policy`）：审批闸门 + 前置条件。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct PolicyDef {
    /// 需人工审批（Human Approval）：为 true 时默认 Deny（WB2005），经 approver 放行才执行。
    #[serde(default)]
    pub require_approval: bool,
    /// 前置条件：全部满足才允许执行（AND）；空表示无条件。
    #[serde(default)]
    pub precondition: Vec<Clause>,
}

/// 动作定义（对应 ODL `actions.<name>`）：对象自有能力，声明式描述其效应与约束。
///
/// 这是 V0.4 把"最小 Action 执行"升级为"完整 Action Engine"的核心：
/// 动作写在对象上，由通用 `ActionEngine` 解释执行，而非 Planner 硬编码；
/// 约束写在 `policy`，由 `Policy` 引擎在编排期校验。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActionDef {
    pub name: String,
    #[serde(default)]
    pub params: Vec<ParamDef>,
    #[serde(default)]
    pub updates: Vec<FieldUpdate>,
    #[serde(default)]
    pub creates: Vec<CreateEffect>,
    /// 动作的 Policy 约束（审批 / 前置条件）；ODL 未声明时为空（无约束，默认允许）。
    #[serde(default)]
    pub policy: PolicyDef,
}

/// 对象类型（对应 ODL `object X:`）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ObjectType {
    pub name: String,
    pub properties: Vec<PropertyDef>,
    /// 对象自有动作（V0.4）；ODL 未声明时为空。
    #[serde(default)]
    pub actions: Vec<ActionDef>,
}

/// 有类型关系边及其 OWL 对象属性特征（对应 ODL `relation X:`）。
fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelationDef {
    pub name: String,
    pub source: String,
    pub target: String,
    /// 逆关系的名称；两端点各自声明互逆即可，投影会去重。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inverse_of: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub functional: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub inverse_functional: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub symmetric: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub transitive: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub reflexive: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub irreflexive: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub asymmetric: bool,
}

/// 整份 ODL 解析出的 IR。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct OntologyIR {
    pub object_types: Vec<ObjectType>,
    /// 全局有类型关系；action 创建的隐式边也会进入同一 DL 投影。
    #[serde(default)]
    pub relations: Vec<RelationDef>,
}

impl OntologyIR {
    /// 按名称查找对象类型（codegen / query 复用）。
    pub fn find(&self, name: &str) -> Option<&ObjectType> {
        self.object_types.iter().find(|o| o.name == name)
    }
}
