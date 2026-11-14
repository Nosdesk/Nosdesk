use chrono::{DateTime, Utc};
use diesel::deserialize::{self, FromSql};
use diesel::pg::{Pg, PgValue};
use diesel::prelude::*;
use diesel::serialize::{self, IsNull, Output, ToSql};
use serde::{Deserialize, Serialize};
use std::io::Write;
use uuid::Uuid;

// =====================================================================
// Rules engine. Phase 1 ships the
// manual-trigger surface; the data model below is the unified shape
// Phase 2 (event triggers), Phase 3 (time-elapsed), and Phase 4
// (webhook actions) extend without schema changes.
// =====================================================================

/// Workflow state of a rule. `draft` is the editor's initial state,
/// `dry_run` writes shadow rule_applications rows the admin reviews
/// before flipping to `live` (automated triggers only; a manual rule
/// has nothing to shadow), `live` is in-flight, `paused` is a live
/// rule an admin stopped (agents can't apply it, nothing fires it),
/// `archived` is a soft-deleted row that the picker / engine ignore.
/// See decisions 12 + 32 in the plan: archived is the only state
/// hard-delete is permitted from.
#[derive(
    Debug,
    Serialize,
    Deserialize,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    diesel::deserialize::FromSqlRow,
    diesel::expression::AsExpression,
)]
#[diesel(sql_type = crate::schema::sql_types::RuleState)]
pub enum RuleState {
    #[serde(rename = "draft")]
    Draft,
    #[serde(rename = "dry_run")]
    DryRun,
    #[serde(rename = "live")]
    Live,
    #[serde(rename = "paused")]
    Paused,
    #[serde(rename = "archived")]
    Archived,
}

impl RuleState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::DryRun => "dry_run",
            Self::Live => "live",
            Self::Paused => "paused",
            Self::Archived => "archived",
        }
    }
}

impl ToSql<crate::schema::sql_types::RuleState, Pg> for RuleState {
    fn to_sql<'b>(&'b self, out: &mut Output<'b, '_, Pg>) -> serialize::Result {
        out.write_all(self.as_str().as_bytes())?;
        Ok(IsNull::No)
    }
}

impl FromSql<crate::schema::sql_types::RuleState, Pg> for RuleState {
    fn from_sql(bytes: PgValue) -> deserialize::Result<Self> {
        match bytes.as_bytes() {
            b"draft" => Ok(Self::Draft),
            b"dry_run" => Ok(Self::DryRun),
            b"live" => Ok(Self::Live),
            b"paused" => Ok(Self::Paused),
            b"archived" => Ok(Self::Archived),
            other => Err(format!("unknown rule_state: {}", String::from_utf8_lossy(other)).into()),
        }
    }
}

/// What kind of event a rule reacts to. `manual` (Phase 1) is fired
/// from the agent Actions toolbar; the rest (Phase 2+) are fired by
/// the engine subscribing to the existing `sync_actions` stream
/// (ticket_created / ticket_updated / ticket_replied) or the
/// per-ticket due_at queue (time_elapsed).
#[derive(
    Debug,
    Serialize,
    Deserialize,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    diesel::deserialize::FromSqlRow,
    diesel::expression::AsExpression,
)]
#[diesel(sql_type = crate::schema::sql_types::RuleTriggerKind)]
pub enum RuleTriggerKind {
    #[serde(rename = "manual")]
    Manual,
    #[serde(rename = "ticket_created")]
    TicketCreated,
    #[serde(rename = "ticket_updated")]
    TicketUpdated,
    #[serde(rename = "ticket_replied")]
    TicketReplied,
    #[serde(rename = "time_elapsed")]
    TimeElapsed,
}

impl RuleTriggerKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::TicketCreated => "ticket_created",
            Self::TicketUpdated => "ticket_updated",
            Self::TicketReplied => "ticket_replied",
            Self::TimeElapsed => "time_elapsed",
        }
    }
}

impl ToSql<crate::schema::sql_types::RuleTriggerKind, Pg> for RuleTriggerKind {
    fn to_sql<'b>(&'b self, out: &mut Output<'b, '_, Pg>) -> serialize::Result {
        out.write_all(self.as_str().as_bytes())?;
        Ok(IsNull::No)
    }
}

impl FromSql<crate::schema::sql_types::RuleTriggerKind, Pg> for RuleTriggerKind {
    fn from_sql(bytes: PgValue) -> deserialize::Result<Self> {
        match bytes.as_bytes() {
            b"manual" => Ok(Self::Manual),
            b"ticket_created" => Ok(Self::TicketCreated),
            b"ticket_updated" => Ok(Self::TicketUpdated),
            b"ticket_replied" => Ok(Self::TicketReplied),
            b"time_elapsed" => Ok(Self::TimeElapsed),
            other => Err(format!(
                "unknown rule_trigger_kind: {}",
                String::from_utf8_lossy(other)
            )
            .into()),
        }
    }
}

/// Outcome of one fire attempt. Captured on every `rule_applications`
/// row so the admin log can answer "why didn't this rule fire?"
/// without log-tailing. See plan §4.3.
#[derive(
    Debug,
    Serialize,
    Deserialize,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    diesel::deserialize::FromSqlRow,
    diesel::expression::AsExpression,
)]
#[diesel(sql_type = crate::schema::sql_types::RuleApplicationStatus)]
pub enum RuleApplicationStatus {
    #[serde(rename = "succeeded")]
    Succeeded,
    #[serde(rename = "dry_run")]
    DryRun,
    #[serde(rename = "skipped_preflight")]
    SkippedPreflight,
    #[serde(rename = "skipped_condition_unmet")]
    SkippedConditionUnmet,
    #[serde(rename = "suppressed_recursion_budget")]
    SuppressedRecursionBudget,
    #[serde(rename = "suppressed_loop_guard")]
    SuppressedLoopGuard,
    #[serde(rename = "failed")]
    Failed,
}

impl RuleApplicationStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Succeeded => "succeeded",
            Self::DryRun => "dry_run",
            Self::SkippedPreflight => "skipped_preflight",
            Self::SkippedConditionUnmet => "skipped_condition_unmet",
            Self::SuppressedRecursionBudget => "suppressed_recursion_budget",
            Self::SuppressedLoopGuard => "suppressed_loop_guard",
            Self::Failed => "failed",
        }
    }
}

impl ToSql<crate::schema::sql_types::RuleApplicationStatus, Pg> for RuleApplicationStatus {
    fn to_sql<'b>(&'b self, out: &mut Output<'b, '_, Pg>) -> serialize::Result {
        out.write_all(self.as_str().as_bytes())?;
        Ok(IsNull::No)
    }
}

impl FromSql<crate::schema::sql_types::RuleApplicationStatus, Pg> for RuleApplicationStatus {
    fn from_sql(bytes: PgValue) -> deserialize::Result<Self> {
        match bytes.as_bytes() {
            b"succeeded" => Ok(Self::Succeeded),
            b"dry_run" => Ok(Self::DryRun),
            b"skipped_preflight" => Ok(Self::SkippedPreflight),
            b"skipped_condition_unmet" => Ok(Self::SkippedConditionUnmet),
            b"suppressed_recursion_budget" => Ok(Self::SuppressedRecursionBudget),
            b"suppressed_loop_guard" => Ok(Self::SuppressedLoopGuard),
            b"failed" => Ok(Self::Failed),
            other => Err(format!(
                "unknown rule_application_status: {}",
                String::from_utf8_lossy(other)
            )
            .into()),
        }
    }
}

/// One rule. Read view; INSERTs go via `NewRule` and UPDATEs via
/// `RuleUpdate`. `reads_set` and `writes_set` are derived from
/// `conditions` and `actions` at save time by the repository helper.
#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable)]
#[diesel(table_name = crate::schema::rules)]
pub struct Rule {
    pub id: i32,
    pub workspace_id: i32,
    pub name: String,
    pub description: Option<String>,
    pub trigger_kind: RuleTriggerKind,
    pub trigger_config: serde_json::Value,
    pub conditions: serde_json::Value,
    pub actions: serde_json::Value,
    // Postgres TEXT[] elements are nullable by default. The repository
    // helpers only ever insert non-null values; reads expose the
    // Option<String> shape Diesel's schema requires. Convert at the
    // boundary in the API layer if a flat Vec<String> is needed.
    pub reads_set: Vec<Option<String>>,
    pub writes_set: Vec<Option<String>>,
    pub state: RuleState,
    pub priority: i32,
    pub last_fired_at: Option<DateTime<Utc>>,
    pub fire_count: i32,
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub archived_at: Option<DateTime<Utc>>,
}

impl Rule {
    /// An archived rule is read-only: no edits and no state changes. The
    /// rules list's Delete stamps only `archived_at`, moving the state to
    /// archived sets both, so either marks it. Restoring one has to clear
    /// both.
    pub fn is_archived(&self) -> bool {
        self.archived_at.is_some() || self.state == RuleState::Archived
    }
}

/// INSERT row for `rules`. The repository populates `reads_set` and
/// `writes_set` from the conditions / actions trees before passing
/// this in, so the engine's skip-on-no-reads-changed query path and
/// the self-referential save linter both work off durable columns.
#[derive(Debug, Insertable)]
#[diesel(table_name = crate::schema::rules)]
pub struct NewRule {
    pub workspace_id: i32,
    pub name: String,
    pub description: Option<String>,
    pub trigger_kind: RuleTriggerKind,
    pub trigger_config: serde_json::Value,
    pub conditions: serde_json::Value,
    pub actions: serde_json::Value,
    pub reads_set: Vec<Option<String>>,
    pub writes_set: Vec<Option<String>>,
    pub state: RuleState,
    pub priority: i32,
    pub created_by: Option<Uuid>,
}

/// PATCH row for `rules`. Every field is `Option`-wrapped so the
/// editor's partial updates round-trip without clobbering unsent
/// fields. The repository's update path recomputes `reads_set` /
/// `writes_set` when conditions or actions move.
#[derive(Debug, Default, AsChangeset)]
#[diesel(table_name = crate::schema::rules)]
pub struct RuleUpdate {
    pub name: Option<String>,
    pub description: Option<Option<String>>,
    pub trigger_kind: Option<RuleTriggerKind>,
    pub trigger_config: Option<serde_json::Value>,
    pub conditions: Option<serde_json::Value>,
    pub actions: Option<serde_json::Value>,
    pub reads_set: Option<Vec<Option<String>>>,
    pub writes_set: Option<Vec<Option<String>>>,
    pub state: Option<RuleState>,
    pub priority: Option<i32>,
    pub last_fired_at: Option<Option<DateTime<Utc>>>,
    pub fire_count: Option<i32>,
    pub archived_at: Option<Option<DateTime<Utc>>>,
}

/// One immutable snapshot of a rule's fields at save time. The
/// repository never INSERTs into this table directly; the
/// `rules_version_on_insert` and `rules_version_on_update` triggers
/// in the migration do.
#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable)]
#[diesel(table_name = crate::schema::rule_versions)]
pub struct RuleVersion {
    pub id: i32,
    pub rule_id: i32,
    pub workspace_id: i32,
    pub version: i32,
    pub name: String,
    pub description: Option<String>,
    pub trigger_kind: RuleTriggerKind,
    pub trigger_config: serde_json::Value,
    pub conditions: serde_json::Value,
    pub actions: serde_json::Value,
    pub state: RuleState,
    pub priority: i32,
    pub saved_by: Option<Uuid>,
    pub saved_at: DateTime<Utc>,
}

/// One row per fire attempt. See plan §4.3 for the shape: succeeded
/// rows are tight (no payloads); dry_run + failed + suppressed_* +
/// skipped_* rows carry condition / action snapshots for the
/// inspector to render.
#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable)]
#[diesel(table_name = crate::schema::rule_applications)]
pub struct RuleApplication {
    pub id: i64,
    pub workspace_id: i32,
    pub rule_id: i32,
    pub rule_version: i32,
    pub ticket_id: i32,
    pub status: RuleApplicationStatus,
    pub correlation_id: Option<Uuid>,
    pub actor_uuid: Option<Uuid>,
    pub actor_kind: String,
    pub originating_event_id: Option<Uuid>,
    pub originating_event_kind: Option<String>,
    pub condition_evaluation: Option<serde_json::Value>,
    pub actions_taken: Option<serde_json::Value>,
    pub actions_skipped: Option<serde_json::Value>,
    pub failure_reason: Option<String>,
    pub applied_at: DateTime<Utc>,
}

/// INSERT row for `rule_applications`. Built by the engine inside
/// the apply transaction so the correlation_id stitches with the
/// audit_log + sync_actions rows the same apply produced.
#[derive(Debug, Insertable)]
#[diesel(table_name = crate::schema::rule_applications)]
pub struct NewRuleApplication {
    pub workspace_id: i32,
    pub rule_id: i32,
    pub rule_version: i32,
    pub ticket_id: i32,
    pub status: RuleApplicationStatus,
    pub correlation_id: Option<Uuid>,
    pub actor_uuid: Option<Uuid>,
    pub actor_kind: String,
    pub originating_event_id: Option<Uuid>,
    pub originating_event_kind: Option<String>,
    pub condition_evaluation: Option<serde_json::Value>,
    pub actions_taken: Option<serde_json::Value>,
    pub actions_skipped: Option<serde_json::Value>,
    pub failure_reason: Option<String>,
}

// =====================================================================
// Typed condition tree (plan §5.2). The `conditions` column holds `[]`
// (no conditions, the only value a manual rule takes) or one node in
// the canonical shape below; `services::rules::conditions` validates a
// tree at save time and evaluates it against a ticket.
// =====================================================================

/// One node of a rule's condition tree, in its persisted JSON shape:
/// `{"kind": "leaf", "field": .., "op": .., "value": ..}`,
/// `{"kind": "and" | "or", "children": [..]}` or
/// `{"kind": "not", "child": {..}}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Condition {
    And {
        children: Vec<Condition>,
    },
    Or {
        children: Vec<Condition>,
    },
    Not {
        child: Box<Condition>,
    },
    Leaf {
        field: ConditionField,
        op: ConditionOp,
        /// Absent for `is_null` / `is_not_null`.
        #[serde(default)]
        value: serde_json::Value,
    },
}

/// What a condition leaf reads. `event.*` and `reply.*` describe the
/// event that fired the rule and `clock.*` the elapsed time a
/// time-elapsed rule waited for; every other field reads the ticket.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ConditionField {
    #[serde(rename = "ticket.status")]
    Status,
    #[serde(rename = "ticket.priority")]
    Priority,
    #[serde(rename = "ticket.category_id")]
    CategoryId,
    #[serde(rename = "ticket.requester_uuid")]
    RequesterUuid,
    #[serde(rename = "ticket.assignee_uuid")]
    AssigneeUuid,
    #[serde(rename = "ticket.tag_ids")]
    TagIds,
    #[serde(rename = "ticket.title")]
    Title,
    #[serde(rename = "ticket.body")]
    Body,
    #[serde(rename = "ticket.source_channel_id")]
    SourceChannelId,
    #[serde(rename = "ticket.organization_id")]
    OrganizationId,
    #[serde(rename = "ticket.workflow_state_id")]
    WorkflowStateId,
    #[serde(rename = "ticket.workflow_state.category")]
    WorkflowStateCategory,
    #[serde(rename = "ticket.created_at")]
    CreatedAt,
    #[serde(rename = "ticket.updated_at")]
    UpdatedAt,
    #[serde(rename = "ticket.first_response_at")]
    FirstResponseAt,
    #[serde(rename = "ticket.resolved_at")]
    ResolvedAt,
    #[serde(rename = "ticket.requester.email")]
    RequesterEmail,
    /// The requester's organization as their profile in this
    /// workspace names it.
    #[serde(rename = "ticket.requester.organization")]
    RequesterOrganization,
    #[serde(rename = "ticket.requester.organization_id")]
    RequesterOrganizationId,
    #[serde(rename = "event.changed_fields")]
    EventChangedFields,
    #[serde(rename = "reply.kind")]
    ReplyKind,
    #[serde(rename = "reply.author_role")]
    ReplyAuthorRole,
    #[serde(rename = "clock.minutes_since")]
    ClockMinutesSince,
}

/// A field's value type, which decides the operators it takes (plan
/// §5.2's compatibility matrix).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConditionFieldKind {
    String,
    Enum,
    IntId,
    Uuid,
    IntArray,
    StringArray,
    Timestamp,
    Number,
}

impl ConditionFieldKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::String => "string",
            Self::Enum => "enum",
            Self::IntId => "int_id",
            Self::Uuid => "uuid",
            Self::IntArray => "int_array",
            Self::StringArray => "string_array",
            Self::Timestamp => "timestamp",
            Self::Number => "number",
        }
    }

    /// The operators a field of this kind takes.
    pub fn allowed_ops(self) -> &'static [ConditionOp] {
        use ConditionOp::*;
        match self {
            Self::String => &[
                Eq,
                Neq,
                In,
                NotIn,
                Contains,
                NotContains,
                StartsWith,
                EndsWith,
                MatchesRegex,
                IsNull,
                IsNotNull,
            ],
            Self::Enum | Self::IntId | Self::Uuid => &[Eq, Neq, In, NotIn, IsNull, IsNotNull],
            Self::IntArray | Self::StringArray => &[HasAny, HasAll, IsNull, IsNotNull],
            Self::Timestamp => &[Gt, Lt, Gte, Lte, IsNull, IsNotNull],
            Self::Number => &[Gt, Lt, Gte, Lte],
        }
    }
}

impl ConditionField {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Status => "ticket.status",
            Self::Priority => "ticket.priority",
            Self::CategoryId => "ticket.category_id",
            Self::RequesterUuid => "ticket.requester_uuid",
            Self::AssigneeUuid => "ticket.assignee_uuid",
            Self::TagIds => "ticket.tag_ids",
            Self::Title => "ticket.title",
            Self::Body => "ticket.body",
            Self::SourceChannelId => "ticket.source_channel_id",
            Self::OrganizationId => "ticket.organization_id",
            Self::WorkflowStateId => "ticket.workflow_state_id",
            Self::WorkflowStateCategory => "ticket.workflow_state.category",
            Self::CreatedAt => "ticket.created_at",
            Self::UpdatedAt => "ticket.updated_at",
            Self::FirstResponseAt => "ticket.first_response_at",
            Self::ResolvedAt => "ticket.resolved_at",
            Self::RequesterEmail => "ticket.requester.email",
            Self::RequesterOrganization => "ticket.requester.organization",
            Self::RequesterOrganizationId => "ticket.requester.organization_id",
            Self::EventChangedFields => "event.changed_fields",
            Self::ReplyKind => "reply.kind",
            Self::ReplyAuthorRole => "reply.author_role",
            Self::ClockMinutesSince => "clock.minutes_since",
        }
    }

    /// The field named `name`, if there is one.
    pub fn parse(name: &str) -> Option<Self> {
        serde_json::from_value(serde_json::Value::String(name.to_string())).ok()
    }

    pub fn kind(self) -> ConditionFieldKind {
        match self {
            Self::Title
            | Self::Body
            | Self::RequesterEmail
            | Self::RequesterOrganization
            | Self::ReplyAuthorRole => ConditionFieldKind::String,
            Self::Status | Self::Priority | Self::WorkflowStateCategory | Self::ReplyKind => {
                ConditionFieldKind::Enum
            }
            Self::CategoryId
            | Self::SourceChannelId
            | Self::OrganizationId
            | Self::WorkflowStateId
            | Self::RequesterOrganizationId => ConditionFieldKind::IntId,
            Self::RequesterUuid | Self::AssigneeUuid => ConditionFieldKind::Uuid,
            Self::TagIds => ConditionFieldKind::IntArray,
            Self::EventChangedFields => ConditionFieldKind::StringArray,
            Self::CreatedAt | Self::UpdatedAt | Self::FirstResponseAt | Self::ResolvedAt => {
                ConditionFieldKind::Timestamp
            }
            Self::ClockMinutesSince => ConditionFieldKind::Number,
        }
    }

    /// Whether the field can be empty. A ticket always has a title,
    /// priority, status and timestamps of its own, so `is_null` on one of
    /// those never matches and `is_not_null` always does. An array field
    /// counts as empty when it has no entries.
    pub fn nullable(self) -> bool {
        !matches!(
            self,
            Self::Title
                | Self::Priority
                | Self::Status
                | Self::WorkflowStateId
                | Self::WorkflowStateCategory
                | Self::CreatedAt
                | Self::UpdatedAt
                | Self::ReplyKind
                | Self::ClockMinutesSince
        )
    }

    /// The values an enum field takes.
    pub fn enum_values(self) -> &'static [&'static str] {
        match self {
            Self::Priority => &["none", "low", "medium", "high", "urgent"],
            Self::WorkflowStateCategory => &[
                "triage",
                "backlog",
                "active",
                "in_review",
                "done",
                "cancelled",
                "merged",
            ],
            Self::ReplyKind => &["customer", "agent"],
            _ => &[],
        }
    }

    /// The only trigger a field means anything under: the event that fired
    /// the rule carries it. `None` for ticket fields, which every trigger
    /// reads.
    pub fn trigger_only(self) -> Option<RuleTriggerKind> {
        match self {
            Self::EventChangedFields => Some(RuleTriggerKind::TicketUpdated),
            Self::ReplyKind | Self::ReplyAuthorRole => Some(RuleTriggerKind::TicketReplied),
            Self::ClockMinutesSince => Some(RuleTriggerKind::TimeElapsed),
            _ => None,
        }
    }

    /// Why a rule can't condition on this field yet, for fields with no
    /// data behind them.
    pub fn unavailable(self) -> Option<&'static str> {
        match self {
            Self::Status => Some("ticket.status is gone; use ticket.workflow_state.category"),
            Self::Body => Some("conditions can't read the ticket body yet"),
            Self::SourceChannelId => Some("conditions can't read the ticket's channel yet"),
            Self::OrganizationId => {
                Some("tickets have no organization; use ticket.requester.organization")
            }
            Self::RequesterOrganizationId => {
                Some("organizations have no id; use ticket.requester.organization")
            }
            _ => None,
        }
    }

    /// The `reads_set` keys a leaf on this field adds (plan §11.2). Status
    /// and the workflow state read the same column; requester details read
    /// the requester too, since changing the requester changes them. Event
    /// and clock fields aren't ticket state and add nothing.
    pub fn reads(self) -> &'static [&'static str] {
        match self {
            Self::Status => &["ticket.workflow_state.category"],
            Self::WorkflowStateId | Self::WorkflowStateCategory => {
                &["ticket.workflow_state_id", "ticket.workflow_state.category"]
            }
            Self::RequesterEmail => &["ticket.requester.email", "ticket.requester_uuid"],
            Self::RequesterOrganization => {
                &["ticket.requester.organization", "ticket.requester_uuid"]
            }
            Self::RequesterOrganizationId => {
                &["ticket.requester.organization_id", "ticket.requester_uuid"]
            }
            Self::EventChangedFields
            | Self::ReplyKind
            | Self::ReplyAuthorRole
            | Self::ClockMinutesSince => &[],
            Self::Priority => &["ticket.priority"],
            Self::CategoryId => &["ticket.category_id"],
            Self::RequesterUuid => &["ticket.requester_uuid"],
            Self::AssigneeUuid => &["ticket.assignee_uuid"],
            Self::TagIds => &["ticket.tag_ids"],
            Self::Title => &["ticket.title"],
            Self::Body => &["ticket.body"],
            Self::SourceChannelId => &["ticket.source_channel_id"],
            Self::OrganizationId => &["ticket.organization_id"],
            Self::CreatedAt => &["ticket.created_at"],
            Self::UpdatedAt => &["ticket.updated_at"],
            Self::FirstResponseAt => &["ticket.first_response_at"],
            Self::ResolvedAt => &["ticket.resolved_at"],
        }
    }
}

/// A condition leaf's comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConditionOp {
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
}

impl ConditionOp {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Eq => "eq",
            Self::Neq => "neq",
            Self::In => "in",
            Self::NotIn => "not_in",
            Self::Contains => "contains",
            Self::NotContains => "not_contains",
            Self::StartsWith => "starts_with",
            Self::EndsWith => "ends_with",
            Self::Gt => "gt",
            Self::Lt => "lt",
            Self::Gte => "gte",
            Self::Lte => "lte",
            Self::MatchesRegex => "matches_regex",
            Self::IsNull => "is_null",
            Self::IsNotNull => "is_not_null",
            Self::HasAny => "has_any",
            Self::HasAll => "has_all",
        }
    }
}
