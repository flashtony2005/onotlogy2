//! ODL 解析器 —— 把 .odl 文本解析为 `OntologyIR`（对应 REQ-001 / REQ-002）
//!
//! 语法（V0.1，确定性，无 LLM）：
//! ```text
//! object Customer:
//!   properties:
//!     name: string
//!     level: string
//! ```
//! 不支持 Relation / Event 语法（归 V0.2+）。

use crate::ir::{ObjectType, OntologyIR, PropertyDef, ScalarType};
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

/// 解析 .odl 源文本为 IR。
pub fn parse_odl(src: &str) -> Result<OntologyIR, OdlError> {
    let mut ir = OntologyIR::default();
    // 暂存「声明行号 + 对象」，便于缺 property 时回带真实行列号（TC-001-b 契约）。
    let mut declared: Vec<(u32, ObjectType)> = Vec::new();
    let mut current: Option<(u32, ObjectType)> = None;
    let mut in_properties = false;

    for (idx, raw) in src.lines().enumerate() {
        let line_no = (idx + 1) as u32;
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if let Some(rest) = line.strip_prefix("object ") {
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
                },
            ));
            in_properties = false;
            continue;
        }

        if line == "properties:" {
            if current.is_none() {
                return Err(OdlError::new(
                    E_NO_PROP_BLOCK,
                    "properties: 出现在 object 之外",
                    line_no,
                    1,
                ));
            }
            in_properties = true;
            continue;
        }

        if let Some((pname, ptype)) = line.split_once(':') {
            if !in_properties {
                return Err(OdlError::new(
                    E_NO_PROP_BLOCK,
                    format!("属性 '{pname}' 必须在 properties: 块内"),
                    line_no,
                    1,
                ));
            }
            let pname = pname.trim();
            let ptype = ptype.trim();
            if pname.is_empty() {
                return Err(OdlError::new(E_EMPTY_PROP_NAME, "属性名为空", line_no, 1));
            }
            let ty = ScalarType::from_str(ptype)
                .map_err(|e| OdlError::new(E_UNKNOWN_TYPE, e, line_no, (pname.len() + 2) as u32))?;
            if let Some((_, obj)) = current.as_mut() {
                obj.properties.push(PropertyDef {
                    name: pname.to_string(),
                    ty,
                });
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
