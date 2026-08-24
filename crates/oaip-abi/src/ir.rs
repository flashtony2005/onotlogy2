//! Ontology IR —— 语言无关的中间表示（对应 REQ-001 / REQ-002）

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// 标量类型。V0.1 仅支持基础标量；Relation / Event 类型在 V0.2+。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
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

/// A composable data-range expression attached to a typed ODL property.
///
/// The property declaration supplies the leading datatype; unqualified
/// `datatype` and `restriction` denote that same base datatype.  A union may
/// additionally contain an explicit `Base` branch, which is the only sanctioned
/// cross-datatype extension and keeps executable rows within a finite declared
/// set of scalar types.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PropertyDataRange {
    Datatype,
    Base(ScalarType),
    OneOf(Vec<String>),
    Restriction(Vec<PropertyFacet>),
    Complement(Box<PropertyDataRange>),
    Union(Vec<PropertyDataRange>),
    Intersection(Vec<PropertyDataRange>),
}

impl PropertyDataRange {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Datatype => "datatype",
            Self::Base(_) => "datatype",
            Self::OneOf(_) => "one_of",
            Self::Restriction(_) => "restriction",
            Self::Complement(_) => "complement",
            Self::Union(_) => "union",
            Self::Intersection(_) => "intersection",
        }
    }

    pub fn children(&self) -> &[PropertyDataRange] {
        match self {
            Self::Union(children) | Self::Intersection(children) => children,
            _ => &[],
        }
    }
}

/// One lexical XSD-style constraint attached to an ODL property.
///
/// Ordering bounds remain lexical so `decimal` never travels through an IEEE
/// float and date/date-time forms can be validated against their declared base
/// datatype before entering the knowledge kernel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PropertyFacet {
    MinCardinality(String),
    MaxCardinality(String),
    OneOf(Vec<String>),
    Range(PropertyDataRange),
    MinInclusive(String),
    MinExclusive(String),
    MaxInclusive(String),
    MaxExclusive(String),
    MinLength(String),
    MaxLength(String),
    TotalDigits(String),
    FractionDigits(String),
    Pattern(String),
}

impl PropertyFacet {
    pub fn name(&self) -> &'static str {
        match self {
            Self::MinCardinality(_) => "min_cardinality",
            Self::MaxCardinality(_) => "max_cardinality",
            Self::OneOf(_) => "one_of",
            Self::Range(_) => "range",
            Self::MinInclusive(_) => "min_inclusive",
            Self::MinExclusive(_) => "min_exclusive",
            Self::MaxInclusive(_) => "max_inclusive",
            Self::MaxExclusive(_) => "max_exclusive",
            Self::MinLength(_) => "min_length",
            Self::MaxLength(_) => "max_length",
            Self::TotalDigits(_) => "total_digits",
            Self::FractionDigits(_) => "fraction_digits",
            Self::Pattern(_) => "pattern",
        }
    }

    pub fn value(&self) -> &str {
        match self {
            Self::OneOf(_) | Self::Range(_) => "",
            Self::MinCardinality(value)
            | Self::MaxCardinality(value)
            | Self::MinInclusive(value)
            | Self::MinExclusive(value)
            | Self::MaxInclusive(value)
            | Self::MaxExclusive(value)
            | Self::MinLength(value)
            | Self::MaxLength(value)
            | Self::TotalDigits(value)
            | Self::FractionDigits(value)
            | Self::Pattern(value) => value,
        }
    }

    /// Lexical alternatives attached to `one_of`.
    pub fn one_of_values(&self) -> Option<&[String]> {
        match self {
            Self::OneOf(values) => Some(values),
            _ => None,
        }
    }
}

/// 单个属性定义。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PropertyDef {
    pub name: String,
    pub ty: ScalarType,
    #[serde(default)]
    pub constraints: Vec<PropertyFacet>,
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
    /// Declared executable taxonomy edges; each target must be another object type.
    #[serde(default)]
    pub extends: Vec<String>,
    pub properties: Vec<PropertyDef>,
    /// 对象自有动作（V0.4）；ODL 未声明时为空。
    #[serde(default)]
    pub actions: Vec<ActionDef>,
    /// Necessary object-property conditions declared directly on this class.
    #[serde(default)]
    pub restrictions: Vec<ObjectPropertyRestriction>,
    /// Necessary data-property conditions declared directly on this class.
    #[serde(default)]
    pub data_restrictions: Vec<DataPropertyRestriction>,
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
    /// Ordered object properties whose composition implies this relation.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub composed_of: Vec<String>,
    /// Minimum number of outgoing edges required for every source instance.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_cardinality: Option<u32>,
    /// Maximum number of outgoing edges allowed for every source instance.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_cardinality: Option<u32>,
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

impl RelationDef {
    /// Validate declared participation bounds at both language and governance boundaries.
    pub fn validate_cardinality(&self) -> Result<(), String> {
        if let (Some(min), Some(max)) = (self.min_cardinality, self.max_cardinality)
            && min > max
        {
            return Err(format!(
                "relation `{}` has min cardinality {min} greater than max cardinality {max}",
                self.name
            ));
        }
        Ok(())
    }
}

/// A declared object-property inclusion between two executable relations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelationHierarchy {
    pub sub_relation: String,
    pub super_relation: String,
}

/// A declared data-property inclusion between executable scalar properties.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataPropertyHierarchy {
    pub sub_property: String,
    pub super_property: String,
}

/// A named class equivalent to a disjoint union of declared classes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DisjointUnionGroup {
    pub class: String,
    pub members: Vec<String>,
}

/// A named class whose extension is exactly a set of named individuals.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectOneOfGroup {
    pub class: String,
    pub individuals: Vec<String>,
}

/// A named-class necessary condition over a declared executable relation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectPropertyRestriction {
    pub relation: String,
    pub kind: ObjectRestrictionKind,
}

/// The supported object-property restriction constructors.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ObjectRestrictionKind {
    /// `relation: some Class`
    Some { class: String },
    /// `relation: only Class`
    Only { class: String },
    /// `relation: value individual`
    Value { individual: String },
    /// `relation: min N Class`
    Min { cardinality: u32, class: String },
    /// `relation: max N Class`
    Max { cardinality: u32, class: String },
}

/// A named-class necessary condition over a declared scalar property.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DataPropertyRestriction {
    pub property: String,
    pub kind: DataRestrictionKind,
}

/// The supported data-property restriction constructors.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DataRestrictionKind {
    /// `property: some integer`
    Some { datatype: ScalarType },
    /// `property: only string`
    Only { datatype: ScalarType },
    /// `property: min N integer`
    Min {
        cardinality: u32,
        datatype: ScalarType,
    },
    /// `property: max N string`
    Max {
        cardinality: u32,
        datatype: ScalarType,
    },
    /// `property: value "active"`
    Value { literal: DataLiteral },
}

/// A typed literal used by `value` data-property restrictions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DataLiteral {
    Boolean(bool),
    Integer(i64),
    Double(f64),
    String(String),
}

/// 整份 ODL 解析出的 IR。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct OntologyIR {
    pub object_types: Vec<ObjectType>,
    /// 全局有类型关系；action 创建的隐式边也会进入同一 DL 投影。
    #[serde(default)]
    pub relations: Vec<RelationDef>,
    /// Named-class disjointness axioms declared directly in executable ODL.
    #[serde(default)]
    pub disjoint_groups: Vec<Vec<String>>,
    /// Named-class equivalence axioms declared directly in executable ODL.
    #[serde(default)]
    pub equivalent_groups: Vec<Vec<String>>,
    /// Named disjoint-union definitions declared directly in executable ODL.
    #[serde(default)]
    pub disjoint_unions: Vec<DisjointUnionGroup>,
    /// Named individual-enumeration definitions declared directly in executable ODL.
    #[serde(default)]
    pub object_one_ofs: Vec<ObjectOneOfGroup>,
    /// Object-property inclusions declared directly between executable relations.
    #[serde(default)]
    pub relation_hierarchy: Vec<RelationHierarchy>,
    /// Named relation disjointness groups declared directly in executable ODL.
    #[serde(default)]
    pub disjoint_relation_groups: Vec<Vec<String>>,
    /// Named relation equivalence groups declared directly in executable ODL.
    #[serde(default)]
    pub equivalent_relation_groups: Vec<Vec<String>>,
    /// Data-property inclusions declared directly between executable properties.
    #[serde(default)]
    pub data_property_hierarchy: Vec<DataPropertyHierarchy>,
    /// Named data-property disjointness groups declared directly in executable ODL.
    #[serde(default)]
    pub disjoint_data_property_groups: Vec<Vec<String>>,
    /// Named data-property equivalence groups declared directly in executable ODL.
    #[serde(default)]
    pub equivalent_data_property_groups: Vec<Vec<String>>,
}

impl OntologyIR {
    /// 按名称查找对象类型（codegen / query 复用）。
    pub fn find(&self, name: &str) -> Option<&ObjectType> {
        self.object_types.iter().find(|o| o.name == name)
    }

    /// Validate that direct object-property restrictions refer to declared
    /// executable relations and classes. Hand-built serialized IR cannot bypass
    /// the ODL grammar.
    pub fn validate_object_restrictions(&self) -> Result<(), ObjectRestrictionError> {
        let mut classes = self
            .object_types
            .iter()
            .map(|object| object.name.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        let mut relations = self
            .relations
            .iter()
            .map(|relation| relation.name.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        for object in &self.object_types {
            for action in &object.actions {
                for created in &action.creates {
                    if let Some(edge) = created.link_edge.as_ref() {
                        relations.insert(edge);
                        classes.insert(created.object_type.as_str());
                    }
                }
            }
        }

        let mut restriction_index = 0usize;
        for object in &self.object_types {
            for restriction in &object.restrictions {
                let index = restriction_index;
                restriction_index += 1;
                if restriction.relation.trim().is_empty() {
                    return Err(ObjectRestrictionError {
                        index,
                        object: object.name.clone(),
                        message: "restriction relation is empty".to_owned(),
                    });
                }
                if !relations.contains(restriction.relation.as_str()) {
                    return Err(ObjectRestrictionError {
                        index,
                        object: object.name.clone(),
                        message: format!(
                            "restriction references undeclared object property `{}`",
                            restriction.relation
                        ),
                    });
                }
                match &restriction.kind {
                    ObjectRestrictionKind::Min { cardinality: 0, .. }
                    | ObjectRestrictionKind::Max { cardinality: 0, .. } => {
                        return Err(ObjectRestrictionError {
                            index,
                            object: object.name.clone(),
                            message: "cardinality restriction must be at least 1".to_owned(),
                        });
                    }
                    ObjectRestrictionKind::Some { class }
                    | ObjectRestrictionKind::Only { class }
                    | ObjectRestrictionKind::Min { class, .. }
                    | ObjectRestrictionKind::Max { class, .. } => {
                        Self::validate_restriction_filler(class, &classes, &mut |message| {
                            ObjectRestrictionError {
                                index,
                                object: object.name.clone(),
                                message,
                            }
                        })?;
                    }
                    ObjectRestrictionKind::Value { individual } => {
                        if individual.trim().is_empty() {
                            return Err(ObjectRestrictionError {
                                index,
                                object: object.name.clone(),
                                message: "has-value individual is empty".to_owned(),
                            });
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// Validate that direct data-property restrictions refer to declared
    /// scalar properties with compatible types. Hand-built serialized IR cannot
    /// bypass the ODL grammar.
    pub fn validate_data_restrictions(&self) -> Result<(), DataRestrictionError> {
        let mut restriction_index = 0usize;
        for object in &self.object_types {
            let properties: std::collections::BTreeMap<&str, ScalarType> = object
                .properties
                .iter()
                .map(|property| (property.name.as_str(), property.ty))
                .collect();
            for restriction in &object.data_restrictions {
                let index = restriction_index;
                restriction_index += 1;
                if restriction.property.trim().is_empty() {
                    return Err(DataRestrictionError {
                        index,
                        object: object.name.clone(),
                        message: "data restriction property is empty".to_owned(),
                    });
                }
                let Some(&declared) = properties.get(restriction.property.as_str()) else {
                    return Err(DataRestrictionError {
                        index,
                        object: object.name.clone(),
                        message: format!(
                            "data restriction references undeclared property `{}`",
                            restriction.property
                        ),
                    });
                };
                match &restriction.kind {
                    DataRestrictionKind::Min { cardinality, .. }
                    | DataRestrictionKind::Max { cardinality, .. }
                        if *cardinality > 1 =>
                    {
                        return Err(DataRestrictionError {
                            index,
                            object: object.name.clone(),
                            message: "data cardinality restriction must be 0 or 1".to_owned(),
                        });
                    }
                    DataRestrictionKind::Some { datatype }
                    | DataRestrictionKind::Only { datatype }
                    | DataRestrictionKind::Min { datatype, .. }
                    | DataRestrictionKind::Max { datatype, .. } => {
                        if *datatype != declared {
                            return Err(DataRestrictionError {
                                index,
                                object: object.name.clone(),
                                message: format!(
                                    "data restriction datatype `{datatype:?}` does not match declared type `{declared:?}` for property `{}`",
                                    restriction.property
                                ),
                            });
                        }
                    }
                    DataRestrictionKind::Value { literal } => {
                        let literal_type = match literal {
                            DataLiteral::Boolean(_) => ScalarType::Bool,
                            DataLiteral::Integer(_) => ScalarType::Int,
                            DataLiteral::Double(_) => ScalarType::Float,
                            DataLiteral::String(_) => ScalarType::String,
                        };
                        if literal_type != declared {
                            return Err(DataRestrictionError {
                                index,
                                object: object.name.clone(),
                                message: format!(
                                    "data restriction literal does not match declared type `{declared:?}` for property `{}`",
                                    restriction.property
                                ),
                            });
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn validate_restriction_filler<'a>(
        class: &'a str,
        classes: &std::collections::BTreeSet<&'a str>,
        error: &mut dyn FnMut(String) -> ObjectRestrictionError,
    ) -> Result<(), ObjectRestrictionError> {
        if class.trim().is_empty() {
            return Err(error("restriction class is empty".to_owned()));
        }
        if !classes.contains(class) {
            return Err(error(format!(
                "restriction references undeclared class `{class}`"
            )));
        }
        Ok(())
    }

    /// Validate relation participation bounds so hand-built serialized IR cannot
    /// bypass ODL rules.
    pub fn validate_relation_cardinalities(&self) -> Result<(), String> {
        for relation in &self.relations {
            relation.validate_cardinality()?;
        }
        Ok(())
    }

    /// Validate that every inverse declaration has a unique target and agrees
    /// with any reciprocal declaration and typed endpoints at governance
    /// boundaries.  A one-sided declaration is valid because the OWL axiom is
    /// inherently symmetric.
    pub fn validate_inverse_relations(&self) -> Result<(), InverseRelationError> {
        let by_name = self
            .relations
            .iter()
            .map(|relation| (relation.name.as_str(), relation))
            .collect::<std::collections::BTreeMap<_, _>>();
        let mut inverse_targets = std::collections::BTreeMap::<&str, &String>::new();
        for relation in &self.relations {
            let Some(inverse_name) = relation.inverse_of.as_ref() else {
                continue;
            };
            if inverse_name == &relation.name {
                return Err(InverseRelationError::new(
                    &relation.name,
                    "cannot declare itself as its own inverse",
                ));
            }
            if let std::collections::btree_map::Entry::Occupied(entry) =
                inverse_targets.entry(inverse_name.as_str())
                && entry.get().as_str() != relation.name.as_str()
            {
                return Err(InverseRelationError::new(
                    &relation.name,
                    format!(
                        "declares inverse '{inverse_name}', but that property is already declared as the inverse of '{}'",
                        entry.get()
                    ),
                ));
            } else {
                inverse_targets.insert(inverse_name.as_str(), &relation.name);
            }
            let Some(inverse) = by_name.get(inverse_name.as_str()) else {
                return Err(InverseRelationError::new(
                    &relation.name,
                    format!("references undeclared inverse relation '{inverse_name}'"),
                ));
            };
            if inverse.inverse_of.is_some()
                && inverse.inverse_of.as_deref() != Some(relation.name.as_str())
            {
                return Err(InverseRelationError::new(
                    &relation.name,
                    format!("is not reciprocated by inverse relation '{inverse_name}'"),
                ));
            }
            if (relation.source.as_str(), relation.target.as_str())
                != (inverse.target.as_str(), inverse.source.as_str())
            {
                return Err(InverseRelationError::new(
                    &relation.name,
                    format!("must reverse the endpoints of inverse relation '{inverse_name}'"),
                ));
            }
        }
        Ok(())
    }

    /// Validate named disjoint/equivalent class groups at both language and
    /// governance boundaries so hand-built serialized IR cannot bypass ODL rules.
    pub fn validate_class_groups(&self) -> Result<(), ClassGroupError> {
        let names = self
            .object_types
            .iter()
            .map(|object| object.name.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        Self::validate_class_group("disjoint", &self.disjoint_groups, &names)?;
        Self::validate_class_group("equivalent", &self.equivalent_groups, &names)?;
        Ok(())
    }

    /// Validate disjoint-union definitions at both language and governance
    /// boundaries so hand-built serialized IR cannot bypass ODL rules.
    pub fn validate_disjoint_unions(&self) -> Result<(), DisjointUnionError> {
        let names = self
            .object_types
            .iter()
            .map(|object| object.name.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        let mut seen_classes = std::collections::BTreeSet::new();
        for (index, group) in self.disjoint_unions.iter().enumerate() {
            if group.class.trim().is_empty() {
                return Err(DisjointUnionError {
                    index,
                    message: "disjoint_union contains an empty class name".to_owned(),
                });
            }
            if !names.contains(group.class.as_str()) {
                return Err(DisjointUnionError {
                    index,
                    message: format!(
                        "disjoint_union references undeclared object type '{}'",
                        group.class
                    ),
                });
            }
            if !seen_classes.insert(group.class.as_str()) {
                return Err(DisjointUnionError {
                    index,
                    message: format!("disjoint_union repeats defined class '{}'", group.class),
                });
            }
            if group.members.len() < 2 {
                return Err(DisjointUnionError {
                    index,
                    message: "disjoint_union must contain at least two member classes".to_owned(),
                });
            }
            let mut members = std::collections::BTreeSet::new();
            for name in &group.members {
                if name.trim().is_empty() {
                    return Err(DisjointUnionError {
                        index,
                        message: "disjoint_union contains an empty member class".to_owned(),
                    });
                }
                if !names.contains(name.as_str()) {
                    return Err(DisjointUnionError {
                        index,
                        message: format!(
                            "disjoint_union references undeclared object type '{name}'"
                        ),
                    });
                }
                if name == &group.class {
                    return Err(DisjointUnionError {
                        index,
                        message: format!(
                            "disjoint_union class '{}' cannot be its own member",
                            group.class
                        ),
                    });
                }
                if !members.insert(name.as_str()) {
                    return Err(DisjointUnionError {
                        index,
                        message: format!("disjoint_union repeats member class '{name}'"),
                    });
                }
            }
        }
        Ok(())
    }

    /// Validate named individual-enumeration definitions at both language and
    /// governance boundaries so hand-built serialized IR cannot bypass ODL rules.
    pub fn validate_object_one_ofs(&self) -> Result<(), DisjointUnionError> {
        let names = self
            .object_types
            .iter()
            .map(|object| object.name.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        let mut seen_classes = std::collections::BTreeSet::new();
        for (index, group) in self.object_one_ofs.iter().enumerate() {
            if group.class.trim().is_empty() {
                return Err(DisjointUnionError {
                    index,
                    message: "one_of contains an empty class name".to_owned(),
                });
            }
            if !names.contains(group.class.as_str()) {
                return Err(DisjointUnionError {
                    index,
                    message: format!("one_of references undeclared object type '{}'", group.class),
                });
            }
            if !seen_classes.insert(group.class.as_str()) {
                return Err(DisjointUnionError {
                    index,
                    message: format!("one_of repeats defined class '{}'", group.class),
                });
            }
            if group.individuals.is_empty() {
                return Err(DisjointUnionError {
                    index,
                    message: "one_of must contain at least one individual".to_owned(),
                });
            }
            let mut seen = std::collections::BTreeSet::new();
            for name in &group.individuals {
                if name.trim().is_empty() {
                    return Err(DisjointUnionError {
                        index,
                        message: "one_of contains an empty individual".to_owned(),
                    });
                }
                if !seen.insert(name.as_str()) {
                    return Err(DisjointUnionError {
                        index,
                        message: format!("one_of repeats individual '{name}'"),
                    });
                }
            }
        }
        Ok(())
    }

    /// Validate relation hierarchy and disjointness at both language and
    /// governance boundaries so hand-built serialized IR cannot bypass ODL rules.
    pub fn validate_relation_property_groups(
        &self,
        relation_names: &std::collections::BTreeSet<&str>,
    ) -> Result<(), RelationPropertyGroupError> {
        let mut seen_edges = std::collections::BTreeSet::new();
        for (index, edge) in self.relation_hierarchy.iter().enumerate() {
            if edge.sub_relation.trim().is_empty() || edge.super_relation.trim().is_empty() {
                return Err(RelationPropertyGroupError {
                    index,
                    message: "relation hierarchy contains an empty property name".to_owned(),
                });
            }
            if !relation_names.contains(edge.sub_relation.as_str())
                || !relation_names.contains(edge.super_relation.as_str())
            {
                return Err(RelationPropertyGroupError {
                    index,
                    message: format!(
                        "relation hierarchy references undeclared relation '{} -> {}'",
                        edge.sub_relation, edge.super_relation
                    ),
                });
            }
            if edge.sub_relation == edge.super_relation {
                return Err(RelationPropertyGroupError {
                    index,
                    message: format!(
                        "relation '{}' cannot be its own sub-relation",
                        edge.sub_relation
                    ),
                });
            }
            if !seen_edges.insert((edge.sub_relation.as_str(), edge.super_relation.as_str())) {
                return Err(RelationPropertyGroupError {
                    index,
                    message: format!(
                        "duplicate relation hierarchy '{} -> {}'",
                        edge.sub_relation, edge.super_relation
                    ),
                });
            }
        }

        Self::validate_disjoint_relation_group(&self.disjoint_relation_groups, relation_names)
    }

    /// Validate relation equivalence groups at both language and governance
    /// boundaries so hand-built serialized IR cannot bypass ODL rules.
    pub fn validate_equivalent_relation_groups(
        &self,
        relation_names: &std::collections::BTreeSet<&str>,
    ) -> Result<(), RelationPropertyGroupError> {
        let mut seen_groups = std::collections::BTreeSet::new();
        for (index, group) in self.equivalent_relation_groups.iter().enumerate() {
            if group.len() < 2 {
                return Err(RelationPropertyGroupError {
                    index,
                    message: "equivalent_relation group must contain at least two relations"
                        .to_owned(),
                });
            }
            let mut members = std::collections::BTreeSet::new();
            for name in group {
                if name.trim().is_empty() {
                    return Err(RelationPropertyGroupError {
                        index,
                        message: "equivalent_relation group contains an empty relation name"
                            .to_owned(),
                    });
                }
                if !relation_names.contains(name.as_str()) {
                    return Err(RelationPropertyGroupError {
                        index,
                        message: format!(
                            "equivalent_relation group references undeclared relation '{name}'"
                        ),
                    });
                }
                if !members.insert(name.as_str()) {
                    return Err(RelationPropertyGroupError {
                        index,
                        message: format!("equivalent_relation group repeats relation '{name}'"),
                    });
                }
            }
            if !seen_groups.insert(members) {
                return Err(RelationPropertyGroupError {
                    index,
                    message: "duplicate equivalent_relation group".to_owned(),
                });
            }
        }
        Ok(())
    }

    /// Validate data-property hierarchy and disjointness at both language and
    /// governance boundaries so hand-built serialized IR cannot bypass ODL rules.
    pub fn validate_data_property_groups(
        &self,
        property_names: &std::collections::BTreeSet<&str>,
    ) -> Result<(), DataPropertyGroupError> {
        let mut seen_edges = std::collections::BTreeSet::new();
        for (index, edge) in self.data_property_hierarchy.iter().enumerate() {
            if edge.sub_property.trim().is_empty() || edge.super_property.trim().is_empty() {
                return Err(DataPropertyGroupError {
                    index,
                    message: "data_property hierarchy contains an empty property name".to_owned(),
                });
            }
            if !property_names.contains(edge.sub_property.as_str())
                || !property_names.contains(edge.super_property.as_str())
            {
                return Err(DataPropertyGroupError {
                    index,
                    message: format!(
                        "data_property hierarchy references undeclared property '{} -> {}'",
                        edge.sub_property, edge.super_property
                    ),
                });
            }
            if edge.sub_property == edge.super_property {
                return Err(DataPropertyGroupError {
                    index,
                    message: format!(
                        "property '{}' cannot be its own sub-property",
                        edge.sub_property
                    ),
                });
            }
            if !seen_edges.insert((edge.sub_property.as_str(), edge.super_property.as_str())) {
                return Err(DataPropertyGroupError {
                    index,
                    message: format!(
                        "duplicate data_property hierarchy '{} -> {}'",
                        edge.sub_property, edge.super_property
                    ),
                });
            }
        }

        Self::validate_disjoint_data_property_group(
            &self.disjoint_data_property_groups,
            property_names,
        )
    }

    /// Validate data-property equivalence groups at both language and governance
    /// boundaries so hand-built serialized IR cannot bypass ODL rules.
    pub fn validate_equivalent_data_property_groups(
        &self,
        property_names: &std::collections::BTreeSet<&str>,
    ) -> Result<(), DataPropertyGroupError> {
        let mut seen_groups = std::collections::BTreeSet::new();
        for (index, group) in self.equivalent_data_property_groups.iter().enumerate() {
            if group.len() < 2 {
                return Err(DataPropertyGroupError {
                    index,
                    message: "equivalent_data_property group must contain at least two properties"
                        .to_owned(),
                });
            }
            let mut members = std::collections::BTreeSet::new();
            for name in group {
                if name.trim().is_empty() {
                    return Err(DataPropertyGroupError {
                        index,
                        message: "equivalent_data_property group contains an empty property name"
                            .to_owned(),
                    });
                }
                if !property_names.contains(name.as_str()) {
                    return Err(DataPropertyGroupError {
                        index,
                        message: format!(
                            "equivalent_data_property group references undeclared property '{name}'"
                        ),
                    });
                }
                if !members.insert(name.as_str()) {
                    return Err(DataPropertyGroupError {
                        index,
                        message: format!(
                            "equivalent_data_property group repeats property '{name}'"
                        ),
                    });
                }
            }
            if !seen_groups.insert(members) {
                return Err(DataPropertyGroupError {
                    index,
                    message: "duplicate equivalent_data_property group".to_owned(),
                });
            }
        }
        Ok(())
    }

    fn validate_disjoint_data_property_group(
        groups: &[Vec<String>],
        property_names: &std::collections::BTreeSet<&str>,
    ) -> Result<(), DataPropertyGroupError> {
        let mut seen_groups = std::collections::BTreeSet::new();
        for (index, group) in groups.iter().enumerate() {
            if group.len() < 2 {
                return Err(DataPropertyGroupError {
                    index,
                    message: "disjoint_data_property group must contain at least two properties"
                        .to_owned(),
                });
            }
            let mut members = std::collections::BTreeSet::new();
            for name in group {
                if name.trim().is_empty() {
                    return Err(DataPropertyGroupError {
                        index,
                        message: "disjoint_data_property group contains an empty property name"
                            .to_owned(),
                    });
                }
                if !property_names.contains(name.as_str()) {
                    return Err(DataPropertyGroupError {
                        index,
                        message: format!(
                            "disjoint_data_property group references undeclared property '{name}'"
                        ),
                    });
                }
                if !members.insert(name.as_str()) {
                    return Err(DataPropertyGroupError {
                        index,
                        message: format!("disjoint_data_property group repeats property '{name}'"),
                    });
                }
            }
            if !seen_groups.insert(members) {
                return Err(DataPropertyGroupError {
                    index,
                    message: "duplicate disjoint_data_property group".to_owned(),
                });
            }
        }
        Ok(())
    }

    fn validate_disjoint_relation_group(
        groups: &[Vec<String>],
        relation_names: &std::collections::BTreeSet<&str>,
    ) -> Result<(), RelationPropertyGroupError> {
        let mut seen_groups = std::collections::BTreeSet::new();
        for (index, group) in groups.iter().enumerate() {
            if group.len() < 2 {
                return Err(RelationPropertyGroupError {
                    index,
                    message: "disjoint_relation group must contain at least two relations"
                        .to_owned(),
                });
            }
            let mut members = std::collections::BTreeSet::new();
            for name in group {
                if name.trim().is_empty() {
                    return Err(RelationPropertyGroupError {
                        index,
                        message: "disjoint_relation group contains an empty relation name"
                            .to_owned(),
                    });
                }
                if !relation_names.contains(name.as_str()) {
                    return Err(RelationPropertyGroupError {
                        index,
                        message: format!(
                            "disjoint_relation group references undeclared relation '{name}'"
                        ),
                    });
                }
                if !members.insert(name.as_str()) {
                    return Err(RelationPropertyGroupError {
                        index,
                        message: format!("disjoint_relation group repeats relation '{name}'"),
                    });
                }
            }
            if !seen_groups.insert(members) {
                return Err(RelationPropertyGroupError {
                    index,
                    message: "duplicate disjoint_relation group".to_owned(),
                });
            }
        }
        Ok(())
    }

    fn validate_class_group(
        kind: &'static str,
        groups: &[Vec<String>],
        names: &std::collections::BTreeSet<&str>,
    ) -> Result<(), ClassGroupError> {
        let mut seen_groups = std::collections::BTreeSet::new();
        for (index, group) in groups.iter().enumerate() {
            if group.len() < 2 {
                return Err(ClassGroupError {
                    index,
                    kind,
                    message: format!("{kind} group must contain at least two classes"),
                });
            }
            let mut members = std::collections::BTreeSet::new();
            for name in group {
                if name.trim().is_empty() {
                    return Err(ClassGroupError {
                        index,
                        kind,
                        message: format!("{kind} group contains an empty class name"),
                    });
                }
                if !names.contains(name.as_str()) {
                    return Err(ClassGroupError {
                        index,
                        kind,
                        message: format!("{kind} group references undeclared object type '{name}'"),
                    });
                }
                if !members.insert(name.as_str()) {
                    return Err(ClassGroupError {
                        index,
                        kind,
                        message: format!("{kind} group repeats class '{name}'"),
                    });
                }
            }
            if !seen_groups.insert(members) {
                return Err(ClassGroupError {
                    index,
                    kind,
                    message: format!("duplicate {kind} group"),
                });
            }
        }
        Ok(())
    }

    /// Validate the strict executable taxonomy shared by the ODL frontend and
    /// governance boundary.  Returning the source object lets parsers retain
    /// declaration line information without duplicating graph semantics.
    pub fn validate_extends(&self) -> Result<(), ExtendsError> {
        let names = self
            .object_types
            .iter()
            .map(|object| object.name.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        for object in &self.object_types {
            let mut seen = std::collections::BTreeSet::new();
            for parent in &object.extends {
                if parent.trim().is_empty() {
                    return Err(ExtendsError::new(&object.name, "extends 列表含有空父类型"));
                }
                if !seen.insert(parent.as_str()) {
                    return Err(ExtendsError::new(
                        &object.name,
                        format!("重复继承父类型 '{parent}'"),
                    ));
                }
                if !names.contains(parent.as_str()) {
                    return Err(ExtendsError::new(
                        &object.name,
                        format!("继承未声明对象类型 '{parent}'"),
                    ));
                }
                if parent == &object.name {
                    return Err(ExtendsError::new(&object.name, "不能继承自身"));
                }
            }
        }

        // A strict taxonomy cannot cycle; otherwise distinct `extends` edges
        // silently collapse into equivalence at the DL layer.
        for start in &self.object_types {
            let mut stack = vec![(start.name.as_str(), vec![start.name.as_str()])];
            while let Some((name, path)) = stack.pop() {
                let parents = self
                    .object_types
                    .iter()
                    .find(|object| object.name == name)
                    .map(|object| object.extends.iter().as_slice())
                    .unwrap_or_default();
                for parent in parents {
                    if *parent == start.name {
                        let mut cycle_path = path.clone();
                        cycle_path.push(parent);
                        return Err(ExtendsError::new(
                            &start.name,
                            format!("继承关系形成环: {}", cycle_path.join(" -> ")),
                        ));
                    }
                    let mut next_path = path.clone();
                    next_path.push(parent);
                    stack.push((parent.as_str(), next_path));
                }
            }
        }
        Ok(())
    }
}

/// An invalid named-class group, identified by declaration order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassGroupError {
    pub index: usize,
    pub kind: &'static str,
    pub message: String,
}

/// An invalid disjoint-union definition, identified by declaration order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisjointUnionError {
    pub index: usize,
    pub message: String,
}

/// An invalid direct object-property restriction, attributed to its class.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectRestrictionError {
    pub index: usize,
    pub object: String,
    pub message: String,
}

/// An invalid direct data-property restriction, attributed to its class.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataRestrictionError {
    pub index: usize,
    pub object: String,
    pub message: String,
}

/// An invalid relation property hierarchy/group, identified by declaration order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelationPropertyGroupError {
    pub index: usize,
    pub message: String,
}

/// An invalid inverse-relation declaration, attributed to its source relation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InverseRelationError {
    pub relation: String,
    pub message: String,
}

impl InverseRelationError {
    fn new(relation: &str, message: impl Into<String>) -> Self {
        Self {
            relation: relation.to_owned(),
            message: message.into(),
        }
    }
}

/// An invalid data-property hierarchy/group, identified by declaration order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataPropertyGroupError {
    pub index: usize,
    pub message: String,
}

/// An invalid executable-taxonomy edge, attributed to its declaring object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtendsError {
    pub object: String,
    pub message: String,
}

impl ExtendsError {
    fn new(object: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            object: object.into(),
            message: message.into(),
        }
    }
}
