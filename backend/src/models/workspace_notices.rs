use chrono::{DateTime, Utc};
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::schema::workspace_notices;

/// A known-issue notice shown to requesters on the portal and guest pages.
#[derive(Debug, Clone, Queryable, Selectable, Serialize)]
#[diesel(table_name = workspace_notices)]
pub struct WorkspaceNotice {
    pub id: i32,
    pub title: String,
    pub body: Option<String>,
    /// `info`, `degraded` or `outage`.
    pub severity: String,
    pub starts_at: DateTime<Utc>,
    pub ends_at: DateTime<Utc>,
    /// The ticket tracking the issue, which requesters can follow.
    pub incident_ticket_id: Option<i32>,
    pub created_by: Option<Uuid>,
    #[serde(skip)]
    pub workspace_id: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Fields staff set when posting or editing a notice.
/// An edit writes every field, so clearing the body or the incident ticket
/// sticks (`None` sets NULL rather than leaving the column alone).
#[derive(Debug, Clone, Deserialize, Insertable, AsChangeset)]
#[diesel(table_name = workspace_notices, treat_none_as_null = true)]
pub struct NoticeFields {
    pub title: String,
    pub body: Option<String>,
    pub severity: String,
    pub starts_at: DateTime<Utc>,
    pub ends_at: DateTime<Utc>,
    pub incident_ticket_id: Option<i32>,
}
