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
pub fn find_by_id(conn: &mut DbConnection, id: i32) -> QueryResult<Option<Workspace>> {
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
pub fn find_by_slug(conn: &mut DbConnection, slug: &str) -> QueryResult<Option<Workspace>> {
    workspaces::table
        .filter(workspaces::slug.eq(slug))
        .filter(workspaces::archived_at.is_null())
        .first(conn)
        .optional()
}

/// Check whether a user is a member of a given workspace.
/// Returns the membership row when present, `None` otherwise.
/// Wired into the cookie auth middleware as a 403 short-circuit
/// (Item U) — a logged-in user hitting a subdomain they don't
/// belong to gets 403 instead of the app shell with empty RLS-
/// filtered queries.
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

/// Add a user to the given workspace. Called from every user-
/// creation flow (admin invite, guest portal, channels ingest,
/// OAuth provisioning, setup_initial_admin bootstrap) so newly-
/// created users get the `workspace_members` row that the
/// Item U 403 gate requires.
///
/// `role` is the workspace-membership role
/// (`owner` / `admin` / `member`), not the global user role.
/// Callers usually map `UserRole::Admin -> "admin"`, everything
/// else -> "member" (same shape as the 2026-05-23 migration
/// backfill). Idempotent via `ON CONFLICT DO NOTHING` so re-
/// invocation during testing or restore doesn't blow up on the
/// composite PK.
///
/// `workspace_id` is passed explicitly rather than read from the
/// GUC because some callers (bootstrap admin setup) run before
/// any workspace context has been threaded through.
pub fn add_membership(
    conn: &mut DbConnection,
    workspace_id: i32,
    user_uuid: Uuid,
    role: &str,
) -> QueryResult<usize> {
    diesel::sql_query(
        "INSERT INTO workspace_members (workspace_id, user_uuid, role) \
         VALUES ($1, $2, $3) \
         ON CONFLICT (workspace_id, user_uuid) DO NOTHING",
    )
    .bind::<diesel::sql_types::Integer, _>(workspace_id)
    .bind::<diesel::sql_types::Uuid, _>(user_uuid)
    .bind::<diesel::sql_types::Text, _>(role)
    .execute(conn)
}
