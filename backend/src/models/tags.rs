use chrono::{DateTime, Utc};
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// === Tags ====================================================
//
// Free-form, multi-valued labels on tickets — flexible second
// axis to the fixed `category_id`. Workspace-scoped namespace;
// admin / staff can create + assign. See migration
// `2026-05-09-310000_ticket_tags`.

#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable)]
#[diesel(table_name = crate::schema::tags)]
pub struct Tag {
    pub id: i32,
    pub name: String,
    /// Display colour token. Same vocabulary as workflow_states
    /// (slate / gray / blue / purple / green / amber / rose /
    /// subtle). NULL means "use the neutral default."
    pub color: Option<String>,
    pub description: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub archived_at: Option<DateTime<Utc>>,
    pub workspace_id: i32,
}

#[derive(Debug, Default, Serialize, Deserialize, Insertable, AsChangeset)]
#[diesel(table_name = crate::schema::tags)]
pub struct NewTag {
    pub name: String,
    pub color: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize, AsChangeset)]
#[diesel(table_name = crate::schema::tags)]
pub struct TagUpdate {
    pub name: Option<String>,
    pub color: Option<Option<String>>,
    pub description: Option<Option<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::ticket_tags)]
pub struct NewTicketTag {
    pub ticket_id: i32,
    pub tag_id: i32,
    pub created_by: Option<Uuid>,
}
