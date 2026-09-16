use chrono::NaiveDateTime;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ---------- Canned responses ----------

/// Reusable reply template that techs can pull into the ticket
/// composer with one click. Shared across the team (not per-user);
/// `created_by` is informational.
#[derive(Debug, Clone, Serialize, Deserialize, Queryable, Identifiable)]
#[diesel(table_name = crate::schema::canned_responses)]
pub struct CannedResponse {
    pub id: i32,
    pub title: String,
    pub body: String,
    pub created_by: Option<Uuid>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
    pub workspace_id: i32,
}

#[derive(Debug, Insertable, Deserialize)]
#[diesel(table_name = crate::schema::canned_responses)]
pub struct NewCannedResponse {
    pub title: String,
    pub body: String,
    pub created_by: Option<Uuid>,
}

/// Partial-update payload. `Option<T>` fields leave the column
/// untouched when `None`.
#[derive(Debug, Default, Deserialize, AsChangeset)]
#[diesel(table_name = crate::schema::canned_responses)]
pub struct CannedResponseUpdate {
    pub title: Option<String>,
    pub body: Option<String>,
    pub updated_at: Option<NaiveDateTime>,
}

/// API shape for the admin list page: a canned response plus its
/// rolling 30-day insertion count. The composer picker doesn't read
/// `inserts_30d` but the field is cheap to include, so we ship one
/// list endpoint instead of two.
#[derive(Debug, Clone, Serialize)]
pub struct CannedResponseListItem {
    pub id: i32,
    pub title: String,
    pub body: String,
    pub created_by: Option<Uuid>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
    pub workspace_id: i32,
    /// Insertions in the last 30 days. `0` for templates that have
    /// never been used or were last used >30d ago.
    pub inserts_30d: i64,
}

impl CannedResponseListItem {
    pub fn from_parts(row: CannedResponse, inserts_30d: i64) -> Self {
        Self {
            id: row.id,
            title: row.title,
            body: row.body,
            created_by: row.created_by,
            created_at: row.created_at,
            updated_at: row.updated_at,
            workspace_id: row.workspace_id,
            inserts_30d,
        }
    }
}

/// Insertable for `canned_response_insertions`. One row per use of
/// a canned response in the composer. Append-only workspace-local
/// usage log; the admin list page rolls these into the 30-day
/// counter column.
#[derive(Debug, Insertable)]
#[diesel(table_name = crate::schema::canned_response_insertions)]
pub struct NewCannedResponseInsertion {
    pub canned_response_id: i32,
    pub user_uuid: Option<Uuid>,
    pub ticket_id: Option<i32>,
    pub workspace_id: i32,
}

/// Read-only "starter template" served by the admin endpoint as a
/// browseable catalog. Selecting one pre-fills the editor; nothing
/// is persisted until the admin clicks Save.
#[derive(Debug, Clone, Serialize)]
pub struct CannedResponseStarter {
    /// Stable identifier used by the frontend to address one
    /// starter in the catalog. Not a database id.
    pub slug: &'static str,
    pub title: &'static str,
    pub body: &'static str,
}
