//! Workspace lookup + membership repository.
//!
//! Read-only surface for Phase 2a: the middleware needs to
//! resolve a workspace from a slug (hosted mode) or load the
//! bootstrap workspace at boot (self-hosted). Write paths
//! (create / archive / lifecycle) are deferred to Phase 5.
//!
//! `workspaces` is a global table — it doesn't carry a
//! workspace_id of its own. The membership join table is
//! likewise a meta-table; the membership row IS the
//! workspace-scope assertion for a user.

use diesel::prelude::*;
use uuid::Uuid;

use crate::db::DbConnection;
use crate::models::{Workspace, WorkspaceMember};
use crate::schema::{workspace_members, workspaces};

/// Load a workspace by id. Returns `None` if the workspace
/// doesn't exist or is soft-archived.
pub fn find_by_id(
    conn: &mut DbConnection,
    id: i32,
) -> QueryResult<Option<Workspace>> {
    workspaces::table
        .filter(workspaces::id.eq(id))
        .filter(workspaces::archived_at.is_null())
        .first(conn)
        .optional()
}

/// Load a workspace by URL slug. Used by the hosted-mode
/// middleware to resolve `acme.nosdesk.com` -> the Acme
/// workspace row. Returns `None` if the slug doesn't match an
/// active workspace.
pub fn find_by_slug(
    conn: &mut DbConnection,
    slug: &str,
) -> QueryResult<Option<Workspace>> {
    workspaces::table
        .filter(workspaces::slug.eq(slug))
        .filter(workspaces::archived_at.is_null())
        .first(conn)
        .optional()
}

/// Check whether a user is a member of a given workspace.
/// Returns the membership row when present, `None` otherwise.
/// Phase 2e wires this into the auth middleware as a 403 gate;
/// Phase 2a just exposes the lookup.
pub fn membership(
    conn: &mut DbConnection,
    workspace_id: i32,
    user_uuid: Uuid,
) -> QueryResult<Option<WorkspaceMember>> {
    workspace_members::table
        .filter(workspace_members::workspace_id.eq(workspace_id))
        .filter(workspace_members::user_uuid.eq(user_uuid))
        .first(conn)
        .optional()
}
