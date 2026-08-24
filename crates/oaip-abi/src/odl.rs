//! ODL 解析器 —— 把 .odl 文本解析为 `OntologyIR`（对应 REQ-001 / REQ-002）
//!
//! 语法（V0.1~V0.4，确定性，无 LLM）：
//! ```text
//! object Customer:
//!   properties:
//!     name: string
//!     level: string
//!   actions:                       # V0.4 完整 Action Engine：对象自有能力
//!     updateLevel:
//!       params:
//!         level: string
//!       updates:
//!         - field: level
//!           value: level            # 引用参数；self / self.<field> / "字面量" 亦支持
//!     createOrder:
//!       params:
//!         amount: number
//!       creates:
//!         - type: Order
//!           data:
//!             amount: amount
//!             status: "NEW"
//!           link: places            # 建立 target --places--> Order
//! ```
//! Event 顶层语法暂未开放；action 内的 effects 已是声明式表达。

use crate::data_range::parse_data_range;
use crate::data_range::validate_data_range;
use crate::ir::{
    ActionDef, ActionPlanning, Clause, CompareOp, CreateEffect, DataLiteral, DataPropertyHierarchy,
    DataPropertyRestriction, DataRestrictionKind, DisjointUnionGroup, FieldUpdate,
    ObjectOneOfGroup, ObjectPropertyRestriction, ObjectRestrictionKind, ObjectType, OntologyIR,
    OpportunityRule, OutcomeTiming, ParamDef, PolicyDef, PropertyDataRange, PropertyDef,
    PropertyFacet, RelationDef, RelationHierarchy, ScalarType, ValueExpr,
};
use crate::validation::validate_property_constraints;
use std::str::FromStr;

/// ODL 解析错误：结构化、含行列号与错误码，绝不 panic。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("ODL 错误 #{code} @ {line}:{col} — {message}")]
pub struct OdlError {
    pub code: u32,
    pub message: String,
    pub line: u32,
    pub col: u32,
}

impl OdlError {
    pub fn new(code: u32, message: impl Into<String>, line: u32, col: u32) -> Self {
        Self {
            code,
            message: message.into(),
            line,
            col,
        }
    }
}

// 错误码（语义明确，便于断言）
const E_OBJECT_NAME: u32 = 1004; // object 声明缺少名称
const E_DUP_OBJECT: u32 = 1006; // 重复对象定义（同名生成重复 struct）
const E_NO_PROP_BLOCK: u32 = 1005; // 属性出现在 properties: 块外
const E_EMPTY_PROP_NAME: u32 = 1003; // 属性名为空 / 无法解析行
const E_UNKNOWN_TYPE: u32 = 1002; // 未知标量类型
const E_MISSING_PROPERTY: u32 = 1001; // object 无任何 property
const E_RELATION_NAME: u32 = 1007; // relation 声明缺少名称
const E_DUP_RELATION: u32 = 1008; // 重复 relation 定义或与 action 边冲突
const E_RELATION_ENDPOINT: u32 = 1009; // relation 端点不是已声明对象类型
const E_RELATION_CONTRADICTION: u32 = 1010;
const E_INVALID_PROPERTY_CONSTRAINT: u32 = 1011; // relation 特征互相矛盾或引用无效
const E_INVALID_OBJECT_HIERARCHY: u32 = 1012; // object extends 引用无效或形成继承环
const E_INVALID_CLASS_GROUP: u32 = 1013; // disjoint/equivalent 类组声明无效
const E_INVALID_DISJOINT_UNION: u32 = 1014; // disjoint_union 类定义无效
const E_INVALID_OBJECT_RESTRICTION: u32 = 1015; // 对象属性限制引用无效
const E_INVALID_DATA_RESTRICTION: u32 = 1016; // 数据属性限制引用无效或类型不匹配
const E_INVALID_OBJECT_ONE_OF: u32 = 1017; // one_of 类个体枚举定义无效
const E_INVALID_DATATYPE: u32 = 1018; // 自定义 datatype 定义无效或重复
const E_INVALID_INDIVIDUAL_GROUP: u32 = 1019; // same/different individual 组声明无效

/// 解析上下文中的块（基于缩进的简易状态机）。
#[derive(Debug, Clone, Copy, PartialEq)]
enum Section {
    Top,
    Object,
    Properties,
    Actions,
    Action,
    ActionParams,
    ActionUpdates,
    ActionCreates,
    CreateData,
    ActionPolicy,
    ActionPlanning,
    Opportunities,
    OpportunityRule,
    OpportunityCondition,
    ActionPrecondition,
    ObjectRestrictions,
    DataRestrictions,
    Relation,
}

/// 解析 .odl 源文本为 IR。
pub fn parse_odl(src: &str) -> Result<OntologyIR, OdlError> {
    let mut ir = OntologyIR::default();
    // 暂存「声明行号 + 对象」，便于缺 property 时回带真实行列号（TC-001-b 契约）。
    let mut declared: Vec<(u32, ObjectType)> = Vec::new();
    let mut current: Option<(u32, ObjectType)> = None;
    let mut section: Vec<(Section, usize)> = vec![(Section::Top, 0)];
    let mut cur_action: Option<(u32, ActionDef)> = None;
    let mut cur_update: Option<FieldUpdate> = None;
    let mut cur_create: Option<CreateEffect> = None;
    let mut cur_clause: Option<Clause> = None;
    let mut cur_opportunity: Option<(u32, OpportunityRule)> = None;
    let mut opportunity_fields = [false; 6];
    let mut declared_relations: Vec<(u32, RelationDef)> = Vec::new();
    let mut current_relation: Option<(u32, RelationDef)> = None;
    let mut link_lines: std::collections::BTreeMap<String, u32> = std::collections::BTreeMap::new();
    let mut declared_disjoint: Vec<(u32, Vec<String>)> = Vec::new();
    let mut declared_equivalent: Vec<(u32, Vec<String>)> = Vec::new();
    let mut declared_disjoint_unions: Vec<(u32, DisjointUnionGroup)> = Vec::new();
    let mut declared_object_one_ofs: Vec<(u32, ObjectOneOfGroup)> = Vec::new();
    let mut declared_restrictions: Vec<(u32, String, ObjectPropertyRestriction)> = Vec::new();
    let mut declared_data_restrictions: Vec<(u32, String, DataPropertyRestriction)> = Vec::new();
    let mut declared_sub_relations: Vec<(u32, (String, String))> = Vec::new();
    let mut declared_disjoint_relations: Vec<(u32, Vec<String>)> = Vec::new();
    let mut declared_equivalent_relations: Vec<(u32, Vec<String>)> = Vec::new();
    let mut declared_sub_data_properties: Vec<(u32, (String, String))> = Vec::new();
    let mut declared_disjoint_data_properties: Vec<(u32, Vec<String>)> = Vec::new();
    let mut declared_equivalent_data_properties: Vec<(u32, Vec<String>)> = Vec::new();
    let mut declared_same_individuals: Vec<(u32, Vec<String>)> = Vec::new();
    let mut declared_different_individuals: Vec<(u32, Vec<String>)> = Vec::new();
    let mut declared_datatypes: std::collections::BTreeMap<
        String,
        (ScalarType, PropertyDataRange),
    > = std::collections::BTreeMap::new();
    let mut planning_fields = [false; 3];

    let indent_of = |raw: &str| raw.len() - raw.trim_start().len();

    'line: for (idx, raw) in src.lines().enumerate() {
        let line_no = (idx + 1) as u32;
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let indent = indent_of(raw);

        // Named datatype definition.  The executable IR intentionally stores the
        // expanded base type/range; governance and reasoning therefore see the
        // same closed semantics whether input came from ODL or proposal JSON.
        if let Some(rest) = line.strip_prefix("datatype ") {
            flush_update(&mut cur_update, &mut cur_action);
            flush_create(&mut cur_create, &mut cur_action);
            flush_action(&mut cur_action, &mut current);
            if let Some(obj) = current.take() {
                declared.push(obj);
            }
            if let Some(relation) = current_relation.take() {
                declared_relations.push(relation);
            }

            let invalid = |message: String| OdlError::new(E_INVALID_DATATYPE, message, line_no, 1);
            let (name, definition) = rest.split_once('=').ok_or_else(|| {
                invalid("datatype declaration needs `Name = base range-expression`".to_owned())
            })?;
            let name = name.trim();
            if name.is_empty()
                || name.contains(char::is_whitespace)
                || ScalarType::from_str(name).is_ok()
            {
                return Err(invalid(
                    "datatype name must be a non-empty identifier distinct from base scalar types"
                        .to_owned(),
                ));
            }
            if declared_datatypes.contains_key(name) {
                return Err(invalid(format!("duplicate datatype definition `{name}`")));
            }

            let definition = definition.trim();
            let Some((base, range_source)) = definition.split_once(char::is_whitespace) else {
                return Err(invalid(
                    "datatype definition needs a base scalar type and range expression".to_owned(),
                ));
            };
            let ty = ScalarType::from_str(base.trim()).map_err(&invalid)?;
            let range = parse_data_range(range_source.trim()).map_err(&invalid)?;
            validate_data_range(name, ty, &range).map_err(invalid)?;
            declared_datatypes.insert(name.to_string(), (ty, range));
            section = vec![(Section::Top, 0)];
            continue 'line;
        }

        // Common OWL covered-class pattern: one named class is exactly the
        // union of a pairwise-disjoint set of named classes.
        if let Some(rest) = line.strip_prefix("disjoint_union ") {
            flush_update(&mut cur_update, &mut cur_action);
            flush_create(&mut cur_create, &mut cur_action);
            flush_action(&mut cur_action, &mut current);
            if let Some(obj) = current.take() {
                declared.push(obj);
            }
            if let Some(relation) = current_relation.take() {
                declared_relations.push(relation);
            }

            let invalid =
                |message: String| OdlError::new(E_INVALID_DISJOINT_UNION, message, line_no, 1);
            let (class, members_source) = rest.split_once('=').ok_or_else(|| {
                invalid("disjoint_union declaration needs `Class = Member, ...`".to_owned())
            })?;
            let class = class.trim();
            if class.is_empty() || class.contains(char::is_whitespace) {
                return Err(invalid(
                    "disjoint_union contains an empty or invalid class name".to_owned(),
                ));
            }
            let members = members_source
                .split(',')
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>();
            if members.len() < 2 {
                return Err(invalid(
                    "disjoint_union must contain at least two member classes".to_owned(),
                ));
            }
            declared_disjoint_unions.push((
                line_no,
                DisjointUnionGroup {
                    class: class.to_string(),
                    members,
                },
            ));
            section = vec![(Section::Top, 0)];
            continue 'line;
        }

        // Named individual-enumeration: one named class contains exactly these individuals.
        if let Some(rest) = line.strip_prefix("one_of ") {
            flush_update(&mut cur_update, &mut cur_action);
            flush_create(&mut cur_create, &mut cur_action);
            flush_action(&mut cur_action, &mut current);
            if let Some(obj) = current.take() {
                declared.push(obj);
            }
            if let Some(relation) = current_relation.take() {
                declared_relations.push(relation);
            }

            let invalid =
                |message: String| OdlError::new(E_INVALID_OBJECT_ONE_OF, message, line_no, 1);
            let (class, individuals_source) = rest.split_once('=').ok_or_else(|| {
                invalid("one_of declaration needs `Class = Individual, ...`".to_owned())
            })?;
            let class = class.trim();
            if class.is_empty() || class.contains(char::is_whitespace) {
                return Err(invalid(
                    "one_of contains an empty or invalid class name".to_owned(),
                ));
            }
            let individuals = individuals_source
                .split(',')
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>();
            if individuals.is_empty() {
                return Err(invalid(
                    "one_of must contain at least one individual".to_owned(),
                ));
            }
            declared_object_one_ofs.push((
                line_no,
                ObjectOneOfGroup {
                    class: class.to_string(),
                    individuals,
                },
            ));
            section = vec![(Section::Top, 0)];
            continue 'line;
        }

        // Named-class DL 组声明：让常见 TBox 约束无需手写 proposal JSON。
        for (keyword, is_same) in [
            ("same_individuals ", true),
            ("different_individuals ", false),
        ] {
            let Some(mut rest) = line.strip_prefix(keyword) else {
                continue;
            };
            flush_update(&mut cur_update, &mut cur_action);
            flush_create(&mut cur_create, &mut cur_action);
            flush_action(&mut cur_action, &mut current);
            if let Some(obj) = current.take() {
                declared.push(obj);
            }
            if let Some(relation) = current_relation.take() {
                declared_relations.push(relation);
            }
            rest = rest.trim();
            let group = rest
                .split(',')
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>();
            if group.len() < 2 {
                return Err(OdlError::new(
                    E_INVALID_INDIVIDUAL_GROUP,
                    format!(
                        "{} declaration needs at least two individuals",
                        keyword.trim()
                    ),
                    line_no,
                    1,
                ));
            }
            if is_same {
                declared_same_individuals.push((line_no, group));
            } else {
                declared_different_individuals.push((line_no, group));
            }
            section = vec![(Section::Top, 0)];
            continue 'line;
        }

        for (keyword, is_disjoint) in [("disjoint ", true), ("equivalent ", false)] {
            let Some(mut rest) = line.strip_prefix(keyword) else {
                continue;
            };
            flush_update(&mut cur_update, &mut cur_action);
            flush_create(&mut cur_create, &mut cur_action);
            flush_action(&mut cur_action, &mut current);
            if let Some(obj) = current.take() {
                declared.push(obj);
            }
            if let Some(relation) = current_relation.take() {
                declared_relations.push(relation);
            }
            rest = rest.trim();
            let group = rest
                .split(',')
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>();
            if group.len() < 2 {
                return Err(OdlError::new(
                    E_INVALID_CLASS_GROUP,
                    format!("{} 声明至少需要两个对象类型", keyword.trim()),
                    line_no,
                    1,
                ));
            }
            if is_disjoint {
                declared_disjoint.push((line_no, group));
            } else {
                declared_equivalent.push((line_no, group));
            }
            section = vec![(Section::Top, 0)];
            continue 'line;
        }

        // Scalar-property DL 组声明：常见数据属性公理无需手写 proposal。
        for (keyword, kind) in [
            ("sub_data_property ", "hierarchy"),
            ("disjoint_data_property ", "disjoint"),
            ("equivalent_data_property ", "equivalent"),
        ] {
            let Some(rest) = line.strip_prefix(keyword) else {
                continue;
            };
            flush_update(&mut cur_update, &mut cur_action);
            flush_create(&mut cur_create, &mut cur_action);
            flush_action(&mut cur_action, &mut current);
            if let Some(obj) = current.take() {
                declared.push(obj);
            }
            if let Some(relation) = current_relation.take() {
                declared_relations.push(relation);
            }
            let group = rest
                .trim()
                .split(',')
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>();
            if kind == "hierarchy" {
                if group.len() != 2 {
                    return Err(OdlError::new(
                        E_INVALID_PROPERTY_CONSTRAINT,
                        "sub_data_property declaration needs exactly two properties",
                        line_no,
                        1,
                    ));
                }
                declared_sub_data_properties.push((line_no, (group[0].clone(), group[1].clone())));
            } else if kind == "equivalent" {
                if group.len() < 2 {
                    return Err(OdlError::new(
                        E_INVALID_PROPERTY_CONSTRAINT,
                        "equivalent_data_property declaration needs at least two properties",
                        line_no,
                        1,
                    ));
                }
                declared_equivalent_data_properties.push((line_no, group));
            } else {
                if group.len() < 2 {
                    return Err(OdlError::new(
                        E_INVALID_PROPERTY_CONSTRAINT,
                        "disjoint_data_property declaration needs at least two properties",
                        line_no,
                        1,
                    ));
                }
                declared_disjoint_data_properties.push((line_no, group));
            }
            section = vec![(Section::Top, 0)];
            continue 'line;
        }

        // Relation-property DL 组声明：常见对象属性公理无需手写 proposal。
        for (keyword, kind) in [
            ("sub_relation ", "hierarchy"),
            ("disjoint_relation ", "disjoint"),
            ("equivalent_relation ", "equivalent"),
        ] {
            let Some(rest) = line.strip_prefix(keyword) else {
                continue;
            };
            flush_update(&mut cur_update, &mut cur_action);
            flush_create(&mut cur_create, &mut cur_action);
            flush_action(&mut cur_action, &mut current);
            if let Some(obj) = current.take() {
                declared.push(obj);
            }
            if let Some(relation) = current_relation.take() {
                declared_relations.push(relation);
            }
            let group = rest
                .trim()
                .split(',')
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>();
            if kind == "hierarchy" {
                if group.len() != 2 {
                    return Err(OdlError::new(
                        E_INVALID_PROPERTY_CONSTRAINT,
                        "sub_relation declaration needs exactly two relations",
                        line_no,
                        1,
                    ));
                }
                declared_sub_relations.push((line_no, (group[0].clone(), group[1].clone())));
            } else if kind == "equivalent" {
                if group.len() < 2 {
                    return Err(OdlError::new(
                        E_INVALID_PROPERTY_CONSTRAINT,
                        "equivalent_relation declaration needs at least two relations",
                        line_no,
                        1,
                    ));
                }
                declared_equivalent_relations.push((line_no, group));
            } else {
                if group.len() < 2 {
                    return Err(OdlError::new(
                        E_INVALID_PROPERTY_CONSTRAINT,
                        "disjoint_relation declaration needs at least two relations",
                        line_no,
                        1,
                    ));
                }
                declared_disjoint_relations.push((line_no, group));
            }
            section = vec![(Section::Top, 0)];
            continue 'line;
        }

        // relation 声明：全局有类型边，和 object 同级。
        if let Some(rest) = line.strip_prefix("relation ") {
            flush_update(&mut cur_update, &mut cur_action);
            flush_create(&mut cur_create, &mut cur_action);
            flush_action(&mut cur_action, &mut current);
            if let Some(obj) = current.take() {
                declared.push(obj);
            }
            if let Some(relation) = current_relation.take() {
                declared_relations.push(relation);
            }
            let name = rest.trim_end_matches(':').trim();
            if name.is_empty() {
                return Err(OdlError::new(
                    E_RELATION_NAME,
                    "relation 声明缺少名称",
                    line_no,
                    1,
                ));
            }
            current_relation = Some((
                line_no,
                RelationDef {
                    name: name.to_string(),
                    ..RelationDef::default()
                },
            ));
            section = vec![(Section::Relation, indent)];
            continue;
        }

        // object 声明（优先级最高，先处理）
        if let Some(rest) = line.strip_prefix("object ") {
            flush_update(&mut cur_update, &mut cur_action);
            flush_create(&mut cur_create, &mut cur_action);
            flush_action(&mut cur_action, &mut current);
            if let Some(rule) = cur_opportunity.as_mut() {
                flush_condition(&mut cur_clause, rule);
            }
            flush_opportunity(&mut cur_opportunity, &mut opportunity_fields, &mut current)?;
            if let Some(obj) = current.take() {
                declared.push(obj);
            }
            if let Some(relation) = current_relation.take() {
                declared_relations.push(relation);
            }
            let definition = rest.trim_end_matches(':').trim();
            let mut parts = definition.split_whitespace();
            let Some(name) = parts.next() else {
                return Err(OdlError::new(
                    E_OBJECT_NAME,
                    "object 声明缺少名称",
                    line_no,
                    1,
                ));
            };
            let modifiers: Vec<&str> = parts.collect();
            let extends = match modifiers.first() {
                None => Vec::new(),
                Some(&"extends") => {
                    let mut parents = Vec::new();
                    for part in modifiers[1..].join(" ").split(',') {
                        let parent = part.trim();
                        if parent.is_empty() {
                            return Err(OdlError::new(
                                E_INVALID_OBJECT_HIERARCHY,
                                format!("object '{name}' 的 extends 列表含有空父类型"),
                                line_no,
                                1,
                            ));
                        }
                        parents.push(parent.to_string());
                    }
                    parents
                }
                Some(modifier) => {
                    return Err(OdlError::new(
                        E_INVALID_OBJECT_HIERARCHY,
                        format!("object '{name}' 含有未知声明修饰符 '{modifier}'"),
                        line_no,
                        1,
                    ));
                }
            };
            current = Some((
                line_no,
                ObjectType {
                    name: name.to_string(),
                    extends,
                    properties: Vec::new(),
                    actions: Vec::new(),
                    restrictions: Vec::new(),
                    data_restrictions: Vec::new(),
                    opportunity_rules: Vec::new(),
                },
            ));
            section = vec![(Section::Object, indent)];
            continue;
        }

        // 缩进回退：离开比当前行更深的块（离开时冲刷对应临时态）
        while section.len() > 1
            && indent
                <= section
                    .last()
                    .expect("section 栈非空：解析入口必压 Object 帧")
                    .1
        {
            match section.last().expect("section 栈非空").0 {
                Section::ActionUpdates => flush_update(&mut cur_update, &mut cur_action),
                Section::ActionCreates => flush_create(&mut cur_create, &mut cur_action),
                Section::ActionPrecondition => flush_clause(&mut cur_clause, &mut cur_action),
                Section::ActionPlanning => {
                    if !planning_fields.iter().all(|seen| *seen) {
                        return Err(OdlError::new(
                            E_INVALID_PROPERTY_CONSTRAINT,
                            "action planning 必须完整声明 cost、prior_confidence 和 outcome_timing",
                            line_no,
                            1,
                        ));
                    }
                }
                Section::OpportunityCondition => {
                    if let Some(rule) = cur_opportunity.as_mut() {
                        flush_condition(&mut cur_clause, rule);
                    }
                }
                Section::OpportunityRule => {
                    flush_opportunity(&mut cur_opportunity, &mut opportunity_fields, &mut current)?
                }
                Section::Action => flush_action(&mut cur_action, &mut current),
                Section::Relation => {
                    if let Some(relation) = current_relation.take() {
                        declared_relations.push(relation);
                    }
                }
                _ => {}
            }
            section.pop();
        }
        let cur = *section
            .last()
            .expect("section 栈非空：解析入口必压 Object 帧");

        // 列表项（actions 的 updates/creates 用 `- ` 开头）
        if let Some(content) = line.strip_prefix("- ") {
            let content = content.trim();
            match cur.0 {
                Section::ActionUpdates => {
                    flush_update(&mut cur_update, &mut cur_action);
                    // 列表项语法：`- field: <字段名>`（field 为关键字，值为目标字段名）
                    if let Some(v) = list_item_value(content, "field") {
                        cur_update = Some(FieldUpdate {
                            field: v.to_string(),
                            value: ValueExpr::Literal(serde_json::Value::Null),
                        });
                    }
                }
                Section::ActionCreates => {
                    flush_create(&mut cur_create, &mut cur_action);
                    if let Some(v) = list_item_value(content, "type") {
                        cur_create = Some(CreateEffect {
                            object_type: v.to_string(),
                            id: None,
                            data: Vec::new(),
                            link_edge: None,
                            subject_field: None,
                            decision_edge: None,
                        });
                    }
                }
                Section::ActionPrecondition => {
                    flush_clause(&mut cur_clause, &mut cur_action);
                    // 列表项语法：`- field: <字段或参数>`（field 为关键字，值为目标字段/参数）
                    if let Some(v) = list_item_value(content, "field") {
                        cur_clause = Some(Clause {
                            field: v.to_string(),
                            op: CompareOp::Eq,
                            value: ValueExpr::Literal(serde_json::Value::Null),
                        });
                    }
                }
                Section::OpportunityCondition => {
                    if let Some(rule) = cur_opportunity.as_mut() {
                        flush_condition(&mut cur_clause, rule);
                    }
                    if let Some(v) = list_item_value(content, "field") {
                        cur_clause = Some(Clause {
                            field: v.to_string(),
                            op: CompareOp::Eq,
                            value: ValueExpr::Literal(serde_json::Value::Null),
                        });
                    }
                }
                _ => {
                    return Err(OdlError::new(
                        E_NO_PROP_BLOCK,
                        format!("列表项 '-' 出现在非预期块内: {line}"),
                        line_no,
                        1,
                    ));
                }
            }
            continue;
        }

        // 块头（bare `word:`）
        if let Some(header) = parse_header(line) {
            match (cur.0, header) {
                (Section::Relation, "source") | (Section::Relation, "target") => {
                    return Err(OdlError::new(
                        E_NO_PROP_BLOCK,
                        format!("无法识别的块 '{header}'"),
                        line_no,
                        1,
                    ));
                }
                (Section::Object, "properties") => section.push((Section::Properties, indent)),
                (Section::Object, "restrictions") => {
                    section.push((Section::ObjectRestrictions, indent))
                }
                (Section::Object, "data_restrictions") => {
                    section.push((Section::DataRestrictions, indent))
                }
                (Section::Object, "actions") => section.push((Section::Actions, indent)),
                (Section::Object, "opportunities") => {
                    section.push((Section::Opportunities, indent))
                }
                (Section::Actions, name) => {
                    flush_action(&mut cur_action, &mut current);
                    cur_action = Some((
                        line_no,
                        ActionDef {
                            name: name.to_string(),
                            params: Vec::new(),
                            updates: Vec::new(),
                            creates: Vec::new(),
                            policy: PolicyDef::default(),
                            planning: None,
                        },
                    ));
                    section.push((Section::Action, indent));
                }
                (Section::Action, "params") => section.push((Section::ActionParams, indent)),
                (Section::Action, "updates") => section.push((Section::ActionUpdates, indent)),
                (Section::Action, "creates") => section.push((Section::ActionCreates, indent)),
                (Section::Action, "policy") => section.push((Section::ActionPolicy, indent)),
                (Section::Action, "planning") => {
                    planning_fields = [false; 3];
                    section.push((Section::ActionPlanning, indent));
                }
                (Section::Opportunities, name) => {
                    flush_opportunity(&mut cur_opportunity, &mut opportunity_fields, &mut current)?;
                    cur_opportunity = Some((
                        line_no,
                        OpportunityRule {
                            name: name.to_string(),
                            subject_type: String::new(),
                            estimated_value: 0.0,
                            evidence: String::new(),
                            when: Vec::new(),
                            goal_metric: None,
                            urgency_field: String::new(),
                            urgency_horizon: 0.0,
                            cost_budget: None,
                            metric_budget: None,
                            link_edge: String::new(),
                        },
                    ));
                    opportunity_fields = [false; 6];
                    section.push((Section::OpportunityRule, indent));
                }
                (Section::OpportunityRule, "when") => {
                    // 条件是重复列表项；子块边界必须高于列表缩进，避免下一条同级 `- field` 被误判为离开。
                    section.push((Section::OpportunityCondition, indent + 1))
                }
                (Section::ActionPlanning, "goal_metric")
                | (Section::OpportunityRule, "goal_metric") => {
                    return Err(OdlError::new(
                        E_INVALID_PROPERTY_CONSTRAINT,
                        "goal_metric 不能为空".to_string(),
                        line_no,
                        1,
                    ));
                }
                (Section::ActionCreates, "data") => section.push((Section::CreateData, indent)),
                (Section::ActionPolicy, "precondition") => {
                    section.push((Section::ActionPrecondition, indent))
                }
                _ => {
                    return Err(OdlError::new(
                        E_NO_PROP_BLOCK,
                        format!("无法识别的块 '{header}'"),
                        line_no,
                        1,
                    ));
                }
            }
            continue;
        }

        // 条目（key: value）
        if let Some((k, v)) = line.split_once(':') {
            let k = k.trim();
            let v = v.trim();
            let param_names: Vec<String> = cur_action
                .as_ref()
                .map(|(_, a)| a.params.iter().map(|p| p.name.clone()).collect())
                .unwrap_or_default();
            match cur.0 {
                Section::Relation => {
                    let boolean_keys = [
                        "functional",
                        "inverse_functional",
                        "symmetric",
                        "transitive",
                        "reflexive",
                        "irreflexive",
                        "asymmetric",
                    ];
                    if k != "source"
                        && k != "target"
                        && k != "inverse_of"
                        && k != "composed_of"
                        && k != "min_cardinality"
                        && k != "max_cardinality"
                        && !boolean_keys.contains(&k)
                    {
                        return Err(OdlError::new(
                            E_NO_PROP_BLOCK,
                            format!("'{k}' 不是 relation 属性"),
                            line_no,
                            1,
                        ));
                    }
                    if v.is_empty() {
                        return Err(OdlError::new(
                            E_EMPTY_PROP_NAME,
                            format!("relation {k} 不能为空"),
                            line_no,
                            1,
                        ));
                    }
                    let Some((_, relation)) = current_relation.as_mut() else {
                        continue;
                    };
                    match k {
                        "source" => relation.source = v.to_string(),
                        "target" => relation.target = v.to_string(),
                        "inverse_of" => relation.inverse_of = Some(v.to_string()),
                        "composed_of" => {
                            relation.composed_of =
                                v.split_whitespace().map(str::to_string).collect::<Vec<_>>();
                            if relation.composed_of.len() < 2 {
                                return Err(OdlError::new(
                                    E_INVALID_PROPERTY_CONSTRAINT,
                                    "relation composed_of must contain at least two properties",
                                    line_no,
                                    1,
                                ));
                            }
                        }
                        "min_cardinality" | "max_cardinality" => {
                            let value = v.parse::<u32>().map_err(|_| {
                                OdlError::new(
                                    E_INVALID_PROPERTY_CONSTRAINT,
                                    format!("relation {k} must be an unsigned integer"),
                                    line_no,
                                    1,
                                )
                            })?;
                            if k == "min_cardinality" {
                                relation.min_cardinality = Some(value);
                            } else {
                                relation.max_cardinality = Some(value);
                            }
                            relation.validate_cardinality().map_err(|message| {
                                OdlError::new(E_INVALID_PROPERTY_CONSTRAINT, message, line_no, 1)
                            })?;
                        }
                        _ => {
                            if v != "true" && v != "false" {
                                return Err(OdlError::new(
                                    E_RELATION_CONTRADICTION,
                                    format!("relation 特征 {k} 只能是 true/false"),
                                    line_no,
                                    1,
                                ));
                            }
                            let value = v == "true";
                            match k {
                                "functional" => relation.functional = value,
                                "inverse_functional" => relation.inverse_functional = value,
                                "symmetric" => relation.symmetric = value,
                                "transitive" => relation.transitive = value,
                                "reflexive" => relation.reflexive = value,
                                "irreflexive" => relation.irreflexive = value,
                                "asymmetric" => relation.asymmetric = value,
                                _ => unreachable!("boolean_keys 已约束分支"),
                            }
                        }
                    }
                }
                Section::Properties => {
                    if k.is_empty() {
                        return Err(OdlError::new(E_EMPTY_PROP_NAME, "属性名为空", line_no, 1));
                    }
                    let col = (k.len() + 2) as u32;
                    let (ty, constraints) = parse_property_definition(v, &declared_datatypes)
                        .map_err(|error| {
                            let (code, message) = match error {
                                PropertyDefinitionError::UnknownType(message) => {
                                    (E_UNKNOWN_TYPE, message)
                                }
                                PropertyDefinitionError::Invalid(message) => (
                                    E_INVALID_PROPERTY_CONSTRAINT,
                                    format!("property constraint: {message}"),
                                ),
                            };
                            OdlError::new(code, message, line_no, col)
                        })?;
                    if let Some((_, obj)) = current.as_mut() {
                        obj.properties.push(PropertyDef {
                            name: k.to_string(),
                            ty,
                            constraints,
                        });
                    }
                }
                Section::ActionParams => {
                    if k.is_empty() {
                        return Err(OdlError::new(E_EMPTY_PROP_NAME, "参数名为空", line_no, 1));
                    }
                    let ty = ScalarType::from_str(v).map_err(|e| {
                        OdlError::new(E_UNKNOWN_TYPE, e, line_no, (k.len() + 2) as u32)
                    })?;
                    if let Some((_, a)) = cur_action.as_mut() {
                        a.params.push(ParamDef {
                            name: k.to_string(),
                            ty,
                        });
                    }
                }
                Section::ActionUpdates => {
                    if k == "value"
                        && let Some(u) = cur_update.as_mut()
                    {
                        u.value = parse_value_expr(v, &param_names);
                    }
                }
                Section::CreateData => {
                    if let Some(c) = cur_create.as_mut() {
                        c.data
                            .push((k.to_string(), parse_value_expr(v, &param_names)));
                    }
                }
                Section::ActionCreates => {
                    if k == "link"
                        && let Some(c) = cur_create.as_mut()
                    {
                        c.link_edge = Some(v.to_string());
                        link_lines.insert(v.to_string(), line_no);
                    } else if k == "id"
                        && let Some(c) = cur_create.as_mut()
                    {
                        c.id = Some(v.to_string());
                    } else if k == "subject_field"
                        && let Some(c) = cur_create.as_mut()
                    {
                        if v.trim().is_empty() {
                            return Err(OdlError::new(
                                E_INVALID_PROPERTY_CONSTRAINT,
                                "subject_field 不能为空".to_string(),
                                line_no,
                                1,
                            ));
                        }
                        c.subject_field = Some(v.to_string());
                    } else if k == "decision_edge"
                        && let Some(c) = cur_create.as_mut()
                    {
                        if v.trim().is_empty() {
                            return Err(OdlError::new(
                                E_INVALID_PROPERTY_CONSTRAINT,
                                "decision_edge 不能为空".to_string(),
                                line_no,
                                1,
                            ));
                        }
                        c.decision_edge = Some(v.to_string());
                        link_lines.insert(v.to_string(), line_no);
                    }
                }
                Section::ActionPolicy => {
                    if k == "require_approval"
                        && let Some((_, a)) = cur_action.as_mut()
                    {
                        a.policy.require_approval = v == "true";
                    }
                }
                Section::ActionPlanning => {
                    let invalid = |message: String| {
                        OdlError::new(E_INVALID_PROPERTY_CONSTRAINT, message, line_no, 1)
                    };
                    let Some((_, action)) = cur_action.as_mut() else {
                        continue;
                    };
                    let planning = action.planning.get_or_insert(ActionPlanning {
                        cost: 0.0,
                        prior_confidence: 0.0,
                        outcome_timing: OutcomeTiming::Immediate,
                        goal_metric: None,
                        benefit_field: None,
                        benefit_factor: None,
                    });
                    match k {
                        "cost" | "prior_confidence" => {
                            let value = v
                                .parse::<f64>()
                                .map_err(|_| invalid(format!("{k} 必须是有限数值，得到 `{v}`")))?;
                            if !value.is_finite() || value < 0.0 {
                                return Err(invalid(format!("{k} 不能为负或无限")));
                            }
                            if k == "cost" {
                                planning.cost = value;
                                planning_fields[0] = true;
                            } else {
                                if value > 1.0 {
                                    return Err(invalid("prior_confidence 必须在 [0,1] 内".into()));
                                }
                                planning.prior_confidence = value;
                                planning_fields[1] = true;
                            }
                        }
                        "outcome_timing" => {
                            planning.outcome_timing = match v {
                                "immediate" => OutcomeTiming::Immediate,
                                "deferred" => OutcomeTiming::Deferred,
                                _ => {
                                    return Err(invalid(
                                        "outcome_timing 只能是 immediate 或 deferred".into(),
                                    ));
                                }
                            };
                            planning_fields[2] = true;
                        }
                        "goal_metric" => {
                            if v.trim().is_empty() {
                                return Err(invalid("goal_metric 不能为空".to_string()));
                            }
                            planning.goal_metric = Some(v.to_string());
                        }
                        "benefit_field" => {
                            if v.trim().is_empty() {
                                return Err(invalid("benefit_field 不能为空".into()));
                            }
                            planning.benefit_field = Some(v.to_string());
                        }
                        "benefit_factor" => {
                            let value = v.parse::<f64>().map_err(|_| {
                                invalid(format!("benefit_factor 必须是有限数值，得到 `{v}`"))
                            })?;
                            if !value.is_finite() || value < 0.0 || value > 1.0 {
                                return Err(invalid("benefit_factor 必须在 [0,1] 内".into()));
                            }
                            planning.benefit_factor = Some(value);
                        }
                        _ => return Err(invalid(format!("'{k}' 不是 action planning 属性"))),
                    }
                }
                Section::ActionPrecondition => {
                    if k == "op"
                        && let Some(c) = cur_clause.as_mut()
                    {
                        c.op = parse_compare_op(v);
                    } else if k == "value"
                        && let Some(c) = cur_clause.as_mut()
                    {
                        c.value = parse_value_expr(v, &param_names);
                    }
                }
                Section::OpportunityRule => {
                    let invalid = |message: String| {
                        OdlError::new(E_INVALID_PROPERTY_CONSTRAINT, message, line_no, 1)
                    };
                    let Some((_, rule)) = cur_opportunity.as_mut() else {
                        continue;
                    };
                    match k {
                        "subject_type" => {
                            if v.trim().is_empty() {
                                return Err(invalid("subject_type 不能为空".into()));
                            }
                            rule.subject_type = v.to_string();
                            opportunity_fields[0] = true;
                        }
                        "estimated_value" => {
                            let value = v.parse::<f64>().map_err(|_| {
                                invalid(format!("estimated_value 必须是有限数值，得到 `{v}`"))
                            })?;
                            if !value.is_finite() || value < 0.0 {
                                return Err(invalid("estimated_value 不能为负或无限".into()));
                            }
                            rule.estimated_value = value;
                            opportunity_fields[1] = true;
                        }
                        "evidence" => {
                            let parsed: serde_json::Value =
                                serde_json::from_str(v).map_err(|error| {
                                    invalid(format!("evidence 必须是非空 JSON 字符串：{error}"))
                                })?;
                            let text = parsed
                                .as_str()
                                .ok_or_else(|| invalid("evidence 必须是非空 JSON 字符串".into()))?;
                            if text.is_empty() {
                                return Err(invalid("evidence 不能为空".into()));
                            }
                            rule.evidence = text.to_owned();
                            opportunity_fields[2] = true;
                        }
                        "goal_metric" => {
                            if v.trim().is_empty() {
                                return Err(invalid("goal_metric 不能为空".to_string()));
                            }
                            rule.goal_metric = Some(v.to_string());
                        }
                        "urgency_field" => {
                            if v.trim().is_empty() {
                                return Err(invalid("urgency_field 不能为空".into()));
                            }
                            rule.urgency_field = v.to_string();
                            opportunity_fields[3] = true;
                        }
                        "urgency_horizon" => {
                            let value = v.parse::<f64>().map_err(|_| {
                                invalid(format!("urgency_horizon 必须是正数值，得到 `{v}`"))
                            })?;
                            if !value.is_finite() || value <= 0.0 {
                                return Err(invalid("urgency_horizon 必须大于零".into()));
                            }
                            rule.urgency_horizon = value;
                            opportunity_fields[4] = true;
                        }
                        "cost_budget" => {
                            let value = v.parse::<f64>().map_err(|_| {
                                invalid(format!("cost_budget 必须是有限数值，得到 `{v}`"))
                            })?;
                            if !value.is_finite() || value < 0.0 {
                                return Err(invalid("cost_budget 不能为负或无限".into()));
                            }
                            rule.cost_budget = Some(value);
                        }
                        "metric_budget" => {
                            let value = v.parse::<f64>().map_err(|_| {
                                invalid(format!("metric_budget 必须是有限数值，得到 `{v}`"))
                            })?;
                            if !value.is_finite() || value < 0.0 {
                                return Err(invalid("metric_budget 不能为负或无限".into()));
                            }
                            rule.metric_budget = Some(value);
                        }
                        "link_edge" => {
                            if v.trim().is_empty() {
                                return Err(invalid("link_edge 不能为空".into()));
                            }
                            rule.link_edge = v.to_string();
                            link_lines.insert(v.to_string(), line_no);
                            opportunity_fields[5] = true;
                        }
                        _ => return Err(invalid(format!("'{k}' 不是 opportunity 规则属性"))),
                    }
                }
                Section::OpportunityCondition => match k {
                    "op" => {
                        if let Some(c) = cur_clause.as_mut() {
                            c.op = parse_compare_op(v);
                        }
                    }
                    "value" => {
                        if let Some(c) = cur_clause.as_mut() {
                            c.value = parse_value_expr(v, &[]);
                        }
                    }
                    _ => {
                        return Err(OdlError::new(
                            E_INVALID_PROPERTY_CONSTRAINT,
                            format!("'{k}' 不是 opportunity condition 属性"),
                            line_no,
                            1,
                        ));
                    }
                },
                Section::ObjectRestrictions => {
                    let invalid = |message: String| {
                        OdlError::new(E_INVALID_OBJECT_RESTRICTION, message, line_no, 1)
                    };
                    if k.is_empty() || k.contains(char::is_whitespace) {
                        return Err(invalid(
                            "restriction contains an empty or invalid relation".to_owned(),
                        ));
                    }
                    let Some((kind, filler)) = v.split_once(char::is_whitespace) else {
                        return Err(invalid(
                            "restriction needs `relation: some Class`, `relation: only Class`, `relation: value individual`, `relation: min N Class`, `relation: max N Class`, or `relation: exact N Class`"
                                .to_owned(),
                        ));
                    };
                    let filler = filler.trim();
                    let is_cardinality = matches!(kind, "min" | "max" | "exact");
                    if filler.is_empty()
                        || (!is_cardinality && filler.contains(char::is_whitespace))
                    {
                        return Err(invalid(
                            "restriction filler is empty or contains whitespace".to_owned(),
                        ));
                    }
                    let kind = match kind {
                        "some" => ObjectRestrictionKind::Some {
                            class: filler.to_string(),
                        },
                        "only" => ObjectRestrictionKind::Only {
                            class: filler.to_string(),
                        },
                        "value" => ObjectRestrictionKind::Value {
                            individual: filler.to_string(),
                        },
                        "min" | "max" | "exact" => {
                            let (num, class) =
                                filler.split_once(char::is_whitespace).ok_or_else(|| {
                                    invalid(format!(
                                        "{kind} cardinality restriction needs `{kind} N Class`"
                                    ))
                                })?;
                            let num = num.trim();
                            let class = class.trim();
                            if class.is_empty() || class.contains(char::is_whitespace) {
                                return Err(invalid(format!(
                                    "{kind} cardinality restriction class is empty or contains whitespace"
                                )));
                            }
                            let cardinality: u32 = num
                                .parse()
                                .map_err(|_| invalid(format!("invalid cardinality '{num}'")))?;
                            if kind == "min" {
                                ObjectRestrictionKind::Min {
                                    cardinality,
                                    class: class.to_string(),
                                }
                            } else if kind == "max" {
                                ObjectRestrictionKind::Max {
                                    cardinality,
                                    class: class.to_string(),
                                }
                            } else {
                                ObjectRestrictionKind::Exact {
                                    cardinality,
                                    class: class.to_string(),
                                }
                            }
                        }
                        _ => {
                            return Err(invalid(format!(
                                "unknown restriction constructor '{kind}'; expected some, only, value, min, max, or exact"
                            )));
                        }
                    };
                    let restriction = ObjectPropertyRestriction {
                        relation: k.to_string(),
                        kind,
                    };
                    if let Some((_, obj)) = current.as_mut() {
                        obj.restrictions.push(restriction.clone());
                    }
                    declared_restrictions.push((line_no, k.to_string(), restriction));
                }
                Section::DataRestrictions => {
                    let invalid = |message: String| {
                        OdlError::new(E_INVALID_DATA_RESTRICTION, message, line_no, 1)
                    };
                    if k.is_empty() || k.contains(char::is_whitespace) {
                        return Err(invalid(
                            "data restriction contains an empty or invalid property".to_owned(),
                        ));
                    }
                    let Some((kind, filler)) = v.split_once(char::is_whitespace) else {
                        return Err(invalid(
                            "data restriction needs `property: some type`, `property: only type`, `property: min N type`, `property: max N type`, `property: exact N type`, or `property: value literal`"
                                .to_owned(),
                        ));
                    };
                    let filler = filler.trim();
                    let kind = match kind {
                        "some" => {
                            if filler.is_empty() || filler.contains(char::is_whitespace) {
                                return Err(invalid(
                                    "data restriction filler is empty or contains whitespace"
                                        .to_owned(),
                                ));
                            }
                            let datatype = ScalarType::from_str(filler).map_err(invalid)?;
                            DataRestrictionKind::Some { datatype }
                        }
                        "only" => {
                            if filler.is_empty() || filler.contains(char::is_whitespace) {
                                return Err(invalid(
                                    "data restriction filler is empty or contains whitespace"
                                        .to_owned(),
                                ));
                            }
                            let datatype = ScalarType::from_str(filler).map_err(invalid)?;
                            DataRestrictionKind::Only { datatype }
                        }
                        "min" | "max" | "exact" => {
                            let (num, datatype) =
                                filler.split_once(char::is_whitespace).ok_or_else(|| {
                                    invalid(format!(
                                        "{kind} data cardinality restriction needs `{kind} N Type`"
                                    ))
                                })?;
                            let num = num.trim();
                            let datatype = datatype.trim();
                            if datatype.is_empty() || datatype.contains(char::is_whitespace) {
                                return Err(invalid(format!(
                                    "{kind} data cardinality restriction type is empty or contains whitespace"
                                )));
                            }
                            let cardinality: u32 = num.parse().map_err(|_| {
                                invalid(format!("invalid data cardinality '{num}'"))
                            })?;
                            let datatype = ScalarType::from_str(datatype).map_err(invalid)?;
                            if kind == "min" {
                                DataRestrictionKind::Min {
                                    cardinality,
                                    datatype,
                                }
                            } else if kind == "max" {
                                DataRestrictionKind::Max {
                                    cardinality,
                                    datatype,
                                }
                            } else {
                                DataRestrictionKind::Exact {
                                    cardinality,
                                    datatype,
                                }
                            }
                        }
                        "value" => {
                            if filler.is_empty() {
                                return Err(invalid(
                                    "data restriction filler is empty or contains whitespace"
                                        .to_owned(),
                                ));
                            }
                            let literal = parse_data_literal(filler).map_err(&invalid)?;
                            DataRestrictionKind::Value { literal }
                        }
                        _ => {
                            return Err(invalid(format!(
                                "unknown data restriction constructor '{kind}'; expected some, only, min, max, exact, or value"
                            )));
                        }
                    };
                    let restriction = DataPropertyRestriction {
                        property: k.to_string(),
                        kind,
                    };
                    if let Some((_, obj)) = current.as_mut() {
                        obj.data_restrictions.push(restriction.clone());
                    }
                    declared_data_restrictions.push((line_no, k.to_string(), restriction));
                }
                _ => {
                    return Err(OdlError::new(
                        E_NO_PROP_BLOCK,
                        format!("'{k}' 不在预期块内"),
                        line_no,
                        1,
                    ));
                }
            }
            continue;
        }

        return Err(OdlError::new(
            E_EMPTY_PROP_NAME,
            format!("无法解析行: {line}"),
            line_no,
            1,
        ));
    }

    if section
        .last()
        .expect("section 栈非空：解析入口必压 Object 帧")
        .0
        == Section::ActionPlanning
        && !planning_fields.iter().all(|seen| *seen)
    {
        return Err(OdlError::new(
            E_INVALID_PROPERTY_CONSTRAINT,
            "action planning 必须完整声明 cost、prior_confidence 和 outcome_timing",
            src.lines().count() as u32,
            1,
        ));
    }

    // 冲刷尾部临时态
    if let Some(rule) = cur_opportunity.as_mut() {
        flush_condition(&mut cur_clause, rule);
    }
    flush_opportunity(&mut cur_opportunity, &mut opportunity_fields, &mut current)?;
    flush_update(&mut cur_update, &mut cur_action);
    flush_create(&mut cur_create, &mut cur_action);
    flush_clause(&mut cur_clause, &mut cur_action);
    flush_action(&mut cur_action, &mut current);
    if let Some(obj) = current.take() {
        declared.push(obj);
    }
    if let Some(relation) = current_relation.take() {
        declared_relations.push(relation);
    }

    // 每个 object 必须至少含一个 property（TC-001-b：缺 property → 错误码，带回声明行列号）
    let mut object_lines = std::collections::BTreeMap::new();
    let mut opportunity_lines = std::collections::BTreeMap::new();
    for (ln, obj) in &declared {
        if obj.properties.is_empty() {
            return Err(OdlError::new(
                E_MISSING_PROPERTY,
                format!("object '{}' 缺少 property", obj.name),
                *ln,
                1,
            ));
        }
        // 重复对象名检测：同名会生成重复 Rust struct 导致编译失败，编译期即拦截。
        if ir.object_types.iter().any(|o| o.name == obj.name) {
            return Err(OdlError::new(
                E_DUP_OBJECT,
                format!("重复对象定义 '{0}'（同名对象只能声明一次）", obj.name),
                *ln,
                1,
            ));
        }
        object_lines.insert(obj.name.as_str(), *ln);
        for rule in &obj.opportunity_rules {
            opportunity_lines.insert(rule.name.as_str(), *ln);
        }
        ir.object_types.push(obj.clone());
    }

    let mut seen_opportunities = std::collections::BTreeSet::new();
    ir.relations = declared_relations.iter().map(|(_, r)| r.clone()).collect();
    for object in &ir.object_types {
        let relation_matches = |name: &str, source: &str, target: &str| {
            ir.relations.iter().any(|relation| {
                relation.name == name && relation.source == source && relation.target == target
            })
        };
        for action in &object.actions {
            if let Some(planning) = &action.planning
                && (planning.benefit_field.is_some() != planning.benefit_factor.is_some()
                    || !planning.benefit_field.as_ref().is_none_or(|field| {
                        matches!(
                            object
                                .properties
                                .iter()
                                .find(|p| &p.name == field)
                                .map(|p| &p.ty),
                            Some(ScalarType::Int | ScalarType::Float)
                        )
                    }))
            {
                return Err(OdlError::new(
                    E_INVALID_PROPERTY_CONSTRAINT,
                    format!(
                        "action '{}' 的 benefit_field/benefit_factor 必须成对声明且字段是 int/float 属性",
                        action.name
                    ),
                    1,
                    1,
                ));
            }
            for created in &action.creates {
                let deferred = action
                    .planning
                    .as_ref()
                    .is_some_and(|planning| planning.outcome_timing == OutcomeTiming::Deferred);
                let meta_line = created
                    .decision_edge
                    .as_ref()
                    .and_then(|edge| link_lines.get(edge))
                    .copied()
                    .unwrap_or(1);
                if !deferred {
                    if created.subject_field.is_some() || created.decision_edge.is_some() {
                        return Err(OdlError::new(
                            E_INVALID_PROPERTY_CONSTRAINT,
                            format!(
                                "immediate action '{}' 的创建效应不能声明 deferred 证据元数据",
                                action.name
                            ),
                            meta_line,
                            1,
                        ));
                    }
                    continue;
                }
                let target = ir
                    .object_types
                    .iter()
                    .find(|item| item.name == created.object_type);
                let valid_subject = created.subject_field.as_ref().is_some_and(|field| {
                    target.is_some_and(|item| {
                        item.properties
                            .iter()
                            .any(|property| &property.name == field)
                    }) && created
                        .data
                        .iter()
                        .any(|(key, value)| key == field && matches!(value, ValueExpr::SelfId))
                });
                let valid_decision = created
                    .decision_edge
                    .as_ref()
                    .is_some_and(|edge| relation_matches(edge, "Decision", &created.object_type));
                if !valid_subject || !valid_decision {
                    return Err(OdlError::new(
                        E_INVALID_PROPERTY_CONSTRAINT,
                        format!(
                            "deferred action '{}' 的创建效应必须声明 self 主体字段和 Decision 证据边",
                            action.name
                        ),
                        meta_line,
                        1,
                    ));
                }
            }
        }
        for rule in &object.opportunity_rules {
            if !seen_opportunities.insert(rule.name.as_str()) {
                return Err(OdlError::new(
                    E_INVALID_PROPERTY_CONSTRAINT,
                    format!("重复 opportunity 规则 '{}'", rule.name),
                    opportunity_lines
                        .get(rule.name.as_str())
                        .copied()
                        .unwrap_or(1),
                    1,
                ));
            }
            if rule.subject_type != object.name {
                return Err(OdlError::new(
                    E_INVALID_PROPERTY_CONSTRAINT,
                    format!(
                        "opportunity '{}' 的 subject_type 必须是所在对象 '{}'",
                        rule.name, object.name
                    ),
                    opportunity_lines
                        .get(rule.name.as_str())
                        .copied()
                        .unwrap_or(1),
                    1,
                ));
            }
            if rule.metric_budget.is_some() && rule.goal_metric.is_none() {
                return Err(OdlError::new(
                    E_INVALID_PROPERTY_CONSTRAINT,
                    format!(
                        "opportunity '{}' 的 metric_budget 必须配合 goal_metric",
                        rule.name
                    ),
                    opportunity_lines
                        .get(rule.name.as_str())
                        .copied()
                        .unwrap_or(1),
                    1,
                ));
            }
            let numeric = |field: &str| {
                matches!(
                    object
                        .properties
                        .iter()
                        .find(|p| p.name == field)
                        .map(|p| &p.ty),
                    Some(ScalarType::Int | ScalarType::Float)
                )
            };
            if !numeric(&rule.urgency_field) {
                return Err(OdlError::new(
                    E_INVALID_PROPERTY_CONSTRAINT,
                    format!(
                        "opportunity '{}' 的 urgency_field 必须是 int/float 属性",
                        rule.name
                    ),
                    opportunity_lines
                        .get(rule.name.as_str())
                        .copied()
                        .unwrap_or(1),
                    1,
                ));
            }
            if !ir.relations.iter().any(|relation| {
                relation.name == rule.link_edge
                    && relation.source == "Opportunity"
                    && relation.target == object.name
            }) {
                return Err(OdlError::new(
                    E_INVALID_PROPERTY_CONSTRAINT,
                    format!(
                        "opportunity '{}' link_edge 必须是 Opportunity -> {} 关系",
                        rule.name, object.name
                    ),
                    opportunity_lines
                        .get(rule.name.as_str())
                        .copied()
                        .unwrap_or(1),
                    1,
                ));
            }
            for clause in &rule.when {
                let Some(field) = clause.field.strip_prefix("self.") else {
                    return Err(OdlError::new(
                        E_INVALID_PROPERTY_CONSTRAINT,
                        format!(
                            "opportunity '{}' condition field must reference self.<property>",
                            rule.name
                        ),
                        opportunity_lines
                            .get(rule.name.as_str())
                            .copied()
                            .unwrap_or(1),
                        1,
                    ));
                };
                if field.is_empty()
                    || field.contains('.')
                    || !object
                        .properties
                        .iter()
                        .any(|property| property.name == field)
                {
                    return Err(OdlError::new(
                        E_INVALID_PROPERTY_CONSTRAINT,
                        format!(
                            "opportunity '{}' references unknown condition field '{field}'",
                            rule.name
                        ),
                        opportunity_lines
                            .get(rule.name.as_str())
                            .copied()
                            .unwrap_or(1),
                        1,
                    ));
                }
                if !matches!(clause.value, ValueExpr::Literal(_)) {
                    return Err(OdlError::new(
                        E_INVALID_PROPERTY_CONSTRAINT,
                        format!(
                            "opportunity '{}' condition value must be a literal",
                            rule.name
                        ),
                        opportunity_lines
                            .get(rule.name.as_str())
                            .copied()
                            .unwrap_or(1),
                        1,
                    ));
                }
            }
        }
    }

    if let Err(error) = ir.validate_extends() {
        return Err(OdlError::new(
            E_INVALID_OBJECT_HIERARCHY,
            format!("object '{}': {}", error.object, error.message),
            object_lines
                .get(error.object.as_str())
                .copied()
                .unwrap_or(1),
            1,
        ));
    }

    ir.disjoint_groups = declared_disjoint.iter().map(|(_, g)| g.clone()).collect();
    ir.equivalent_groups = declared_equivalent.iter().map(|(_, g)| g.clone()).collect();
    ir.disjoint_unions = declared_disjoint_unions
        .iter()
        .map(|(_, group)| group.clone())
        .collect();
    if let Err(error) = ir.validate_class_groups() {
        let lines = if error.kind == "disjoint" {
            &declared_disjoint
        } else {
            &declared_equivalent
        };
        let line = lines.get(error.index).map(|(line, _)| *line).unwrap_or(1);
        return Err(OdlError::new(E_INVALID_CLASS_GROUP, error.message, line, 1));
    }
    if let Err(error) = ir.validate_disjoint_unions() {
        let line = declared_disjoint_unions
            .get(error.index)
            .map(|(line, _)| *line)
            .unwrap_or(1);
        return Err(OdlError::new(
            E_INVALID_DISJOINT_UNION,
            error.message,
            line,
            1,
        ));
    }
    ir.object_one_ofs = declared_object_one_ofs
        .iter()
        .map(|(_, group)| group.clone())
        .collect();
    if let Err(error) = ir.validate_object_one_ofs() {
        let line = declared_object_one_ofs
            .get(error.index)
            .map(|(line, _)| *line)
            .unwrap_or(1);
        return Err(OdlError::new(
            E_INVALID_OBJECT_ONE_OF,
            error.message,
            line,
            1,
        ));
    }
    ir.same_individual_groups = declared_same_individuals
        .iter()
        .map(|(_, group)| group.clone())
        .collect();
    ir.different_individual_groups = declared_different_individuals
        .iter()
        .map(|(_, group)| group.clone())
        .collect();
    if let Err(error) = ir.validate_individual_groups() {
        let lines = if error.kind == "same" {
            &declared_same_individuals
        } else {
            &declared_different_individuals
        };
        let line = lines.get(error.index).map(|(line, _)| *line).unwrap_or(1);
        return Err(OdlError::new(
            E_INVALID_INDIVIDUAL_GROUP,
            error.message,
            line,
            1,
        ));
    }

    let mut endpoints = std::collections::BTreeMap::new();
    let mut seen_relations = std::collections::BTreeSet::new();
    for (ln, relation) in &declared_relations {
        if !seen_relations.insert(relation.name.as_str()) {
            return Err(OdlError::new(
                E_DUP_RELATION,
                format!("重复关系定义 '{}'", relation.name),
                *ln,
                1,
            ));
        }
        if relation.source.is_empty() || relation.target.is_empty() {
            return Err(OdlError::new(
                E_RELATION_ENDPOINT,
                format!("relation '{}' 缺少 source/target", relation.name),
                *ln,
                1,
            ));
        }
        for endpoint in [&relation.source, &relation.target] {
            if !ir
                .object_types
                .iter()
                .any(|object| &object.name == endpoint)
            {
                return Err(OdlError::new(
                    E_RELATION_ENDPOINT,
                    format!(
                        "relation '{}' 引用未声明对象类型 '{endpoint}'",
                        relation.name
                    ),
                    *ln,
                    1,
                ));
            }
        }
        if relation.symmetric && relation.asymmetric || relation.reflexive && relation.irreflexive {
            return Err(OdlError::new(
                E_RELATION_CONTRADICTION,
                format!("relation '{}' 含有互相矛盾的特征", relation.name),
                *ln,
                1,
            ));
        }
        if let Some(inverse) = relation.inverse_of.as_ref() {
            if inverse == &relation.name {
                return Err(OdlError::new(
                    E_RELATION_CONTRADICTION,
                    format!("relation '{}' 不能声明自身为逆关系", relation.name),
                    *ln,
                    1,
                ));
            }
            if !declared_relations
                .iter()
                .any(|(_, other)| other.name == *inverse)
            {
                return Err(OdlError::new(
                    E_RELATION_CONTRADICTION,
                    format!(
                        "relation '{}' 引用未声明的逆关系 '{inverse}'",
                        relation.name
                    ),
                    *ln,
                    1,
                ));
            }
            if let Err(error) = ir.validate_inverse_relations()
                && error.relation == relation.name
            {
                return Err(OdlError::new(
                    E_RELATION_CONTRADICTION,
                    format!("relation '{}': {}", relation.name, error.message),
                    *ln,
                    1,
                ));
            }
        }
        let value = (relation.source.clone(), relation.target.clone());
        if let Some(previous) = endpoints.insert(relation.name.clone(), value.clone())
            && previous != value
        {
            return Err(OdlError::new(
                E_DUP_RELATION,
                format!("relation '{}' 与已有端点定义冲突", relation.name),
                *ln,
                1,
            ));
        }
    }
    ir.relation_hierarchy = declared_sub_relations
        .iter()
        .map(|(_, (sub, sup))| RelationHierarchy {
            sub_relation: sub.clone(),
            super_relation: sup.clone(),
        })
        .collect();
    ir.disjoint_relation_groups = declared_disjoint_relations
        .iter()
        .map(|(_, group)| group.clone())
        .collect();
    ir.equivalent_relation_groups = declared_equivalent_relations
        .iter()
        .map(|(_, group)| group.clone())
        .collect();
    ir.data_property_hierarchy = declared_sub_data_properties
        .iter()
        .map(|(_, (sub, sup))| DataPropertyHierarchy {
            sub_property: sub.clone(),
            super_property: sup.clone(),
        })
        .collect();
    ir.disjoint_data_property_groups = declared_disjoint_data_properties
        .iter()
        .map(|(_, group)| group.clone())
        .collect();
    ir.equivalent_data_property_groups = declared_equivalent_data_properties
        .iter()
        .map(|(_, group)| group.clone())
        .collect();
    let relation_names = declared_relations
        .iter()
        .map(|(_, relation)| relation.name.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    if let Err(error) = ir.validate_relation_property_groups(&relation_names) {
        let line = if error.message.starts_with("disjoint_relation") {
            declared_disjoint_relations
                .get(error.index)
                .map(|(line, _)| *line)
        } else {
            declared_sub_relations
                .get(error.index)
                .map(|(line, _)| *line)
        };
        return Err(OdlError::new(
            E_INVALID_PROPERTY_CONSTRAINT,
            error.message,
            line.unwrap_or(1),
            1,
        ));
    }
    if let Err(error) = ir.validate_equivalent_relation_groups(&relation_names) {
        return Err(OdlError::new(
            E_INVALID_PROPERTY_CONSTRAINT,
            error.message,
            declared_equivalent_relations
                .get(error.index)
                .map(|(line, _)| *line)
                .unwrap_or(1),
            1,
        ));
    }
    let property_names = ir
        .object_types
        .iter()
        .flat_map(|object| {
            object
                .properties
                .iter()
                .map(|property| property.name.as_str())
        })
        .collect::<std::collections::BTreeSet<_>>();
    if let Err(error) = ir.validate_data_property_groups(&property_names) {
        let line = if error.message.starts_with("disjoint_data_property") {
            declared_disjoint_data_properties
                .get(error.index)
                .map(|(line, _)| *line)
        } else {
            declared_sub_data_properties
                .get(error.index)
                .map(|(line, _)| *line)
        };
        return Err(OdlError::new(
            E_INVALID_PROPERTY_CONSTRAINT,
            error.message,
            line.unwrap_or(1),
            1,
        ));
    }
    if let Err(error) = ir.validate_equivalent_data_property_groups(&property_names) {
        return Err(OdlError::new(
            E_INVALID_PROPERTY_CONSTRAINT,
            error.message,
            declared_equivalent_data_properties
                .get(error.index)
                .map(|(line, _)| *line)
                .unwrap_or(1),
            1,
        ));
    }
    if let Err(error) = ir.validate_object_restrictions() {
        let line = declared_restrictions
            .get(error.index)
            .map(|(line, _, _)| *line)
            .unwrap_or(1);
        return Err(OdlError::new(
            E_INVALID_OBJECT_RESTRICTION,
            format!("object '{}': {}", error.object, error.message),
            line,
            1,
        ));
    }
    if let Err(error) = ir.validate_data_restrictions() {
        let line = declared_data_restrictions
            .get(error.index)
            .map(|(line, _, _)| *line)
            .unwrap_or(1);
        return Err(OdlError::new(
            E_INVALID_DATA_RESTRICTION,
            format!("object '{}': {}", error.object, error.message),
            line,
            1,
        ));
    }
    for object in &ir.object_types {
        for action in &object.actions {
            for created in &action.creates {
                let Some(edge) = created.link_edge.as_ref() else {
                    continue;
                };
                let value = (object.name.clone(), created.object_type.clone());
                if let Some(previous) = endpoints.get(edge) {
                    if *previous != value {
                        return Err(OdlError::new(
                            E_DUP_RELATION,
                            format!("action 边 '{edge}' 与显式 relation 端点定义冲突"),
                            link_lines.get(edge).copied().unwrap_or(1),
                            1,
                        ));
                    }
                    continue;
                }
                endpoints.insert(edge.clone(), value);
            }
        }
    }
    for (line_no, relation) in &declared_relations {
        if relation.composed_of.is_empty() {
            continue;
        }
        for property in &relation.composed_of {
            if !endpoints.contains_key(property) {
                return Err(OdlError::new(
                    E_INVALID_PROPERTY_CONSTRAINT,
                    format!(
                        "relation '{}' composes undeclared object property '{property}'",
                        relation.name
                    ),
                    *line_no,
                    1,
                ));
            }
        }
    }

    Ok(ir)
}

/// 解析块头：仅当 `word:` 且无值、无空格时返回头名。
fn parse_header(line: &str) -> Option<&str> {
    let (k, v) = line.split_once(':')?;
    if !v.trim().is_empty() {
        return None;
    }
    let k = k.trim();
    if k.is_empty() || k.contains(' ') {
        return None;
    }
    Some(k)
}

/// Parse a typed literal for data-property `value` restrictions.
fn parse_data_literal(raw: &str) -> Result<DataLiteral, String> {
    if (raw.starts_with('"') && raw.ends_with('"') && raw.len() >= 2)
        || (raw.starts_with('\'') && raw.ends_with('\'') && raw.len() >= 2)
    {
        return Ok(DataLiteral::String(raw[1..raw.len() - 1].to_owned()));
    }
    match raw {
        "true" => return Ok(DataLiteral::Boolean(true)),
        "false" => return Ok(DataLiteral::Boolean(false)),
        _ => {}
    }
    if let Ok(value) = raw.parse::<i64>() {
        return Ok(DataLiteral::Integer(value));
    }
    if let Ok(value) = raw.parse::<f64>() {
        return Ok(DataLiteral::Double(value));
    }
    Err(format!(
        "cannot parse '{raw}' as a data restriction literal; expected quoted string, true/false, integer, or number"
    ))
}

/// 列表项 key 匹配：`- <key>: <value>` → 返回 value（key 不匹配 / 无冒号 → None）。
/// updates/creates/precondition 三条列表项解析共用，消除三处重复的 split_once 模式。
fn list_item_value<'a>(content: &'a str, key: &str) -> Option<&'a str> {
    content
        .split_once(':')
        .filter(|(k, _)| k.trim() == key)
        .map(|(_, v)| v.trim())
}

/// 把 ODL 值文本解析为 `ValueExpr`：引用参数 / self / 字面量（数字 / 布尔 / 字符串）。
/// Split a property declaration into lexical tokens, preserving quoted spaces.
///
/// A quoted value keeps backslashes except for an escaped quote.  Thus
/// `pattern "[A-Z]\d{2}"` preserves the regex escape while allowing readable
/// quoted strings in ODL.
fn split_property_tokens(spec: &str) -> Result<Vec<String>, String> {
    let mut tokens = Vec::new();
    let mut token = String::new();
    let mut quoted = false;
    let mut chars = spec.chars().peekable();

    while let Some(char) = chars.next() {
        if quoted {
            match char {
                '"' => quoted = false,
                '\\' => {
                    if let Some(next) = chars.next() {
                        if next != '"' && next != '\\' {
                            token.push('\\');
                        }
                        token.push(next);
                    } else {
                        return Err("unterminated escape in quoted constraint".to_owned());
                    }
                }
                _ => token.push(char),
            }
        } else if char == '"' {
            quoted = true;
            if !token.is_empty() {
                tokens.push(std::mem::take(&mut token));
            }
        } else if char.is_whitespace() {
            if !token.is_empty() {
                tokens.push(std::mem::take(&mut token));
            }
        } else {
            token.push(char);
        }
    }

    if quoted {
        return Err("unterminated quoted constraint".to_owned());
    }
    if !token.is_empty() {
        tokens.push(token);
    }
    Ok(tokens)
}

/// Distinguish an unknown leading scalar type from an invalid constraint list.
#[derive(Debug)]
enum PropertyDefinitionError {
    UnknownType(String),
    Invalid(String),
}

fn parse_property_definition(
    spec: &str,
    declared_datatypes: &std::collections::BTreeMap<String, (ScalarType, PropertyDataRange)>,
) -> Result<(ScalarType, Vec<PropertyFacet>), PropertyDefinitionError> {
    let (spec, data_range_source) =
        extract_data_range(spec).map_err(PropertyDefinitionError::Invalid)?;
    let tokens = split_property_tokens(&spec).map_err(PropertyDefinitionError::Invalid)?;
    let Some(type_token) = tokens.first() else {
        return Err(PropertyDefinitionError::Invalid(
            "property type is empty".to_owned(),
        ));
    };
    let (ty, inherited_range) = if let Ok(ty) = ScalarType::from_str(type_token) {
        (ty, None)
    } else if let Some((ty, range)) = declared_datatypes.get(type_token) {
        (*ty, Some(range.clone()))
    } else {
        return Err(PropertyDefinitionError::UnknownType(format!(
            "未知标量类型: {type_token}"
        )));
    };
    let mut constraints = Vec::new();
    if inherited_range.is_some() && data_range_source.is_some() {
        return Err(PropertyDefinitionError::Invalid(
            "property cannot combine a named datatype with another range".to_owned(),
        ));
    }
    if let Some(range) = inherited_range {
        constraints.push(PropertyFacet::Range(range));
    } else if let Some(source) = data_range_source {
        let body = &source["range(".len()..source.len() - 1];
        let range = parse_data_range(body).map_err(PropertyDefinitionError::Invalid)?;
        constraints.push(PropertyFacet::Range(range));
    }
    let rest = &tokens[1..];
    if rest.len() % 2 != 0 {
        return Err(PropertyDefinitionError::Invalid(
            "every property constraint needs a value".to_owned(),
        ));
    }

    let mut seen = std::collections::BTreeSet::new();
    for pair in rest.as_chunks::<2>().0 {
        let facet = match pair[0].as_str() {
            "min_cardinality" => PropertyFacet::MinCardinality(pair[1].clone()),
            "max_cardinality" => PropertyFacet::MaxCardinality(pair[1].clone()),
            "one_of" => PropertyFacet::OneOf(
                parse_one_of_values(&pair[1]).map_err(PropertyDefinitionError::Invalid)?,
            ),
            "min_inclusive" => PropertyFacet::MinInclusive(pair[1].clone()),
            "min_exclusive" => PropertyFacet::MinExclusive(pair[1].clone()),
            "max_inclusive" => PropertyFacet::MaxInclusive(pair[1].clone()),
            "max_exclusive" => PropertyFacet::MaxExclusive(pair[1].clone()),
            "min_length" => PropertyFacet::MinLength(pair[1].clone()),
            "max_length" => PropertyFacet::MaxLength(pair[1].clone()),
            "total_digits" => PropertyFacet::TotalDigits(pair[1].clone()),
            "fraction_digits" => PropertyFacet::FractionDigits(pair[1].clone()),
            "pattern" => PropertyFacet::Pattern(pair[1].clone()),
            other => {
                return Err(PropertyDefinitionError::Invalid(format!(
                    "unknown property constraint `{other}`"
                )));
            }
        };
        if !seen.insert(facet.name()) {
            return Err(PropertyDefinitionError::Invalid(format!(
                "duplicate `{}` constraint",
                facet.name()
            )));
        }
        constraints.push(facet);
    }

    validate_property_constraints("property", ty, &constraints)
        .map_err(PropertyDefinitionError::Invalid)?;
    Ok((ty, constraints))
}

/// Remove a top-level `range(...)` expression before ordinary key/value
/// constraint parsing.  Quoted facet values are skipped while locating the
/// balanced expression so patterns such as `pattern "range ("` are inert.
fn extract_data_range(spec: &str) -> Result<(String, Option<String>), String> {
    let bytes = spec.as_bytes();
    let mut escaped = false;
    let mut quoted = false;

    for (index, &byte) in bytes.iter().enumerate() {
        if quoted {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                quoted = false;
            }
            continue;
        }
        if byte == b'"' {
            quoted = true;
            continue;
        }
        let at_word_start = index == 0 || !word_byte(bytes[index - 1]);
        if !at_word_start || !spec[index..].starts_with("range") {
            continue;
        }

        let end = index + "range".len();
        if end < bytes.len() && word_byte(bytes[end]) {
            continue;
        }

        let mut cursor = end;
        while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }

        if cursor < bytes.len() && bytes[cursor] == b'(' {
            let closing = balanced_parenthesis(spec, cursor)?;
            let source = spec[index..=closing].to_owned();
            let remaining = format!("{}{}", &spec[..index], &spec[closing + 1..]);
            return Ok((remaining, Some(source)));
        }
    }

    Ok((spec.to_owned(), None))
}

fn word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn balanced_parenthesis(source: &str, opening: usize) -> Result<usize, String> {
    let mut depth = 0usize;
    let mut escaped = false;
    let mut quoted = false;

    for (index, char) in source.char_indices().skip(opening) {
        if quoted {
            if escaped {
                escaped = false;
            } else if char == '\\' {
                escaped = true;
            } else if char == '"' {
                quoted = false;
            }
            continue;
        }
        match char {
            '"' => quoted = true,
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Ok(index);
                }
            }
            _ => {}
        }
    }
    Err("unterminated range expression".to_owned())
}

fn parse_one_of_values(token: &str) -> Result<Vec<String>, String> {
    let body = token
        .strip_prefix('[')
        .and_then(|body| body.strip_suffix(']'))
        .ok_or_else(|| "one_of requires a [value,value] list".to_owned())?;
    let values = body
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if values.is_empty() {
        return Err("one_of requires at least one value".to_owned());
    }
    Ok(values)
}

fn parse_value_expr(v: &str, param_names: &[String]) -> ValueExpr {
    if v.starts_with('"') {
        // 字符串字面量：经 serde_json 校验闭合与转义（`"a\"b"` → `a"b`；
        // 未闭合 / 非法转义 → 解析失败，按普通文本回落，不静默产出残缺字符串）。
        if let Ok(serde_json::Value::String(s)) = serde_json::from_str::<serde_json::Value>(v) {
            return ValueExpr::Literal(serde_json::Value::String(s));
        }
    }
    if v == "self" {
        return ValueExpr::SelfId;
    }
    if let Some(f) = v.strip_prefix("self.") {
        return ValueExpr::SelfField(f.to_string());
    }
    if param_names.iter().any(|p| p == v) {
        return ValueExpr::Param(v.to_string());
    }
    if let Ok(n) = v.parse::<i64>() {
        return ValueExpr::Literal(serde_json::Value::from(n));
    }
    if let Ok(f) = v.parse::<f64>() {
        return ValueExpr::Literal(serde_json::Value::from(f));
    }
    if v == "true" {
        return ValueExpr::Literal(serde_json::Value::Bool(true));
    }
    if v == "false" {
        return ValueExpr::Literal(serde_json::Value::Bool(false));
    }
    ValueExpr::Literal(serde_json::Value::String(v.to_string()))
}

/// 冲刷当前字段更新到动作。
fn flush_update(u: &mut Option<FieldUpdate>, a: &mut Option<(u32, ActionDef)>) {
    if let (Some(upd), Some((_, act))) = (u.take(), a.as_mut()) {
        act.updates.push(upd);
    }
}

/// 冲刷当前创建效应到动作。
fn flush_create(c: &mut Option<CreateEffect>, a: &mut Option<(u32, ActionDef)>) {
    if let (Some(cr), Some((_, act))) = (c.take(), a.as_mut()) {
        act.creates.push(cr);
    }
}

/// 冲刷当前动作到对象。
fn flush_action(a: &mut Option<(u32, ActionDef)>, o: &mut Option<(u32, ObjectType)>) {
    if let (Some((_, act)), Some((_, obj))) = (a.take(), o.as_mut()) {
        obj.actions.push(act);
    }
}

/// 冲刷当前 Policy 前置条件子句到动作。
fn flush_clause(c: &mut Option<Clause>, a: &mut Option<(u32, ActionDef)>) {
    if let (Some(cl), Some((_, act))) = (c.take(), a.as_mut()) {
        act.policy.precondition.push(cl);
    }
}

/// 冲刷当前机会条件到机会规则。
fn flush_condition(c: &mut Option<Clause>, r: &mut (u32, OpportunityRule)) {
    if let Some(cl) = c.take() {
        r.1.when.push(cl);
    }
}

/// 校验并冲刷完整机会规则；半声明规则在解析阶段失败。
fn flush_opportunity(
    r: &mut Option<(u32, OpportunityRule)>,
    fields: &mut [bool; 6],
    o: &mut Option<(u32, ObjectType)>,
) -> Result<(), OdlError> {
    let Some((line_no, rule)) = r.take() else {
        return Ok(());
    };
    if !fields.iter().all(|seen| *seen) || rule.evidence.is_empty() || rule.when.is_empty() {
        return Err(OdlError::new(
            E_INVALID_PROPERTY_CONSTRAINT,
            "opportunity 规则必须完整声明 subject_type、estimated_value、evidence、urgency、link 和至少一个 when 条件",
            line_no,
            1,
        ));
    }
    if let Some((_, obj)) = o.as_mut() {
        obj.opportunity_rules.push(rule);
    }
    *fields = [false; 6];
    Ok(())
}

/// 把 ODL 比较运算符文本解析为 `CompareOp`（未知 → `Eq` 兜底）。
fn parse_compare_op(v: &str) -> CompareOp {
    match v.trim() {
        "lt" => CompareOp::Lt,
        "le" => CompareOp::Le,
        "gt" => CompareOp::Gt,
        "ge" => CompareOp::Ge,
        "ne" => CompareOp::Ne,
        _ => CompareOp::Eq,
    }
}
