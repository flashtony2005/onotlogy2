//! Runtime validation for persisted ontology data.
//!
//! ODL currently describes scalar types, but it has no required/default marker.
//! Therefore a definition only validates fields that are present: adding a
//! property remains compatible with existing JSON rows, while a changed type
//! is rejected when an old row contains that field. Unknown fields are kept so
//! older data can survive a definition that removes or renames a property.

use chrono::{DateTime, NaiveDate};
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

/// Canonicalize an XSD `decimal` lexical form without converting through a
/// floating-point type.  Exponents are therefore rejected, as required by the
/// datatype, and arbitrarily many significant digits remain exact.
pub fn canonical_decimal(value: &str) -> Option<String> {
    let (negative, body) = match value.strip_prefix('-') {
        Some(body) => (true, body),
        None => (false, value.strip_prefix('+').unwrap_or(value)),
    };
    let (integer, fraction) = match body.split_once('.') {
        Some((integer, fraction)) => (integer, fraction),
        None => (body, ""),
    };
    if integer.is_empty() || !integer.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    if !fraction.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let integer = integer.trim_start_matches('0');
    let integer = if integer.is_empty() { "0" } else { integer };
    let fraction = fraction.trim_end_matches('0');
    if integer == "0" && fraction.is_empty() {
        return Some("0".to_owned());
    }
    let sign = if negative { "-" } else { "" };
    if fraction.is_empty() {
        return Some(format!("{sign}{integer}"));
    }
    Some(format!("{sign}{integer}.{fraction}"))
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
        ScalarType::Decimal => match value {
            // Non-integral `serde_json::Number` is parsed as IEEE f64 in this
            // runtime, so accepting it could silently lose decimal precision.
            // Exact decimal JSON uses a string or an integral number.
            Value::Number(number) if number.is_i64() || number.is_u64() => {
                canonical_decimal(&number.to_string()).is_some()
            }
            Value::String(value) => canonical_decimal(value).is_some(),
            _ => false,
        },
        ScalarType::Date => value
            .as_str()
            .and_then(|value| {
                let date = NaiveDate::parse_from_str(value, "%Y-%m-%d").ok()?;
                (date.to_string() == value).then_some(())
            })
            .is_some(),
        ScalarType::DateTime => value
            .as_str()
            .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
            .is_some(),
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
    fn integer_property_rejects_non_integral_json_number() {
        let current = object("object Customer:\n  properties:\n    score: int\n");
        assert!(matches!(
            validate_object_data(&current, &json!({"score": 30.5})),
            Err(DataValidationError::InvalidPropertyType { .. })
        ));
    }

    #[test]
    fn parses_and_validates_rfc3339_datetime_properties() {
        let current = object("object Event:\n  properties:\n    occurred_at: datetime\n");
        assert_eq!(ScalarType::DateTime, current.properties[0].ty);
        assert!(
            validate_object_data(
                &current,
                &json!({ "occurred_at": "2026-08-23T08:00:00+08:00" })
            )
            .is_ok()
        );
        assert!(matches!(
            validate_object_data(&current, &json!({ "occurred_at": "not-a-date" })),
            Err(DataValidationError::InvalidPropertyType { .. })
        ));
    }

    #[test]
    fn parses_and_validates_calendar_date_properties() {
        let current = object("object Person:\n  properties:\n    birthday: date\n");
        assert_eq!(ScalarType::Date, current.properties[0].ty);
        assert!(validate_object_data(&current, &json!({"birthday": "2024-02-29"})).is_ok());
        for invalid in [
            "2026-02-29", // not a leap year
            "2026-08-23T00:00:00Z",
            "2026-8-23",
            "August 23, 2026",
        ] {
            assert!(matches!(
                validate_object_data(&current, &json!({ "birthday": invalid })),
                Err(DataValidationError::InvalidPropertyType { .. })
            ));
        }
    }

    #[test]
    fn parses_and_validates_exact_decimal_properties() {
        let current = object("object Account:\n  properties:\n    balance: decimal\n");
        assert_eq!(ScalarType::Decimal, current.properties[0].ty);
        for valid in ["123", "+0001.2300", "-0.100", "1.", "0"] {
            assert!(validate_object_data(&current, &json!({ "balance": valid })).is_ok());
        }
        // Integral JSON is exact; fractional JSON would pass through IEEE f64.
        assert!(validate_object_data(&current, &json!({ "balance": 123 })).is_ok());
        assert!(validate_object_data(&current, &json!({ "balance": 12.5 })).is_err());

        for invalid in ["1e3", ".5", " 1.2", "1_2", "1.2.3"] {
            assert!(matches!(
                validate_object_data(&current, &json!({ "balance": invalid })),
                Err(DataValidationError::InvalidPropertyType { .. })
            ));
        }
    }

    #[test]
    fn removed_or_unknown_property_is_preserved() {
        let current = object("object Customer:\n  properties:\n    name: string\n");
        let old_row = json!({"name": "Ada", "legacy_level": "VIP"});
        assert!(validate_object_data(&current, &old_row).is_ok());
    }
}
