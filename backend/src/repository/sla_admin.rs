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
            created_by: actor,
        })
        .get_result(conn)
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
}

#[derive(Debug, Deserialize)]
pub struct WorkingCalendarHolidayBody {
    pub date: NaiveDate,
    /// Free-form label like "Bank holiday" or "Office closed". Optional
    /// because the engine only cares about the date; the label is
    /// purely admin-readable context.
    pub label: Option<String>,
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

pub fn create_holiday(
    conn: &mut DbConnection,
    calendar_id_value: i32,
    body: WorkingCalendarHolidayBody,
) -> QueryResult<WorkingCalendarHoliday> {
    diesel::insert_into(working_calendar_holidays::table)
        .values(&NewWorkingCalendarHoliday {
            calendar_id: calendar_id_value,
            date: body.date,
            label: body.label,
        })
        .get_result(conn)
}

pub fn delete_holiday(conn: &mut DbConnection, id: i32) -> QueryResult<usize> {
    diesel::delete(working_calendar_holidays::table.find(id)).execute(conn)
}
