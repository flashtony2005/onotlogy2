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
//! 不支持 Relation / Event 顶层语法（归 V0.2+）；actions 内的 effects 已是声明式表达。

use crate::ir::{
    ActionDef, Clause, CompareOp, CreateEffect, FieldUpdate, ObjectType, OntologyIR, ParamDef,
    PolicyDef, PropertyDef, ScalarType, ValueExpr,
};
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
const E_NO_PROP_BLOCK: u32 = 1005; // 属性出现在 properties: 块外
const E_EMPTY_PROP_NAME: u32 = 1003; // 属性名为空 / 无法解析行
const E_UNKNOWN_TYPE: u32 = 1002; // 未知标量类型
const E_MISSING_PROPERTY: u32 = 1001; // object 无任何 property

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

    let indent_of = |raw: &str| raw.len() - raw.trim_start().len();

    for (idx, raw) in src.lines().enumerate() {
        let line_no = (idx + 1) as u32;
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let indent = indent_of(raw);

        // object 声明（优先级最高，先处理）
        if let Some(rest) = line.strip_prefix("object ") {
            flush_update(&mut cur_update, &mut cur_action);
            flush_create(&mut cur_create, &mut cur_action);
            flush_action(&mut cur_action, &mut current);
            if let Some(obj) = current.take() {
                declared.push(obj);
            }
            let name = rest.trim_end_matches(':').trim();
            if name.is_empty() {
                return Err(OdlError::new(
                    E_OBJECT_NAME,
                    "object 声明缺少名称",
                    line_no,
                    1,
                ));
            }
            current = Some((
                line_no,
                ObjectType {
                    name: name.to_string(),
                    properties: Vec::new(),
                    actions: Vec::new(),
                },
            ));
            section = vec![(Section::Object, indent)];
            continue;
        }

        // 缩进回退：离开比当前行更深的块（离开时冲刷对应临时态）
        while section.len() > 1 && indent <= section.last().unwrap().1 {
            match section.last().unwrap().0 {
                Section::ActionUpdates => flush_update(&mut cur_update, &mut cur_action),
                Section::ActionCreates => flush_create(&mut cur_create, &mut cur_action),
                Section::ActionPrecondition => flush_clause(&mut cur_clause, &mut cur_action),
                Section::Action => flush_action(&mut cur_action, &mut current),
                _ => {}
            }
            section.pop();
        }
        let cur = *section.last().unwrap();

        // 列表项（actions 的 updates/creates 用 `- ` 开头）
        if let Some(content) = line.strip_prefix("- ") {
            let content = content.trim();
            match cur.0 {
                Section::ActionUpdates => {
                    flush_update(&mut cur_update, &mut cur_action);
                    // 列表项语法：`- field: <字段名>`（field 为关键字，值为目标字段名）
                    if let Some((k, v)) = content.split_once(':')
                        && k.trim() == "field"
                    {
                        cur_update = Some(FieldUpdate {
                            field: v.trim().to_string(),
                            value: ValueExpr::Literal(serde_json::Value::Null),
                        });
                    }
                }
                Section::ActionCreates => {
                    flush_create(&mut cur_create, &mut cur_action);
                    if let Some((k, v)) = content.split_once(':')
                        && k.trim() == "type"
                    {
                        cur_create = Some(CreateEffect {
                            object_type: v.trim().to_string(),
                            id: None,
                            data: Vec::new(),
                            link_edge: None,
                        });
                    }
                }
                Section::ActionPrecondition => {
                    flush_clause(&mut cur_clause, &mut cur_action);
                    // 列表项语法：`- field: <字段或参数>`（field 为关键字，值为目标字段/参数）
                    if let Some((k, v)) = content.split_once(':')
                        && k.trim() == "field"
                    {
                        cur_clause = Some(Clause {
                            field: v.trim().to_string(),
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
                Section::Properties => {
                    if k.is_empty() {
                        return Err(OdlError::new(E_EMPTY_PROP_NAME, "属性名为空", line_no, 1));
                    }
                    let ty = ScalarType::from_str(v).map_err(|e| {
                        OdlError::new(E_UNKNOWN_TYPE, e, line_no, (k.len() + 2) as u32)
                    })?;
                    if let Some((_, obj)) = current.as_mut() {
                        obj.properties.push(PropertyDef {
                            name: k.to_string(),
                            ty,
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

    // 每个 object 必须至少含一个 property（TC-001-b：缺 property → 错误码，带回声明行列号）
    for (ln, obj) in &declared {
        if obj.properties.is_empty() {
            return Err(OdlError::new(
                E_MISSING_PROPERTY,
                format!("object '{}' 缺少 property", obj.name),
                *ln,
                1,
            ));
        }
        ir.object_types.push(obj.clone());
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

/// 把 ODL 值文本解析为 `ValueExpr`：引用参数 / self / 字面量（数字 / 布尔 / 字符串）。
fn parse_value_expr(v: &str, param_names: &[String]) -> ValueExpr {
    if v.len() >= 2 && v.starts_with('"') && v.ends_with('"') {
        return ValueExpr::Literal(serde_json::Value::String(v[1..v.len() - 1].to_string()));
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
