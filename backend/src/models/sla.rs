use chrono::{DateTime, Utc};
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Working calendar — weekly schedule + timezone. Drives the SLA
/// engine's business-hours arithmetic. Rows are workspace-scoped;
/// `is_default = TRUE` is exactly one row, enforced by a partial
/// unique index.
#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable)]
#[diesel(table_name = crate::schema::working_calendars)]
pub struct WorkingCalendar {
    pub id: i32,
    pub name: String,
    pub timezone: String,
    /// JSONB shape: `{ "mon": [["09:00","17:00"]], ... }`. Empty
    /// array for a day means non-working.
    pub schedule: serde_json::Value,
    pub is_default: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub created_by: Option<Uuid>,
    pub workspace_id: i32,
}

/// Per-calendar holiday override. Days listed here count as
/// non-working regardless of what the weekly schedule says.
#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable)]
#[diesel(table_name = crate::schema::working_calendar_holidays)]
pub struct WorkingCalendarHoliday {
    pub id: i32,
    pub calendar_id: i32,
    pub date: chrono::NaiveDate,
    pub label: Option<String>,
    pub workspace_id: i32,
    /// `"none"` (single date) or `"annual"` (MM-DD repeats every
    /// year). The engine expands annual rows into concrete dates at
    /// load time so the arithmetic keeps using a flat
    /// `HashSet<NaiveDate>`.
    pub recurrence: String,
}

/// SLA policy — applies to a ticket when its `priority_filter` /
/// `category_id_filter` match (NULL = wildcard). When more than one
/// policy could match, the highest-id policy wins (last-write); the
/// `is_default` row is the catch-all when nothing else matches.
#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable)]
#[diesel(table_name = crate::schema::sla_policies)]
pub struct SlaPolicy {
    pub id: i32,
    pub name: String,
    pub target_response_minutes: Option<i32>,
    pub target_resolution_minutes: Option<i32>,
    pub working_calendar_id: Option<i32>,
    pub priority_filter: Option<String>,
    pub category_id_filter: Option<i32>,
    pub is_default: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub created_by: Option<Uuid>,
    pub workspace_id: i32,
    pub assignee_group_id_filter: Option<i32>,
    /// When true, a ticket this policy matches gets NO SLA. Combined with the
    /// most-specific-wins matching, an admin scopes SLAs away from a class of
    /// tickets (e.g. requests) by adding a more-specific No-SLA policy that beats
    /// the catch-all default. Targets / calendar are irrelevant when set.
    pub no_sla: bool,
    /// When the clock starts: `"created"` (from ticket creation) or `"activated"`
    /// (from the ticket's first entry into a non-pausing state). Parsed via
    /// [`crate::services::sla::ClockStart`]; unknown values fall back to
    /// `activated`. Must stay the LAST field (positional Queryable).
    pub clock_start: String,
}
