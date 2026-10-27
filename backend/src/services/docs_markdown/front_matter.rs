//! Front matter and small config files, read without trusting them.
//!
//! YAML goes through an event parser (`saphyr-parser`), so the limits apply
//! before any value is built: anchors and aliases are refused (an alias is
//! how a few hundred bytes of YAML expand into gigabytes), nesting and the
//! number of values are bounded, and tags are ignored rather than resolved
//! (`mkdocs.yml` carries `!!python/name:` tags). `+++` blocks are read as
//! TOML.
//!
//! A page's front matter is held to the design's shape: a map whose values
//! are scalars, lists of scalars or flat maps of scalars. A key whose value
//! is deeper than that is kept as [`Value::Unsupported`], so it is reported
//! as not imported rather than failing the whole block.

use std::collections::BTreeMap;

use saphyr_parser::{Event, Parser, ScalarStyle};

use super::{FrontMatter, FrontMatterKind};

/// Largest front matter block read, in bytes.
pub const MAX_FRONT_MATTER_BYTES: usize = 16 * 1024;

/// Largest config file read (`mkdocs.yml`, `_category_.yml`), in bytes.
pub const MAX_CONFIG_BYTES: usize = 256 * 1024;

/// A value read from YAML or TOML.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    List(Vec<Value>),
    /// Keys in source order. Non-string keys are written as text.
    Map(Vec<(String, Value)>),
    /// A value nested deeper than front matter allows.
    Unsupported,
}

impl Value {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s),
            _ => None,
        }
    }

    /// A scalar as text: strings as they are, numbers and booleans as
    /// written.
    pub fn to_text(&self) -> Option<String> {
        match self {
            Value::String(s) => Some(s.clone()),
            Value::Int(n) => Some(n.to_string()),
            Value::Float(f) => Some(f.to_string()),
            Value::Bool(b) => Some(b.to_string()),
            _ => None,
        }
    }

    /// A number, from a number or a string that holds one.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Int(n) => Some(*n as f64),
            Value::Float(f) if f.is_finite() => Some(*f),
            Value::String(s) => s.trim().parse::<f64>().ok().filter(|f| f.is_finite()),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            Value::String(s) => match s.trim().to_ascii_lowercase().as_str() {
                "true" | "yes" => Some(true),
                "false" | "no" => Some(false),
                _ => None,
            },
            _ => None,
        }
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Map(entries) => entries.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    /// Strings from a list, or from a single string.
    pub fn strings(&self) -> Vec<String> {
        match self {
            Value::List(items) => items.iter().filter_map(Value::to_text).collect(),
            other => other.to_text().into_iter().collect(),
        }
    }
}

/// Why a block or file wasn't read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadError {
    TooLarge,
    /// An anchor or alias.
    Anchors,
    TooDeep,
    TooManyValues,
    /// Not a map at the top level.
    NotAMap,
    Invalid(String),
}

impl std::fmt::Display for ReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReadError::TooLarge => f.write_str("too large"),
            ReadError::Anchors => f.write_str("uses anchors or aliases"),
            ReadError::TooDeep => f.write_str("nested too deeply"),
            ReadError::TooManyValues => f.write_str("holds too many values"),
            ReadError::NotAMap => f.write_str("is not a map of keys"),
            ReadError::Invalid(detail) => write!(f, "can't be read: {detail}"),
        }
    }
}

/// Bounds on one YAML document.
#[derive(Debug, Clone, Copy)]
pub struct YamlLimits {
    pub max_bytes: usize,
    /// Lists and maps inside one another.
    pub max_depth: usize,
    pub max_values: usize,
}

impl YamlLimits {
    /// A page's front matter. The depth is bounded loosely here; the shape
    /// check after it marks anything deeper than a flat map unsupported.
    pub const FRONT_MATTER: YamlLimits = YamlLimits {
        max_bytes: MAX_FRONT_MATTER_BYTES,
        max_depth: 8,
        max_values: 2_000,
    };

    /// A config file: MkDocs `nav` nests lists of maps several levels deep.
    pub const CONFIG: YamlLimits = YamlLimits {
        max_bytes: MAX_CONFIG_BYTES,
        max_depth: 32,
        max_values: 20_000,
    };
}

/// The first YAML document in `text`, as a value. An empty document is
/// [`Value::Null`].
pub fn parse_yaml(text: &str, limits: YamlLimits) -> Result<Value, ReadError> {
    if text.len() > limits.max_bytes {
        return Err(ReadError::TooLarge);
    }
    let mut builder = YamlBuilder {
        stack: Vec::new(),
        root: None,
        values: 0,
        limits,
    };
    for item in Parser::new_from_str(text) {
        let (event, _) = item.map_err(|e| ReadError::Invalid(e.to_string()))?;
        if builder.event(event)? {
            break;
        }
    }
    Ok(builder.root.unwrap_or(Value::Null))
}

enum Frame {
    List(Vec<Value>),
    /// The entries so far, and a key waiting for its value.
    Map(Vec<(String, Value)>, Option<String>),
}

struct YamlBuilder {
    stack: Vec<Frame>,
    root: Option<Value>,
    values: usize,
    limits: YamlLimits,
}

impl YamlBuilder {
    /// Feed one event. Returns true once the first document is complete.
    fn event(&mut self, event: Event<'_>) -> Result<bool, ReadError> {
        match event {
            Event::Alias(_) => return Err(ReadError::Anchors),
            Event::Scalar(value, style, anchor, _tag) => {
                if anchor != 0 {
                    return Err(ReadError::Anchors);
                }
                self.count()?;
                self.push(scalar(&value, style));
            }
            Event::SequenceStart(anchor, _) | Event::MappingStart(anchor, _) => {
                if anchor != 0 {
                    return Err(ReadError::Anchors);
                }
                self.count()?;
                if self.stack.len() >= self.limits.max_depth {
                    return Err(ReadError::TooDeep);
                }
                self.stack
                    .push(if matches!(event, Event::SequenceStart(..)) {
                        Frame::List(Vec::new())
                    } else {
                        Frame::Map(Vec::new(), None)
                    });
            }
            Event::SequenceEnd | Event::MappingEnd => {
                let value = match self.stack.pop() {
                    Some(Frame::List(items)) => Value::List(items),
                    Some(Frame::Map(entries, _)) => Value::Map(entries),
                    None => return Err(ReadError::Invalid("unbalanced".into())),
                };
                self.push(value);
            }
            Event::DocumentEnd => return Ok(true),
            Event::StreamEnd => return Ok(true),
            _ => {}
        }
        Ok(false)
    }

    fn count(&mut self) -> Result<(), ReadError> {
        self.values += 1;
        if self.values > self.limits.max_values {
            return Err(ReadError::TooManyValues);
        }
        Ok(())
    }

    fn push(&mut self, value: Value) {
        match self.stack.last_mut() {
            None => {
                if self.root.is_none() {
                    self.root = Some(value);
                }
            }
            Some(Frame::List(items)) => items.push(value),
            Some(Frame::Map(entries, pending)) => match pending.take() {
                Some(key) => entries.push((key, value)),
                None => *pending = Some(key_text(&value)),
            },
        }
    }
}

fn key_text(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::List(_) | Value::Map(_) | Value::Unsupported => String::new(),
        other => other.to_text().unwrap_or_default(),
    }
}

/// A scalar's value under the YAML core schema. Quoted and block scalars are
/// strings; plain ones may be null, booleans or numbers.
fn scalar(text: &str, style: ScalarStyle) -> Value {
    if style != ScalarStyle::Plain {
        return Value::String(text.to_string());
    }
    match text {
        "" | "~" | "null" | "Null" | "NULL" => return Value::Null,
        "true" | "True" | "TRUE" => return Value::Bool(true),
        "false" | "False" | "FALSE" => return Value::Bool(false),
        _ => {}
    }
    let digits = text.strip_prefix(['-', '+']).unwrap_or(text);
    if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) {
        if let Ok(n) = text.parse::<i64>() {
            return Value::Int(n);
        }
    }
    let looks_numeric = digits
        .bytes()
        .next()
        .is_some_and(|b| b.is_ascii_digit() || b == b'.')
        && digits
            .bytes()
            .all(|b| b.is_ascii_digit() || matches!(b, b'.' | b'e' | b'E' | b'-' | b'+'));
    if looks_numeric {
        if let Ok(f) = text.parse::<f64>() {
            if f.is_finite() {
                return Value::Float(f);
            }
        }
    }
    Value::String(text.to_string())
}

/// A page's front matter, read and held to its shape.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Fields {
    /// Top-level keys in source order.
    pub entries: Vec<(String, Value)>,
}

impl Fields {
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.entries.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    /// A non-empty trimmed string value.
    pub fn text(&self, key: &str) -> Option<String> {
        self.get(key)
            .and_then(Value::to_text)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    }

    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().map(|(k, _)| k.as_str())
    }
}

/// Read a page's front matter block: YAML or `+++` TOML, 16 KB at most, a
/// map at the top, values deeper than a flat map marked unsupported.
pub fn read(front_matter: &FrontMatter) -> Result<Fields, ReadError> {
    if front_matter.raw.len() > MAX_FRONT_MATTER_BYTES {
        return Err(ReadError::TooLarge);
    }
    let value = match front_matter.kind {
        FrontMatterKind::Yaml => parse_yaml(&front_matter.raw, YamlLimits::FRONT_MATTER)?,
        FrontMatterKind::Pluses => parse_toml(&front_matter.raw)?,
    };
    let entries = match value {
        Value::Map(entries) => entries,
        Value::Null => Vec::new(),
        _ => return Err(ReadError::NotAMap),
    };
    Ok(Fields {
        entries: entries
            .into_iter()
            .map(|(k, v)| (k, restrict(v, 0)))
            .collect(),
    })
}

/// Scalars anywhere; lists and maps of scalars one level down; anything
/// deeper unsupported.
fn restrict(value: Value, depth: usize) -> Value {
    match value {
        Value::List(items) if depth == 0 => {
            Value::List(items.into_iter().map(|v| restrict(v, depth + 1)).collect())
        }
        Value::Map(entries) if depth == 0 => Value::Map(
            entries
                .into_iter()
                .map(|(k, v)| (k, restrict(v, depth + 1)))
                .collect(),
        ),
        Value::List(_) | Value::Map(_) => Value::Unsupported,
        scalar => scalar,
    }
}

/// A `+++` block as TOML.
fn parse_toml(text: &str) -> Result<Value, ReadError> {
    let table: toml::Table = text
        .parse()
        .map_err(|e: toml::de::Error| ReadError::Invalid(e.message().to_string()))?;
    Ok(toml_value(toml::Value::Table(table), 0))
}

fn toml_value(value: toml::Value, depth: usize) -> Value {
    if depth > YamlLimits::FRONT_MATTER.max_depth {
        return Value::Unsupported;
    }
    match value {
        toml::Value::String(s) => Value::String(s),
        toml::Value::Integer(n) => Value::Int(n),
        toml::Value::Float(f) => Value::Float(f),
        toml::Value::Boolean(b) => Value::Bool(b),
        toml::Value::Datetime(d) => Value::String(d.to_string()),
        toml::Value::Array(items) => Value::List(
            items
                .into_iter()
                .map(|v| toml_value(v, depth + 1))
                .collect(),
        ),
        toml::Value::Table(table) => Value::Map(
            table
                .into_iter()
                .map(|(k, v)| (k, toml_value(v, depth + 1)))
                .collect(),
        ),
    }
}

/// Counts of front matter keys not imported, by key, for Review.
pub type NotImported = BTreeMap<String, u32>;

#[cfg(test)]
mod tests {
    use super::*;

    fn yaml(raw: &str) -> Result<Fields, ReadError> {
        read(&FrontMatter {
            kind: FrontMatterKind::Yaml,
            raw: raw.to_string(),
        })
    }

    #[test]
    fn scalars_lists_and_flat_maps_are_read() {
        let fields = yaml(
            "title: VPN setup\nweight: 3\ndraft: true\nratio: 1.5\nempty:\n\
             aliases: [VPN, \"Remote access\"]\nauthor:\n  name: Sam\n  team: IT\n\
             quoted: \"true\"\n",
        )
        .unwrap();
        assert_eq!(fields.text("title").as_deref(), Some("VPN setup"));
        assert_eq!(fields.get("weight"), Some(&Value::Int(3)));
        assert_eq!(fields.get("draft"), Some(&Value::Bool(true)));
        assert_eq!(fields.get("ratio"), Some(&Value::Float(1.5)));
        assert_eq!(fields.get("empty"), Some(&Value::Null));
        assert_eq!(
            fields.get("aliases").map(Value::strings),
            Some(vec!["VPN".to_string(), "Remote access".to_string()])
        );
        assert_eq!(
            fields.get("author").and_then(|a| a.get("team")),
            Some(&Value::String("IT".into()))
        );
        assert_eq!(fields.get("quoted"), Some(&Value::String("true".into())));
    }

    #[test]
    fn anchors_and_aliases_are_refused() {
        assert_eq!(
            yaml("base: &b x\nother: *b\n"),
            Err(ReadError::Anchors),
            "an alias"
        );
        assert_eq!(yaml("a: &only 1\n"), Err(ReadError::Anchors), "an anchor");
        assert_eq!(
            yaml("a: &m {x: 1}\nb:\n  <<: *m\n"),
            Err(ReadError::Anchors),
            "a merge key"
        );
    }

    #[test]
    fn a_block_over_16_kb_is_refused() {
        let raw = format!(
            "title: x\nbody: \"{}\"\n",
            "a".repeat(MAX_FRONT_MATTER_BYTES)
        );
        assert_eq!(yaml(&raw), Err(ReadError::TooLarge));
    }

    #[test]
    fn nesting_past_a_flat_map_is_unsupported_and_the_rest_is_kept() {
        let fields = yaml("title: Kept\ndeep:\n  a:\n    b: 1\nlist: [[1, 2]]\n").unwrap();
        assert_eq!(fields.text("title").as_deref(), Some("Kept"));
        assert_eq!(
            fields.get("deep"),
            Some(&Value::Map(vec![("a".into(), Value::Unsupported)]))
        );
        assert_eq!(
            fields.get("list"),
            Some(&Value::List(vec![Value::Unsupported]))
        );
    }

    #[test]
    fn very_deep_yaml_is_refused_before_it_is_built() {
        let raw = format!("a: {}{}\n", "[".repeat(200), "]".repeat(200));
        assert_eq!(yaml(&raw), Err(ReadError::TooDeep));
    }

    #[test]
    fn a_list_at_the_top_is_not_front_matter() {
        assert_eq!(yaml("- a\n- b\n"), Err(ReadError::NotAMap));
        assert_eq!(yaml(""), Ok(Fields::default()));
    }

    #[test]
    fn tags_are_ignored_and_invalid_yaml_is_an_error() {
        let config = parse_yaml(
            "markdown_extensions:\n  - pymdownx.emoji:\n      emoji_index: !!python/name:material.extensions.emoji.twemoji\n",
            YamlLimits::CONFIG,
        )
        .unwrap();
        assert!(matches!(
            config.get("markdown_extensions"),
            Some(Value::List(_))
        ));
        assert!(matches!(yaml("a: [unclosed\n"), Err(ReadError::Invalid(_))));
    }

    #[test]
    fn pluses_blocks_are_toml() {
        let fields = read(&FrontMatter {
            kind: FrontMatterKind::Pluses,
            raw: "title = \"Hugo page\"\nweight = 2\ndraft = false\ntags = [\"a\"]\n".into(),
        })
        .unwrap();
        assert_eq!(fields.text("title").as_deref(), Some("Hugo page"));
        assert_eq!(fields.get("weight"), Some(&Value::Int(2)));
        assert_eq!(fields.get("draft"), Some(&Value::Bool(false)));
    }
}
