//! JSON Schema (constrained subset) validator for
//! `asset_kinds.attribute_schema` and the asset
//! `attributes` JSONB that gets stored against it.
//!
//! The full JSON Schema spec is way more than we need; an admin
//! defining "a license has a seat_count integer and a renewal
//! date" should not have to learn `$ref`, `oneOf`, or
//! `additionalProperties` semantics. The subset accepted here
//! covers the form-builder cases (and rejects everything else
//! at schema-validation time, so we never run untrusted
//! complex-schema execution at validate time):
//!
//! - root MUST be `{ "type": "object", "properties": { ... }, "required"? : [...] }`
//! - each property MUST be one of: string, number, integer,
//!   boolean, array (with `items` of a primitive type)
//! - per-property constraints: `enum`, `minLength`, `maxLength`,
//!   `pattern`, `format` (date | date-time | email | uri),
//!   `minimum`, `maximum`, `multipleOf`
//! - object-level `additionalProperties` defaults to false
//!
//! Anything outside that subset is rejected with a descriptive
//! error so the admin UI can surface the failure. The same
//! validator runs at two boundaries:
//!
//! 1. when a kind is created or updated (validate the
//!    *schema* itself, not data)
//! 2. when an asset row is written (validate the row's
//!    `attributes` against the kind's schema)

use regex::Regex;
use serde_json::Value;

#[derive(Debug, thiserror::Error)]
pub enum AttributeSchemaError {
    #[error("attribute_schema must be a JSON object")]
    RootNotObject,
    #[error("attribute_schema root must declare `type: object`")]
    RootTypeWrong,
    #[error("attribute_schema must have a `properties` object")]
    MissingProperties,
    #[error("property `{0}` must be a JSON object")]
    PropertyNotObject(String),
    #[error("property `{0}` is missing `type`")]
    PropertyMissingType(String),
    #[error("property `{property}` has unsupported type `{ty}`")]
    UnsupportedType { property: String, ty: String },
    #[error("property `{0}` has array `items` outside the supported subset")]
    UnsupportedItems(String),
    #[error("property `{property}` declares an unsupported keyword `{keyword}`")]
    UnsupportedKeyword { property: String, keyword: String },
    #[error("property `{property}` has invalid regex in `pattern`: {error}")]
    InvalidPattern { property: String, error: String },
    #[error("property `{property}` has unsupported `format` `{format}`")]
    UnsupportedFormat { property: String, format: String },
    #[error("`required` must be an array of property names")]
    RequiredNotArray,
    #[error("`required` references unknown property `{0}`")]
    RequiredUnknown(String),
}

#[derive(Debug, thiserror::Error)]
pub enum AttributeError {
    #[error("attributes must be a JSON object")]
    NotObject,
    #[error("unexpected property `{0}` (kind schema does not declare it)")]
    UnknownProperty(String),
    #[error("required property `{0}` is missing")]
    MissingRequired(String),
    #[error("property `{property}` has wrong type: expected {expected}, got {actual}")]
    WrongType {
        property: String,
        expected: String,
        actual: String,
    },
    #[error("property `{property}` value not in enum {allowed:?}")]
    NotInEnum {
        property: String,
        allowed: Vec<Value>,
    },
    #[error("property `{property}` must be at least {min} characters")]
    TooShort { property: String, min: u64 },
    #[error("property `{property}` must be at most {max} characters")]
    TooLong { property: String, max: u64 },
    #[error("property `{property}` does not match required pattern")]
    PatternMismatch { property: String },
    #[error("property `{property}` is not a valid {format}")]
    BadFormat { property: String, format: String },
    #[error("property `{property}` must be >= {min}")]
    BelowMinimum { property: String, min: f64 },
    #[error("property `{property}` must be <= {max}")]
    AboveMaximum { property: String, max: f64 },
    #[error("property `{property}` must be a multiple of {step}")]
    NotMultipleOf { property: String, step: f64 },
}

/// Validate that the supplied JSON is a well-formed
/// attribute_schema in our subset. Run this when an admin
/// saves a kind.
pub fn validate_schema(schema: &Value) -> Result<(), AttributeSchemaError> {
    let obj = schema
        .as_object()
        .ok_or(AttributeSchemaError::RootNotObject)?;

    if obj.get("type").and_then(Value::as_str) != Some("object") {
        return Err(AttributeSchemaError::RootTypeWrong);
    }

    let props = obj
        .get("properties")
        .and_then(Value::as_object)
        .ok_or(AttributeSchemaError::MissingProperties)?;

    for (name, prop) in props {
        validate_property_schema(name, prop)?;
    }

    if let Some(required) = obj.get("required") {
        let arr = required
            .as_array()
            .ok_or(AttributeSchemaError::RequiredNotArray)?;
        for entry in arr {
            let key = entry
                .as_str()
                .ok_or(AttributeSchemaError::RequiredNotArray)?;
            if !props.contains_key(key) {
                return Err(AttributeSchemaError::RequiredUnknown(key.to_string()));
            }
        }
    }

    Ok(())
}

fn validate_property_schema(name: &str, prop: &Value) -> Result<(), AttributeSchemaError> {
    let obj = prop
        .as_object()
        .ok_or_else(|| AttributeSchemaError::PropertyNotObject(name.to_string()))?;

    let ty = obj
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| AttributeSchemaError::PropertyMissingType(name.to_string()))?;

    match ty {
        "string" | "number" | "integer" | "boolean" => {}
        "array" => {
            let items = obj
                .get("items")
                .and_then(Value::as_object)
                .ok_or_else(|| AttributeSchemaError::UnsupportedItems(name.to_string()))?;
            let items_ty = items.get("type").and_then(Value::as_str);
            if !matches!(
                items_ty,
                Some("string") | Some("number") | Some("integer") | Some("boolean")
            ) {
                return Err(AttributeSchemaError::UnsupportedItems(name.to_string()));
            }
        }
        other => {
            return Err(AttributeSchemaError::UnsupportedType {
                property: name.to_string(),
                ty: other.to_string(),
            })
        }
    }

    for (keyword, value) in obj {
        match keyword.as_str() {
            "type" | "items" | "enum" | "title" | "description" | "default" => {}
            "minLength" | "maxLength" | "minimum" | "maximum" | "multipleOf" => {
                if !value.is_number() {
                    return Err(AttributeSchemaError::UnsupportedKeyword {
                        property: name.to_string(),
                        keyword: keyword.clone(),
                    });
                }
            }
            "pattern" => {
                let pat =
                    value
                        .as_str()
                        .ok_or_else(|| AttributeSchemaError::UnsupportedKeyword {
                            property: name.to_string(),
                            keyword: keyword.clone(),
                        })?;
                Regex::new(pat).map_err(|err| AttributeSchemaError::InvalidPattern {
                    property: name.to_string(),
                    error: err.to_string(),
                })?;
            }
            "format" => {
                let f = value
                    .as_str()
                    .ok_or_else(|| AttributeSchemaError::UnsupportedKeyword {
                        property: name.to_string(),
                        keyword: keyword.clone(),
                    })?;
                if !matches!(f, "date" | "date-time" | "email" | "uri") {
                    return Err(AttributeSchemaError::UnsupportedFormat {
                        property: name.to_string(),
                        format: f.to_string(),
                    });
                }
            }
            other => {
                return Err(AttributeSchemaError::UnsupportedKeyword {
                    property: name.to_string(),
                    keyword: other.to_string(),
                })
            }
        }
    }

    Ok(())
}

/// Validate a row's `attributes` JSONB against a stored
/// `attribute_schema`. Caller is responsible for having run
/// `validate_schema` on the schema first (typically at admin
/// write time) so this can assume well-formedness.
pub fn validate_attributes(schema: &Value, attributes: &Value) -> Result<(), AttributeError> {
    let attrs = attributes.as_object().ok_or(AttributeError::NotObject)?;

    let props = schema
        .get("properties")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();

    let required: Vec<String> = schema
        .get("required")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();

    for req in &required {
        if !attrs.contains_key(req) {
            return Err(AttributeError::MissingRequired(req.clone()));
        }
    }

    for (name, value) in attrs {
        let Some(prop_schema) = props.get(name) else {
            return Err(AttributeError::UnknownProperty(name.clone()));
        };
        validate_value(name, prop_schema, value)?;
    }

    Ok(())
}

fn validate_value(name: &str, prop: &Value, value: &Value) -> Result<(), AttributeError> {
    let ty = prop.get("type").and_then(Value::as_str).unwrap_or("");

    let actual = json_kind(value);
    let type_ok = match ty {
        "string" => value.is_string(),
        "number" => value.is_number(),
        "integer" => value.is_i64() || value.is_u64(),
        "boolean" => value.is_boolean(),
        "array" => value.is_array(),
        _ => false,
    };
    if !type_ok {
        return Err(AttributeError::WrongType {
            property: name.to_string(),
            expected: ty.to_string(),
            actual: actual.to_string(),
        });
    }

    if let Some(enum_values) = prop.get("enum").and_then(Value::as_array) {
        if !enum_values.iter().any(|allowed| allowed == value) {
            return Err(AttributeError::NotInEnum {
                property: name.to_string(),
                allowed: enum_values.clone(),
            });
        }
    }

    match ty {
        "string" => check_string(name, prop, value.as_str().unwrap_or(""))?,
        "number" | "integer" => check_number(name, prop, value.as_f64().unwrap_or(0.0))?,
        "array" => {
            if let Some(items) = prop.get("items") {
                let arr = value.as_array().unwrap();
                for item in arr {
                    validate_value(name, items, item)?;
                }
            }
        }
        _ => {}
    }

    Ok(())
}

fn check_string(name: &str, prop: &Value, value: &str) -> Result<(), AttributeError> {
    if let Some(min) = prop.get("minLength").and_then(Value::as_u64) {
        if (value.chars().count() as u64) < min {
            return Err(AttributeError::TooShort {
                property: name.to_string(),
                min,
            });
        }
    }
    if let Some(max) = prop.get("maxLength").and_then(Value::as_u64) {
        if (value.chars().count() as u64) > max {
            return Err(AttributeError::TooLong {
                property: name.to_string(),
                max,
            });
        }
    }
    if let Some(pat) = prop.get("pattern").and_then(Value::as_str) {
        let re = Regex::new(pat).map_err(|_| AttributeError::PatternMismatch {
            property: name.to_string(),
        })?;
        if !re.is_match(value) {
            return Err(AttributeError::PatternMismatch {
                property: name.to_string(),
            });
        }
    }
    if let Some(format) = prop.get("format").and_then(Value::as_str) {
        let ok = match format {
            "date" => chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d").is_ok(),
            "date-time" => chrono::DateTime::parse_from_rfc3339(value).is_ok(),
            "email" => is_email(value),
            "uri" => url::Url::parse(value).is_ok(),
            _ => true,
        };
        if !ok {
            return Err(AttributeError::BadFormat {
                property: name.to_string(),
                format: format.to_string(),
            });
        }
    }
    Ok(())
}

fn check_number(name: &str, prop: &Value, value: f64) -> Result<(), AttributeError> {
    if let Some(min) = prop.get("minimum").and_then(Value::as_f64) {
        if value < min {
            return Err(AttributeError::BelowMinimum {
                property: name.to_string(),
                min,
            });
        }
    }
    if let Some(max) = prop.get("maximum").and_then(Value::as_f64) {
        if value > max {
            return Err(AttributeError::AboveMaximum {
                property: name.to_string(),
                max,
            });
        }
    }
    if let Some(step) = prop.get("multipleOf").and_then(Value::as_f64) {
        if step > 0.0 {
            let ratio = value / step;
            if (ratio - ratio.round()).abs() > 1e-9 {
                return Err(AttributeError::NotMultipleOf {
                    property: name.to_string(),
                    step,
                });
            }
        }
    }
    Ok(())
}

fn json_kind(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn is_email(s: &str) -> bool {
    let bytes = s.as_bytes();
    let at = match s.find('@') {
        Some(idx) => idx,
        None => return false,
    };
    if at == 0 || at == bytes.len() - 1 {
        return false;
    }
    let (_local, domain) = s.split_at(at + 1);
    domain.contains('.') && !domain.starts_with('.') && !domain.ends_with('.')
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn schema_with(props: Value, required: Option<Vec<&str>>) -> Value {
        let mut s = json!({ "type": "object", "properties": props });
        if let Some(req) = required {
            s["required"] = json!(req);
        }
        s
    }

    #[test]
    fn empty_schema_accepts_empty_attributes() {
        let s = schema_with(json!({}), None);
        validate_schema(&s).unwrap();
        validate_attributes(&s, &json!({})).unwrap();
    }

    #[test]
    fn rejects_unknown_keyword_on_property() {
        let s = schema_with(json!({"x": {"type": "string", "weird": 1}}), None);
        assert!(matches!(
            validate_schema(&s),
            Err(AttributeSchemaError::UnsupportedKeyword { .. })
        ));
    }

    #[test]
    fn rejects_unsupported_type() {
        let s = schema_with(json!({"x": {"type": "null"}}), None);
        assert!(matches!(
            validate_schema(&s),
            Err(AttributeSchemaError::UnsupportedType { .. })
        ));
    }

    #[test]
    fn rejects_required_unknown() {
        let s = schema_with(json!({"x": {"type": "string"}}), Some(vec!["y"]));
        assert!(matches!(
            validate_schema(&s),
            Err(AttributeSchemaError::RequiredUnknown(_))
        ));
    }

    #[test]
    fn rejects_pattern_with_bad_regex() {
        let s = schema_with(json!({"x": {"type": "string", "pattern": "["}}), None);
        assert!(matches!(
            validate_schema(&s),
            Err(AttributeSchemaError::InvalidPattern { .. })
        ));
    }

    #[test]
    fn validates_required_missing() {
        let s = schema_with(json!({"x": {"type": "string"}}), Some(vec!["x"]));
        validate_schema(&s).unwrap();
        assert!(matches!(
            validate_attributes(&s, &json!({})),
            Err(AttributeError::MissingRequired(_))
        ));
    }

    #[test]
    fn rejects_unknown_attribute() {
        let s = schema_with(json!({"x": {"type": "string"}}), None);
        assert!(matches!(
            validate_attributes(&s, &json!({"y": "nope"})),
            Err(AttributeError::UnknownProperty(_))
        ));
    }

    #[test]
    fn enforces_type() {
        let s = schema_with(json!({"x": {"type": "integer"}}), None);
        assert!(matches!(
            validate_attributes(&s, &json!({"x": "five"})),
            Err(AttributeError::WrongType { .. })
        ));
        validate_attributes(&s, &json!({"x": 5})).unwrap();
    }

    #[test]
    fn enforces_enum() {
        let s = schema_with(json!({"x": {"type": "string", "enum": ["a", "b"]}}), None);
        assert!(matches!(
            validate_attributes(&s, &json!({"x": "c"})),
            Err(AttributeError::NotInEnum { .. })
        ));
        validate_attributes(&s, &json!({"x": "a"})).unwrap();
    }

    #[test]
    fn enforces_string_length_and_pattern() {
        let s = schema_with(
            json!({"x": {"type": "string", "minLength": 2, "maxLength": 5, "pattern": "^[a-z]+$"}}),
            None,
        );
        assert!(matches!(
            validate_attributes(&s, &json!({"x": "a"})),
            Err(AttributeError::TooShort { .. })
        ));
        assert!(matches!(
            validate_attributes(&s, &json!({"x": "abcdef"})),
            Err(AttributeError::TooLong { .. })
        ));
        assert!(matches!(
            validate_attributes(&s, &json!({"x": "AB"})),
            Err(AttributeError::PatternMismatch { .. })
        ));
        validate_attributes(&s, &json!({"x": "abc"})).unwrap();
    }

    #[test]
    fn enforces_number_bounds_and_step() {
        let s = schema_with(
            json!({"x": {"type": "number", "minimum": 1, "maximum": 10, "multipleOf": 0.5}}),
            None,
        );
        assert!(matches!(
            validate_attributes(&s, &json!({"x": 0.5})),
            Err(AttributeError::BelowMinimum { .. })
        ));
        assert!(matches!(
            validate_attributes(&s, &json!({"x": 11})),
            Err(AttributeError::AboveMaximum { .. })
        ));
        assert!(matches!(
            validate_attributes(&s, &json!({"x": 1.3})),
            Err(AttributeError::NotMultipleOf { .. })
        ));
        validate_attributes(&s, &json!({"x": 2.5})).unwrap();
    }

    #[test]
    fn enforces_format() {
        let s = schema_with(json!({"x": {"type": "string", "format": "date"}}), None);
        assert!(matches!(
            validate_attributes(&s, &json!({"x": "not-a-date"})),
            Err(AttributeError::BadFormat { .. })
        ));
        validate_attributes(&s, &json!({"x": "2025-01-15"})).unwrap();
    }

    #[test]
    fn validates_array_items() {
        let s = schema_with(
            json!({"tags": {"type": "array", "items": {"type": "string"}}}),
            None,
        );
        validate_attributes(&s, &json!({"tags": ["a", "b"]})).unwrap();
        assert!(matches!(
            validate_attributes(&s, &json!({"tags": ["a", 1]})),
            Err(AttributeError::WrongType { .. })
        ));
    }
}
