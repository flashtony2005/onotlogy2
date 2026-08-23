//! Runtime validation for persisted ontology data.
//!
//! ODL currently describes scalar types, but it has no required/default marker.
//! Therefore a definition only validates fields that are present: adding a
//! property remains compatible with existing JSON rows, while a changed type
//! is rejected when an old row contains that field. Unknown fields are kept so
//! older data can survive a definition that removes or renames a property.

use chrono::{DateTime, FixedOffset, NaiveDate};
use regex::Regex;
use serde_json::Value;
use thiserror::Error;

use crate::ir::{ObjectType, PropertyFacet, ScalarType};

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
    #[error("属性 {field} 违反约束：{message}")]
    ConstraintViolation { field: String, message: String },
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

/// Compile the conservative XSD-pattern subset accepted across ODL and DL.
///
/// XSD patterns are implicitly fully anchored. Rust regex syntax is close but
/// not identical, so uncertain constructs fail closed instead of being silently
/// reinterpreted as Rust regular expressions.
pub fn compile_xsd_pattern_subset(pattern: &str) -> Result<Regex, String> {
    let mut escaped = false;
    let mut in_class = false;
    let mut chars = pattern.char_indices().peekable();

    while let Some((_, char)) = chars.next() {
        if escaped {
            escaped = false;
            if matches!(char, '^' | '$' | 'A' | 'z' | 'b' | 'B') {
                return Err(format!("unsupported pattern escape `\\{char}`"));
            }
            if char == 'p' || char == 'P' {
                if chars.peek().copied().map(|(_, value)| value) != Some('{') {
                    return Err("unsupported Unicode escape".to_owned());
                }
                chars.next();
                let mut depth = 1usize;
                let mut category = String::new();
                while let Some((_, value)) = chars.next_if(|&(_, _)| depth > 0) {
                    match value {
                        '{' => depth += 1,
                        '}' => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        _ => category.push(value),
                    }
                }
                if depth != 0
                    || category.is_empty()
                    || !category
                        .chars()
                        .all(|value| value.is_ascii_alphanumeric() || value == '-' || value == '_')
                {
                    return Err("invalid Unicode character category".to_owned());
                }
            }
            continue;
        }

        match char {
            '\\' => escaped = true,
            '^' | '$' => {
                return Err(format!(
                    "explicit anchor `{char}` is redundant in an XSD pattern"
                ));
            }
            '[' => {
                if in_class {
                    return Err("nested or subtracted character classes".to_owned());
                }
                in_class = true;
                if chars.peek().copied().map(|(_, value)| value) == Some('^') {
                    chars.next();
                }
            }
            ']' if in_class => in_class = false,
            '|' if in_class => return Err("non-XSD character-class union".to_owned()),
            '(' if !in_class && chars.peek().copied().map(|(_, value)| value) == Some('?') => {
                return Err("inline group syntax".to_owned());
            }
            '&' if in_class && chars.peek().copied().map(|(_, value)| value) == Some('&') => {
                return Err("non-XSD character-class intersection".to_owned());
            }
            _ => {}
        }
    }

    if escaped || in_class {
        return Err("unterminated escape or character class".to_owned());
    }

    Regex::new(&format!("^(?:{pattern})$")).map_err(|error| error.to_string())
}

/// Validate constraints attached to a property definition.
pub fn validate_property_constraints(
    _name: &str,
    ty: ScalarType,
    constraints: &[PropertyFacet],
) -> Result<(), String> {
    let mut seen = std::collections::BTreeSet::new();
    let allowed = |facet: &PropertyFacet| match ty {
        ScalarType::String => matches!(
            facet,
            PropertyFacet::MinLength(_) | PropertyFacet::MaxLength(_) | PropertyFacet::Pattern(_)
        ),
        ScalarType::Int | ScalarType::Float | ScalarType::Date | ScalarType::DateTime => matches!(
            facet,
            PropertyFacet::MinInclusive(_)
                | PropertyFacet::MinExclusive(_)
                | PropertyFacet::MaxInclusive(_)
                | PropertyFacet::MaxExclusive(_)
        ),
        ScalarType::Decimal => matches!(
            facet,
            PropertyFacet::MinInclusive(_)
                | PropertyFacet::MinExclusive(_)
                | PropertyFacet::MaxInclusive(_)
                | PropertyFacet::MaxExclusive(_)
                | PropertyFacet::TotalDigits(_)
                | PropertyFacet::FractionDigits(_)
        ),
        ScalarType::Bool => false,
    };

    for facet in constraints {
        if !seen.insert(facet.name()) {
            return Err(format!("duplicate `{}` facet", facet.name()));
        }
        if !allowed(facet) {
            return Err(format!("`{}` is not valid for `{}`", facet.name(), ty));
        }
        match facet {
            PropertyFacet::MinInclusive(value)
            | PropertyFacet::MinExclusive(value)
            | PropertyFacet::MaxInclusive(value)
            | PropertyFacet::MaxExclusive(value) => {
                ordering_bound_value(ty, value)?;
            }
            PropertyFacet::MinLength(value)
            | PropertyFacet::MaxLength(value)
            | PropertyFacet::TotalDigits(value)
            | PropertyFacet::FractionDigits(value) => {
                parse_non_negative_usize(value)?;
            }
            PropertyFacet::Pattern(value) => {
                compile_xsd_pattern_subset(value)?;
            }
        }
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq)]
enum OrderedValue {
    Integer(i128),
    Float(f64),
    Decimal(String),
    Date(NaiveDate),
    DateTime(DateTime<FixedOffset>),
}

fn ordering_bound_value(ty: ScalarType, value: &str) -> Result<OrderedValue, String> {
    match ty {
        ScalarType::Int => Ok(OrderedValue::Integer(
            value.parse::<i64>().map_err(|_| "invalid integer bound")? as i128,
        )),
        ScalarType::Float => Ok(OrderedValue::Float(
            value
                .parse::<f64>()
                .ok()
                .filter(|value| value.is_finite())
                .ok_or("invalid float bound")?,
        )),
        ScalarType::Decimal => Ok(OrderedValue::Decimal(
            canonical_decimal(value).ok_or_else(|| "invalid decimal bound".to_owned())?,
        )),
        ScalarType::Date => Ok(OrderedValue::Date(strict_date(value)?)),
        ScalarType::DateTime => Ok(OrderedValue::DateTime(
            value
                .parse::<DateTime<FixedOffset>>()
                .map_err(|_| "invalid date-time bound")?,
        )),
        _ => Err("ordering facet is not supported for this datatype".to_owned()),
    }
}

fn strict_date(value: &str) -> Result<NaiveDate, String> {
    let parsed = NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .map_err(|_| "invalid calendar date".to_owned())?;
    if parsed.to_string() != value {
        return Err("non-canonical calendar date".to_owned());
    }
    Ok(parsed)
}

fn parse_non_negative_usize(value: &str) -> Result<usize, String> {
    value
        .parse::<usize>()
        .map_err(|_| "bound must be a non-negative integer".to_owned())
}

fn json_integer(value: &Value) -> Option<i128> {
    if let Some(number) = value.as_i64() {
        Some(number as i128)
    } else {
        value.as_u64().map(|number| number as i128)
    }
}

fn compare_decimal(left: &str, right: &str) -> Option<std::cmp::Ordering> {
    fn parts(value: &str) -> Option<(bool, String, String)> {
        let canonical = canonical_decimal(value)?;
        let negative = canonical.starts_with('-');
        let body = canonical.strip_prefix('-').unwrap_or(&canonical);
        let (integer, fraction) = body.split_once('.').unwrap_or((body, ""));
        Some((negative, integer.to_owned(), fraction.to_owned()))
    }

    let (left_negative, left_integer, left_fraction) = parts(left)?;
    let (right_negative, right_integer, right_fraction) = parts(right)?;
    let unsigned = {
        let integers = left_integer
            .len()
            .cmp(&right_integer.len())
            .then_with(|| left_integer.cmp(&right_integer));
        integers.then_with(|| {
            let common = left_fraction.len().min(right_fraction.len());
            let ordering = left_fraction[..common].cmp(&right_fraction[..common]);
            ordering.then_with(|| left_fraction.len().cmp(&right_fraction.len()))
        })
    };
    if left_negative != right_negative {
        return Some(if left_negative {
            std::cmp::Ordering::Less
        } else {
            std::cmp::Ordering::Greater
        });
    }
    let unsigned = if left_negative {
        unsigned.reverse()
    } else {
        unsigned
    };
    Some(if unsigned == std::cmp::Ordering::Equal {
        std::cmp::Ordering::Equal
    } else {
        unsigned
    })
}

fn constraint_admits(value: &Value, ty: ScalarType, facet: &PropertyFacet) -> bool {
    let ordering = || {
        let bound_value = match facet {
            PropertyFacet::MinInclusive(value)
            | PropertyFacet::MinExclusive(value)
            | PropertyFacet::MaxInclusive(value)
            | PropertyFacet::MaxExclusive(value) => value,
            _ => return None,
        };
        let actual = match ty {
            ScalarType::Int => json_integer(value).map(OrderedValue::Integer),
            ScalarType::Float => value
                .as_f64()
                .filter(|value| value.is_finite())
                .map(OrderedValue::Float),
            ScalarType::Decimal => value
                .as_str()
                .and_then(canonical_decimal)
                .map(OrderedValue::Decimal),
            ScalarType::Date => value
                .as_str()
                .and_then(|value| strict_date(value).ok())
                .map(OrderedValue::Date),
            ScalarType::DateTime => value
                .as_str()
                .and_then(|value| value.parse().ok())
                .map(OrderedValue::DateTime),
            _ => None,
        }?;
        let bound = ordering_bound_value(ty, bound_value).ok()?;
        match (actual, bound) {
            (OrderedValue::Integer(actual), OrderedValue::Integer(bound)) => {
                Some(actual.cmp(&bound))
            }
            (OrderedValue::Float(actual), OrderedValue::Float(bound)) => actual.partial_cmp(&bound),
            (OrderedValue::Decimal(actual), OrderedValue::Decimal(bound)) => {
                compare_decimal(&actual, &bound)
            }
            (OrderedValue::Date(actual), OrderedValue::Date(bound)) => Some(actual.cmp(&bound)),
            (OrderedValue::DateTime(actual), OrderedValue::DateTime(bound)) => {
                Some(actual.cmp(&bound))
            }
            _ => None,
        }
    };

    match facet {
        PropertyFacet::MinInclusive(_) => {
            ordering().is_some_and(|ordering| ordering != std::cmp::Ordering::Less)
        }
        PropertyFacet::MinExclusive(_) => {
            ordering().is_some_and(|ordering| ordering == std::cmp::Ordering::Greater)
        }
        PropertyFacet::MaxInclusive(_) => {
            ordering().is_some_and(|ordering| ordering != std::cmp::Ordering::Greater)
        }
        PropertyFacet::MaxExclusive(_) => {
            ordering().is_some_and(|ordering| ordering == std::cmp::Ordering::Less)
        }
        PropertyFacet::MinLength(bound) => {
            let Some(bound) = bound.parse::<usize>().ok() else {
                return false;
            };
            value
                .as_str()
                .is_some_and(|value| value.chars().count() >= bound)
        }
        PropertyFacet::MaxLength(bound) => {
            let Some(bound) = bound.parse::<usize>().ok() else {
                return false;
            };
            value
                .as_str()
                .is_some_and(|value| value.chars().count() <= bound)
        }
        PropertyFacet::Pattern(pattern) => {
            let Ok(regex) = compile_xsd_pattern_subset(pattern) else {
                return false;
            };
            value.as_str().is_some_and(|value| regex.is_match(value))
        }
        PropertyFacet::TotalDigits(bound) | PropertyFacet::FractionDigits(bound) => {
            let Some(bound) = bound.parse::<usize>().ok() else {
                return false;
            };
            let Some(canonical) = value.as_str().and_then(canonical_decimal) else {
                return false;
            };
            let body = canonical.strip_prefix('-').unwrap_or(&canonical);
            let (integer, fraction) = body.split_once('.').unwrap_or((body, ""));
            let count = if matches!(facet, PropertyFacet::TotalDigits(_)) {
                let zero_empty = integer == "0" && fraction.is_empty();
                if zero_empty {
                    0
                } else {
                    integer.len() + fraction.len()
                }
            } else {
                fraction.len()
            };
            count <= bound
        }
    }
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
        if let Err(message) =
            validate_property_constraints(&property.name, property.ty, &property.constraints)
        {
            return Err(DataValidationError::ConstraintViolation {
                field: property.name.clone(),
                message,
            });
        }
        for facet in &property.constraints {
            if !constraint_admits(value, property.ty, facet) {
                return Err(DataValidationError::ConstraintViolation {
                    field: property.name.clone(),
                    message: format!("`{}` rejects {:?}", facet.name(), value),
                });
            }
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
    fn parses_inline_property_constraints() {
        let current = object(
            r#"object Customer:
  properties:
    code: string min_length 2 max_length 8 pattern "[A-Z]{2}[0-9]{4}"
    age: int min_inclusive 18 max_exclusive 65
    balance: decimal total_digits 10 fraction_digits 2
"#,
        );
        let code = &current.properties[0];
        assert_eq!(ScalarType::String, code.ty);
        assert_eq!(3, code.constraints.len());
        assert_eq!("min_length", code.constraints[0].name());
        assert_eq!("2", code.constraints[0].value());
        assert_eq!("pattern", code.constraints[2].name());
        assert_eq!("[A-Z]{2}[0-9]{4}", code.constraints[2].value());

        let age = &current.properties[1];
        assert_eq!("min_inclusive", age.constraints[0].name());
        assert_eq!("18", age.constraints[0].value());

        let balance = &current.properties[2];
        assert_eq!("total_digits", balance.constraints[0].name());
        assert_eq!("fraction_digits", balance.constraints[1].name());
        assert!(
            validate_object_data(
                &current,
                &serde_json::json!({
                    "code": "AB1234", "age": 30, "balance": "1234.56"
                })
            )
            .is_ok()
        );
    }

    #[test]
    fn property_constraints_reject_invalid_values_and_bounds() {
        let current = object(
            r#"object Customer:
  properties:
    code: string min_length 2 pattern "[A-Z]{2}[0-9]{4}"
    age: int min_inclusive 18 max_inclusive 65
    balance: decimal min_exclusive 0 fraction_digits 2
"#,
        );
        assert!(
            validate_object_data(
                &current,
                &serde_json::json!({ "code": "AB1234", "age": 30, "balance": "0.01" })
            )
            .is_ok()
        );
        for invalid in [
            ("code", serde_json::json!("AB123")),
            ("code", serde_json::json!("ab1234")),
            ("code", serde_json::json!("A")),
            ("age", serde_json::json!(17)),
            ("age", serde_json::json!(66)),
            ("balance", serde_json::json!("-0.01")),
            ("balance", serde_json::json!("0.001")),
        ] {
            let mut data = serde_json::json!({
                "code": "AB1234", "age": 30, "balance": "0.01"
            });
            data.as_object_mut()
                .unwrap()
                .insert(invalid.0.into(), invalid.1);
            let error = validate_object_data(&current, &data).unwrap_err();
            assert!(
                matches!(error, DataValidationError::ConstraintViolation { .. }),
                "unexpected error: {error:?}"
            );
        }

        let explicit_anchor = parse_odl(
            r#"object Customer:
  properties:
    code: string pattern "^AB""
"#,
        )
        .unwrap_err();
        assert_eq!(1011, explicit_anchor.code);
        assert!(explicit_anchor.message.contains("constraint"));

        let wrong_facet =
            parse_odl("object Customer:\n  properties:\n    age: int min_length 2\n").unwrap_err();
        assert_eq!(1011, wrong_facet.code);
    }

    #[test]
    fn removed_or_unknown_property_is_preserved() {
        let current = object("object Customer:\n  properties:\n    name: string\n");
        let old_row = json!({"name": "Ada", "legacy_level": "VIP"});
        assert!(validate_object_data(&current, &old_row).is_ok());
    }
}
