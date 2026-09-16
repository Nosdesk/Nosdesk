use super::serialize_optional_uuid_as_string;
use super::tickets::Ticket;
use chrono::NaiveDateTime;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// User ticket views for tracking recently viewed tickets
#[derive(Debug, Serialize, Deserialize, Identifiable, Queryable, Associations)]
#[diesel(belongs_to(Ticket))]
#[diesel(table_name = crate::schema::user_ticket_views)]
pub struct UserTicketView {
    pub id: i32,
    pub user_uuid: Uuid,
    pub ticket_id: i32,
    pub first_viewed_at: NaiveDateTime,
    pub last_viewed_at: NaiveDateTime,
    pub view_count: i32,
    pub workspace_id: i32,
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::user_ticket_views)]
pub struct NewUserTicketView {
    pub user_uuid: Uuid,
    pub ticket_id: i32,
}

#[derive(Debug, Serialize, Deserialize, AsChangeset)]
#[diesel(table_name = crate::schema::user_ticket_views)]
pub struct UpdateUserTicketView {
    pub last_viewed_at: NaiveDateTime,
    pub view_count: i32,
}

// Response structure for recent tickets API
#[derive(Debug, Serialize, Deserialize)]
pub struct RecentTicket {
    pub id: i32,
    pub title: String,
    /// The frontend resolves this to a category / colour via the
    /// workspace workflow-states store.
    pub workflow_state_id: i32,
    #[serde(serialize_with = "serialize_optional_uuid_as_string")]
    pub requester: Option<Uuid>,
    #[serde(serialize_with = "serialize_optional_uuid_as_string")]
    pub assignee: Option<Uuid>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
    pub last_viewed_at: NaiveDateTime,
    pub view_count: i32,
}
