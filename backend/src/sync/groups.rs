//! Sync group computation.
//!
//! Each `sync_actions` row carries a `groups TEXT[]` column tagged at
//! write time. The delta query then filters with
//! `groups && $allowed_groups` (GIN-indexed array overlap) — a row
//! reaches a client if any of its groups are in the client's allowed
//! set.
//!
//! Group strings follow `<scope>:<id>` convention:
//! - `workspace:<n>` — global (Nosdesk is single-workspace today,
//!   always `workspace:1`).
//! - `ticket:<id>` — visible to anyone with read access to the
//!   ticket. The ticket itself is in this group.
//! - `project:<id>` — anyone with the project visible.
//! - `group:<id>` — members of an internal user group.
//! - `user:<uuid>` — direct-to-user (notifications, mentions).
//!
//! The read-side complement is [`allowed_for_user`], which lists
//! every group the requesting user can see.

use diesel::prelude::*;
use uuid::Uuid;

use crate::db::DbConnection;
use crate::models::{Ticket, User};
use crate::schema::{project_tickets, user_groups};

const WORKSPACE_GROUP: &str = "workspace:1";

/// Groups attached to a ticket-scoped event.
pub fn for_ticket(conn: &mut DbConnection, ticket: &Ticket) -> QueryResult<Vec<String>> {
    let mut out = vec![
        WORKSPACE_GROUP.to_string(),
        format!("ticket:{}", ticket.id),
    ];
    let project_ids: Vec<i32> = project_tickets::table
        .filter(project_tickets::ticket_id.eq(ticket.id))
        .select(project_tickets::project_id)
        .load(conn)?;
    out.extend(project_ids.iter().map(|id| format!("project:{}", id)));
    Ok(out)
}

/// Groups attached to a project-scoped event.
pub fn for_project(project_id: i32) -> Vec<String> {
    vec![
        WORKSPACE_GROUP.to_string(),
        format!("project:{}", project_id),
    ]
}

/// Groups attached to a cycle-scoped event. Cycles live under a
/// project; the cycle itself is also its own group so a future
/// "cycle detail" route can subscribe directly.
pub fn for_cycle(cycle_id: i32, project_id: i32) -> Vec<String> {
    vec![
        WORKSPACE_GROUP.to_string(),
        format!("project:{}", project_id),
        format!("cycle:{}", cycle_id),
    ]
}

/// Groups attached to a workspace-wide config event (workflow_states,
/// site settings, etc.).
pub fn workspace() -> Vec<String> {
    vec![WORKSPACE_GROUP.to_string()]
}

/// Groups attached to an event that targets a single user (direct
/// notifications, profile changes).
pub fn for_user(user_uuid: Uuid) -> Vec<String> {
    vec![WORKSPACE_GROUP.to_string(), format!("user:{}", user_uuid)]
}

/// Read-side: every group the user can see. The sync engine's delta
/// handler computes this once per request and folds it into the
/// `groups && $allowed` filter.
pub fn allowed_for_user(conn: &mut DbConnection, user: &User) -> QueryResult<Vec<String>> {
    let mut allowed = vec![
        WORKSPACE_GROUP.to_string(),
        format!("user:{}", user.uuid),
    ];

    let group_ids: Vec<i32> = user_groups::table
        .filter(user_groups::user_uuid.eq(user.uuid))
        .select(user_groups::group_id)
        .load(conn)?;
    allowed.extend(group_ids.iter().map(|id| format!("group:{}", id)));

    // Project visibility: today every authenticated user can see every
    // project; tighten in a follow-up once project-level ACLs land.
    use crate::schema::projects;
    let project_ids: Vec<i32> = projects::table.select(projects::id).load(conn)?;
    allowed.extend(project_ids.iter().map(|id| format!("project:{}", id)));

    Ok(allowed)
}
