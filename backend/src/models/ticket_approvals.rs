use chrono::{DateTime, Utc};
use diesel::prelude::*;
use serde::Serialize;
use uuid::Uuid;

use crate::schema::ticket_approvals;

/// One approver's part in a ticket's approval round. `decision` is `None`
/// while they haven't answered.
#[derive(Debug, Clone, Serialize, Queryable, Selectable)]
#[diesel(table_name = ticket_approvals)]
pub struct TicketApproval {
    pub id: i32,
    pub ticket_id: i32,
    pub approver_uuid: Uuid,
    pub decision: Option<String>,
    pub comment: Option<String>,
    pub channel: Option<String>,
    pub decided_by: Option<Uuid>,
    pub decided_at: Option<DateTime<Utc>>,
    pub round: i32,
    pub workspace_id: i32,
    pub created_at: DateTime<Utc>,
}
