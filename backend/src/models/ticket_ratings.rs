use chrono::{DateTime, Utc};
use diesel::prelude::*;
use serde::Serialize;
use uuid::Uuid;

use crate::schema::ticket_ratings;

/// A requester's answer to "is it fixed?" on a ticket.
#[derive(Debug, Clone, Queryable, Selectable, Serialize)]
#[diesel(table_name = ticket_ratings)]
pub struct TicketRating {
    pub ticket_id: i32,
    pub rater_uuid: Uuid,
    /// `good` (fixed) or `bad` (still needs help).
    pub rating: String,
    pub comment: Option<String>,
    #[serde(skip)]
    pub workspace_id: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
