//! Custom-field schema validator: a constrained JSON-Schema subset for
//! admin-defined fields, shared by any entity that carries configurable
//! attributes (assets, users). The full JSON Schema spec is far more than a
//! form-builder needs; this subset covers the field-builder cases and rejects
//! everything else at schema-validation time.
//!
//! Accepted shape:
//! - root MUST be `{ "type": "object", "properties": { ... }, "required"?: [...] }`
//! - each property MUST be string | number | integer | boolean | array (with
//!   `items` of a primitive type)
//! - per-property constraints: `enum`, `minLength`, `maxLength`, `pattern`,
//!   `format` (date | date-time | email | uri | user-uuid | asset-ref),
//!   `minimum`, `maximum`, `multipleOf`
//! - Nosdesk extension keywords (syntactic only): `assetKind` (scopes an
//!   `asset-ref` picker), `synced` (bool; the field is fed read-only by a
//!   directory/asset sync rather than typed by a human)
//!
//! The same validator runs at two boundaries: when a schema is created/updated
//! (validate the schema) and when a row is written (validate its values).

use dashmap::DashMap;
use once_cell::sync::Lazy;
use regex::Regex;
use serde_json::Value;

/// Compiled-regex cache keyed by the schema's `pattern` source.
/// Attribute schemas are stable across many writes (every save
/// of an asset of the same kind re-validates against the same
/// patterns), so re-compiling on each request burns measurable
/// CPU. Bounded only by the set of distinct patterns admins
/// define across kinds, which is small. Insertion is rare,
/// reads dominate, so DashMap's sharded read path is the right
/// fit.
///
/// A pattern that fails to compile is intentionally NOT cached:
/// we want every call to surface the compilation error to the
/// caller and we don't want a one-off bad write to poison the
/// cache for later writes that fix the schema.
static PATTERN_CACHE: Lazy<DashMap<String, Regex>> = Lazy::new(DashMap::new);

fn compile_pattern(pat: &str) -> Result<Regex, regex::Error> {
    if let Some(re) = PATTERN_CACHE.get(pat) {
        return Ok(re.clone());
    }
    let re = Regex::new(pat)?;
    PATTERN_CACHE.insert(pat.to_string(), re.clone());
    Ok(re)
}

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
                // Compile-once and seed the runtime cache so the
                // first asset write of this kind doesn't pay the
                // compilation cost. compile_pattern handles its
                // own caching internally.
                compile_pattern(pat).map_err(|err| AttributeSchemaError::InvalidPattern {
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
                // `user-uuid` and `asset-ref` are Nosdesk extensions
                // to the JSON Schema vocabulary, both operating on
                // string-typed values: a stored UUID for users and a
                // stringified integer asset id for asset references.
                // Storing asset refs as strings avoids the "what type
                // is the FK?" question at the JSONB layer and lines
                // up with how user-uuid is stored.
                if !matches!(
                    f,
                    "date" | "date-time" | "email" | "uri" | "user-uuid" | "asset-ref"
                ) {
                    return Err(AttributeSchemaError::UnsupportedFormat {
                        property: name.to_string(),
                        format: f.to_string(),
                    });
                }
            }
            "assetKind" => {
                // Optional scope hint for `format: "asset-ref"`
                // properties. Tells the data-entry picker which kind
                // to filter on. The validator itself doesn't enforce
                // the kind matches at write time (saves a DB lookup
                // per attribute write); the picker only offers valid
                // targets, and stale references behave like any other
                // dangling FK in the absence of a real foreign key.
                if !value.is_string() {
                    return Err(AttributeSchemaError::UnsupportedKeyword {
                        property: name.to_string(),
                        keyword: keyword.clone(),
                    });
                }
            }
            "synced" => {
                // Nosdesk extension: this field is populated read-only by a
                // directory/asset sync (e.g. Entra `officeLocation`) rather
                // than typed by a human. Syntactic only; the sync + UI honour
                // it. Must be a boolean.
                if !value.is_boolean() {
                    return Err(AttributeSchemaError::UnsupportedKeyword {
                        property: name.to_string(),
                        keyword: keyword.clone(),
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
        let re = compile_pattern(pat).map_err(|_| AttributeError::PatternMismatch {
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
            "uri" => is_web_uri(value),
            // Syntactic checks only: confirm the value looks like
            // a UUID / a positive integer id. Existence is not
            // verified against `users` / `assets` here, so deleting
            // a referenced target leaves a stale reference rather
            // than blocking the delete. The picker on the data-
            // entry side only offers valid targets, so stale refs
            // only arise from out-of-band edits; we treat them as
            // the same shape of problem as any dangling FK.
            "user-uuid" => uuid::Uuid::parse_str(value).is_ok(),
            "asset-ref" => value.parse::<i32>().ok().filter(|n| *n > 0).is_some(),
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

/// Permissive email validator. Not RFC 5322 complete — that
/// surface is too sharp for an attribute-schema validator — but
/// catches the obvious shapes of bad input that the v1 check
/// allowed through: whitespace anywhere, empty local part,
/// consecutive dots, leading/trailing dots in either side of
/// the @, and a domain without at least one dot.
fn is_email(s: &str) -> bool {
    if s.is_empty() || s.chars().any(char::is_whitespace) {
        return false;
    }
    // Exactly one '@'. Multi-@ inputs (foo@bar@baz) are
    // unconditionally rejected.
    let at = match s.find('@') {
        Some(idx) if !s[idx + 1..].contains('@') => idx,
        _ => return false,
    };
    let (local, rest) = s.split_at(at);
    let domain = &rest[1..];
    if local.is_empty() || domain.is_empty() {
        return false;
    }
    if local.starts_with('.') || local.ends_with('.') || local.contains("..") {
        return false;
    }
    // Domain must have at least one dot and no empty labels.
    if !domain.contains('.') || domain.starts_with('.') || domain.ends_with('.') {
        return false;
    }
    if domain.contains("..") {
        return false;
    }
    true
}

/// Tighter `format: "uri"` validator. JSON Schema's `uri` is
/// RFC 3986 (mailto:, javascript:, data:, file:, ... all valid)
/// but for the asset-attribute use case we only want web URLs:
/// admins typing "vendor portal URL" or "documentation link"
/// always mean http or https. Anything else is rejected at the
/// validator boundary so the frontend can render the value as
/// a link without an XSS audit on the rendering side.
fn is_web_uri(s: &str) -> bool {
    match url::Url::parse(s) {
        Ok(u) => {
            matches!(u.scheme(), "http" | "https")
                && u.host_str().map(|h| !h.is_empty()).unwrap_or(false)
        }
        Err(_) => false,
    }
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
    fn email_format_tightened() {
        // Bare local-part / domain shapes that the v1 validator
        // accidentally accepted: missing TLD, leading/trailing
        // dots, consecutive dots, multi-@, whitespace.
        assert!(!is_email(""));
        assert!(!is_email("foo"));
        assert!(!is_email("foo@bar")); // no dot in domain
        assert!(!is_email("@example.com")); // empty local
        assert!(!is_email("foo@")); // empty domain
        assert!(!is_email(".foo@example.com")); // local leads with .
        assert!(!is_email("foo.@example.com")); // local ends with .
        assert!(!is_email("a..b@example.com")); // consecutive dots
        assert!(!is_email("foo@example..com")); // consecutive dots in domain
        assert!(!is_email("foo bar@example.com")); // whitespace
        assert!(!is_email("foo@bar@example.com")); // multi-@

        assert!(is_email("foo@example.com"));
        assert!(is_email("foo.bar@example.co.uk"));
    }

    #[test]
    fn web_uri_format_rejects_non_http() {
        // The tightened validator rejects mailto, javascript,
        // data, file, etc.; the v1 validator (url::Url::parse)
        // accepted all of these.
        assert!(!is_web_uri(""));
        assert!(!is_web_uri("not a url"));
        assert!(!is_web_uri("mailto:foo@example.com"));
        assert!(!is_web_uri("javascript:alert(1)"));
        assert!(!is_web_uri("data:text/html,<script>"));
        assert!(!is_web_uri("file:///etc/passwd"));
        assert!(!is_web_uri("ftp://example.com/file"));
        // http/https with a host are the only acceptable shapes.
        assert!(is_web_uri("http://example.com"));
        assert!(is_web_uri("https://example.com/path?q=1"));
    }

    #[test]
    fn pattern_cache_returns_consistent_results() {
        // Compiling the same pattern twice should return the same
        // regex from cache, and both must match the same inputs.
        let pat = r"^[A-Z]{2}\d{4}$";
        let r1 = compile_pattern(pat).unwrap();
        let r2 = compile_pattern(pat).unwrap();
        assert_eq!(r1.is_match("AB1234"), r2.is_match("AB1234"));
        assert!(r1.is_match("XY9999"));
        assert!(!r1.is_match("ab1234"));
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

    #[test]
    fn user_uuid_format_accepts_valid_uuid_and_rejects_garbage() {
        let s = schema_with(
            json!({"owner": {"type": "string", "format": "user-uuid"}}),
            None,
        );
        validate_attributes(
            &s,
            &json!({"owner": "019dcf49-a30e-7af2-9ede-78a96a4b6832"}),
        )
        .unwrap();
        assert!(matches!(
            validate_attributes(&s, &json!({"owner": "not-a-uuid"})),
            Err(AttributeError::BadFormat { .. })
        ));
        assert!(matches!(
            validate_attributes(&s, &json!({"owner": ""})),
            Err(AttributeError::BadFormat { .. })
        ));
    }

    #[test]
    fn asset_ref_format_requires_positive_integer_string() {
        let s = schema_with(
            json!({"docks_to": {"type": "string", "format": "asset-ref"}}),
            None,
        );
        validate_attributes(&s, &json!({"docks_to": "42"})).unwrap();
        assert!(matches!(
            validate_attributes(&s, &json!({"docks_to": "0"})),
            Err(AttributeError::BadFormat { .. })
        ));
        assert!(matches!(
            validate_attributes(&s, &json!({"docks_to": "-5"})),
            Err(AttributeError::BadFormat { .. })
        ));
        assert!(matches!(
            validate_attributes(&s, &json!({"docks_to": "not-an-int"})),
            Err(AttributeError::BadFormat { .. })
        ));
    }

    #[test]
    fn asset_ref_schema_accepts_asset_kind_scope_hint() {
        // The `assetKind` keyword scopes the picker on the data
        // entry side. The validator allows it next to format=asset
        // -ref without enforcing the target exists or matches.
        validate_schema(&json!({
            "type": "object",
            "properties": {
                "docks_to": {
                    "type": "string",
                    "format": "asset-ref",
                    "assetKind": "monitor"
                }
            }
        }))
        .unwrap();
    }

    #[test]
    fn user_uuid_schema_round_trips_validate_schema() {
        validate_schema(&json!({
            "type": "object",
            "properties": {
                "primary_user": {
                    "type": "string",
                    "format": "user-uuid"
                }
            }
        }))
        .unwrap();
    }

    #[test]
    fn asset_kind_keyword_rejects_non_string_value() {
        assert!(matches!(
            validate_schema(&json!({
                "type": "object",
                "properties": {
                    "docks_to": {
                        "type": "string",
                        "format": "asset-ref",
                        "assetKind": 42
                    }
                }
            })),
            Err(AttributeSchemaError::UnsupportedKeyword { .. })
        ));
    }
}
