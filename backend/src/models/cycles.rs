use chrono::{DateTime, Utc};
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Cycle: project-scoped, time-boxed bucket of tickets. Tickets
/// join via the `cycle_tickets` link table. The architecture spec
/// (§ 10 phase 6) targets TSTZRANGE + GIST for the span column;
/// v1 ships two TIMESTAMPTZ columns (start_at, end_at) so the
/// stock Diesel mapping does not need a tuple-of-bound shim. A
/// follow-up can add a real range column if calendar/gantt views
/// need the overlap GIST.
#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable)]
#[diesel(table_name = crate::schema::cycles)]
pub struct Cycle {
    pub id: i32,
    pub uuid: Uuid,
    pub project_id: i32,
    pub name: String,
    pub start_at: Option<DateTime<Utc>>,
    pub end_at: Option<DateTime<Utc>>,
    pub state: String,
    pub completion_snapshot: Option<serde_json::Value>,
    pub completed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub archived_at: Option<DateTime<Utc>>,
    pub created_by: Option<Uuid>,
    pub workspace_id: i32,
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::cycles)]
pub struct NewCycle {
    pub project_id: i32,
    pub name: String,
    pub start_at: Option<DateTime<Utc>>,
    pub end_at: Option<DateTime<Utc>>,
    pub state: String,
    pub created_by: Option<Uuid>,
}

#[derive(Debug, Default, Serialize, Deserialize, AsChangeset)]
#[diesel(table_name = crate::schema::cycles)]
pub struct CycleUpdate {
    pub name: Option<String>,
    pub start_at: Option<Option<DateTime<Utc>>>,
    pub end_at: Option<Option<DateTime<Utc>>>,
    pub state: Option<String>,
    pub completion_snapshot: Option<Option<serde_json::Value>>,
    pub completed_at: Option<Option<DateTime<Utc>>>,
    pub archived_at: Option<Option<DateTime<Utc>>>,
}

/// Many-to-many between cycles and tickets. The partial unique
/// index on `ticket_id` enforces "one cycle per ticket" until the
/// multi-cycle use case lands.
#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable, Insertable)]
#[diesel(primary_key(cycle_id, ticket_id))]
#[diesel(table_name = crate::schema::cycle_tickets)]
pub struct CycleTicket {
    pub cycle_id: i32,
    pub ticket_id: i32,
    pub added_at: DateTime<Utc>,
    pub added_by: Option<Uuid>,
    pub workspace_id: i32,
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::cycle_tickets)]
pub struct NewCycleTicket {
    pub cycle_id: i32,
    pub ticket_id: i32,
    pub added_by: Option<Uuid>,
}
