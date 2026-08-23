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

use crate::ir::{
    ActionDef, Clause, CompareOp, CreateEffect, FieldUpdate, ObjectType, OntologyIR, ParamDef,
    PolicyDef, PropertyDef, PropertyFacet, RelationDef, ScalarType, ValueExpr,
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
    ActionPrecondition,
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
    let mut declared_relations: Vec<(u32, RelationDef)> = Vec::new();
    let mut current_relation: Option<(u32, RelationDef)> = None;
    let mut link_lines: std::collections::BTreeMap<String, u32> = std::collections::BTreeMap::new();
    let mut declared_disjoint: Vec<(u32, Vec<String>)> = Vec::new();
    let mut declared_equivalent: Vec<(u32, Vec<String>)> = Vec::new();

    let indent_of = |raw: &str| raw.len() - raw.trim_start().len();

    'line: for (idx, raw) in src.lines().enumerate() {
        let line_no = (idx + 1) as u32;
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let indent = indent_of(raw);

        // Named-class DL 组声明：让常见 TBox 约束无需手写 proposal JSON。
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
                (Section::Object, "actions") => section.push((Section::Actions, indent)),
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
                        },
                    ));
                    section.push((Section::Action, indent));
                }
                (Section::Action, "params") => section.push((Section::ActionParams, indent)),
                (Section::Action, "updates") => section.push((Section::ActionUpdates, indent)),
                (Section::Action, "creates") => section.push((Section::ActionCreates, indent)),
                (Section::Action, "policy") => section.push((Section::ActionPolicy, indent)),
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
                    let (ty, constraints) = parse_property_definition(v).map_err(|error| {
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
                    }
                }
                Section::ActionPolicy => {
                    if k == "require_approval"
                        && let Some((_, a)) = cur_action.as_mut()
                    {
                        a.policy.require_approval = v == "true";
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

    // 冲刷尾部临时态
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
        ir.object_types.push(obj.clone());
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
    if let Err(error) = ir.validate_class_groups() {
        let lines = if error.kind == "disjoint" {
            &declared_disjoint
        } else {
            &declared_equivalent
        };
        let line = lines.get(error.index).map(|(line, _)| *line).unwrap_or(1);
        return Err(OdlError::new(E_INVALID_CLASS_GROUP, error.message, line, 1));
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
    ir.relations = declared_relations.iter().map(|(_, r)| r.clone()).collect();
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
) -> Result<(ScalarType, Vec<PropertyFacet>), PropertyDefinitionError> {
    let tokens = split_property_tokens(spec).map_err(PropertyDefinitionError::Invalid)?;
    let Some(type_token) = tokens.first() else {
        return Err(PropertyDefinitionError::Invalid(
            "property type is empty".to_owned(),
        ));
    };
    let ty = ScalarType::from_str(type_token).map_err(PropertyDefinitionError::UnknownType)?;
    let rest = &tokens[1..];
    if rest.len() % 2 != 0 {
        return Err(PropertyDefinitionError::Invalid(
            "every property constraint needs a value".to_owned(),
        ));
    }

    let mut seen = std::collections::BTreeSet::new();
    let mut constraints = Vec::new();
    for pair in rest.as_chunks::<2>().0 {
        let facet = match pair[0].as_str() {
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
