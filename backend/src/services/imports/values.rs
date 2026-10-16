//! Match values: what each distinct value in a mapped column means.
//!
//! A field whose values link to a closed set (a kind, a role, a priority, a
//! workflow state, yes or no) or must read as a number or a date has a
//! [`Matcher`]. Every distinct value of such a column gets an answer
//! ([`ValueAnswer`]) before the importer sees the row: the admin's own,
//! stored on the job, or else the one matching suggests, in layers:
//!
//! 1. Exact: the choice's key or label, ignoring case and spacing, or a
//!    number or date that reads. No pill.
//! 2. Saved: the workspace's answer from an earlier import of the type
//!    (`import_value_answers`). Dropped when its target is gone.
//! 3. Guessed: a known other name for a choice, then a close spelling of a
//!    label five characters or longer (Jaro-Winkler at 0.88).
//! 4. No match: the field's default. A required field, a yes or no, and a
//!    value that doesn't read skip the row (the importer's own error says
//!    why); an optional choice is left blank.
//!
//! [`resolve`] works the answers out and [`apply`] writes them into the
//! mapped grid, so the dry run and the commit read the same rows. Answers
//! are keyed by field, context and the value as [`normalize_value`] gives
//! it. The context is empty for a field answered the same on every row. A
//! field the importer answers per row context (an asset attribute, per the
//! row's kind) has one column per context, each with its own matcher, and
//! takes the rows of that context only. Settling runs in two phases: the
//! fields without a context first (the kind among them), so each row's
//! context is read after its kind answer.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::str::FromStr;

use bigdecimal::BigDecimal;
use serde::Serialize;
use uuid::Uuid;

use crate::db::DbConnection;
use crate::repository::imports as repo;

use super::csv_parser::{ParsedCsv, MAX_ROWS};
use super::mapping::{ColumnMapping, MappedGrid, MappingError, Provenance, SourceHeader};
use super::types::{
    DateOrder, DecimalMark, FieldSpec, ImportOptions, ImportType, RowOutcome, RowResult,
    ValueAnswer, ValueAnswers, ValueType,
};

/// A value as answers are keyed by it: trimmed, lowercased, with runs of
/// spaces as one.
pub fn normalize_value(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// [`normalize_value`] with `_` and `-` read as spaces, for comparing a
/// value with a choice's key or label.
pub fn loose(value: &str) -> String {
    normalize_value(&value.replace(['_', '-'], " "))
}

/// One entry a value can link to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Choice {
    /// What the cell becomes: a kind slug, a role or priority key, a
    /// workflow state's name.
    pub value: String,
    pub label: String,
    /// Why the entry can't be picked here, when it can't.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disabled: Option<String>,
}

impl Choice {
    pub fn new(value: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            label: label.into(),
            disabled: None,
        }
    }
}

/// How a field's values are matched.
#[derive(Debug, Clone)]
pub enum Matcher {
    /// Values link to one of a closed set.
    Choice {
        choices: Vec<Choice>,
        /// Other spellings that count as exact, normalized, to a choice's
        /// value (`y` is yes).
        aliases: Vec<(String, String)>,
        /// Known other names, normalized, to a choice's value: Guessed.
        synonyms: Vec<(String, String)>,
        /// A value no choice matches doesn't read (yes or no): skip by
        /// default even on an optional field.
        strict: bool,
    },
    /// Values must read as a number with this decimal mark.
    Number(DecimalMark),
    /// Values must read as a date ([`parse_date`]) in this order.
    Date(DateOrder),
}

/// Spellings of yes, normalized.
const YES: &[&str] = &["yes", "y", "true", "1", "oui", "ja"];
/// Spellings of no, normalized.
const NO: &[&str] = &["no", "n", "false", "0", "non", "nee"];

/// A yes or no cell as a boolean field takes it.
pub fn read_boolean(raw: &str) -> Option<bool> {
    let key = normalize_value(raw);
    if YES.contains(&key.as_str()) {
        Some(true)
    } else if NO.contains(&key.as_str()) {
        Some(false)
    } else {
        None
    }
}

/// An enum field's options, each its own label.
pub fn enum_choices(field: &FieldSpec) -> Option<Vec<Choice>> {
    match (&field.value_type, &field.enum_values) {
        (ValueType::Enum, Some(values)) => Some(
            values
                .iter()
                .map(|v| Choice::new(v.clone(), v.clone()))
                .collect(),
        ),
        _ => None,
    }
}

/// Yes and no, as a boolean field's cells take them.
pub fn boolean_matcher() -> Matcher {
    let alias = |words: &[&str], value: &str| {
        words
            .iter()
            .map(|w| (w.to_string(), value.to_string()))
            .collect::<Vec<_>>()
    };
    let mut aliases = alias(YES, "true");
    aliases.extend(alias(NO, "false"));
    Matcher::Choice {
        choices: vec![Choice::new("true", "Yes"), Choice::new("false", "No")],
        aliases,
        synonyms: Vec::new(),
        strict: true,
    }
}

impl Matcher {
    fn choices(&self) -> &[Choice] {
        match self {
            Self::Choice { choices, .. } => choices,
            _ => &[],
        }
    }

    /// The choice `target` names, if it can be picked.
    fn pickable(&self, target: &serde_json::Value) -> Option<&Choice> {
        let target = target.as_str()?;
        self.choices()
            .iter()
            .find(|c| c.value == target && c.disabled.is_none())
    }
}

/// A number cell as the importers read it: `Ok(None)` when blank, the value
/// when it reads with `mark` and fits `numeric(12,3)`, else the reason.
///
/// A currency symbol or code before or after the number is passed over
/// (`$7,450.00`, `A$12`, `7 450 €`, `EUR 12`). The thousands separator is
/// the other mark (`,` for a `.` mark, `.` for a `,` mark) or a space, and
/// is taken only in groups of three (`1,234.56`, `1.234,56`, `1 234,56`).
/// Under a `,` mark, one `,` followed by three digits (`1,234`) could be
/// either and is refused rather than guessed.
pub fn parse_decimal(raw: &str, mark: DecimalMark) -> Result<Option<BigDecimal>, String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Ok(None);
    }
    let text = number_text(raw, mark)?;
    let value =
        BigDecimal::from_str(&text).map_err(|_| format!("'{raw}' is not a valid decimal"))?;
    let max = BigDecimal::from_str(MAX_DECIMAL).expect("constant parses");
    if value.abs() >= max {
        return Err(format!("'{raw}' is too large (at most 999999999.999)"));
    }
    Ok(Some(value))
}

/// Currency symbols passed over around a number.
const CURRENCY_SYMBOLS: &[char] = &['$', '€', '£', '¥', '₹', '₩', '₽', '₪', '₫', '¢'];

/// Spaces that group thousands: plain, no-break, narrow no-break, thin.
const GROUP_SPACES: &[char] = &[' ', '\u{a0}', '\u{202f}', '\u{2009}'];

/// `raw` without a leading or trailing currency symbol or code, and its
/// sign: `-$7.50` and `$-7.50` are both negative.
fn strip_currency(raw: &str) -> (bool, &str) {
    fn sign(text: &str) -> (bool, &str) {
        match text.strip_prefix('-') {
            Some(rest) => (true, rest.trim_start()),
            None => (false, text.strip_prefix('+').unwrap_or(text).trim_start()),
        }
    }
    let (mut negative, mut text) = sign(raw.trim());
    // A code of up to three capitals before a symbol (`A$`, `US$`), a
    // symbol alone, or a three-letter code (`USD 12`, `EUR12`).
    let letters = text.chars().take_while(char::is_ascii_uppercase).count();
    let after = &text[letters..];
    if letters <= 3 && after.starts_with(CURRENCY_SYMBOLS) {
        let symbol = after.chars().next().map_or(0, char::len_utf8);
        text = after[symbol..].trim_start();
    } else if letters == 3 {
        text = after.trim_start();
    }
    if !negative {
        (negative, text) = sign(text);
    }
    if let Some(rest) = text.strip_suffix(CURRENCY_SYMBOLS) {
        text = rest.trim_end();
    } else {
        let letters = text
            .chars()
            .rev()
            .take_while(char::is_ascii_uppercase)
            .count();
        if letters == 3 {
            text = text[..text.len() - 3].trim_end();
        }
    }
    (negative, text)
}

/// A number cell as plain decimal text (`-1234.56`), its currency and
/// thousands separators dropped, or why it doesn't read.
fn number_text(raw: &str, mark: DecimalMark) -> Result<String, String> {
    let (negative, body) = strip_currency(raw);
    let (decimal, group) = match mark {
        DecimalMark::Dot => ('.', ','),
        DecimalMark::Comma => (',', '.'),
    };
    let invalid = || {
        match mark {
        DecimalMark::Dot if raw.contains(',') && !raw.contains('.') => format!(
            "'{raw}' is not a valid decimal; if ',' is the decimal mark, choose ',' at the Map step"
        ),
        DecimalMark::Comma if raw.contains('.') => format!(
            "'{raw}' has a '.' that isn't a thousands separator, but the decimal mark is ','; if '.' is the decimal mark, choose '.' at the Map step"
        ),
        _ => format!("'{raw}' is not a valid decimal"),
    }
    };
    let digits = |s: &str| s.chars().all(|c| c.is_ascii_digit());
    let mut parts = body.split(decimal);
    let whole = parts.next().unwrap_or("");
    let fraction = parts.next();
    if parts.next().is_some() || !fraction.is_none_or(digits) {
        return Err(invalid());
    }
    let grouped = whole.contains(group) || whole.contains(GROUP_SPACES);
    let whole = if grouped {
        // One separator throughout, a lead group of one to three digits,
        // then groups of three.
        let separator = whole.chars().find(|c| !c.is_ascii_digit()).unwrap_or(group);
        if separator != group && !GROUP_SPACES.contains(&separator) {
            return Err(invalid());
        }
        let groups: Vec<&str> = whole.split(separator).collect();
        let lead = groups[0];
        let lead_ok = (1..=3).contains(&lead.len()) && digits(lead);
        if !lead_ok || groups[1..].iter().any(|g| g.len() != 3 || !digits(g)) {
            return Err(invalid());
        }
        groups.concat()
    } else {
        if !digits(whole) || (whole.is_empty() && fraction.is_none_or(str::is_empty)) {
            return Err(invalid());
        }
        whole.to_string()
    };
    let fraction = fraction.unwrap_or("");
    // `1,234` under a `,` mark: one and a bit, or a thousand and more.
    if mark == DecimalMark::Comma
        && !grouped
        && fraction.len() == 3
        && (1..=3).contains(&whole.len())
        && !whole.starts_with('0')
    {
        return Err(format!(
            "'{raw}' could be {whole}{fraction} or {whole}.{fraction}; write it with a thousands separator or more decimals, or check the decimal mark at the Map step"
        ));
    }
    let sign = if negative { "-" } else { "" };
    let whole = if whole.is_empty() {
        "0"
    } else {
        whole.as_str()
    };
    Ok(if fraction.is_empty() {
        format!("{sign}{whole}")
    } else {
        format!("{sign}{whole}.{fraction}")
    })
}

/// Largest magnitude `numeric(12,3)` holds once rounded to 3 places.
const MAX_DECIMAL: &str = "999999999.9995";

/// A date cell as the importers read it: year first (`2024-03-14`, also
/// with `/` or `.`) in any order, or day and month in `order` before a
/// four-digit year (`14/03/2024`). A time after the date, as a workbook
/// writes a date-time, is passed over.
pub fn parse_date(raw: &str, order: DateOrder) -> Option<chrono::NaiveDate> {
    let raw = raw.trim();
    let (date, time) = match raw.split_once(['T', ' ']) {
        Some((date, time)) => (date, Some(time.trim())),
        None => (raw, None),
    };
    if time.is_some_and(|t| !t.starts_with(|c: char| c.is_ascii_digit()) || !t.contains(':')) {
        return None;
    }
    let parts: Vec<&str> = date.split(['-', '/', '.']).collect();
    let [a, b, c] = parts.as_slice() else {
        return None;
    };
    if !parts
        .iter()
        .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
    {
        return None;
    }
    let (year, month, day) = if a.len() == 4 {
        (*a, *b, *c)
    } else if c.len() == 4 {
        match order {
            DateOrder::Dmy => (*c, *b, *a),
            DateOrder::Mdy => (*c, *a, *b),
            DateOrder::Iso => return None,
        }
    } else {
        return None;
    };
    if month.len() > 2 || day.len() > 2 {
        return None;
    }
    chrono::NaiveDate::from_ymd_opt(year.parse().ok()?, month.parse().ok()?, day.parse().ok()?)
}

/// How a date in `order` is written, for an error message.
pub fn date_hint(order: DateOrder) -> &'static str {
    match order {
        DateOrder::Iso => "YYYY-MM-DD",
        DateOrder::Dmy => "DD/MM/YYYY",
        DateOrder::Mdy => "MM/DD/YYYY",
    }
}

/// Which layer of matching settled a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Layer {
    Exact,
    Saved,
    Guessed,
    NoMatch,
}

/// Why a value was guessed, or why it has no match.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum Reason {
    /// A known other name for the choice.
    Synonym,
    /// Spelled close to this label.
    CloseTo { label: String },
    /// Names a choice that can't be picked here.
    Disabled { label: String, note: String },
    /// The cell doesn't read as the field's type.
    Unreadable,
}

/// A saved answer and where it came from.
#[derive(Debug, Clone)]
pub struct SavedAnswer {
    pub answer: ValueAnswer,
    pub provenance: Provenance,
}

/// The workspace's saved answers for one import type, by field, context
/// and normalized value.
#[derive(Debug, Clone, Default)]
pub struct Memory {
    entries: HashMap<(String, String, String), SavedAnswer>,
}

impl Memory {
    pub fn insert(&mut self, field: &str, context: &str, value: &str, saved: SavedAnswer) {
        self.entries.insert(
            (field.to_string(), context.to_string(), value.to_string()),
            saved,
        );
    }

    fn get(&self, field: &str, context: &str, value: &str) -> Option<&SavedAnswer> {
        self.entries
            .get(&(field.to_string(), context.to_string(), value.to_string()))
    }

    /// Every saved answer for `job_type`, with who gave it and when. An
    /// answer that no longer reads is left out.
    pub fn load(conn: &mut DbConnection, job_type: ImportType) -> diesel::QueryResult<Self> {
        let rows = repo::list_value_answers(conn, job_type.as_str())?;
        let mut names: HashMap<Uuid, Option<String>> = HashMap::new();
        let mut memory = Self::default();
        for row in rows {
            let Ok(answer) = serde_json::from_value::<ValueAnswer>(row.answer.clone()) else {
                continue;
            };
            let by_name = match row.answered_by {
                Some(uuid) => match names.get(&uuid) {
                    Some(name) => name.clone(),
                    None => {
                        let name = match crate::repository::users::get_user_by_uuid(&uuid, conn) {
                            Ok(user) => Some(user.name),
                            Err(diesel::result::Error::NotFound) => None,
                            Err(e) => return Err(e),
                        };
                        names.insert(uuid, name.clone());
                        name
                    }
                },
                None => None,
            };
            let provenance = Provenance {
                job_id: row.job_id.unwrap_or_default(),
                by: row.answered_by,
                by_name,
                at: row.updated_at,
            };
            memory.insert(
                &row.field,
                &row.context,
                &row.normalized_value,
                SavedAnswer { answer, provenance },
            );
        }
        Ok(memory)
    }
}

/// Every distinct value of one mapped column, with its answer.
#[derive(Debug, Clone)]
pub struct ResolvedColumn {
    pub field: String,
    /// The source column that feeds the field.
    pub index: usize,
    /// The row context the column answers for (a kind slug); empty when
    /// the field is answered the same on every row.
    pub context: String,
    /// The context's name, for the card title.
    pub context_label: Option<String>,
    pub value_type: ValueType,
    pub required: bool,
    pub matcher: Matcher,
    /// Blank cells are a value of their own (`key` empty): a blank kind.
    pub blank_is_value: bool,
    /// In order of first appearance in the file.
    pub values: Vec<ResolvedValue>,
}

#[derive(Debug, Clone)]
pub struct ResolvedValue {
    /// The value as answers key it ([`normalize_value`]); empty for blank.
    pub key: String,
    /// The first spelling in the file.
    pub text: String,
    /// The file lines holding it.
    pub rows: Vec<usize>,
    pub layer: Layer,
    pub reason: Option<Reason>,
    /// What matching suggests; `None` keeps the cell as it is.
    pub suggested: Option<ValueAnswer>,
    /// The admin's answer stored on the job, when it still fits.
    pub answer: Option<ValueAnswer>,
    pub saved: Option<Provenance>,
}

impl ResolvedValue {
    /// The answer the rows take.
    pub fn effective(&self) -> Option<&ValueAnswer> {
        self.answer.as_ref().or(self.suggested.as_ref())
    }
}

/// The answers a mapped grid takes, column by column.
#[derive(Debug, Clone, Default)]
pub struct Resolution {
    pub columns: Vec<ResolvedColumn>,
    /// Each row's context, by source row, when a column answers per
    /// context. A row without one takes no contextual answer.
    pub row_contexts: HashMap<usize, String>,
}

/// One context of a field answered per row: its key (a kind slug), its
/// name, and the field as rows of that context read it.
#[derive(Debug, Clone)]
pub struct ValueContext {
    pub key: String,
    pub label: String,
    pub spec: FieldSpec,
}

/// How one mapped column's values are matched, for one context.
#[derive(Debug, Clone)]
pub struct ColumnMatcher {
    pub spec: FieldSpec,
    /// The source column.
    pub index: usize,
    /// Empty for a field answered the same on every row.
    pub context: String,
    pub context_label: Option<String>,
    pub matcher: Matcher,
}

/// Whether blank cells of `field` are a value with an answer of their own:
/// a blank kind keeps an existing asset's kind and leaves a new one without,
/// so the admin can give new rows one.
fn blank_is_value(job_type: ImportType, field: &str, options: &ImportOptions) -> bool {
    job_type == ImportType::Assets && field == "kind" && options.default_kind.is_none()
}

/// The matcher a field's values take: the importer's choices, an enum's
/// options, yes or no, a number or a date; `None` when its values aren't
/// matched.
fn matcher_for(
    importer: &dyn super::Importer,
    conn: &mut DbConnection,
    spec: &FieldSpec,
    options: &ImportOptions,
) -> diesel::QueryResult<Option<Matcher>> {
    Ok(match importer.value_choices(conn, spec)? {
        Some(choices) => Some(Matcher::Choice {
            choices,
            aliases: Vec::new(),
            synonyms: importer
                .value_synonyms(&spec.id)
                .iter()
                .map(|(from, to)| (normalize_value(from), to.to_string()))
                .collect(),
            strict: false,
        }),
        None => match spec.value_type {
            ValueType::Boolean => Some(boolean_matcher()),
            ValueType::Number => Some(Matcher::Number(options.decimal_mark())),
            ValueType::Date => Some(Matcher::Date(options.date_order)),
            _ => None,
        },
    })
}

/// The fields of `job_type` with a matcher, among those `mapping` feeds,
/// in source column order: one per field, or one per context for a field
/// answered per row context.
pub fn matchers(
    conn: &mut DbConnection,
    job_type: ImportType,
    fields: &[FieldSpec],
    mapping: &ColumnMapping,
    options: &ImportOptions,
) -> diesel::QueryResult<Vec<ColumnMatcher>> {
    let importer = super::importer_for(job_type);
    let mut out = Vec::new();
    for spec in fields {
        let Some(&index) = mapping.get(&spec.id) else {
            continue;
        };
        match importer.value_contexts(conn, spec)? {
            Some(contexts) => {
                for context in contexts {
                    let Some(matcher) =
                        matcher_for(importer.as_ref(), conn, &context.spec, options)?
                    else {
                        continue;
                    };
                    out.push(ColumnMatcher {
                        spec: context.spec,
                        index,
                        context: context.key,
                        context_label: Some(context.label),
                        matcher,
                    });
                }
            }
            None => {
                if let Some(matcher) = matcher_for(importer.as_ref(), conn, spec, options)? {
                    out.push(ColumnMatcher {
                        spec: spec.clone(),
                        index,
                        context: String::new(),
                        context_label: None,
                        matcher,
                    });
                }
            }
        }
    }
    out.sort_by_key(|m| m.index);
    Ok(out)
}

/// What matching suggests for one non-blank value, in layers.
fn prematch(
    field: &str,
    context: &str,
    required: bool,
    matcher: &Matcher,
    key: &str,
    text: &str,
    memory: &Memory,
) -> (
    Layer,
    Option<Reason>,
    Option<ValueAnswer>,
    Option<Provenance>,
) {
    let skip = Some(ValueAnswer::Skip);
    let link = |value: &str| {
        Some(ValueAnswer::Match {
            target: serde_json::Value::String(value.to_string()),
        })
    };
    let saved = || {
        memory
            .get(field, context, key)
            .filter(|s| fits(matcher, required, key, &s.answer).is_ok())
            .filter(|s| !matches!(s.answer, ValueAnswer::Skip | ValueAnswer::Replace { .. }))
    };
    match matcher {
        Matcher::Number(mark) => match parse_decimal(text, *mark) {
            Ok(_) => (Layer::Exact, None, None, None),
            Err(_) => (Layer::NoMatch, Some(Reason::Unreadable), skip, None),
        },
        Matcher::Date(order) => match parse_date(text, *order) {
            Some(_) => (Layer::Exact, None, None, None),
            None => (Layer::NoMatch, Some(Reason::Unreadable), skip, None),
        },
        Matcher::Choice {
            choices,
            aliases,
            synonyms,
            strict,
        } => {
            let wanted = loose(key);
            let exact = choices
                .iter()
                .find(|c| loose(&c.value) == wanted)
                .or_else(|| choices.iter().find(|c| loose(&c.label) == wanted))
                .or_else(|| {
                    aliases
                        .iter()
                        .find(|(alias, _)| alias == key)
                        .and_then(|(_, value)| choices.iter().find(|c| &c.value == value))
                });
            let default = if required || *strict {
                skip
            } else {
                Some(ValueAnswer::LeaveBlank)
            };
            if let Some(choice) = exact {
                return match &choice.disabled {
                    None => (Layer::Exact, None, link(&choice.value), None),
                    Some(note) => (
                        Layer::NoMatch,
                        Some(Reason::Disabled {
                            label: choice.label.clone(),
                            note: note.clone(),
                        }),
                        default,
                        None,
                    ),
                };
            }
            if let Some(s) = saved() {
                return (
                    Layer::Saved,
                    None,
                    Some(s.answer.clone()),
                    Some(s.provenance.clone()),
                );
            }
            let pickable = |value: &str| {
                choices
                    .iter()
                    .find(|c| c.value == value && c.disabled.is_none())
            };
            if let Some(choice) = synonyms
                .iter()
                .find(|(from, _)| from == key)
                .and_then(|(_, to)| pickable(to))
            {
                return (
                    Layer::Guessed,
                    Some(Reason::Synonym),
                    link(&choice.value),
                    None,
                );
            }
            if let Some(choice) = closest(&wanted, choices) {
                return (
                    Layer::Guessed,
                    Some(Reason::CloseTo {
                        label: choice.label.clone(),
                    }),
                    link(&choice.value),
                    None,
                );
            }
            (
                Layer::NoMatch,
                (*strict).then_some(Reason::Unreadable),
                default,
                None,
            )
        }
    }
}

/// Least Jaro-Winkler similarity for a guess.
const FUZZY_THRESHOLD: f64 = 0.88;
/// Shortest label a close spelling is guessed for.
const FUZZY_MIN_CHARS: usize = 5;

/// The pickable choice whose label is spelled closest to `value`, when close
/// enough.
fn closest<'a>(value: &str, choices: &'a [Choice]) -> Option<&'a Choice> {
    choices
        .iter()
        .filter(|c| c.disabled.is_none())
        .filter_map(|c| {
            let label = loose(&c.label);
            (label.chars().count() >= FUZZY_MIN_CHARS).then(|| (jaro_winkler(value, &label), c))
        })
        .filter(|(score, _)| *score >= FUZZY_THRESHOLD)
        .max_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, c)| c)
}

/// Jaro-Winkler similarity of two strings, 0 to 1.
pub fn jaro_winkler(a: &str, b: &str) -> f64 {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let window = (a.len().max(b.len()) / 2).saturating_sub(1);
    let mut a_matched = vec![false; a.len()];
    let mut b_matched = vec![false; b.len()];
    let mut matches = 0usize;
    for (i, ca) in a.iter().enumerate() {
        let start = i.saturating_sub(window);
        let end = (i + window + 1).min(b.len());
        for j in start..end {
            if !b_matched[j] && b[j] == *ca {
                a_matched[i] = true;
                b_matched[j] = true;
                matches += 1;
                break;
            }
        }
    }
    if matches == 0 {
        return 0.0;
    }
    let a_seq = a
        .iter()
        .zip(&a_matched)
        .filter(|(_, m)| **m)
        .map(|(c, _)| c);
    let b_seq = b
        .iter()
        .zip(&b_matched)
        .filter(|(_, m)| **m)
        .map(|(c, _)| c);
    let transpositions = a_seq.zip(b_seq).filter(|(x, y)| x != y).count() / 2;
    let m = matches as f64;
    let jaro = (m / a.len() as f64 + m / b.len() as f64 + (m - transpositions as f64) / m) / 3.0;
    let prefix = a.iter().zip(&b).take(4).take_while(|(x, y)| x == y).count();
    jaro + prefix as f64 * 0.1 * (1.0 - jaro)
}

/// Whether `answer` can be given to value `key` of a field: the error, worded
/// for the admin, when it can't.
fn fits(matcher: &Matcher, required: bool, key: &str, answer: &ValueAnswer) -> Result<(), String> {
    let blank = key.is_empty();
    match (matcher, answer) {
        (_, ValueAnswer::Skip) => Ok(()),
        (Matcher::Choice { .. }, ValueAnswer::Match { target }) => match matcher.pickable(target) {
            Some(_) => Ok(()),
            None => Err(format!(
                "'{}' isn't one of the choices",
                target
                    .as_str()
                    .map_or_else(|| target.to_string(), String::from)
            )),
        },
        // A blank kind left as it is: existing assets keep theirs.
        (_, ValueAnswer::LeaveBlank) if blank || !required => Ok(()),
        (_, ValueAnswer::LeaveBlank) => Err("it's required, so it can't be left blank".to_string()),
        (Matcher::Number(mark), ValueAnswer::Replace { value }) if !blank => {
            match parse_decimal(value, *mark) {
                Ok(_) => Ok(()),
                Err(_) => Err(format!("'{value}' isn't a number")),
            }
        }
        (Matcher::Date(order), ValueAnswer::Replace { value }) if !blank => {
            match parse_date(value, *order) {
                Some(_) => Ok(()),
                None => Err(format!("'{value}' isn't a date ({})", date_hint(*order))),
            }
        }
        (_, ValueAnswer::Replace { .. }) => {
            Err("can't be replaced with typed text; pick an entry".to_string())
        }
        (_, ValueAnswer::Match { .. }) => {
            Err("can't link to an entry; replace it with a value instead".to_string())
        }
        (_, ValueAnswer::Create | ValueAnswer::KeepText) => {
            Err("can't create an entry or keep text here".to_string())
        }
    }
}

/// Work out every mapped value's answer: the admin's stored one when it
/// still fits, else what matching suggests. `grid` is the mapped grid
/// ([`super::mapping::map_job_grid`]), one column per field id. A column
/// with a context reads only the rows `row_contexts` gives that context.
pub fn resolve(
    job_type: ImportType,
    grid: &ParsedCsv,
    matchers: Vec<ColumnMatcher>,
    options: &ImportOptions,
    memory: &Memory,
    stored: &ValueAnswers,
    row_contexts: &HashMap<usize, String>,
) -> Resolution {
    let mut columns = Vec::with_capacity(matchers.len());
    for ColumnMatcher {
        spec,
        index,
        context,
        context_label,
        matcher,
    } in matchers
    {
        let Some(at) = grid.headers.iter().position(|h| h == &spec.id) else {
            continue;
        };
        let blank_value = context.is_empty() && blank_is_value(job_type, &spec.id, options);
        let answered = stored
            .get(&spec.id)
            .and_then(|contexts| contexts.get(&context));
        let mut order: Vec<String> = Vec::new();
        let mut found: HashMap<String, (String, Vec<usize>)> = HashMap::new();
        for row in &grid.rows {
            if !context.is_empty() && row_contexts.get(&row.source_row) != Some(&context) {
                continue;
            }
            let cell = row.cells.get(at).map(|c| c.trim()).unwrap_or("");
            if cell.is_empty() && !blank_value {
                continue;
            }
            let key = normalize_value(cell);
            let entry = found.entry(key.clone()).or_insert_with(|| {
                order.push(key);
                (cell.to_string(), Vec::new())
            });
            entry.1.push(row.source_row);
        }
        let values = order
            .into_iter()
            .map(|key| {
                let (text, rows) = found.remove(&key).unwrap_or_default();
                let (layer, reason, suggested, saved) = if key.is_empty() {
                    // Blank kind: left as it is unless answered.
                    (Layer::NoMatch, None, Some(ValueAnswer::LeaveBlank), None)
                } else {
                    prematch(
                        &spec.id,
                        &context,
                        spec.required,
                        &matcher,
                        &key,
                        &text,
                        memory,
                    )
                };
                let answer = answered
                    .and_then(|values| values.get(&key))
                    .filter(|a| fits(&matcher, spec.required, &key, a).is_ok())
                    .cloned();
                ResolvedValue {
                    key,
                    text,
                    rows,
                    layer,
                    reason,
                    suggested,
                    answer,
                    saved,
                }
            })
            .collect();
        columns.push(ResolvedColumn {
            field: spec.id.clone(),
            index,
            context,
            context_label,
            value_type: spec.value_type,
            required: spec.required,
            matcher,
            blank_is_value: blank_value,
            values,
        });
    }
    Resolution {
        columns,
        row_contexts: row_contexts.clone(),
    }
}

/// Load what [`resolve`] needs and resolve, in two phases: the columns
/// without a context first, then, once each row's context reads from the
/// grid with those answers written, the columns answered per context.
pub fn prepare(
    conn: &mut DbConnection,
    job_type: ImportType,
    fields: &[FieldSpec],
    mapping: &ColumnMapping,
    options: &ImportOptions,
    grid: &ParsedCsv,
    stored: &ValueAnswers,
) -> diesel::QueryResult<Resolution> {
    let all = matchers(conn, job_type, fields, mapping, options)?;
    if all.is_empty() {
        return Ok(Resolution::default());
    }
    let memory = Memory::load(conn, job_type)?;
    let (contextual, plain): (Vec<ColumnMatcher>, Vec<ColumnMatcher>) =
        all.into_iter().partition(|m| !m.context.is_empty());
    let mut resolution = resolve(
        job_type,
        grid,
        plain,
        options,
        &memory,
        stored,
        &HashMap::new(),
    );
    if contextual.is_empty() {
        return Ok(resolution);
    }
    let mut staged = MappedGrid {
        grid: grid.clone(),
        blocked: Vec::new(),
    };
    let mut staged_options = options.clone();
    if let Some(kind) = apply(&resolution, &mut staged) {
        staged_options.default_kind = Some(kind);
    }
    let row_contexts =
        super::importer_for(job_type).row_contexts(conn, &staged.grid, &staged_options)?;
    let more = resolve(
        job_type,
        grid,
        contextual,
        options,
        &memory,
        stored,
        &row_contexts,
    );
    resolution.columns.extend(more.columns);
    resolution.columns.sort_by_key(|c| c.index);
    resolution.row_contexts = row_contexts;
    Ok(resolution)
}

/// Why rows holding `text` are held back by the admin's Skip.
pub fn skip_message(text: &str) -> String {
    if text.is_empty() {
        "blank, which is set to Skip these rows".to_string()
    } else {
        format!("'{text}' is set to Skip these rows")
    }
}

/// Write the answers into the mapped grid: a link writes the choice's key,
/// Replace with writes its text, Leave blank clears the cell, and the
/// admin's Skip holds the rows back as skipped. A suggested Skip leaves the
/// cell for the importer, whose own error skips the row and says why. An
/// answer for blank kind cells is the kind new rows take: the returned
/// default kind.
pub fn apply(resolution: &Resolution, mapped: &mut MappedGrid) -> Option<String> {
    let mut default_kind = None;
    let mut held: BTreeMap<usize, Vec<(Option<String>, String)>> = BTreeMap::new();
    let mut writes: Vec<(usize, &str, HashMap<String, Option<String>>)> = Vec::new();
    for column in &resolution.columns {
        let Some(at) = mapped.grid.headers.iter().position(|h| h == &column.field) else {
            continue;
        };
        let mut cells: HashMap<String, Option<String>> = HashMap::new();
        for value in &column.values {
            let Some(answer) = value.effective() else {
                continue;
            };
            if value.key.is_empty() {
                match answer {
                    ValueAnswer::Match { target } => {
                        default_kind = target.as_str().map(String::from);
                    }
                    ValueAnswer::Skip if value.answer.is_some() => {
                        hold(&mut held, &value.rows, &column.field, &value.text);
                    }
                    _ => {}
                }
                continue;
            }
            let cell = match answer {
                ValueAnswer::Match { target } => target.as_str().map(String::from),
                ValueAnswer::Replace { value } => Some(value.trim().to_string()),
                ValueAnswer::LeaveBlank => Some(String::new()),
                ValueAnswer::Skip if value.answer.is_some() => {
                    hold(&mut held, &value.rows, &column.field, &value.text);
                    None
                }
                _ => None,
            };
            if let Some(cell) = cell {
                cells.insert(value.key.clone(), Some(cell));
            }
        }
        if !cells.is_empty() {
            writes.push((at, column.context.as_str(), cells));
        }
    }
    for row in &mut mapped.grid.rows {
        let row_context = resolution.row_contexts.get(&row.source_row);
        for (at, context, cells) in &writes {
            if !context.is_empty() && row_context.map(String::as_str) != Some(*context) {
                continue;
            }
            let Some(cell) = row.cells.get_mut(*at) else {
                continue;
            };
            if let Some(Some(new)) = cells.get(&normalize_value(cell)) {
                *cell = new.clone();
            }
        }
    }
    if !held.is_empty() {
        mapped
            .grid
            .rows
            .retain(|row| !held.contains_key(&row.source_row));
        mapped.blocked.extend(
            held.into_iter()
                .map(|(row, errors)| RowResult::skipped(row, errors)),
        );
        mapped.blocked.sort_by_key(|r| r.row);
    }
    default_kind
}

fn hold(
    held: &mut BTreeMap<usize, Vec<(Option<String>, String)>>,
    rows: &[usize],
    field: &str,
    text: &str,
) {
    for &row in rows {
        held.entry(row)
            .or_default()
            .push((Some(field.to_string()), skip_message(text)));
    }
}

/// Resolve the job's answers and write them into `mapped`, setting the
/// default kind a blank kind's answer gives new rows on `options`.
#[allow(clippy::too_many_arguments)]
pub fn settle(
    conn: &mut DbConnection,
    job_type: ImportType,
    fields: &[FieldSpec],
    mapping: &ColumnMapping,
    options: &mut ImportOptions,
    stored: &ValueAnswers,
    mapped: &mut MappedGrid,
) -> diesel::QueryResult<Resolution> {
    let resolution = prepare(
        conn,
        job_type,
        fields,
        mapping,
        options,
        &mapped.grid,
        stored,
    )?;
    if let Some(kind) = apply(&resolution, mapped) {
        options.default_kind = Some(kind);
    }
    Ok(resolution)
}

/// Check answers the admin sent against the fields matched here and re-key
/// them by [`normalize_value`]. A field answered per context takes answers
/// under each of its contexts; any other field under the empty one.
pub fn validate(
    answers: ValueAnswers,
    columns: &[ColumnMatcher],
    blank_fields: &HashSet<String>,
) -> Result<ValueAnswers, MappingError> {
    let mut out = ValueAnswers::new();
    for (field, contexts) in answers {
        let of_field: Vec<&ColumnMatcher> = columns.iter().filter(|m| m.spec.id == field).collect();
        let Some(first) = of_field.first() else {
            return Err(MappingError(format!(
                "'{field}' isn't a mapped column with values to answer"
            )));
        };
        let per_context = !first.context.is_empty();
        for (context, values) in contexts {
            let Some(column) = of_field.iter().find(|m| m.context == context) else {
                return Err(MappingError(if per_context {
                    format!(
                        "{} answers are given per kind, and '{context}' isn't a kind that has it",
                        first.spec.label
                    )
                } else {
                    format!(
                        "{} answers take no context; got '{context}'",
                        first.spec.label
                    )
                }));
            };
            let spec = &column.spec;
            if values.len() > MAX_ROWS {
                return Err(MappingError(format!(
                    "{} has more answers than a file has rows",
                    spec.label
                )));
            }
            let mut keyed = BTreeMap::new();
            for (value, answer) in values {
                let key = normalize_value(&value);
                if key.is_empty() && (per_context || !blank_fields.contains(&field)) {
                    return Err(MappingError(format!(
                        "blank cells in {} take no answer",
                        spec.label
                    )));
                }
                if let Err(why) = fits(&column.matcher, spec.required, &key, &answer) {
                    let shown = if key.is_empty() {
                        "(blank)"
                    } else {
                        value.trim()
                    };
                    return Err(MappingError(format!("{}, '{shown}': {why}", spec.label)));
                }
                if keyed.insert(key, answer).is_some() {
                    return Err(MappingError(format!(
                        "'{}' in {} is answered twice",
                        value.trim(),
                        spec.label
                    )));
                }
            }
            out.entry(field.clone()).or_default().insert(context, keyed);
        }
    }
    Ok(out)
}

/// The fields whose blank cells take an answer, for [`validate`].
pub fn blank_fields(
    job_type: ImportType,
    columns: &[ColumnMatcher],
    options: &ImportOptions,
) -> HashSet<String> {
    columns
        .iter()
        .filter(|m| m.context.is_empty() && blank_is_value(job_type, &m.spec.id, options))
        .map(|m| m.spec.id.clone())
        .collect()
}

/// Matched values the Match values step lists per column at most; the
/// count says how many there are.
const MATCHED_SHOWN: usize = 200;

/// What `GET /admin/import/{id}/values` answers.
#[derive(Debug, Clone, Serialize)]
pub struct ValuesView {
    /// Rows the last check read.
    pub rows_checked: usize,
    /// Rows the last check skipped.
    pub rows_skipped: usize,
    /// Of those, the rows no answer here explains, so changing answers
    /// doesn't bring them back.
    pub skipped_other: Vec<usize>,
    pub columns: Vec<ColumnView>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ColumnView {
    pub field: String,
    /// The row context the column answers for (a kind slug); empty when
    /// the field is answered the same on every row.
    pub context: String,
    /// The context's name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_label: Option<String>,
    /// The source column.
    pub index: usize,
    /// The file's header.
    pub header: String,
    pub value_type: ValueType,
    pub required: bool,
    /// What a value can link to; empty for numbers and dates.
    pub choices: Vec<Choice>,
    /// Distinct values in the column.
    pub distinct: usize,
    /// Of those, matched exactly: no answer needed.
    pub matched: usize,
    /// Every value that isn't an exact match, then the exact ones, at
    /// most [`MATCHED_SHOWN`] of those, most rows first.
    pub values: Vec<ValueView>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ValueView {
    pub key: String,
    pub text: String,
    pub blank: bool,
    /// File lines holding the value.
    pub rows: Vec<usize>,
    #[serde(rename = "match")]
    pub layer: Layer,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<Reason>,
    /// What matching suggests; absent when the cell is kept as it is.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggested: Option<ValueAnswer>,
    /// The admin's answer stored on the job.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub answer: Option<ValueAnswer>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub saved: Option<Provenance>,
    /// Rows holding it that the last check skipped.
    pub skipped_rows: Vec<usize>,
}

/// The Match values step's view of a resolution, against the stored rows of
/// the last check.
pub fn view(
    resolution: &Resolution,
    headers: &[SourceHeader],
    checked: &[RowResult],
) -> ValuesView {
    let skipped: HashMap<usize, &RowResult> = checked
        .iter()
        .filter(|r| r.outcome == RowOutcome::Skipped)
        .map(|r| (r.row, r))
        .collect();

    // A row is explained when every error it has is on a matched column
    // whose value there is answered Skip.
    let mut skip_rows: HashMap<&str, HashSet<usize>> = HashMap::new();
    for column in &resolution.columns {
        let rows = skip_rows.entry(column.field.as_str()).or_default();
        for value in &column.values {
            let skips = match value.effective() {
                Some(ValueAnswer::Skip) => true,
                // A blank kind left as it is skips new rows only, as the
                // last check found.
                Some(ValueAnswer::LeaveBlank) => value.key.is_empty(),
                _ => false,
            };
            if skips {
                rows.extend(&value.rows);
            }
        }
    }
    let mut skipped_other: Vec<usize> = skipped
        .values()
        .filter(|r| {
            r.errors.is_empty()
                || !r.errors.iter().all(|e| {
                    e.column
                        .as_deref()
                        .and_then(|c| skip_rows.get(c))
                        .is_some_and(|rows| rows.contains(&r.row))
                })
        })
        .map(|r| r.row)
        .collect();
    skipped_other.sort_unstable();

    let columns = resolution
        .columns
        .iter()
        .filter(|column| !column.values.is_empty())
        .map(|column| {
            let mut values: Vec<ValueView> = column
                .values
                .iter()
                .map(|v| {
                    let skipped_rows: Vec<usize> = v
                        .rows
                        .iter()
                        .copied()
                        .filter(|r| skipped.contains_key(r))
                        .collect();
                    // A blank kind none of whose rows were skipped needs
                    // nothing.
                    let layer = if v.key.is_empty() && skipped_rows.is_empty() && v.answer.is_none()
                    {
                        Layer::Exact
                    } else {
                        v.layer
                    };
                    ValueView {
                        key: v.key.clone(),
                        text: v.text.clone(),
                        blank: v.key.is_empty(),
                        rows: v.rows.clone(),
                        layer,
                        reason: v.reason.clone(),
                        suggested: v.suggested.clone(),
                        answer: v.answer.clone(),
                        saved: v.saved.clone(),
                        skipped_rows,
                    }
                })
                .collect();
            let distinct = values.len();
            let is_exact = |v: &ValueView| v.layer == Layer::Exact && v.answer.is_none();
            let matched = values.iter().filter(|v| is_exact(v)).count();
            // Values to read first, then the exact ones, most rows first.
            values.sort_by_key(|v| (is_exact(v), std::cmp::Reverse(v.rows.len())));
            let mut exact_seen = 0;
            values.retain(|v| {
                if !is_exact(v) {
                    return true;
                }
                exact_seen += 1;
                exact_seen <= MATCHED_SHOWN
            });
            // Exact numbers and dates are just values; only their count shows.
            if !matches!(column.matcher, Matcher::Choice { .. }) {
                values.retain(|v| !is_exact(v));
            }
            ColumnView {
                field: column.field.clone(),
                context: column.context.clone(),
                context_label: column.context_label.clone(),
                index: column.index,
                header: headers
                    .get(column.index)
                    .map(|h| h.name.clone())
                    .unwrap_or_default(),
                value_type: column.value_type,
                required: column.required,
                choices: column.matcher.choices().to_vec(),
                distinct,
                matched,
                values,
            }
        })
        .collect();

    ValuesView {
        rows_checked: checked.len(),
        rows_skipped: skipped.len(),
        skipped_other,
        columns,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::imports::csv_parser::SourceRow;
    use crate::services::imports::types::FieldGroup;

    fn kinds() -> Matcher {
        Matcher::Choice {
            choices: vec![
                Choice::new("camera", "Camera"),
                Choice::new("camera_support", "Camera support"),
                Choice::new("laptop", "Laptop"),
            ],
            aliases: Vec::new(),
            synonyms: vec![("cam".into(), "camera".into())],
            strict: false,
        }
    }

    fn spec(id: &str, value_type: ValueType, required: bool) -> FieldSpec {
        FieldSpec::fixed(
            ImportType::Assets,
            id,
            id,
            FieldGroup::Core,
            value_type,
            required,
        )
    }

    fn grid(headers: &[&str], rows: &[&[&str]]) -> ParsedCsv {
        ParsedCsv {
            headers: headers.iter().map(|h| h.to_string()).collect(),
            rows: rows
                .iter()
                .enumerate()
                .map(|(i, cells)| SourceRow {
                    source_row: i + 2,
                    cells: cells.iter().map(|c| c.to_string()).collect(),
                    cell_errors: Vec::new(),
                })
                .collect(),
            delimiter: b',',
        }
    }

    fn link(value: &str) -> ValueAnswer {
        ValueAnswer::Match {
            target: serde_json::Value::String(value.into()),
        }
    }

    #[test]
    fn values_normalize_by_case_and_spacing() {
        assert_eq!(normalize_value("  In   Use "), "in use");
        assert_eq!(normalize_value("CAM"), "cam");
        assert_eq!(normalize_value(""), "");
    }

    #[test]
    fn jaro_winkler_scores_close_spellings_high() {
        assert!(jaro_winkler("camra", "camera") >= FUZZY_THRESHOLD);
        assert!(jaro_winkler("laptp", "laptop") >= FUZZY_THRESHOLD);
        assert!(jaro_winkler("tripod", "camera") < FUZZY_THRESHOLD);
        assert_eq!(jaro_winkler("same", "same"), 1.0);
    }

    #[test]
    fn choice_values_match_in_layers() {
        let memory = {
            let mut m = Memory::default();
            m.insert(
                "kind",
                "",
                "b/cast",
                SavedAnswer {
                    answer: link("laptop"),
                    provenance: Provenance {
                        job_id: Uuid::nil(),
                        by: None,
                        by_name: Some("Sam".into()),
                        at: chrono::Utc::now(),
                    },
                },
            );
            m
        };
        let k = kinds();
        let layer = |key: &str| prematch("kind", "", true, &k, key, key, &memory);
        assert_eq!(layer("camera").0, Layer::Exact);
        assert_eq!(layer("camera support").2, Some(link("camera_support")));
        assert_eq!(layer("b/cast").0, Layer::Saved);
        assert_eq!(layer("cam").0, Layer::Guessed);
        assert_eq!(layer("cam").1, Some(Reason::Synonym));
        let camra = layer("camra");
        assert_eq!(camra.0, Layer::Guessed);
        assert_eq!(
            camra.1,
            Some(Reason::CloseTo {
                label: "Camera".into()
            })
        );
        assert_eq!(layer("tripod").0, Layer::NoMatch);
        // A required field skips what doesn't match; an optional one leaves
        // it blank.
        assert_eq!(layer("tripod").2, Some(ValueAnswer::Skip));
        let optional = prematch("kind", "", false, &k, "tripod", "tripod", &memory);
        assert_eq!(optional.2, Some(ValueAnswer::LeaveBlank));
    }

    #[test]
    fn short_labels_are_never_guessed() {
        let k = Matcher::Choice {
            choices: vec![Choice::new("low", "Low")],
            aliases: Vec::new(),
            synonyms: Vec::new(),
            strict: false,
        };
        let (layer, ..) = prematch("priority", "", false, &k, "lwo", "lwo", &Memory::default());
        assert_eq!(layer, Layer::NoMatch);
    }

    #[test]
    fn a_disabled_choice_is_no_match_with_its_note() {
        let k = Matcher::Choice {
            choices: vec![Choice {
                value: "on_loan".into(),
                label: "On loan".into(),
                disabled: Some("Set by a loan after import".into()),
            }],
            aliases: Vec::new(),
            synonyms: Vec::new(),
            strict: false,
        };
        let (layer, reason, answer, _) = prematch(
            "status",
            "",
            false,
            &k,
            "on loan",
            "On loan",
            &Memory::default(),
        );
        assert_eq!(layer, Layer::NoMatch);
        assert!(matches!(reason, Some(Reason::Disabled { .. })));
        assert_eq!(answer, Some(ValueAnswer::LeaveBlank));
    }

    #[test]
    fn booleans_read_yes_and_no_and_skip_the_rest() {
        let b = boolean_matcher();
        let memory = Memory::default();
        assert_eq!(
            prematch("f", "", false, &b, "y", "y", &memory).2,
            Some(link("true"))
        );
        assert_eq!(
            prematch("f", "", false, &b, "nee", "nee", &memory).2,
            Some(link("false"))
        );
        let maybe = prematch("f", "", false, &b, "maybe", "maybe", &memory);
        assert_eq!(maybe.0, Layer::NoMatch);
        assert_eq!(maybe.1, Some(Reason::Unreadable));
        assert_eq!(maybe.2, Some(ValueAnswer::Skip));
    }

    #[test]
    fn numbers_and_dates_that_dont_read_skip_by_default() {
        let memory = Memory::default();
        let n = Matcher::Number(DecimalMark::Dot);
        assert_eq!(
            prematch("q", "", false, &n, "2", "2", &memory).0,
            Layer::Exact
        );
        let two = prematch("q", "", false, &n, "two", "two", &memory);
        assert_eq!((two.0, two.2), (Layer::NoMatch, Some(ValueAnswer::Skip)));
        let d = Matcher::Date(DateOrder::Iso);
        assert_eq!(
            prematch("d", "", false, &d, "2024-03-14", "2024-03-14", &memory).0,
            Layer::Exact
        );
        assert_eq!(
            prematch("d", "", false, &d, "31/02/2023", "31/02/2023", &memory).0,
            Layer::NoMatch
        );
    }

    #[test]
    fn answers_fit_their_field() {
        let k = kinds();
        assert!(fits(&k, true, "tripod", &link("camera")).is_ok());
        assert!(fits(&k, true, "tripod", &link("boat")).is_err());
        assert!(fits(&k, true, "tripod", &ValueAnswer::LeaveBlank).is_err());
        assert!(fits(&k, true, "", &ValueAnswer::LeaveBlank).is_ok());
        assert!(fits(&k, false, "tripod", &ValueAnswer::LeaveBlank).is_ok());
        assert!(fits(&k, true, "tripod", &ValueAnswer::Create).is_err());
        let n = Matcher::Number(DecimalMark::Dot);
        let replace = |v: &str| ValueAnswer::Replace { value: v.into() };
        assert!(fits(&n, false, "two", &replace("2")).is_ok());
        assert!(fits(&n, false, "two", &replace("three")).is_err());
        assert!(fits(&n, false, "two", &link("2")).is_err());
        assert!(fits(
            &Matcher::Date(DateOrder::Iso),
            false,
            "x",
            &replace("2024-02-28")
        )
        .is_ok());
        assert!(fits(
            &Matcher::Date(DateOrder::Iso),
            false,
            "x",
            &replace("28/02/2024")
        )
        .is_err());
    }

    fn column(spec: FieldSpec, index: usize, matcher: Matcher) -> ColumnMatcher {
        ColumnMatcher {
            spec,
            index,
            context: String::new(),
            context_label: None,
            matcher,
        }
    }

    fn columns() -> Vec<ColumnMatcher> {
        vec![
            column(spec("kind", ValueType::Enum, true), 1, kinds()),
            column(
                spec("quantity", ValueType::Number, false),
                2,
                Matcher::Number(DecimalMark::Dot),
            ),
        ]
    }

    fn resolved(
        rows: &[&[&str]],
        stored: &ValueAnswers,
        options: &ImportOptions,
    ) -> (Resolution, MappedGrid) {
        let parsed = grid(&["name", "kind", "quantity"], rows);
        let resolution = resolve(
            ImportType::Assets,
            &parsed,
            columns(),
            options,
            &Memory::default(),
            stored,
            &HashMap::new(),
        );
        let mapped = MappedGrid {
            grid: parsed,
            blocked: Vec::new(),
        };
        (resolution, mapped)
    }

    fn answers(field: &str, pairs: &[(&str, ValueAnswer)]) -> ValueAnswers {
        let mut values = BTreeMap::new();
        for (k, a) in pairs {
            values.insert(k.to_string(), a.clone());
        }
        let mut contexts = BTreeMap::new();
        contexts.insert(String::new(), values);
        let mut out = ValueAnswers::new();
        out.insert(field.to_string(), contexts);
        out
    }

    #[test]
    fn apply_writes_answers_and_holds_back_skips() {
        let mut stored = answers("kind", &[("tripod", ValueAnswer::Skip)]);
        stored.extend(answers(
            "quantity",
            &[("two", ValueAnswer::Replace { value: "2".into() })],
        ));
        let (resolution, mut mapped) = resolved(
            &[
                &["A", "Cam", "two"],
                &["B", "tripod", "1"],
                &["C", "Camera", "lots"],
            ],
            &stored,
            &ImportOptions::default(),
        );
        let default_kind = apply(&resolution, &mut mapped);
        assert_eq!(default_kind, None);
        let cells: Vec<Vec<String>> = mapped.grid.rows.iter().map(|r| r.cells.clone()).collect();
        // Cam guessed as camera, two replaced, lots kept for the importer
        // to refuse.
        assert_eq!(
            cells,
            vec![
                vec!["A".to_string(), "camera".into(), "2".into()],
                vec!["C".to_string(), "camera".into(), "lots".into()],
            ]
        );
        assert_eq!(mapped.blocked.len(), 1);
        assert_eq!(mapped.blocked[0].row, 3);
        assert_eq!(mapped.blocked[0].errors[0].column.as_deref(), Some("kind"));
        assert_eq!(
            mapped.blocked[0].errors[0].message,
            "'tripod' is set to Skip these rows"
        );
    }

    #[test]
    fn a_blank_kind_answer_is_the_kind_new_rows_take() {
        let stored = answers("kind", &[("", link("laptop"))]);
        let (resolution, mut mapped) = resolved(
            &[&["A", "", "1"], &["B", "camera", "1"]],
            &stored,
            &ImportOptions::default(),
        );
        assert_eq!(apply(&resolution, &mut mapped), Some("laptop".to_string()));
        // The blank cell stays blank, so an existing asset keeps its kind.
        assert_eq!(mapped.grid.rows[0].cells[1], "");

        // A default kind already set means blank cells aren't a value.
        let options = ImportOptions {
            default_kind: Some("camera".into()),
            ..Default::default()
        };
        let (resolution, _) = resolved(&[&["A", "", "1"]], &stored, &options);
        assert!(resolution.columns[0].values.is_empty());
    }

    #[test]
    fn a_stored_answer_that_no_longer_fits_falls_back() {
        let stored = answers("kind", &[("tripod", link("boat"))]);
        let (resolution, _) = resolved(&[&["A", "tripod", ""]], &stored, &ImportOptions::default());
        let value = &resolution.columns[0].values[0];
        assert_eq!(value.answer, None);
        assert_eq!(value.effective(), Some(&ValueAnswer::Skip));
    }

    #[test]
    fn validate_rekeys_and_refuses_what_doesnt_fit() {
        let columns = columns();
        let blanks: HashSet<String> = ["kind".to_string()].into();
        let ok = validate(
            answers("kind", &[("  TriPod ", link("camera_support"))]),
            &columns,
            &blanks,
        )
        .expect("valid");
        assert!(ok["kind"][""].contains_key("tripod"));

        for (bad, expected) in [
            (
                answers("name", &[("x", ValueAnswer::Skip)]),
                "isn't a mapped column",
            ),
            (
                answers("kind", &[("tripod", link("boat"))]),
                "isn't one of the choices",
            ),
            (
                answers("kind", &[("tripod", ValueAnswer::LeaveBlank)]),
                "can't be left blank",
            ),
            (
                answers(
                    "quantity",
                    &[("two", ValueAnswer::Replace { value: "x".into() })],
                ),
                "isn't a number",
            ),
            (
                answers("quantity", &[("", ValueAnswer::Skip)]),
                "take no answer",
            ),
            (
                answers(
                    "kind",
                    &[("Cam", ValueAnswer::Skip), ("cam", ValueAnswer::Skip)],
                ),
                "answered twice",
            ),
        ] {
            let err = validate(bad, &columns, &blanks).unwrap_err().0;
            assert!(err.contains(expected), "{expected}: {err}");
        }
    }

    #[test]
    fn the_view_orders_values_and_explains_skips() {
        let (resolution, _) = resolved(
            &[
                &["A", "camera", "two"],
                &["B", "tripod", "1"],
                &["C", "tripod", "1"],
                &["", "camera", "1"],
            ],
            &ValueAnswers::new(),
            &ImportOptions::default(),
        );
        let checked = vec![
            RowResult::skipped(2, vec![(Some("quantity".into()), "bad".into())]),
            RowResult::skipped(3, vec![(Some("kind".into()), "unknown".into())]),
            RowResult::skipped(4, vec![(Some("kind".into()), "unknown".into())]),
            RowResult::skipped(5, vec![(Some("name".into()), "required".into())]),
        ];
        let view = view(&resolution, &[], &checked);
        assert_eq!(view.rows_skipped, 4);
        assert_eq!(view.skipped_other, vec![5]);
        let kind = &view.columns[0];
        assert_eq!((kind.distinct, kind.matched), (2, 1));
        assert_eq!(kind.values[0].text, "tripod");
        assert_eq!(kind.values[0].skipped_rows, vec![3, 4]);
        assert_eq!(kind.values[1].text, "camera");
        // Numbers list only what doesn't read.
        let quantity = &view.columns[1];
        assert_eq!(quantity.values.len(), 1);
        assert_eq!(quantity.values[0].text, "two");
        assert_eq!(quantity.matched, 1);
    }
    #[test]
    fn dates_read_year_first_or_in_the_chosen_order() {
        let ymd = |y, m, d| chrono::NaiveDate::from_ymd_opt(y, m, d);
        assert_eq!(parse_date("2024-03-14", DateOrder::Iso), ymd(2024, 3, 14));
        assert_eq!(parse_date("2024/3/4", DateOrder::Dmy), ymd(2024, 3, 4));
        assert_eq!(parse_date("14/03/2024", DateOrder::Dmy), ymd(2024, 3, 14));
        assert_eq!(parse_date("03/14/2024", DateOrder::Mdy), ymd(2024, 3, 14));
        assert_eq!(parse_date("14.3.2024", DateOrder::Dmy), ymd(2024, 3, 14));
        assert_eq!(
            parse_date("2024-03-14T09:30:00", DateOrder::Iso),
            ymd(2024, 3, 14)
        );
        for (raw, order) in [
            ("14/03/2024", DateOrder::Iso),
            ("14/03/2024", DateOrder::Mdy),
            ("31/02/2023", DateOrder::Dmy),
            ("14/03/24", DateOrder::Dmy),
            ("2024-03-14Tnoon", DateOrder::Iso),
            ("March 14", DateOrder::Dmy),
            ("", DateOrder::Iso),
        ] {
            assert_eq!(parse_date(raw, order), None, "{raw}");
        }
        assert_eq!(read_boolean(" Yes "), Some(true));
        assert_eq!(read_boolean("NEE"), Some(false));
        assert_eq!(read_boolean("maybe"), None);
    }

    /// A column answered per context reads only its context's rows, and
    /// its answers write only into them.
    #[test]
    fn contextual_columns_answer_their_own_rows() {
        let parsed = grid(
            &["name", "condition"],
            &[&["Cam", "good"], &["Van", "good"], &["Van 2", "good"]],
        );
        let options_for = |options: &[&str]| Matcher::Choice {
            choices: options.iter().map(|o| Choice::new(*o, *o)).collect(),
            aliases: Vec::new(),
            synonyms: Vec::new(),
            strict: false,
        };
        let contextual = |context: &str, options: &[&str]| ColumnMatcher {
            spec: spec("condition", ValueType::Enum, false),
            index: 1,
            context: context.to_string(),
            context_label: Some(context.to_string()),
            matcher: options_for(options),
        };
        let row_contexts: HashMap<usize, String> = [
            (2, "camera".to_string()),
            (3, "vehicle".to_string()),
            (4, "vehicle".to_string()),
        ]
        .into();
        let mut stored = ValueAnswers::new();
        stored.entry("condition".to_string()).or_default().insert(
            "vehicle".to_string(),
            [("good".to_string(), link("Roadworthy"))].into(),
        );
        let resolution = resolve(
            ImportType::Assets,
            &parsed,
            vec![
                contextual("camera", &["Good", "Damaged"]),
                contextual("vehicle", &["Roadworthy"]),
            ],
            &ImportOptions::default(),
            &Memory::default(),
            &stored,
            &row_contexts,
        );
        assert_eq!(resolution.columns[0].values[0].rows, vec![2]);
        assert_eq!(resolution.columns[0].values[0].layer, Layer::Exact);
        assert_eq!(resolution.columns[1].values[0].rows, vec![3, 4]);
        assert_eq!(resolution.columns[1].values[0].layer, Layer::NoMatch);

        let mut mapped = MappedGrid {
            grid: parsed,
            blocked: Vec::new(),
        };
        apply(&resolution, &mut mapped);
        let cells: Vec<&str> = mapped
            .grid
            .rows
            .iter()
            .map(|r| r.cells[1].as_str())
            .collect();
        assert_eq!(cells, vec!["Good", "Roadworthy", "Roadworthy"]);
    }
    #[test]
    fn numbers_read_past_currency_and_thousands_separators() {
        let dot = |raw: &str| {
            parse_decimal(raw, DecimalMark::Dot).map(|v| v.map(|v| v.normalized().to_string()))
        };
        let comma = |raw: &str| {
            parse_decimal(raw, DecimalMark::Comma).map(|v| v.map(|v| v.normalized().to_string()))
        };
        let some = |v: &str| Ok(Some(v.to_string()));
        assert_eq!(dot("$7,450.00"), some("7450"));
        assert_eq!(dot("A$ 12.5"), some("12.5"));
        assert_eq!(dot("US$1,000"), some("1000"));
        assert_eq!(dot("USD 99.95"), some("99.95"));
        assert_eq!(dot("-$1,234.5"), some("-1234.5"));
        assert_eq!(dot("1 234 567.25"), some("1234567.25"));
        assert_eq!(dot("1,234"), some("1234"));
        assert_eq!(dot("0.125"), some("0.125"));
        assert_eq!(comma("€1.234,56"), some("1234.56"));
        assert_eq!(comma("1.234,56 €"), some("1234.56"));
        assert_eq!(comma("1 234,56"), some("1234.56"));
        assert_eq!(comma("1\u{a0}234,56 EUR"), some("1234.56"));
        assert_eq!(comma("12,5"), some("12.5"));
        assert_eq!(comma("0,125"), some("0.125"));
        assert_eq!(comma("1234,567"), some("1234.567"));

        let refused = |result: Result<Option<String>, String>, expected: &str| {
            let err = result.unwrap_err();
            assert!(err.contains(expected), "{expected}: {err}");
        };
        refused(comma("1,234"), "could be 1234 or 1.234");
        refused(comma("12.5"), "decimal mark is ','");
        refused(dot("12,5"), "choose ',' at the Map step");
        refused(dot("1,23,456"), "not a valid decimal");
        refused(dot("1.234.567"), "not a valid decimal");
        refused(comma("1.234 567,5"), "decimal mark is ','");
        refused(dot("two"), "'two' is not a valid decimal");
        refused(dot("$"), "not a valid decimal");
        refused(dot("12 apples"), "not a valid decimal");
    }
}
