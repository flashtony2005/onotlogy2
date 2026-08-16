//! Delta 校验（M5 拆分）：白名单操作 + 结构约束（propose 前门）。

use super::types::MutationError;

/// Delta 白名单操作（结构化变更；AI 自由文本改 Skill 一律拒绝）。
const DELTA_OPS: &[&str] = &["insert_step", "modify", "delete_step", "replace"];

pub fn validate_delta(delta: &serde_json::Value) -> Result<(), MutationError> {
    let obj = delta.as_object().ok_or(MutationError::DeltaNotObject)?;
    if obj.is_empty() {
        return Err(MutationError::DeltaNotObject);
    }
    for (op, spec) in obj {
        if !DELTA_OPS.contains(&op.as_str()) {
            return Err(MutationError::UnknownOp(op.clone()));
        }
        let spec_obj = spec
            .as_object()
            .ok_or_else(|| MutationError::MissingField(format!("{op}: 操作体必须是对象")))?;
        match op.as_str() {
            "insert_step" => {
                require_str(spec_obj, "step", op)?;
                if spec_obj
                    .get("after")
                    .is_some_and(|a| !a.is_string() || a.as_str().unwrap_or("").is_empty())
                {
                    return Err(MutationError::MissingField(format!("{op}.after")));
                }
            }
            "modify" => require_str(spec_obj, "step", op)?,
            "delete_step" => require_str(spec_obj, "step", op)?,
            "replace" => {
                require_str(spec_obj, "step", op)?;
                require_str(spec_obj, "with", op)?;
            }
            _ => unreachable!("白名单已过滤"),
        }
    }
    Ok(())
}

fn require_str(
    spec: &serde_json::Map<String, serde_json::Value>,
    key: &str,
    op: &str,
) -> Result<(), MutationError> {
    match spec.get(key).and_then(|v| v.as_str()) {
        Some(s) if !s.is_empty() => Ok(()),
        _ => Err(MutationError::MissingField(format!("{op}.{key}"))),
    }
}
