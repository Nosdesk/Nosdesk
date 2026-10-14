//! Who can be assigned a ticket: a platform admin, or someone whose role in
//! the ticket's workspace is Agent or above (the same people
//! `user_helpers::user_can_handle_tickets` admits). `update_ticket_partial`
//! refuses anyone else, and everything that picks an assignee (rule steps,
//! assignment rules, approvals, a new occurrence of a recurring ticket) picks
//! from these people only.
//!
//! The workspace is always named, never left to row security alone, so an
//! elevated connection reads the same answer as a tenant one.

use std::collections::HashSet;

use diesel::dsl::exists;
use diesel::pg::Pg;
use diesel::prelude::*;
use diesel::sql_types::Bool;
use uuid::Uuid;

use crate::db::DbConnection;
use crate::models::{PlatformRole, WorkspaceRole};
use crate::repository::tickets::TicketWriteError;
use crate::schema::{users, workspace_members};

/// The rule, as a filter on `users`: a platform admin, or a current member of
/// `workspace_id` at Agent or above. Every check of who can be assigned goes
/// through it, the people list's `assignable` filter included.
pub fn assignable_in(
    workspace_id: i32,
) -> Box<dyn BoxableExpression<users::table, Pg, SqlType = Bool>> {
    let roles = [
        WorkspaceRole::Owner,
        WorkspaceRole::Admin,
        WorkspaceRole::Agent,
    ]
    .map(|r| r.as_str());
    let staff = workspace_members::table
        .filter(workspace_members::workspace_id.eq(workspace_id))
        .filter(workspace_members::removed_at.is_null())
        .filter(workspace_members::role.eq_any(roles))
        .select(workspace_members::user_uuid);
    Box::new(
        users::platform_role
            .eq(PlatformRole::PlatformAdmin.as_str())
            .or(users::uuid.eq_any(staff)),
    )
}

/// Whether `user` can be assigned tickets in `workspace_id`.
pub fn is_assignable(conn: &mut DbConnection, workspace_id: i32, user: Uuid) -> QueryResult<bool> {
    diesel::select(exists(
        users::table
            .filter(users::uuid.eq(user))
            .filter(users::deleted_at.is_null())
            .filter(assignable_in(workspace_id)),
    ))
    .get_result(conn)
}

/// Refuse `user` as an assignee in `workspace_id` unless they can work
/// tickets there.
pub fn ensure_assignable(
    conn: &mut DbConnection,
    workspace_id: i32,
    user: Uuid,
) -> Result<(), TicketWriteError> {
    if is_assignable(conn, workspace_id, user)? {
        Ok(())
    } else {
        Err(TicketWriteError::IneligibleAssignee(user))
    }
}

/// [`ensure_assignable`] in the workspace the connection is pinned to, for a
/// ticket that doesn't exist yet. Refuses when nothing is pinned.
pub fn ensure_assignable_here(conn: &mut DbConnection, user: Uuid) -> Result<(), TicketWriteError> {
    match pinned_workspace_id(conn)? {
        Some(workspace_id) => ensure_assignable(conn, workspace_id, user),
        None => Err(TicketWriteError::IneligibleAssignee(user)),
    }
}

/// Which of `candidates` can be assigned tickets in the pinned workspace.
/// None when nothing is pinned, as [`ensure_assignable_here`] refuses.
pub fn assignable_among(
    conn: &mut DbConnection,
    candidates: &[Uuid],
) -> QueryResult<HashSet<Uuid>> {
    if candidates.is_empty() {
        return Ok(HashSet::new());
    }
    let Some(workspace_id) = pinned_workspace_id(conn)? else {
        return Ok(HashSet::new());
    };
    Ok(users::table
        .filter(users::uuid.eq_any(candidates))
        .filter(users::deleted_at.is_null())
        .filter(assignable_in(workspace_id))
        .select(users::uuid)
        .load::<Uuid>(conn)?
        .into_iter()
        .collect())
}

/// The members of `group_id` (directly or through an included group, in the
/// group's order) who can be assigned tickets in the pinned workspace.
pub fn eligible_members(conn: &mut DbConnection, group_id: i32) -> QueryResult<Vec<Uuid>> {
    let members: Vec<Uuid> = crate::repository::groups::get_users_in_group(conn, group_id)?
        .into_iter()
        .map(|u| u.uuid)
        .collect();
    let assignable = assignable_among(conn, &members)?;
    Ok(members
        .into_iter()
        .filter(|u| assignable.contains(u))
        .collect())
}

fn pinned_workspace_id(conn: &mut DbConnection) -> QueryResult<Option<i32>> {
    diesel::select(crate::repository::pinned_workspace().nullable()).get_result(conn)
}
