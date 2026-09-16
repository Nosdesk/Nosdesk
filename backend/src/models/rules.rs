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
/// before flipping to `live`, `live` is in-flight, `archived` is a
/// soft-deleted row that the picker / engine ignore. See decisions
/// 12 + 32 in the plan: archived is the only state hard-delete is
/// permitted from.
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
    #[serde(rename = "archived")]
    Archived,
}

impl RuleState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::DryRun => "dry_run",
            Self::Live => "live",
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
