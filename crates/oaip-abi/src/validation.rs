//! Runtime validation for persisted ontology data.
//!
//! ODL currently describes scalar types, but it has no required/default marker.
//! Therefore a definition only validates fields that are present: adding a
//! property remains compatible with existing JSON rows, while a changed type
//! is rejected when an old row contains that field. Unknown fields are kept so
//! older data can survive a definition that removes or renames a property.

use serde_json::Value;
use thiserror::Error;

use crate::ir::{ObjectType, ScalarType};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DataValidationError {
    #[error("对象数据必须是 JSON object，实际为 {actual}")]
    NotObject { actual: String },
    #[error("属性 {field} 类型不匹配：期望 {expected}，实际 {actual}")]
    InvalidPropertyType {
        field: String,
        expected: String,
        actual: String,
    },
}

/// Validate one persisted JSON row against the current object definition.
///
/// Missing properties are valid because ODL has no required-property syntax.
/// This is the compatibility rule that lets an additive ODL change read old
/// rows without rewriting the database table or JSON payload.
pub fn validate_object_data(
    definition: &ObjectType,
    data: &Value,
) -> Result<(), DataValidationError> {
    let object = data
        .as_object()
        .ok_or_else(|| DataValidationError::NotObject {
            actual: json_type(data).to_string(),
        })?;

    for property in &definition.properties {
        let Some(value) = object.get(&property.name) else {
            continue;
        };
        if !matches_scalar(value, property.ty) {
            return Err(DataValidationError::InvalidPropertyType {
                field: property.name.clone(),
                expected: property.ty.to_string(),
                actual: json_type(value).to_string(),
            });
        }
    }
    Ok(())
}

fn matches_scalar(value: &Value, ty: ScalarType) -> bool {
    match ty {
        ScalarType::String => value.is_string(),
        ScalarType::Int => value.as_i64().is_some() || value.as_u64().is_some(),
        ScalarType::Float => value.is_number(),
        ScalarType::Bool => value.is_boolean(),
    }
}

fn json_type(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_odl;
    use serde_json::json;

    fn object(odl: &str) -> ObjectType {
        parse_odl(odl)
            .unwrap()
            .object_types
            .into_iter()
            .next()
            .unwrap()
    }

    #[test]
    fn additive_property_change_accepts_old_rows() {
        let current = object("object Customer:\n  properties:\n    name: string\n    score: int\n");
        let old_row = json!({"name": "Ada"});
        assert!(validate_object_data(&current, &old_row).is_ok());
    }

    #[test]
    fn changed_property_type_rejects_incompatible_old_value() {
        let current = object("object Customer:\n  properties:\n    score: int\n");
        let old_row = json!({"score": "high"});
        assert!(matches!(
            validate_object_data(&current, &old_row),
            Err(DataValidationError::InvalidPropertyType { .. })
        ));
    }

    #[test]
    fn removed_or_unknown_property_is_preserved() {
        let current = object("object Customer:\n  properties:\n    name: string\n");
        let old_row = json!({"name": "Ada", "legacy_level": "VIP"});
        assert!(validate_object_data(&current, &old_row).is_ok());
    }
}
