//! Sync group computation.
//!
//! Each `sync_actions` row carries a `groups TEXT[]` column tagged at
//! write time. The delta query then filters with
//! `groups && $allowed_groups` (GIN-indexed array overlap) — a row
//! reaches a client if any of its groups are in the client's allowed
//! set.
//!
//! Group strings follow `<scope>:<id>` convention:
//! - `workspace:<id>` — every member of the workspace.
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

/// Write-side placeholder for the event's workspace group. Every emit
/// runs inside a workspace-pinned transaction (`app.workspace_id` —
/// the GUC that feeds the tenant tables' `workspace_id` column
/// defaults and the RLS policies), and `emit::record` resolves this
/// placeholder to `workspace:<pinned id>` inside the INSERT itself.
/// Group computation therefore never needs to know the workspace id,
/// and the stored group can never disagree with the row's own
/// `workspace_id` column.
pub const WORKSPACE_GROUP: &str = "workspace";

/// Groups attached to a ticket-scoped event.
pub fn for_ticket(conn: &mut DbConnection, ticket: &Ticket) -> QueryResult<Vec<String>> {
    let mut out = vec![WORKSPACE_GROUP.to_string(), format!("ticket:{}", ticket.id)];
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
    // The workspace grant is the connection's pinned workspace — the
    // same GUC the RLS policies scope every read below by. Unpinned
    // (no resolved workspace on the request) means no workspace-wide
    // grant: the caller gets only their user group, mirroring RLS
    // returning no tenant rows.
    let mut allowed = Vec::new();
    if let Some(ws) = crate::sync::session::current_workspace_id(conn)? {
        allowed.push(format!("workspace:{}", ws));
    }
    allowed.push(format!("user:{}", user.uuid));

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

/// Admit the `ticket:<id>` groups in `requested_csv` that `vis` can
/// read, appending them to `granted`. Ticket groups are deliberately
/// absent from [`allowed_for_user`] — a user can reach an unbounded
/// number of tickets, so enumerating them per request is wasteful.
/// Instead they're authorized dynamically per-ticket via
/// [`ticket_visibility::can_view_ticket`], mirroring the SSE ticket
/// topic gate. Tickets the caller can't read are silently skipped (a
/// 403 would leak existence). Idempotent against groups already in
/// `granted`.
///
/// Shared by the bootstrap and delta paths so a client's ticket
/// subscription resolves identically for the snapshot and the live
/// pull.
pub fn admit_ticket_groups(
    conn: &mut DbConnection,
    requested_csv: &str,
    vis: &crate::repository::ticket_visibility::VisibilityContext,
    granted: &mut Vec<String>,
) {
    use std::collections::HashSet;
    let mut seen: HashSet<String> = granted.iter().cloned().collect();
    for raw in requested_csv.split(',') {
        let g = raw.trim();
        let Some(suffix) = g.strip_prefix("ticket:") else {
            continue;
        };
        if seen.contains(g) {
            continue;
        }
        let Ok(ticket_id) = suffix.parse::<i32>() else {
            continue;
        };
        if crate::repository::ticket_visibility::can_view_ticket(conn, vis, ticket_id)
            .unwrap_or(false)
        {
            seen.insert(g.to_string());
            granted.push(g.to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::actor::ActorContext;
    use crate::sync::session;
    use crate::test_helpers::{setup_test_connection, TestFixtures};

    #[test]
    fn allowed_for_user_grants_the_pinned_workspace_group() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "allowed_ws_user", "admin");

        // Pin a non-default workspace: the grant must follow the pin,
        // not a constant. No workspaces row is needed — nothing here
        // writes; RLS just scopes the project / user-group reads.
        let actor = ActorContext::user_at_workspace(user.uuid, 4242);
        let allowed =
            session::with_actor_context(&mut conn, &actor, |c| allowed_for_user(c, &user))
                .expect("allowed_for_user under a pin");

        assert!(allowed.contains(&"workspace:4242".to_string()));
        assert!(!allowed.iter().any(|g| g == "workspace:1"));
        assert!(allowed.contains(&format!("user:{}", user.uuid)));
    }

    #[test]
    fn allowed_for_user_unpinned_grants_no_workspace_group() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "allowed_unpinned_user", "admin");

        // Clear the ambient test pin: an unpinned request must not get
        // a workspace-wide grant (the fail-closed shape RLS gives the
        // row reads themselves).
        diesel::sql_query("SELECT set_config('app.workspace_id', '', false)")
            .execute(&mut conn)
            .expect("clear the workspace pin");

        let allowed = allowed_for_user(&mut conn, &user).expect("allowed_for_user unpinned");
        assert!(!allowed.iter().any(|g| g.starts_with("workspace:")));
        assert!(allowed.contains(&format!("user:{}", user.uuid)));
    }
}
