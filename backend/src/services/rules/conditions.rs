//! Rule conditions (plan §5.2): the save-time check and the evaluator.
//!
//! [`compile`] checks a `conditions` tree against the operator matrix
//! and the save-time rules and returns a [`Predicate`], which evaluates
//! the tree against one ticket's [`TicketFacts`].
//!
//! On an empty field (no assignee, no requester email, no tags) the
//! comparisons that look for a value (`eq`, `in`, `contains`, `gt`,
//! `has_any`, ..) don't match and their negations (`neq`, `not_in`,
//! `not_contains`) do. Text comparisons ignore case.

use std::collections::{BTreeSet, HashMap};

use chrono::{DateTime, Utc};
use regex::{Regex, RegexBuilder};
use serde_json::Value;
use uuid::Uuid;

use crate::models::{
    Condition, ConditionField, ConditionFieldKind, ConditionOp, RuleTriggerKind, TicketPriority,
    WorkflowStateCategory,
};

/// How a rule's conditions are named in an error path.
const ROOT: &str = "conditions";
/// Bounds on one tree, so a saved rule stays cheap to evaluate on every
/// event.
const MAX_DEPTH: usize = 8;
const MAX_NODES: usize = 100;
const MAX_LIST: usize = 500;
const MAX_TEXT: usize = 1000;
const MAX_PATTERN: usize = 500;
const REGEX_SIZE_LIMIT: usize = 1 << 20;

/// Why a condition tree can't be saved. `path` points at the node
/// (`conditions.children[1].child`); `field` and `op` name the leaf when
/// the problem is in one, so the editor can mark it.
#[derive(Debug, Clone, PartialEq)]
pub struct ConditionError {
    pub path: String,
    pub field: Option<ConditionField>,
    pub op: Option<ConditionOp>,
    pub message: String,
}

impl ConditionError {
    fn at(path: &str, message: impl Into<String>) -> Self {
        Self {
            path: path.to_string(),
            field: None,
            op: None,
            message: message.into(),
        }
    }

    fn leaf(
        path: &str,
        field: ConditionField,
        op: ConditionOp,
        message: impl Into<String>,
    ) -> Self {
        Self {
            path: path.to_string(),
            field: Some(field),
            op: Some(op),
            message: message.into(),
        }
    }

    pub fn field_kind(&self) -> Option<ConditionFieldKind> {
        self.field.map(ConditionField::kind)
    }
}

impl std::fmt::Display for ConditionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.path, self.message)
    }
}

/// A checked condition tree, ready to evaluate. No root means no
/// conditions: every ticket matches.
#[derive(Debug, Default)]
pub struct Predicate {
    root: Option<Condition>,
    regexes: HashMap<String, Regex>,
}

/// The ids a tree names, which must exist in the rule's workspace.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct ConditionRefs {
    pub category_ids: BTreeSet<i32>,
    pub workflow_state_ids: BTreeSet<i32>,
    pub tag_ids: BTreeSet<i32>,
}

/// What a condition reads from one ticket. Text a ticket doesn't have
/// (an empty organization) is `None`.
#[derive(Debug, Clone)]
pub struct TicketFacts {
    pub id: i32,
    pub title: String,
    pub priority: TicketPriority,
    pub category_id: Option<i32>,
    pub requester_uuid: Option<Uuid>,
    pub assignee_uuid: Option<Uuid>,
    pub workflow_state_id: i32,
    pub workflow_category: Option<WorkflowStateCategory>,
    pub tag_ids: Vec<i32>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub first_response_at: Option<DateTime<Utc>>,
    pub resolved_at: Option<DateTime<Utc>>,
    pub requester_email: Option<String>,
    pub requester_organization: Option<String>,
}

/// What a condition reads from the event that fired the rule.
#[derive(Debug, Clone, Default)]
pub struct EventFacts {
    /// The `reads_set` names of the columns a ticket update changed.
    pub changed_fields: Vec<String>,
    /// `customer` or `agent`.
    pub reply_kind: Option<String>,
    pub reply_author_role: Option<String>,
    pub minutes_since: Option<i64>,
}

/// Check `conditions` for a rule fired by `trigger` and compile it. `[]`
/// is no conditions; anything else is one condition object.
pub fn compile(conditions: &Value, trigger: RuleTriggerKind) -> Result<Predicate, ConditionError> {
    let root: Condition = match conditions {
        Value::Array(items) if items.is_empty() => return Ok(Predicate::default()),
        Value::Array(_) => {
            return Err(ConditionError::at(
                ROOT,
                "conditions are one condition, not a list; put several in an and group",
            ))
        }
        Value::Object(_) => serde_json::from_value(conditions.clone())
            .map_err(|e| ConditionError::at(ROOT, e.to_string()))?,
        _ => {
            return Err(ConditionError::at(
                ROOT,
                "conditions must be [] or a condition object",
            ))
        }
    };
    let mut checker = Checker {
        trigger,
        nodes: 0,
        regexes: HashMap::new(),
    };
    checker.node(&root, ROOT, 1)?;
    Ok(Predicate {
        root: Some(root),
        regexes: checker.regexes,
    })
}

struct Checker {
    trigger: RuleTriggerKind,
    nodes: usize,
    regexes: HashMap<String, Regex>,
}

impl Checker {
    fn node(&mut self, node: &Condition, path: &str, depth: usize) -> Result<(), ConditionError> {
        self.nodes += 1;
        if self.nodes > MAX_NODES {
            return Err(ConditionError::at(
                path,
                format!("a rule takes at most {MAX_NODES} conditions"),
            ));
        }
        if depth > MAX_DEPTH {
            return Err(ConditionError::at(
                path,
                format!("groups nest at most {MAX_DEPTH} deep"),
            ));
        }
        match node {
            Condition::And { children } | Condition::Or { children } => {
                if children.is_empty() {
                    return Err(ConditionError::at(
                        path,
                        "a group needs at least one condition",
                    ));
                }
                for (i, child) in children.iter().enumerate() {
                    self.node(child, &format!("{path}.children[{i}]"), depth + 1)?;
                }
                if matches!(node, Condition::And { .. }) {
                    check_and_can_hold(children, path)?;
                }
                Ok(())
            }
            Condition::Not { child } => self.node(child, &format!("{path}.child"), depth + 1),
            Condition::Leaf { field, op, value } => self.leaf(*field, *op, value, path),
        }
    }

    fn leaf(
        &mut self,
        field: ConditionField,
        op: ConditionOp,
        value: &Value,
        path: &str,
    ) -> Result<(), ConditionError> {
        use ConditionFieldKind as K;
        use ConditionOp::*;

        let err = |message: String| ConditionError::leaf(path, field, op, message);
        let name = field.as_str();
        if let Some(reason) = field.unavailable() {
            return Err(err(reason.to_string()));
        }
        if let Some(only) = field.trigger_only() {
            if only != self.trigger {
                return Err(err(format!(
                    "{name} only applies to {} rules",
                    only.as_str()
                )));
            }
        }
        let kind = field.kind();
        if !kind.allowed_ops().contains(&op) {
            return Err(err(format!(
                "{} doesn't apply to {name}, a {} field",
                op.as_str(),
                kind.as_str()
            )));
        }
        match op {
            IsNull | IsNotNull => {
                if !value.is_null() {
                    return Err(err(format!("{} takes no value", op.as_str())));
                }
                if !field.nullable() {
                    let outcome = if op == IsNull {
                        "never matches"
                    } else {
                        "always matches"
                    };
                    return Err(err(format!("{name} always has a value, so this {outcome}")));
                }
            }
            Eq | Neq => check_scalar(field, value).map_err(err)?,
            In | NotIn => {
                let items = list(value).map_err(err)?;
                for item in items {
                    check_scalar(field, item).map_err(err)?;
                }
                let every = field.enum_values();
                let lists_every = !every.is_empty()
                    && every
                        .iter()
                        .all(|v| items.iter().any(|i| i.as_str() == Some(v)));
                if lists_every && !field.nullable() {
                    let outcome = if op == In {
                        "always matches"
                    } else {
                        "never matches"
                    };
                    return Err(err(format!(
                        "this lists every value of {name}, so it {outcome}"
                    )));
                }
            }
            Contains | NotContains | StartsWith | EndsWith => {
                text(value).map_err(err)?;
            }
            MatchesRegex => {
                let pattern = text(value).map_err(err)?;
                let regex = compile_pattern(pattern).map_err(err)?;
                self.regexes.insert(pattern.to_string(), regex);
            }
            Gt | Lt | Gte | Lte => match kind {
                K::Timestamp => {
                    timestamp(value).ok_or_else(|| {
                        err("give a date and time like 2026-01-31T09:00:00Z".to_string())
                    })?;
                }
                _ => {
                    let minutes = value
                        .as_i64()
                        .filter(|n| *n >= 0)
                        .ok_or_else(|| err("give a whole number of minutes".to_string()))?;
                    if minutes == 0 && op == Gte {
                        return Err(err("at least 0 minutes always matches".to_string()));
                    }
                    if minutes == 0 && op == Lt {
                        return Err(err("under 0 minutes never matches".to_string()));
                    }
                }
            },
            HasAny | HasAll => {
                let items = list(value).map_err(err)?;
                for item in items {
                    match kind {
                        K::IntArray => id(item).map(drop).map_err(err)?,
                        _ => text(item).map(drop).map_err(err)?,
                    }
                }
            }
        }
        Ok(())
    }
}

/// An `and` whose leaves on one field exclude each other never matches:
/// `eq` two different values, `eq` and `neq` the same one, `in` lists
/// with nothing in common, or `is_null` beside `is_not_null` or a value.
fn check_and_can_hold(children: &[Condition], path: &str) -> Result<(), ConditionError> {
    use ConditionOp::*;

    let mut fields: Vec<(ConditionField, Vec<(ConditionOp, &Value)>)> = Vec::new();
    for child in children {
        if let Condition::Leaf { field, op, value } = child {
            match fields.iter_mut().find(|(f, _)| f == field) {
                Some((_, leaves)) => leaves.push((*op, value)),
                None => fields.push((*field, vec![(*op, value)])),
            }
        }
    }
    for (field, leaves) in fields {
        let kind = field.kind();
        let mut allowed: Option<BTreeSet<String>> = None;
        let mut excluded = BTreeSet::new();
        let (mut null, mut not_null) = (false, false);
        for (op, value) in leaves {
            match op {
                Eq | In => {
                    let these = scalars(kind, value);
                    allowed = Some(match allowed {
                        None => these,
                        Some(before) => before.intersection(&these).cloned().collect(),
                    });
                }
                Neq | NotIn => excluded.extend(scalars(kind, value)),
                IsNull => null = true,
                IsNotNull => not_null = true,
                _ => {}
            }
        }
        let never = (null && (not_null || allowed.is_some()))
            || allowed
                .as_ref()
                .is_some_and(|a| a.iter().all(|v| excluded.contains(v)));
        if never {
            return Err(ConditionError {
                path: path.to_string(),
                field: Some(field),
                op: None,
                message: format!(
                    "the conditions on {} in this and group can't all hold, so it never matches",
                    field.as_str()
                ),
            });
        }
    }
    Ok(())
}

/// The comparable form of one `eq` / `in` value, or each value of a list.
fn scalars(kind: ConditionFieldKind, value: &Value) -> BTreeSet<String> {
    match value {
        Value::Array(items) => items.iter().filter_map(|v| canon(kind, v)).collect(),
        v => canon(kind, v).into_iter().collect(),
    }
}

/// The form a value is compared in: text lowercased, ids as decimal,
/// uuids hyphenated lowercase.
fn canon(kind: ConditionFieldKind, value: &Value) -> Option<String> {
    use ConditionFieldKind as K;
    match kind {
        K::String => value.as_str().map(str::to_lowercase),
        K::Enum => value.as_str().map(str::to_string),
        K::IntId => value.as_i64().map(|n| n.to_string()),
        K::Uuid => value
            .as_str()
            .and_then(|s| Uuid::parse_str(s).ok())
            .map(|u| u.to_string()),
        _ => None,
    }
}

fn check_scalar(field: ConditionField, value: &Value) -> Result<(), String> {
    use ConditionFieldKind as K;
    match field.kind() {
        K::String => text(value).map(drop),
        K::Enum => {
            let values = field.enum_values();
            match value.as_str() {
                Some(v) if values.contains(&v) => Ok(()),
                _ => Err(format!(
                    "{} is one of: {}",
                    field.as_str(),
                    values.join(", ")
                )),
            }
        }
        K::IntId => id(value).map(drop),
        K::Uuid => value
            .as_str()
            .and_then(|s| Uuid::parse_str(s).ok())
            .map(drop)
            .ok_or_else(|| "give a person's id".to_string()),
        other => Err(format!("a {} field takes no single value", other.as_str())),
    }
}

fn text(value: &Value) -> Result<&str, String> {
    match value.as_str() {
        Some("") => Err("give some text; empty text matches everything".to_string()),
        Some(s) if s.chars().count() > MAX_TEXT => {
            Err(format!("keep the text to {MAX_TEXT} characters"))
        }
        Some(s) => Ok(s),
        None => Err("give some text".to_string()),
    }
}

fn id(value: &Value) -> Result<i32, String> {
    value
        .as_i64()
        .filter(|n| *n > 0)
        .and_then(|n| i32::try_from(n).ok())
        .ok_or_else(|| "give an id, a whole number above 0".to_string())
}

fn list(value: &Value) -> Result<&Vec<Value>, String> {
    match value.as_array() {
        Some(items) if items.is_empty() => Err("list at least one value".to_string()),
        Some(items) if items.len() > MAX_LIST => Err(format!("list at most {MAX_LIST} values")),
        Some(items) => Ok(items),
        None => Err("give a list of values".to_string()),
    }
}

fn timestamp(value: &Value) -> Option<DateTime<Utc>> {
    value
        .as_str()
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|t| t.with_timezone(&Utc))
}

/// A pattern matches the whole text, so one opening or closing with an
/// unbounded wildcard is asking for `contains`, `starts_with` or
/// `ends_with` instead, and is refused.
fn compile_pattern(pattern: &str) -> Result<Regex, String> {
    if pattern.chars().count() > MAX_PATTERN {
        return Err(format!("keep the pattern to {MAX_PATTERN} characters"));
    }
    let core = pattern.strip_prefix('^').unwrap_or(pattern);
    let core = match core.strip_suffix('$') {
        Some(rest) if !rest.ends_with('\\') => rest,
        _ => core,
    };
    const WILD: [&str; 4] = [".*?", ".+?", ".*", ".+"];
    if WILD.iter().any(|w| core.starts_with(w)) {
        return Err(
            "a pattern can't open with .* or .+; use contains or ends_with instead".to_string(),
        );
    }
    let open_ended = WILD.iter().any(|w| {
        core.strip_suffix(w)
            .is_some_and(|rest| !rest.ends_with('\\'))
    });
    if open_ended {
        return Err(
            "a pattern can't close with .* or .+; use contains or starts_with instead".to_string(),
        );
    }
    RegexBuilder::new(&format!("^(?:{pattern})$"))
        .case_insensitive(true)
        .size_limit(REGEX_SIZE_LIMIT)
        .build()
        .map_err(|e| format!("the pattern doesn't parse: {e}"))
}

impl Predicate {
    /// No conditions: every ticket matches.
    pub fn is_empty(&self) -> bool {
        self.root.is_none()
    }

    /// The category, status and tag ids the tree names.
    pub fn references(&self) -> ConditionRefs {
        let mut refs = ConditionRefs::default();
        if let Some(root) = &self.root {
            collect_refs(root, &mut refs);
        }
        refs
    }

    /// Whether `ticket` meets the conditions. With no `event`, as when
    /// previewing a rule against past tickets, a leaf on an event field
    /// counts as met: the preview counts the tickets the rest of the tree
    /// matches.
    pub fn matches(&self, ticket: &TicketFacts, event: Option<&EventFacts>) -> bool {
        self.root
            .as_ref()
            .is_none_or(|root| self.eval(root, ticket, event))
    }

    fn eval(&self, node: &Condition, ticket: &TicketFacts, event: Option<&EventFacts>) -> bool {
        match node {
            Condition::And { children } => children.iter().all(|c| self.eval(c, ticket, event)),
            Condition::Or { children } => children.iter().any(|c| self.eval(c, ticket, event)),
            Condition::Not { child } => !self.eval(child, ticket, event),
            Condition::Leaf { field, op, value } => {
                let Some(fact) = fact(*field, ticket, event) else {
                    return true;
                };
                self.leaf(field.kind(), *op, value, fact)
            }
        }
    }

    fn leaf(&self, kind: ConditionFieldKind, op: ConditionOp, value: &Value, fact: Fact) -> bool {
        use ConditionOp::*;
        match fact {
            Fact::Scalar(have) => {
                let have = have.as_deref();
                let equals = |v: &Value| have.is_some() && canon(kind, v).as_deref() == have;
                let lower = || value.as_str().map(str::to_lowercase).unwrap_or_default();
                match op {
                    IsNull => have.is_none(),
                    IsNotNull => have.is_some(),
                    Eq => equals(value),
                    Neq => !equals(value),
                    In => value.as_array().is_some_and(|l| l.iter().any(equals)),
                    NotIn => !value.as_array().is_some_and(|l| l.iter().any(equals)),
                    Contains => have.is_some_and(|h| h.contains(&lower())),
                    NotContains => !have.is_some_and(|h| h.contains(&lower())),
                    StartsWith => have.is_some_and(|h| h.starts_with(&lower())),
                    EndsWith => have.is_some_and(|h| h.ends_with(&lower())),
                    MatchesRegex => have.is_some_and(|h| {
                        value
                            .as_str()
                            .and_then(|p| self.regexes.get(p))
                            .is_some_and(|re| re.is_match(h))
                    }),
                    _ => false,
                }
            }
            Fact::Time(have) => match op {
                IsNull => have.is_none(),
                IsNotNull => have.is_some(),
                _ => match (have, timestamp(value)) {
                    (Some(have), Some(want)) => ordered(op, have.cmp(&want)),
                    _ => false,
                },
            },
            Fact::Number(have) => match (have, value.as_i64()) {
                (Some(have), Some(want)) => ordered(op, have.cmp(&want)),
                _ => false,
            },
            Fact::Ints(have) => {
                let want = || {
                    value
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_i64)
                };
                let has = |n: i64| have.iter().any(|h| i64::from(*h) == n);
                match op {
                    IsNull => have.is_empty(),
                    IsNotNull => !have.is_empty(),
                    HasAny => want().any(has),
                    HasAll => want().all(has),
                    _ => false,
                }
            }
            Fact::Strings(have) => {
                let want = || {
                    value
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_str)
                };
                let has = |s: &str| have.iter().any(|h| h == s);
                match op {
                    IsNull => have.is_empty(),
                    IsNotNull => !have.is_empty(),
                    HasAny => want().any(has),
                    HasAll => want().all(has),
                    _ => false,
                }
            }
        }
    }
}

fn ordered(op: ConditionOp, ord: std::cmp::Ordering) -> bool {
    use std::cmp::Ordering::*;
    use ConditionOp::*;
    matches!(
        (op, ord),
        (Gt, Greater) | (Gte, Greater | Equal) | (Lt, Less) | (Lte, Less | Equal)
    )
}

/// One field's value on a ticket or event, in the form `leaf` compares.
enum Fact<'a> {
    /// See [`canon`].
    Scalar(Option<String>),
    Time(Option<DateTime<Utc>>),
    Number(Option<i64>),
    Ints(&'a [i32]),
    Strings(&'a [String]),
}

/// `None` for an event field when there is no event.
fn fact<'a>(
    field: ConditionField,
    ticket: &'a TicketFacts,
    event: Option<&'a EventFacts>,
) -> Option<Fact<'a>> {
    use ConditionField as F;
    let lower = |s: &Option<String>| s.as_deref().map(str::to_lowercase);
    Some(match field {
        F::Title => Fact::Scalar(Some(ticket.title.to_lowercase())),
        F::Priority => Fact::Scalar(Some(ticket.priority.as_str().to_string())),
        F::CategoryId => Fact::Scalar(ticket.category_id.map(|n| n.to_string())),
        F::RequesterUuid => Fact::Scalar(ticket.requester_uuid.map(|u| u.to_string())),
        F::AssigneeUuid => Fact::Scalar(ticket.assignee_uuid.map(|u| u.to_string())),
        F::WorkflowStateId => Fact::Scalar(Some(ticket.workflow_state_id.to_string())),
        F::WorkflowStateCategory => {
            Fact::Scalar(ticket.workflow_category.map(|c| c.as_str().to_string()))
        }
        F::TagIds => Fact::Ints(&ticket.tag_ids),
        F::CreatedAt => Fact::Time(Some(ticket.created_at)),
        F::UpdatedAt => Fact::Time(Some(ticket.updated_at)),
        F::FirstResponseAt => Fact::Time(ticket.first_response_at),
        F::ResolvedAt => Fact::Time(ticket.resolved_at),
        F::RequesterEmail => Fact::Scalar(lower(&ticket.requester_email)),
        F::RequesterOrganization => Fact::Scalar(lower(&ticket.requester_organization)),
        F::EventChangedFields => Fact::Strings(&event?.changed_fields),
        F::ReplyKind => Fact::Scalar(event?.reply_kind.clone()),
        F::ReplyAuthorRole => Fact::Scalar(lower(&event?.reply_author_role)),
        F::ClockMinutesSince => Fact::Number(event?.minutes_since),
        // Refused at save, so never evaluated.
        F::Status
        | F::Body
        | F::SourceChannelId
        | F::OrganizationId
        | F::RequesterOrganizationId => Fact::Scalar(None),
    })
}

fn collect_refs(node: &Condition, refs: &mut ConditionRefs) {
    match node {
        Condition::And { children } | Condition::Or { children } => {
            for child in children {
                collect_refs(child, refs);
            }
        }
        Condition::Not { child } => collect_refs(child, refs),
        Condition::Leaf { field, value, .. } => {
            let into = match field {
                ConditionField::CategoryId => &mut refs.category_ids,
                ConditionField::WorkflowStateId => &mut refs.workflow_state_ids,
                ConditionField::TagIds => &mut refs.tag_ids,
                _ => return,
            };
            let ids = match value {
                Value::Array(items) => items.iter().filter_map(|v| id(v).ok()).collect(),
                v => id(v).ok().into_iter().collect::<Vec<_>>(),
            };
            into.extend(ids);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use RuleTriggerKind::{TicketCreated, TicketReplied, TicketUpdated, TimeElapsed};

    fn leaf(field: &str, op: &str, value: Value) -> Value {
        json!({ "kind": "leaf", "field": field, "op": op, "value": value })
    }

    fn null_leaf(field: &str, op: &str) -> Value {
        json!({ "kind": "leaf", "field": field, "op": op })
    }

    fn refused(conditions: Value, trigger: RuleTriggerKind) -> ConditionError {
        compile(&conditions, trigger).expect_err(&conditions.to_string())
    }

    fn at(rfc3339: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(rfc3339)
            .unwrap()
            .with_timezone(&Utc)
    }

    fn ticket() -> TicketFacts {
        TicketFacts {
            id: 1,
            title: "Printer on floor 3 is jammed".into(),
            priority: TicketPriority::High,
            category_id: Some(12),
            requester_uuid: Some(Uuid::parse_str("019a0000-0000-7000-8000-00000000000a").unwrap()),
            assignee_uuid: None,
            workflow_state_id: 4,
            workflow_category: Some(WorkflowStateCategory::Active),
            tag_ids: vec![5, 7],
            created_at: at("2026-11-01T09:00:00Z"),
            updated_at: at("2026-11-02T09:00:00Z"),
            first_response_at: None,
            resolved_at: None,
            requester_email: Some("Pat@Example.com".into()),
            requester_organization: None,
        }
    }

    fn matches(conditions: Value, trigger: RuleTriggerKind) -> bool {
        compile(&conditions, trigger)
            .expect("valid")
            .matches(&ticket(), None)
    }

    /// One value each operator takes on each field kind, so every cell of
    /// the plan's matrix is exercised.
    fn sample(kind: ConditionFieldKind, op: ConditionOp) -> Option<Value> {
        use ConditionFieldKind as K;
        use ConditionOp::*;
        Some(match (kind, op) {
            (_, IsNull | IsNotNull) => Value::Null,
            (K::String, Eq | Neq) => json!("pat"),
            (K::String, In | NotIn) => json!(["pat", "sam"]),
            (K::String, Contains | NotContains | StartsWith | EndsWith) => json!("print"),
            (K::String, MatchesRegex) => json!("print(er|ing)"),
            (K::Enum, Eq | Neq) => return None,
            (K::Enum, In | NotIn) => return None,
            (K::IntId, Eq | Neq) => json!(3),
            (K::IntId, In | NotIn) => json!([3, 4]),
            (K::Uuid, Eq | Neq) => json!("019a0000-0000-7000-8000-00000000000a"),
            (K::Uuid, In | NotIn) => json!(["019a0000-0000-7000-8000-00000000000a"]),
            (K::IntArray, HasAny | HasAll) => json!([5]),
            (K::StringArray, HasAny | HasAll) => json!(["ticket.priority"]),
            (K::Timestamp, Gt | Lt | Gte | Lte) => json!("2026-01-31T09:00:00Z"),
            (K::Number, Gt | Lt | Gte | Lte) => json!(30),
            _ => return None,
        })
    }

    const FIELDS: [&str; 18] = [
        "ticket.priority",
        "ticket.category_id",
        "ticket.requester_uuid",
        "ticket.assignee_uuid",
        "ticket.tag_ids",
        "ticket.title",
        "ticket.workflow_state_id",
        "ticket.workflow_state.category",
        "ticket.created_at",
        "ticket.updated_at",
        "ticket.first_response_at",
        "ticket.resolved_at",
        "ticket.requester.email",
        "ticket.requester.organization",
        "event.changed_fields",
        "reply.kind",
        "reply.author_role",
        "clock.minutes_since",
    ];

    const OPS: [ConditionOp; 17] = {
        use ConditionOp::*;
        [
            Eq,
            Neq,
            In,
            NotIn,
            Contains,
            NotContains,
            StartsWith,
            EndsWith,
            Gt,
            Lt,
            Gte,
            Lte,
            MatchesRegex,
            IsNull,
            IsNotNull,
            HasAny,
            HasAll,
        ]
    };

    fn trigger_for(field: ConditionField) -> RuleTriggerKind {
        field.trigger_only().unwrap_or(TicketCreated)
    }

    /// Every field takes exactly the operators its kind allows: an allowed
    /// pairing saves, any other is refused naming the field, operator and
    /// kind.
    #[test]
    fn the_operator_matrix_holds_for_every_field() {
        for name in FIELDS {
            let field = ConditionField::parse(name).expect(name);
            let kind = field.kind();
            for op in OPS {
                let allowed = kind.allowed_ops().contains(&op);
                let value = match op {
                    ConditionOp::Eq | ConditionOp::Neq if kind == ConditionFieldKind::Enum => {
                        json!(field.enum_values()[0])
                    }
                    ConditionOp::In | ConditionOp::NotIn if kind == ConditionFieldKind::Enum => {
                        json!([field.enum_values()[0]])
                    }
                    _ => sample(kind, op).unwrap_or(json!("x")),
                };
                let tree = if value.is_null() {
                    null_leaf(name, op.as_str())
                } else {
                    leaf(name, op.as_str(), value)
                };
                let result = compile(&tree, trigger_for(field));
                if !allowed {
                    let e = result.expect_err(&format!("{name} {}", op.as_str()));
                    assert_eq!(e.field, Some(field));
                    assert_eq!(e.op, Some(op));
                    assert_eq!(
                        e.message,
                        format!(
                            "{} doesn't apply to {name}, a {} field",
                            op.as_str(),
                            kind.as_str()
                        )
                    );
                    continue;
                }
                let null_check = matches!(op, ConditionOp::IsNull | ConditionOp::IsNotNull);
                if null_check && !field.nullable() {
                    assert!(
                        result.is_err(),
                        "{name} {} on a field with a value",
                        op.as_str()
                    );
                } else {
                    result.unwrap_or_else(|e| panic!("{name} {}: {e}", op.as_str()));
                }
            }
        }
    }

    #[test]
    fn the_matrix_matches_the_plan() {
        use ConditionFieldKind as K;
        let ops = |k: K| -> Vec<&str> { k.allowed_ops().iter().map(|o| o.as_str()).collect() };
        assert_eq!(
            ops(K::String),
            [
                "eq",
                "neq",
                "in",
                "not_in",
                "contains",
                "not_contains",
                "starts_with",
                "ends_with",
                "matches_regex",
                "is_null",
                "is_not_null"
            ]
        );
        for k in [K::Enum, K::IntId, K::Uuid] {
            assert_eq!(
                ops(k),
                ["eq", "neq", "in", "not_in", "is_null", "is_not_null"]
            );
        }
        for k in [K::IntArray, K::StringArray] {
            assert_eq!(ops(k), ["has_any", "has_all", "is_null", "is_not_null"]);
        }
        assert_eq!(
            ops(K::Timestamp),
            ["gt", "lt", "gte", "lte", "is_null", "is_not_null"]
        );
        assert_eq!(ops(K::Number), ["gt", "lt", "gte", "lte"]);
    }

    #[test]
    fn empty_conditions_match_every_ticket() {
        let p = compile(&json!([]), TicketCreated).unwrap();
        assert!(p.is_empty());
        assert!(p.matches(&ticket(), None));
        assert_eq!(p.references(), ConditionRefs::default());
    }

    #[test]
    fn the_root_is_one_object() {
        let e = refused(
            json!([leaf("ticket.priority", "eq", json!("high"))]),
            TicketCreated,
        );
        assert_eq!(e.path, "conditions");
        assert!(e.message.contains("not a list"), "{}", e.message);
        refused(json!("ticket.priority"), TicketCreated);
        refused(json!(null), TicketCreated);
    }

    #[test]
    fn unknown_shapes_are_refused() {
        refused(json!({ "kind": "xor", "children": [] }), TicketCreated);
        refused(leaf("ticket.colour", "eq", json!("red")), TicketCreated);
        refused(
            leaf("ticket.priority", "equals", json!("high")),
            TicketCreated,
        );
        // The plan's TS type says `operator`; the stored shape says `op`.
        refused(
            json!({ "kind": "leaf", "field": "ticket.priority", "operator": "eq", "value": "high" }),
            TicketCreated,
        );
    }

    #[test]
    fn fields_with_no_data_yet_are_refused() {
        for name in [
            "ticket.status",
            "ticket.body",
            "ticket.source_channel_id",
            "ticket.organization_id",
            "ticket.requester.organization_id",
        ] {
            let field = ConditionField::parse(name).unwrap();
            let value = match field.kind() {
                ConditionFieldKind::IntId => json!(1),
                _ => json!("open"),
            };
            let e = refused(leaf(name, "eq", value), TicketCreated);
            assert_eq!(e.field, Some(field), "{name}");
            assert_eq!(Some(e.message.as_str()), field.unavailable(), "{name}");
        }
    }

    #[test]
    fn event_fields_belong_to_their_trigger() {
        let e = refused(leaf("reply.kind", "eq", json!("customer")), TicketUpdated);
        assert_eq!(e.message, "reply.kind only applies to ticket_replied rules");
        refused(
            leaf(
                "event.changed_fields",
                "has_any",
                json!(["ticket.priority"]),
            ),
            TicketReplied,
        );
        refused(leaf("clock.minutes_since", "gte", json!(60)), TicketCreated);
        compile(&leaf("reply.kind", "eq", json!("customer")), TicketReplied).unwrap();
        compile(&leaf("clock.minutes_since", "gte", json!(60)), TimeElapsed).unwrap();
    }

    #[test]
    fn values_must_fit_the_field() {
        for (tree, message) in [
            (
                leaf("ticket.priority", "eq", json!("critical")),
                "ticket.priority is one of",
            ),
            (leaf("ticket.category_id", "eq", json!(0)), "give an id"),
            (leaf("ticket.category_id", "eq", json!("12")), "give an id"),
            (
                leaf("ticket.assignee_uuid", "eq", json!("someone")),
                "give a person's id",
            ),
            (
                leaf("ticket.created_at", "gt", json!("yesterday")),
                "give a date and time",
            ),
            (
                leaf("clock.minutes_since", "gt", json!(-5)),
                "whole number of minutes",
            ),
            (
                leaf("ticket.tag_ids", "has_any", json!(["5"])),
                "give an id",
            ),
            (leaf("ticket.title", "eq", json!(5)), "give some text"),
            (
                leaf("ticket.title", "contains", json!("x".repeat(MAX_TEXT + 1))),
                "keep the text",
            ),
            (
                json!({ "kind": "leaf", "field": "ticket.assignee_uuid", "op": "is_null", "value": "x" }),
                "takes no value",
            ),
        ] {
            let trigger = if tree["field"] == "clock.minutes_since" {
                TimeElapsed
            } else {
                TicketCreated
            };
            let e = refused(tree.clone(), trigger);
            assert!(e.message.contains(message), "{tree}: {}", e.message);
        }
    }

    #[test]
    fn empty_lists_are_refused() {
        for (field, op) in [
            ("ticket.priority", "in"),
            ("ticket.priority", "not_in"),
            ("ticket.tag_ids", "has_any"),
            ("ticket.tag_ids", "has_all"),
        ] {
            let e = refused(leaf(field, op, json!([])), TicketCreated);
            assert_eq!(e.message, "list at least one value", "{field} {op}");
        }
        let long: Vec<i32> = (1..=MAX_LIST as i32 + 1).collect();
        refused(leaf("ticket.category_id", "in", json!(long)), TicketCreated);
    }

    #[test]
    fn leaves_that_always_or_never_match_are_refused() {
        for tree in [
            null_leaf("ticket.title", "is_not_null"),
            null_leaf("ticket.priority", "is_null"),
            null_leaf("ticket.created_at", "is_not_null"),
            leaf("ticket.title", "contains", json!("")),
            leaf("ticket.title", "starts_with", json!("")),
            leaf("ticket.title", "not_contains", json!("")),
            leaf(
                "ticket.priority",
                "in",
                json!(["none", "low", "medium", "high", "urgent"]),
            ),
            leaf(
                "ticket.priority",
                "not_in",
                json!(["urgent", "high", "medium", "low", "none"]),
            ),
        ] {
            refused(tree, TicketCreated);
        }
        refused(leaf("clock.minutes_since", "gte", json!(0)), TimeElapsed);
        refused(leaf("clock.minutes_since", "lt", json!(0)), TimeElapsed);
        // A nullable field's emptiness is a real question.
        compile(&null_leaf("ticket.assignee_uuid", "is_null"), TicketCreated).unwrap();
        compile(&null_leaf("ticket.tag_ids", "is_null"), TicketCreated).unwrap();
    }

    #[test]
    fn regex_patterns_are_anchored_and_bounded() {
        for pattern in [
            ".*printer",
            ".+printer",
            "printer.*",
            "printer.+?",
            "^.*printer$",
        ] {
            let e = refused(
                leaf("ticket.title", "matches_regex", json!(pattern)),
                TicketCreated,
            );
            assert!(e.message.contains(".*"), "{pattern}: {}", e.message);
        }
        refused(
            leaf("ticket.title", "matches_regex", json!("(unclosed")),
            TicketCreated,
        );
        refused(
            leaf(
                "ticket.title",
                "matches_regex",
                json!("a".repeat(MAX_PATTERN + 1)),
            ),
            TicketCreated,
        );
        // An escaped dot before the star is literal, not a wildcard.
        compile(
            &leaf("ticket.title", "matches_regex", json!("v1\\.*")),
            TicketCreated,
        )
        .unwrap();

        // Anchored: the pattern has to cover the whole title.
        assert!(!matches(
            leaf("ticket.title", "matches_regex", json!("printer")),
            TicketCreated
        ));
        assert!(matches(
            leaf(
                "ticket.title",
                "matches_regex",
                json!("printer on floor \\d+ is (jammed|offline)")
            ),
            TicketCreated
        ));
    }

    #[test]
    fn an_and_group_that_cannot_hold_is_refused() {
        let and = |a: Value, b: Value| json!({ "kind": "and", "children": [a, b] });
        for tree in [
            and(
                leaf("ticket.priority", "eq", json!("high")),
                leaf("ticket.priority", "eq", json!("low")),
            ),
            and(
                leaf("ticket.priority", "eq", json!("high")),
                leaf("ticket.priority", "neq", json!("high")),
            ),
            and(
                leaf("ticket.category_id", "in", json!([1, 2])),
                leaf("ticket.category_id", "in", json!([3])),
            ),
            and(
                leaf("ticket.priority", "in", json!(["high", "urgent"])),
                leaf("ticket.priority", "not_in", json!(["urgent", "high"])),
            ),
            and(
                null_leaf("ticket.assignee_uuid", "is_null"),
                null_leaf("ticket.assignee_uuid", "is_not_null"),
            ),
            and(
                null_leaf("ticket.category_id", "is_null"),
                leaf("ticket.category_id", "eq", json!(3)),
            ),
            // Text compares without case.
            and(
                leaf("ticket.requester.email", "eq", json!("pat@example.com")),
                leaf("ticket.requester.email", "neq", json!("PAT@example.com")),
            ),
        ] {
            let e = refused(tree.clone(), TicketCreated);
            assert_eq!(e.path, "conditions", "{tree}");
            assert!(e.message.contains("can't all hold"), "{tree}");
        }
        // The same pair under `or`, or overlapping lists, is fine.
        compile(
            &json!({ "kind": "or", "children": [
                leaf("ticket.priority", "eq", json!("high")),
                leaf("ticket.priority", "eq", json!("low")),
            ]}),
            TicketCreated,
        )
        .unwrap();
        compile(
            &and(
                leaf("ticket.priority", "in", json!(["high", "urgent"])),
                leaf("ticket.priority", "neq", json!("urgent")),
            ),
            TicketCreated,
        )
        .unwrap();
    }

    #[test]
    fn groups_must_be_non_empty_and_bounded() {
        let e = refused(json!({ "kind": "and", "children": [] }), TicketCreated);
        assert_eq!(e.message, "a group needs at least one condition");

        let mut deep = leaf("ticket.priority", "eq", json!("high"));
        for _ in 0..MAX_DEPTH {
            deep = json!({ "kind": "not", "child": deep });
        }
        let e = refused(deep, TicketCreated);
        assert!(e.message.contains("nest"), "{}", e.message);

        let wide: Vec<Value> = (1..=MAX_NODES as i64)
            .map(|n| leaf("ticket.category_id", "neq", json!(n)))
            .collect();
        let e = refused(json!({ "kind": "or", "children": wide }), TicketCreated);
        assert!(e.message.contains("at most"), "{}", e.message);
    }

    #[test]
    fn an_error_points_at_the_node() {
        let tree = json!({ "kind": "and", "children": [
            leaf("ticket.priority", "eq", json!("high")),
            { "kind": "not", "child": leaf("ticket.title", "gt", json!("a")) },
        ]});
        let e = refused(tree, TicketCreated);
        assert_eq!(e.path, "conditions.children[1].child");
        assert_eq!(e.field, Some(ConditionField::Title));
        assert_eq!(e.op, Some(ConditionOp::Gt));
        assert_eq!(e.field_kind(), Some(ConditionFieldKind::String));
    }

    #[test]
    fn text_fields_compare_without_case() {
        assert!(matches(
            leaf("ticket.title", "contains", json!("PRINTER")),
            TicketCreated
        ));
        assert!(matches(
            leaf("ticket.title", "starts_with", json!("printer on")),
            TicketCreated
        ));
        assert!(matches(
            leaf("ticket.title", "ends_with", json!("Jammed")),
            TicketCreated
        ));
        assert!(!matches(
            leaf("ticket.title", "not_contains", json!("floor")),
            TicketCreated
        ));
        assert!(matches(
            leaf("ticket.requester.email", "ends_with", json!("@example.COM")),
            TicketCreated
        ));
        assert!(matches(
            leaf(
                "ticket.requester.email",
                "in",
                json!(["sam@x.com", "pat@example.com"])
            ),
            TicketCreated
        ));
        assert!(matches(
            leaf("ticket.requester.email", "eq", json!("pat@example.com")),
            TicketCreated
        ));
    }

    #[test]
    fn enums_ids_and_uuids_compare_exactly() {
        assert!(matches(
            leaf("ticket.priority", "eq", json!("high")),
            TicketCreated
        ));
        assert!(matches(
            leaf("ticket.priority", "in", json!(["urgent", "high"])),
            TicketCreated
        ));
        assert!(!matches(
            leaf("ticket.priority", "not_in", json!(["high"])),
            TicketCreated
        ));
        assert!(matches(
            leaf("ticket.workflow_state.category", "eq", json!("active")),
            TicketCreated
        ));
        assert!(matches(
            leaf("ticket.workflow_state_id", "eq", json!(4)),
            TicketCreated
        ));
        assert!(matches(
            leaf("ticket.category_id", "neq", json!(13)),
            TicketCreated
        ));
        assert!(matches(
            leaf(
                "ticket.requester_uuid",
                "eq",
                json!("019A0000-0000-7000-8000-00000000000A")
            ),
            TicketCreated
        ));
    }

    /// Looking for a value on an empty field doesn't match; excluding one
    /// does.
    #[test]
    fn an_empty_field_fails_positive_checks_and_passes_negative_ones() {
        let someone = json!("019a0000-0000-7000-8000-00000000000b");
        assert!(!matches(
            leaf("ticket.assignee_uuid", "eq", someone.clone()),
            TicketCreated
        ));
        assert!(!matches(
            leaf("ticket.assignee_uuid", "in", json!([someone])),
            TicketCreated
        ));
        assert!(matches(
            leaf("ticket.assignee_uuid", "neq", someone.clone()),
            TicketCreated
        ));
        assert!(matches(
            leaf("ticket.assignee_uuid", "not_in", json!([someone])),
            TicketCreated
        ));
        assert!(matches(
            null_leaf("ticket.assignee_uuid", "is_null"),
            TicketCreated
        ));
        assert!(!matches(
            null_leaf("ticket.assignee_uuid", "is_not_null"),
            TicketCreated
        ));
        assert!(!matches(
            leaf("ticket.requester.organization", "contains", json!("acme")),
            TicketCreated
        ));
        assert!(matches(
            leaf(
                "ticket.requester.organization",
                "not_contains",
                json!("acme")
            ),
            TicketCreated
        ));
        assert!(!matches(
            leaf(
                "ticket.first_response_at",
                "lt",
                json!("2030-01-01T00:00:00Z")
            ),
            TicketCreated
        ));
        assert!(matches(
            null_leaf("ticket.resolved_at", "is_null"),
            TicketCreated
        ));
    }

    #[test]
    fn timestamps_compare_in_order() {
        let created = |op: &str, when: &str| leaf("ticket.created_at", op, json!(when));
        assert!(matches(
            created("gt", "2026-10-31T09:00:00Z"),
            TicketCreated
        ));
        assert!(!matches(
            created("gt", "2026-11-01T09:00:00Z"),
            TicketCreated
        ));
        assert!(matches(
            created("gte", "2026-11-01T09:00:00Z"),
            TicketCreated
        ));
        assert!(matches(
            created("lte", "2026-11-01T20:00:00+11:00"),
            TicketCreated
        ));
        assert!(!matches(
            created("lt", "2026-11-01T09:00:00Z"),
            TicketCreated
        ));
    }

    #[test]
    fn tag_lists_match_any_or_all() {
        assert!(matches(
            leaf("ticket.tag_ids", "has_any", json!([1, 7])),
            TicketCreated
        ));
        assert!(!matches(
            leaf("ticket.tag_ids", "has_any", json!([1, 2])),
            TicketCreated
        ));
        assert!(matches(
            leaf("ticket.tag_ids", "has_all", json!([5, 7])),
            TicketCreated
        ));
        assert!(!matches(
            leaf("ticket.tag_ids", "has_all", json!([5, 9])),
            TicketCreated
        ));
        assert!(matches(
            null_leaf("ticket.tag_ids", "is_not_null"),
            TicketCreated
        ));
        assert!(!matches(
            null_leaf("ticket.tag_ids", "is_null"),
            TicketCreated
        ));
    }

    #[test]
    fn groups_combine_their_children() {
        let high = leaf("ticket.priority", "eq", json!("high"));
        let low = leaf("ticket.priority", "eq", json!("low"));
        let printer = leaf("ticket.title", "contains", json!("printer"));
        assert!(matches(
            json!({ "kind": "and", "children": [high, printer] }),
            TicketCreated
        ));
        assert!(!matches(
            json!({ "kind": "and", "children": [low, printer] }),
            TicketCreated
        ));
        assert!(matches(
            json!({ "kind": "or", "children": [low, printer] }),
            TicketCreated
        ));
        assert!(!matches(
            json!({ "kind": "not", "child": printer }),
            TicketCreated
        ));
        // The plan's negated subtree example.
        let spam = json!({
            "kind": "not",
            "child": {
                "kind": "or",
                "children": [
                    leaf("ticket.tag_ids", "has_any", json!([5, 9])),
                    leaf("ticket.requester.email", "ends_with", json!("@spam.example")),
                ]
            }
        });
        assert!(!matches(spam, TicketCreated));
    }

    #[test]
    fn event_facts_decide_event_leaves_and_their_absence_counts_as_met() {
        let changed = compile(
            &json!({ "kind": "and", "children": [
                leaf("event.changed_fields", "has_any", json!(["ticket.priority"])),
                leaf("ticket.priority", "eq", json!("high")),
            ]}),
            TicketUpdated,
        )
        .unwrap();
        let event = |fields: &[&str]| EventFacts {
            changed_fields: fields.iter().map(|f| f.to_string()).collect(),
            ..Default::default()
        };
        assert!(changed.matches(&ticket(), Some(&event(&["ticket.priority"]))));
        assert!(!changed.matches(&ticket(), Some(&event(&["ticket.title"]))));
        assert!(changed.matches(&ticket(), None));

        let replied = compile(&leaf("reply.kind", "eq", json!("customer")), TicketReplied).unwrap();
        let reply = |kind: &str| EventFacts {
            reply_kind: Some(kind.to_string()),
            ..Default::default()
        };
        assert!(replied.matches(&ticket(), Some(&reply("customer"))));
        assert!(!replied.matches(&ticket(), Some(&reply("agent"))));

        let waited = compile(&leaf("clock.minutes_since", "gte", json!(120)), TimeElapsed).unwrap();
        let clock = |minutes: i64| EventFacts {
            minutes_since: Some(minutes),
            ..Default::default()
        };
        assert!(waited.matches(&ticket(), Some(&clock(120))));
        assert!(!waited.matches(&ticket(), Some(&clock(119))));
    }

    #[test]
    fn references_collect_the_ids_to_check() {
        let p = compile(
            &json!({ "kind": "or", "children": [
                leaf("ticket.category_id", "in", json!([3, 4])),
                { "kind": "not", "child": leaf("ticket.workflow_state_id", "eq", json!(9)) },
                leaf("ticket.tag_ids", "has_all", json!([5, 6])),
                leaf("ticket.priority", "eq", json!("high")),
            ]}),
            TicketCreated,
        )
        .unwrap();
        assert_eq!(
            p.references(),
            ConditionRefs {
                category_ids: [3, 4].into(),
                workflow_state_ids: [9].into(),
                tag_ids: [5, 6].into(),
            }
        );
    }

    /// The persisted shape round-trips through the typed model unchanged,
    /// so the archive walker and the editor read what they wrote.
    #[test]
    fn the_canonical_shape_round_trips() {
        let tree = json!({
            "kind": "and",
            "children": [
                { "kind": "leaf", "field": "ticket.priority", "op": "in", "value": ["high", "urgent"] },
                { "kind": "leaf", "field": "ticket.category_id", "op": "eq", "value": 12 },
                { "kind": "not", "child": { "kind": "leaf", "field": "ticket.assignee_uuid", "op": "is_null", "value": null } }
            ]
        });
        let typed: Condition = serde_json::from_value(tree.clone()).unwrap();
        assert_eq!(serde_json::to_value(&typed).unwrap(), tree);
    }
}
