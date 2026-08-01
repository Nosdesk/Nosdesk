//! Admin-side CRUD for SLA policies, working calendars + per-calendar
//! holidays. Read happens through
//! `repository::sla::load_for_pill_computation`; this module covers
//! the writes the admin UI needs.

use chrono::{NaiveDate, Utc};
use diesel::prelude::*;
use serde::Deserialize;
use serde_json::Value;
use uuid::Uuid;

use crate::db::DbConnection;
use crate::models::{SlaPolicy, WorkingCalendar, WorkingCalendarHoliday};
use crate::schema::{sla_policies, working_calendar_holidays, working_calendars};

// ---- Working calendars ----

#[derive(Debug, AsChangeset)]
#[diesel(table_name = working_calendars)]
pub struct WorkingCalendarPatch {
    pub name: Option<String>,
    pub timezone: Option<String>,
    pub schedule: Option<Value>,
    pub is_default: Option<bool>,
    pub updated_at: Option<chrono::DateTime<Utc>>,
}

#[derive(Debug, Insertable)]
#[diesel(table_name = working_calendars)]
pub struct NewWorkingCalendar {
    pub name: String,
    pub timezone: String,
    pub schedule: Value,
    pub is_default: bool,
    pub created_by: Option<Uuid>,
}

#[derive(Debug, Deserialize)]
pub struct WorkingCalendarBody {
    pub name: String,
    pub timezone: Option<String>,
    pub schedule: Value,
    pub is_default: Option<bool>,
}

pub fn list_calendars(conn: &mut DbConnection) -> QueryResult<Vec<WorkingCalendar>> {
    working_calendars::table
        .order(working_calendars::name.asc())
        .load(conn)
}

// sync-pending-wire: SLA config; needs a future SLA aggregate to surface changes
pub fn create_calendar(
    conn: &mut DbConnection,
    body: WorkingCalendarBody,
    actor: Option<Uuid>,
) -> QueryResult<WorkingCalendar> {
    diesel::insert_into(working_calendars::table)
        .values(&NewWorkingCalendar {
            name: body.name,
            timezone: body.timezone.unwrap_or_else(|| "UTC".to_string()),
            schedule: body.schedule,
            is_default: body.is_default.unwrap_or(false),
            created_by: actor,
        })
        .get_result(conn)
}

// sync-pending-wire: SLA config; needs a future SLA aggregate to surface changes
pub fn update_calendar(
    conn: &mut DbConnection,
    id: i32,
    body: WorkingCalendarBody,
) -> QueryResult<WorkingCalendar> {
    let patch = WorkingCalendarPatch {
        name: Some(body.name),
        timezone: Some(body.timezone.unwrap_or_else(|| "UTC".to_string())),
        schedule: Some(body.schedule),
        is_default: body.is_default,
        updated_at: Some(Utc::now()),
    };
    diesel::update(working_calendars::table.find(id))
        .set(&patch)
        .get_result(conn)
}

// sync-pending-wire: SLA config; needs a future SLA aggregate to surface changes
pub fn delete_calendar(conn: &mut DbConnection, id: i32) -> QueryResult<usize> {
    diesel::delete(working_calendars::table.find(id)).execute(conn)
}

// ---- SLA policies ----

#[derive(Debug, Insertable)]
#[diesel(table_name = sla_policies)]
pub struct NewSlaPolicy {
    pub name: String,
    pub target_response_minutes: Option<i32>,
    pub target_resolution_minutes: Option<i32>,
    pub working_calendar_id: Option<i32>,
    pub priority_filter: Option<String>,
    pub category_id_filter: Option<i32>,
    pub assignee_group_id_filter: Option<i32>,
    pub is_default: bool,
    pub no_sla: bool,
    pub clock_start: String,
    pub created_by: Option<Uuid>,
}

#[derive(Debug, AsChangeset)]
#[diesel(table_name = sla_policies)]
pub struct SlaPolicyPatch {
    pub name: Option<String>,
    pub target_response_minutes: Option<Option<i32>>,
    pub target_resolution_minutes: Option<Option<i32>>,
    pub working_calendar_id: Option<Option<i32>>,
    pub priority_filter: Option<Option<String>>,
    pub category_id_filter: Option<Option<i32>>,
    pub assignee_group_id_filter: Option<Option<i32>>,
    pub is_default: Option<bool>,
    pub no_sla: Option<bool>,
    pub clock_start: Option<String>,
    pub updated_at: Option<chrono::DateTime<Utc>>,
}

#[derive(Debug, Deserialize)]
pub struct SlaPolicyBody {
    pub name: String,
    pub target_response_minutes: Option<i32>,
    pub target_resolution_minutes: Option<i32>,
    pub working_calendar_id: Option<i32>,
    pub priority_filter: Option<String>,
    pub category_id_filter: Option<i32>,
    pub assignee_group_id_filter: Option<i32>,
    pub is_default: Option<bool>,
    #[serde(default)]
    pub no_sla: Option<bool>,
    /// `"created"` or `"activated"` (default). See services::sla::ClockStart.
    #[serde(default)]
    pub clock_start: Option<String>,
}

pub fn list_policies(conn: &mut DbConnection) -> QueryResult<Vec<SlaPolicy>> {
    sla_policies::table
        .order(sla_policies::name.asc())
        .load(conn)
}

// sync-pending-wire: SLA config; needs a future SLA aggregate to surface changes
pub fn create_policy(
    conn: &mut DbConnection,
    body: SlaPolicyBody,
    actor: Option<Uuid>,
) -> QueryResult<SlaPolicy> {
    diesel::insert_into(sla_policies::table)
        .values(&NewSlaPolicy {
            name: body.name,
            target_response_minutes: body.target_response_minutes,
            target_resolution_minutes: body.target_resolution_minutes,
            working_calendar_id: body.working_calendar_id,
            priority_filter: body.priority_filter,
            category_id_filter: body.category_id_filter,
            assignee_group_id_filter: body.assignee_group_id_filter,
            is_default: body.is_default.unwrap_or(false),
            no_sla: body.no_sla.unwrap_or(false),
            clock_start: body.clock_start.unwrap_or_else(|| "activated".to_string()),
            created_by: actor,
        })
        .get_result(conn)
}

/// First-run seeder: a default working calendar (Mon-Fri 09:00-17:00 UTC)
/// and a default SLA policy (4h response / 24h resolution) for a freshly-
/// provisioned workspace, so SLA tracking works out of the box. The policy
/// references the calendar just created (not a hardcoded id). No-ops when
/// the workspace already has a working calendar. Caller must run inside an
/// actor context pinned to the target workspace.
///
/// The UTC 9-5 calendar is a neutral placeholder; the owner-facing
/// timezone/language setup phase (tracked separately) is the intended path
/// to a workspace-correct calendar.
// sync-pending-wire: SLA config; needs a future SLA aggregate to surface changes
pub fn seed_defaults_if_empty(
    conn: &mut DbConnection,
    created_by: Option<Uuid>,
) -> QueryResult<()> {
    use diesel::dsl::count_star;

    let existing: i64 = working_calendars::table.select(count_star()).first(conn)?;
    if existing > 0 {
        return Ok(());
    }

    let schedule = serde_json::json!({
        "mon": [["09:00", "17:00"]],
        "tue": [["09:00", "17:00"]],
        "wed": [["09:00", "17:00"]],
        "thu": [["09:00", "17:00"]],
        "fri": [["09:00", "17:00"]],
        "sat": [],
        "sun": [],
    });
    let calendar = create_calendar(
        conn,
        WorkingCalendarBody {
            name: "Default 9-5".to_string(),
            timezone: Some("UTC".to_string()),
            schedule,
            is_default: Some(true),
        },
        created_by,
    )?;

    create_policy(
        conn,
        SlaPolicyBody {
            name: "Default".to_string(),
            target_response_minutes: Some(240),
            target_resolution_minutes: Some(1440),
            working_calendar_id: Some(calendar.id),
            priority_filter: None,
            category_id_filter: None,
            assignee_group_id_filter: None,
            is_default: Some(true),
            no_sla: Some(false),
            clock_start: None, // -> "activated" (the instant-breach fix)
        },
        created_by,
    )?;

    Ok(())
}

// sync-pending-wire: SLA config; needs a future SLA aggregate to surface changes
pub fn update_policy(
    conn: &mut DbConnection,
    id: i32,
    body: SlaPolicyBody,
) -> QueryResult<SlaPolicy> {
    let patch = SlaPolicyPatch {
        name: Some(body.name),
        target_response_minutes: Some(body.target_response_minutes),
        target_resolution_minutes: Some(body.target_resolution_minutes),
        working_calendar_id: Some(body.working_calendar_id),
        priority_filter: Some(body.priority_filter),
        category_id_filter: Some(body.category_id_filter),
        assignee_group_id_filter: Some(body.assignee_group_id_filter),
        is_default: body.is_default,
        no_sla: body.no_sla,
        clock_start: body.clock_start,
        updated_at: Some(Utc::now()),
    };
    diesel::update(sla_policies::table.find(id))
        .set(&patch)
        .get_result(conn)
}

// sync-pending-wire: SLA config; needs a future SLA aggregate to surface changes
pub fn delete_policy(conn: &mut DbConnection, id: i32) -> QueryResult<usize> {
    diesel::delete(sla_policies::table.find(id)).execute(conn)
}

// ---- Working-calendar holidays ----

#[derive(Debug, Insertable)]
#[diesel(table_name = working_calendar_holidays)]
pub struct NewWorkingCalendarHoliday {
    pub calendar_id: i32,
    pub date: NaiveDate,
    pub label: Option<String>,
    pub recurrence: String,
}

#[derive(Debug, Deserialize)]
pub struct WorkingCalendarHolidayBody {
    pub date: NaiveDate,
    /// Free-form label like "Bank holiday" or "Office closed". Optional
    /// because the engine only cares about the date; the label is
    /// purely admin-readable context.
    pub label: Option<String>,
    /// `"none"` (default) or `"annual"`. Unknown values are coerced to
    /// `"none"` so a typo doesn't silently activate recurrence.
    pub recurrence: Option<String>,
}

pub fn list_holidays(
    conn: &mut DbConnection,
    calendar_id_value: i32,
) -> QueryResult<Vec<WorkingCalendarHoliday>> {
    working_calendar_holidays::table
        .filter(working_calendar_holidays::calendar_id.eq(calendar_id_value))
        .order(working_calendar_holidays::date.asc())
        .load(conn)
}

// sync-pending-wire: SLA config; needs a future SLA aggregate to surface changes
pub fn create_holiday(
    conn: &mut DbConnection,
    calendar_id_value: i32,
    body: WorkingCalendarHolidayBody,
) -> QueryResult<WorkingCalendarHoliday> {
    let recurrence = match body.recurrence.as_deref() {
        Some("annual") => "annual".to_string(),
        _ => "none".to_string(),
    };
    diesel::insert_into(working_calendar_holidays::table)
        .values(&NewWorkingCalendarHoliday {
            calendar_id: calendar_id_value,
            date: body.date,
            label: body.label,
            recurrence,
        })
        .get_result(conn)
}

// sync-pending-wire: SLA config; needs a future SLA aggregate to surface changes
pub fn delete_holiday(conn: &mut DbConnection, id: i32) -> QueryResult<usize> {
    diesel::delete(working_calendar_holidays::table.find(id)).execute(conn)
}
