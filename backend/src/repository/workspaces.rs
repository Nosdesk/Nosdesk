//! Workspace lookup + membership + lifecycle repository.
//!
//! Reads, the M5 internal-provisioning write path, and the Phase 4
//! W1 admin lifecycle ops (archive / restore / rename / hard-delete).
//! `workspaces` is a global table — it doesn't carry a workspace_id
//! of its own. The membership join table is likewise a meta-table;
//! the membership row IS the workspace-scope assertion for a user.

use std::time::Duration;

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel::result::{DatabaseErrorKind, Error as DieselError};
use uuid::Uuid;

use crate::db::DbConnection;
use crate::models::{NewWorkspace, Workspace, WorkspaceMember};
use crate::schema::{retired_slugs, workspace_members, workspaces};

/// Staff roles that count toward a workspace's `seat_limit`. Mirrors
/// [`crate::models::WorkspaceRole::is_staff`]; kept as a literal array for the
/// SQL filter and the enforcement trigger.
const STAFF_ROLES: [&str; 3] = ["owner", "admin", "agent"];

/// Returned by [`create_workspace`] so the caller can distinguish a
/// slug-collision from other DB failures without parsing error
/// strings.
#[derive(Debug)]
pub enum CreateWorkspaceError {
    /// The requested slug is unavailable: either an active workspace
    /// holds it (UNIQUE on `workspaces.slug`) or it was retired by a
    /// prior hard delete (`retired_slugs`). We surface it as a typed
    /// outcome so the handler can return a 409 with non-enumerable
    /// wording instead of a 500.
    SlugTaken,
    Db(DieselError),
}

impl From<DieselError> for CreateWorkspaceError {
    fn from(e: DieselError) -> Self {
        match e {
            DieselError::DatabaseError(DatabaseErrorKind::UniqueViolation, _) => Self::SlugTaken,
            other => Self::Db(other),
        }
    }
}

impl std::fmt::Display for CreateWorkspaceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SlugTaken => write!(f, "slug already taken"),
            Self::Db(e) => write!(f, "db error: {e}"),
        }
    }
}

impl std::error::Error for CreateWorkspaceError {}

// sync-audit-only: workspaces row creation is operator-side / control-plane provisioning; never propagated through the per-workspace sync stream (the workspace doesn't exist there yet to receive it)
/// Insert a workspace row. Caller supplies a product-generated UUID
/// per the locked decision (the
/// product owns workspace identity; the control plane mirrors).
/// `plan` is omitted so the DB default (`'free'`) applies.
///
/// Rejects a slug retired by a prior hard delete (P1.2 never-reuse):
/// the `retired_slugs` check and the `workspaces.slug` UNIQUE
/// constraint together guarantee a slug maps to one identity for all
/// time. Both an active collision and a retired slug surface as the
/// same `SlugTaken`, so the caller's 409 stays non-enumerable.
pub fn create_workspace(
    conn: &mut DbConnection,
    record: &NewWorkspace,
) -> Result<Workspace, CreateWorkspaceError> {
    let retired: bool = diesel::select(diesel::dsl::exists(
        retired_slugs::table.filter(retired_slugs::slug.eq(&record.slug)),
    ))
    .get_result(conn)
    .map_err(CreateWorkspaceError::Db)?;
    if retired {
        return Err(CreateWorkspaceError::SlugTaken);
    }

    diesel::insert_into(workspaces::table)
        .values(record)
        .get_result::<Workspace>(conn)
        .map_err(CreateWorkspaceError::from)
}

/// Load a workspace by id. Returns `None` if the workspace
/// doesn't exist or is soft-archived.
pub fn find_by_id(conn: &mut DbConnection, id: i32) -> QueryResult<Option<Workspace>> {
    workspaces::table
        .filter(workspaces::id.eq(id))
        .filter(workspaces::archived_at.is_null())
        .first(conn)
        .optional()
}

/// Load a workspace by its public uuid. Used by selection-based
/// resolution (Model C): the agent app sends the chosen workspace
/// uuid in the `X-Nosdesk-Workspace` header and the auth gate
/// resolves it here before membership-gating. The uuid is the
/// stable, opaque identifier (slug is mutable). Returns `None` for
/// an unknown or soft-archived workspace.
pub fn find_by_uuid(conn: &mut DbConnection, uuid: Uuid) -> QueryResult<Option<Workspace>> {
    workspaces::table
        .filter(workspaces::uuid.eq(uuid))
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

/// Batched `(id, slug, name)` read for active workspaces, for the outbound
/// email resolver's managed-identity path (`support@<slug>.<tenant_domain>`):
/// one indexed read per queue drain instead of a lookup per workspace.
/// `workspaces` is a global table, so this works on the worker's bypass
/// connection and on workspace-pinned connections alike. Archived workspaces
/// are omitted — their mail defers rather than sending from a dead address.
pub fn identity_for_ids(
    conn: &mut DbConnection,
    ids: &[i32],
) -> QueryResult<Vec<(i32, String, String)>> {
    workspaces::table
        .filter(workspaces::id.eq_any(ids))
        .filter(workspaces::archived_at.is_null())
        .select((workspaces::id, workspaces::slug, workspaces::name))
        .load(conn)
}

/// Load a workspace by slug regardless of archive state. The
/// deprovision/restore lifecycle endpoints need this to tell "already
/// archived" (idempotent no-op) apart from "never existed" (404);
/// `find_by_slug` filters archived rows and so can't make that call.
pub fn find_by_slug_any_state(
    conn: &mut DbConnection,
    slug: &str,
) -> QueryResult<Option<Workspace>> {
    workspaces::table
        .filter(workspaces::slug.eq(slug))
        .first(conn)
        .optional()
}

/// Load a workspace by its custom domain hostname (e.g.
/// `support.acme.com`). Used by the hosted-mode middleware to
/// route requests on customer-owned domains to their workspace
/// (M5 Task 5). Returns `None` for hostnames not mapped to any
/// active workspace; the middleware falls back to the subdomain
/// lookup on miss.
pub fn find_by_custom_domain(
    conn: &mut DbConnection,
    hostname: &str,
) -> QueryResult<Option<Workspace>> {
    workspaces::table
        .filter(workspaces::custom_domain.eq(hostname))
        .filter(workspaces::archived_at.is_null())
        .first(conn)
        .optional()
}

/// Count active (non-archived) workspaces. Drives the self-hosted
/// single-workspace license gate (see `admin_workspaces::create_workspace`):
/// Community is capped at one active workspace.
pub fn count_active_workspaces(conn: &mut DbConnection) -> QueryResult<i64> {
    workspaces::table
        .filter(workspaces::archived_at.is_null())
        .count()
        .get_result(conn)
}

// sync-audit-only: control-plane provisioning callback; never propagated through the per-workspace sync stream
/// Set or clear the custom-domain hostname for the workspace
/// matching `slug`. `None` clears the field; `Some(host)` sets it
/// (UNIQUE constraint enforces no two workspaces share the same
/// hostname). Returns the updated workspace row.
pub fn update_custom_domain(
    conn: &mut DbConnection,
    slug: &str,
    hostname: Option<&str>,
) -> QueryResult<Option<Workspace>> {
    diesel::update(workspaces::table.filter(workspaces::slug.eq(slug)))
        .set(workspaces::custom_domain.eq(hostname))
        .get_result::<Workspace>(conn)
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

// sync-audit-only: membership changes are recorded by the tr_audit_workspace_members audit_log trigger (P1.4); no sync_actions aggregate, since workspace_members isn't on the tenant live-sync stream
/// Add a user to the given workspace. Called from every user-
/// creation flow (admin invite, guest portal, channels ingest,
/// OAuth provisioning, setup_initial_admin bootstrap) so newly-
/// created users get the `workspace_members` row that the
/// Item U 403 gate requires.
///
/// `role` is the workspace-membership role
/// (`owner` / `admin` / `member`), not the global user role.
/// Callers usually map `"admin" -> "admin"`, everything
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
    // accepted_at is stamped at insert: every caller of this helper
    // grants an immediately-active membership (bootstrap admin, admin
    // direct-add of an existing user, OAuth provisioning), none of
    // which has a pending-invite step. The email-invite path creates
    // its membership via create_user_with_email and leaves accepted_at
    // NULL until accept_invitation stamps it.
    diesel::sql_query(
        "INSERT INTO workspace_members (workspace_id, user_uuid, role, accepted_at) \
         VALUES ($1, $2, $3, now()) \
         ON CONFLICT (workspace_id, user_uuid) DO NOTHING",
    )
    .bind::<diesel::sql_types::Integer, _>(workspace_id)
    .bind::<diesel::sql_types::Uuid, _>(user_uuid)
    .bind::<diesel::sql_types::Text, _>(role)
    .execute(conn)
}

/// One `role` value read back from a membership upsert's `RETURNING`.
#[derive(QueryableByName, Debug)]
struct MembershipRoleRow {
    #[diesel(sql_type = diesel::sql_types::Text)]
    role: String,
}

/// SELF-VERIFYING membership upsert. Inserts the row with an EXPLICIT
/// `workspace_id`; on conflict, applies `conflict_role` (a SQL fragment:
/// `EXCLUDED.role` to set the new role, or `workspace_members.role` to
/// keep the existing one for first-write-wins provisioning). The
/// `RETURNING role` is the point: a successful upsert ALWAYS yields
/// exactly one row, so `get_result` returns the persisted role — and
/// ERRORS (`NotFound`) if the write produced nothing (a `BEFORE` trigger
/// that returned NULL, a swallowed cancel, etc.). Callers propagate that
/// error rather than logging "applied" over a row that isn't there.
///
/// This is the guard against the "logged success, wrote nothing" class
/// of bug: the membership write can no longer silently no-op.
fn upsert_membership_returning(
    conn: &mut DbConnection,
    workspace_id: i32,
    user_uuid: Uuid,
    role: &str,
    conflict_role: &str,
) -> QueryResult<String> {
    diesel::sql_query(format!(
        "INSERT INTO workspace_members (workspace_id, user_uuid, role, accepted_at) \
         VALUES ($1, $2, $3, now()) \
         ON CONFLICT (workspace_id, user_uuid) DO UPDATE SET role = {conflict_role} \
         RETURNING role"
    ))
    .bind::<diesel::sql_types::Integer, _>(workspace_id)
    .bind::<diesel::sql_types::Uuid, _>(user_uuid)
    .bind::<diesel::sql_types::Text, _>(role)
    .get_result::<MembershipRoleRow>(conn)
    .map(|r| r.role)
}

/// Grant membership if absent, KEEPING the existing role on conflict
/// (first-write-wins — re-projection never escalates/downgrades). Returns
/// the persisted role; errors if the row isn't present after the write.
pub fn ensure_membership(
    conn: &mut DbConnection,
    workspace_id: i32,
    user_uuid: Uuid,
    role: &str,
) -> QueryResult<String> {
    upsert_membership_returning(
        conn,
        workspace_id,
        user_uuid,
        role,
        "workspace_members.role",
    )
}

/// Create-or-set a membership's role (the control-plane role-change path).
/// On conflict SETS the new role. Returns the persisted role; errors if
/// the row isn't present after the write. The last-owner-demotion guard is
/// the caller's responsibility (see `set_member_role`).
pub fn upsert_membership_role(
    conn: &mut DbConnection,
    workspace_id: i32,
    user_uuid: Uuid,
    role: &str,
) -> QueryResult<String> {
    upsert_membership_returning(conn, workspace_id, user_uuid, role, "EXCLUDED.role")
}

/// Count the workspace's staff members (role IN owner/admin/agent) — the seats
/// that count against `seat_limit`. End-user `member` rows are excluded.
///
/// The cap itself is enforced by the `tr_enforce_workspace_seat_limit` DB
/// trigger (added in the `workspace_seat_limit` migration) so it holds across
/// every membership-insert path, not just this repo. This helper backs reads +
/// tests; [`is_seat_limit_violation`] maps the trigger's error to a 403.
pub fn count_staff_members(conn: &mut DbConnection, workspace_id: i32) -> QueryResult<i64> {
    workspace_members::table
        .filter(workspace_members::workspace_id.eq(workspace_id))
        .filter(workspace_members::role.eq_any(STAFF_ROLES))
        .count()
        .get_result(conn)
}

/// True when a Diesel error is the seat-limit trigger firing (a
/// `check_violation` carrying the `workspace_seat_limit` constraint name).
/// Handlers that grant staff memberships use this to return 403 instead of 500.
pub fn is_seat_limit_violation(err: &DieselError) -> bool {
    matches!(
        err,
        DieselError::DatabaseError(DatabaseErrorKind::CheckViolation, info)
            if info.constraint_name() == Some("workspace_seat_limit")
    )
}

// sync-audit-only: control-plane-driven staff-seat cap update; `workspaces` is a global meta-table, not on any tenant's live-sync stream
/// Set (or clear) a workspace's staff `seat_limit` by slug. The control plane
/// calls this to lift the trial cap (`None` = unlimited) when the subscription
/// activates. Returns the number of rows updated (0 if the slug is unknown).
pub fn set_seat_limit(
    conn: &mut DbConnection,
    slug: &str,
    seat_limit: Option<i32>,
) -> QueryResult<usize> {
    diesel::update(workspaces::table.filter(workspaces::slug.eq(slug)))
        .set(workspaces::seat_limit.eq(seat_limit))
        .execute(conn)
}

/// Whether the workspace's push notifications carry rich context (`detailed`,
/// the default) or only the generic type label (`private`, "tap to view").
/// Read from `settings.notification_push_detail`; absent/unknown → detailed.
pub fn get_notification_push_detail(
    conn: &mut DbConnection,
    workspace_id: i32,
) -> QueryResult<bool> {
    let settings: Option<serde_json::Value> = workspaces::table
        .filter(workspaces::id.eq(workspace_id))
        .select(workspaces::settings)
        .first(conn)
        .optional()?;
    Ok(settings
        .as_ref()
        .and_then(|s| s.get("notification_push_detail"))
        .and_then(|v| v.as_str())
        != Some("private"))
}

// sync-audit-only: server-side workspace notification preference on the global `workspaces` meta-table; not on any tenant's live-sync stream
/// Set the workspace's push content level (admin): `true` = detailed (rich
/// context), `false` = private ("tap to view"). Merges into the `settings`
/// JSONB via `jsonb_set` so other keys are preserved.
pub fn set_notification_push_detail(
    conn: &mut DbConnection,
    workspace_id: i32,
    detailed: bool,
) -> QueryResult<usize> {
    let value = if detailed { "detailed" } else { "private" };
    diesel::sql_query(
        "UPDATE workspaces SET settings = jsonb_set(COALESCE(settings, '{}'::jsonb), \
         '{notification_push_detail}', to_jsonb($1::text)) WHERE id = $2",
    )
    .bind::<diesel::sql_types::Text, _>(value)
    .bind::<diesel::sql_types::Integer, _>(workspace_id)
    .execute(conn)
}

/// Resolve the workspace that should be the audit-context root for a
/// credential-verified-but-pre-session action on this user (MFA
/// enable at login, password-reset confirm, invitation accept). In
/// self-hosted there is one membership; in hosted it picks the
/// lowest-id membership as a deterministic "primary" (a primary-
/// membership flag can refine this later).
///
/// Returns `DieselError::NotFound` when the user has no memberships.
/// An earlier revision silently fell back to
/// [`BOOTSTRAP_WORKSPACE_ID`](crate::sync::actor::BOOTSTRAP_WORKSPACE_ID),
/// which under hosted multi-tenancy mis-attributed the audit row
/// (and the actor context pin) to the control-plane tenant — a quiet
/// tenancy violation. Callers now surface the failure as a 500 with
/// the operator-readable cause, which is correct: a credential-
/// verified user with zero memberships is a data-corruption state
/// the user can't recover from on their own, so a silent
/// workspace-1 attribution would only deepen the inconsistency.
pub fn primary_workspace_for_user(conn: &mut DbConnection, user_uuid: Uuid) -> QueryResult<i32> {
    workspace_members::table
        .filter(workspace_members::user_uuid.eq(user_uuid))
        .order(workspace_members::workspace_id.asc())
        .select(workspace_members::workspace_id)
        .first::<i32>(conn)
}

// =====================================================================
// Phase 4 W1: workspace lifecycle ops
// =====================================================================

/// List every workspace. `include_archived = false` filters out
/// soft-archived rows (the admin UI default); `true` returns all
/// rows so the admin can see the tombstoned set before hard delete.
pub fn list_workspaces(
    conn: &mut DbConnection,
    include_archived: bool,
) -> QueryResult<Vec<Workspace>> {
    let mut query = workspaces::table.into_boxed();
    if !include_archived {
        query = query.filter(workspaces::archived_at.is_null());
    }
    query.order(workspaces::id.asc()).load(conn)
}

// sync-audit-only: workspace lifecycle is operator-side; the workspace itself disappearing isn't an event the tenant's sync stream can carry
/// Soft-archive a workspace. Sets `archived_at = NOW()` and returns
/// the updated row. The workspace stops appearing in routing
/// lookups (`find_by_slug` / `find_by_custom_domain` / `find_by_id`
/// all filter `archived_at IS NULL`) but rows persist so an
/// accidental archive is reversible until the grace window elapses
/// and the scheduler hard-deletes. Returns `Ok(None)` if no row
/// matches `id` (already gone, or never existed).
pub fn archive_workspace(conn: &mut DbConnection, id: i32) -> QueryResult<Option<Workspace>> {
    diesel::update(workspaces::table.filter(workspaces::id.eq(id)))
        .set(workspaces::archived_at.eq(Some(Utc::now())))
        .get_result::<Workspace>(conn)
        .optional()
}

// sync-audit-only: workspace lifecycle is operator-side
/// Clear `archived_at` so the workspace is routable again. Used by
/// the admin restore path when an archive was accidental. Safe to
/// call on an already-active workspace (idempotent — the column is
/// already NULL so this is a no-op write).
pub fn restore_workspace(conn: &mut DbConnection, id: i32) -> QueryResult<Option<Workspace>> {
    diesel::update(workspaces::table.filter(workspaces::id.eq(id)))
        .set(workspaces::archived_at.eq::<Option<DateTime<Utc>>>(None))
        .get_result::<Workspace>(conn)
        .optional()
}

// sync-audit-only: workspace lifecycle is operator-side
/// Rename a workspace. Only the display `name` changes — the slug
/// is intentionally not editable here (it has DNS implications +
/// drives link rot). Returns the updated row, or `Ok(None)` if no
/// workspace matched `id`.
pub fn rename_workspace(
    conn: &mut DbConnection,
    id: i32,
    new_name: &str,
) -> QueryResult<Option<Workspace>> {
    diesel::update(workspaces::table.filter(workspaces::id.eq(id)))
        .set(workspaces::name.eq(new_name))
        .get_result::<Workspace>(conn)
        .optional()
}

/// Default grace window between archive and hard delete.
const DEFAULT_HARD_DELETE_GRACE_DAYS: u64 = 30;

/// Grace window between `archived_at` and irreversible delete.
/// Reads `WORKSPACE_HARD_DELETE_GRACE_DAYS` from the environment,
/// falling back to 30 days. The scheduler reads this on every tick
/// so operators can change it without a restart.
pub fn purge_grace_window() -> Duration {
    let days = std::env::var("WORKSPACE_HARD_DELETE_GRACE_DAYS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(DEFAULT_HARD_DELETE_GRACE_DAYS);
    Duration::from_secs(days * 24 * 60 * 60)
}

/// List archived workspaces whose grace window has elapsed and are
/// therefore eligible for hard delete. `cutoff` is the latest
/// `archived_at` value that still qualifies (i.e. the function
/// returns rows with `archived_at <= cutoff`). The scheduler
/// computes `cutoff = NOW() - purge_grace_window()` and passes it
/// in so the value is testable.
pub fn list_workspaces_pending_purge(
    conn: &mut DbConnection,
    cutoff: DateTime<Utc>,
) -> QueryResult<Vec<Workspace>> {
    workspaces::table
        .filter(workspaces::archived_at.is_not_null())
        .filter(workspaces::archived_at.le(Some(cutoff)))
        .order(workspaces::archived_at.asc())
        .load(conn)
}

// sync-audit-only: workspace lifecycle is operator-side
/// Hard-delete a workspace. Refuses unless the row is archived AND
/// its `archived_at` is older than `cutoff` — the cutoff guard is
/// part of the WHERE clause so a race against a restore can't
/// purge a freshly-active workspace. Cascades via the existing
/// `ON DELETE CASCADE` FKs on every tenant table; no manual
/// cascade logic here. Returns the number of rows deleted (0 if
/// the workspace was never archived, never existed, or the
/// archived_at predates the cutoff).
///
/// Before the cascade frees the slug, the slug is recorded in
/// `retired_slugs` so it can never be reused (P1.2). Both callers run
/// this inside a BYPASSRLS transaction, so the tombstone and the
/// delete commit atomically: a purged workspace's slug is always
/// reserved, never half-freed.
pub fn hard_delete_workspace(
    conn: &mut DbConnection,
    id: i32,
    cutoff: DateTime<Utc>,
) -> QueryResult<usize> {
    // Resolve the eligible row first, under the same archived+cutoff
    // guard the DELETE uses, so a race against restore can't tombstone
    // a workspace that's about to be active again. No eligible row =>
    // nothing to purge (preserves the 0-rows contract).
    let eligible: Option<(String, Uuid)> = workspaces::table
        .filter(workspaces::id.eq(id))
        .filter(workspaces::archived_at.is_not_null())
        .filter(workspaces::archived_at.le(Some(cutoff)))
        .select((workspaces::slug, workspaces::uuid))
        .first(conn)
        .optional()?;
    let Some((slug, workspace_uuid)) = eligible else {
        return Ok(0);
    };

    // Reserve the slug. ON CONFLICT keeps it idempotent (a retried
    // purge, or the unreachable case of a slug already retired).
    diesel::insert_into(retired_slugs::table)
        .values((
            retired_slugs::slug.eq(&slug),
            retired_slugs::workspace_uuid.eq(workspace_uuid),
        ))
        .on_conflict(retired_slugs::slug)
        .do_nothing()
        .execute(conn)?;

    // Purge the workspace: every workspace FK cascades (see the
    // cascade_workspace_deletes migration), so removing the row deletes the
    // workspace's tenant data in one statement. Suppress audit (and sync)
    // capture for the cascade via the same GUC the audit-read path uses: the
    // cascade would otherwise fire a per-row audit trigger with no workspace
    // GUC to attribute to, and any audit row it wrote would dangle against the
    // workspace being removed in the same statement. Runs inside the caller's
    // transaction (PlatformConn::run / with_actor_bypass_context), so SET LOCAL
    // is scoped to the purge.
    diesel::sql_query("SET LOCAL nosdesk.in_audit_read = 'true'").execute(conn)?;

    diesel::delete(
        workspaces::table
            .filter(workspaces::id.eq(id))
            .filter(workspaces::archived_at.is_not_null())
            .filter(workspaces::archived_at.le(Some(cutoff))),
    )
    .execute(conn)
}

// =====================================================================
// Phase 4 W3: membership management
// =====================================================================

/// List every workspace the user is a member of, joined to the
/// workspace row itself for the frontend switcher. Filters
/// archived workspaces (the switcher shouldn't surface a workspace
/// that's queued for hard delete). Returns rows in stable order
/// (workspace.id asc) so the switcher's display order is
/// deterministic across renders.
pub fn list_memberships_for_user(
    conn: &mut DbConnection,
    user_uuid: Uuid,
) -> QueryResult<Vec<(WorkspaceMember, Workspace)>> {
    workspace_members::table
        .inner_join(workspaces::table.on(workspaces::id.eq(workspace_members::workspace_id)))
        .filter(workspace_members::user_uuid.eq(user_uuid))
        .filter(workspaces::archived_at.is_null())
        .order(workspaces::id.asc())
        .select((WorkspaceMember::as_select(), Workspace::as_select()))
        .load::<(WorkspaceMember, Workspace)>(conn)
}

/// List every membership row for a workspace. Used by the admin
/// member-management UI.
pub fn list_workspace_members(
    conn: &mut DbConnection,
    workspace_id: i32,
) -> QueryResult<Vec<WorkspaceMember>> {
    workspace_members::table
        .filter(workspace_members::workspace_id.eq(workspace_id))
        .order(workspace_members::user_uuid.asc())
        .load(conn)
}

/// Count owner-role memberships in a workspace. Used by
/// [`remove_membership`] and [`update_membership_role`] to enforce
/// the "at least one owner" invariant — a workspace with zero
/// owners has no one who can manage it.
pub fn count_workspace_owners(conn: &mut DbConnection, workspace_id: i32) -> QueryResult<i64> {
    workspace_members::table
        .filter(workspace_members::workspace_id.eq(workspace_id))
        .filter(workspace_members::role.eq("owner"))
        .count()
        .get_result(conn)
}

// sync-audit-only: removals are recorded by the tr_audit_workspace_members audit_log trigger (P1.4); no sync_actions aggregate
/// Remove a user's membership in a workspace. Refuses to remove
/// the last `owner` row by returning `Ok(0)` with no rows deleted
/// (the handler maps this to 409). Returns the number of rows
/// removed (0 if the user wasn't a member or removal would orphan
/// the workspace, 1 on success).
pub fn remove_membership(
    conn: &mut DbConnection,
    workspace_id: i32,
    user_uuid: Uuid,
) -> QueryResult<usize> {
    // Need the row first so we can check whether removing it would
    // leave the workspace owner-less.
    let existing = workspace_members::table
        .filter(workspace_members::workspace_id.eq(workspace_id))
        .filter(workspace_members::user_uuid.eq(user_uuid))
        .first::<WorkspaceMember>(conn)
        .optional()?;
    let row = match existing {
        Some(r) => r,
        None => return Ok(0),
    };

    if row.role == "owner" {
        let owners = count_workspace_owners(conn, workspace_id)?;
        if owners <= 1 {
            return Ok(0);
        }
    }

    diesel::delete(
        workspace_members::table
            .filter(workspace_members::workspace_id.eq(workspace_id))
            .filter(workspace_members::user_uuid.eq(user_uuid)),
    )
    .execute(conn)
}

/// Outcome of [`update_membership_role`] so the handler can map
/// each failure mode to the right HTTP shape without parsing
/// errors.
#[derive(Debug)]
pub enum UpdateMembershipRoleResult {
    Updated(WorkspaceMember),
    /// No row matched `(workspace_id, user_uuid)`.
    NotFound,
    /// Demoting this row would leave the workspace owner-less.
    LastOwner,
}

/// The persisted workspace role for a member, or `None` if the user isn't a
/// member. A read; used by the group->role mapper to skip no-op role writes.
pub fn get_membership_role(
    conn: &mut DbConnection,
    workspace_id: i32,
    user_uuid: Uuid,
) -> QueryResult<Option<String>> {
    workspace_members::table
        .filter(workspace_members::workspace_id.eq(workspace_id))
        .filter(workspace_members::user_uuid.eq(user_uuid))
        .select(workspace_members::role)
        .first::<String>(conn)
        .optional()
}

// sync-audit-only: accepted_at is a display-only membership timestamp on the audited workspace_members table (tr_audit_workspace_members); the 403 gate checks row existence, not this column, and no sync aggregate subscribes
/// Stamp `accepted_at = now()` on any still-pending memberships for a
/// user (they proved ownership by accepting an invitation). Best-effort:
/// the caller ignores the row count. Must run inside actor + workspace
/// context so the audit trigger has its workspace pin.
pub fn mark_memberships_accepted(conn: &mut DbConnection, user_uuid: Uuid) -> QueryResult<usize> {
    diesel::update(
        workspace_members::table
            .filter(workspace_members::user_uuid.eq(user_uuid))
            .filter(workspace_members::accepted_at.is_null()),
    )
    .set(workspace_members::accepted_at.eq(chrono::Utc::now()))
    .execute(conn)
}

// sync-audit-only: role changes are recorded by the tr_audit_workspace_members audit_log trigger (P1.4); no sync_actions aggregate. This is the sanctioned path to CORRECT a wrong role (projection grants are first-write-wins / immutable, see oauth_provisioning::add_membership)
/// Change a member's role. Refuses to demote the last `owner` for
/// the same reason [`remove_membership`] refuses to delete it.
/// `new_role` is the validated string form
/// (`"owner"` / `"admin"` / `"agent"` / `"member"`) — callers
/// should round-trip through [`WorkspaceRole::as_str`] to avoid
/// typo classes of bugs.
pub fn update_membership_role(
    conn: &mut DbConnection,
    workspace_id: i32,
    user_uuid: Uuid,
    new_role: &str,
) -> QueryResult<UpdateMembershipRoleResult> {
    let existing = workspace_members::table
        .filter(workspace_members::workspace_id.eq(workspace_id))
        .filter(workspace_members::user_uuid.eq(user_uuid))
        .first::<WorkspaceMember>(conn)
        .optional()?;
    let row = match existing {
        Some(r) => r,
        None => return Ok(UpdateMembershipRoleResult::NotFound),
    };

    // Demoting an owner whose owner-count is exactly 1 would leave
    // the workspace orphaned.
    if row.role == "owner" && new_role != "owner" {
        let owners = count_workspace_owners(conn, workspace_id)?;
        if owners <= 1 {
            return Ok(UpdateMembershipRoleResult::LastOwner);
        }
    }

    let updated = diesel::update(
        workspace_members::table
            .filter(workspace_members::workspace_id.eq(workspace_id))
            .filter(workspace_members::user_uuid.eq(user_uuid)),
    )
    .set(workspace_members::role.eq(new_role))
    .get_result::<WorkspaceMember>(conn)?;
    Ok(UpdateMembershipRoleResult::Updated(updated))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::actor::ActorContext;
    use crate::sync::session::with_actor_bypass_context;
    use crate::test_helpers::setup_test_pool;

    /// Workspaces lifecycle writes require the `nosdesk_admin`
    /// BYPASSRLS role; `nosdesk_app` only has SELECT on the
    /// workspaces table (see migration `2026-05-24-140000_tighten_app_grants`).
    /// Production handlers go through `PlatformConn::run` which
    /// wraps every closure in `with_actor_bypass_context`; the
    /// unit tests do the same so they exercise the realistic shape.
    fn as_admin<T, E>(
        conn: &mut DbConnection,
        f: impl FnOnce(&mut DbConnection) -> Result<T, E>,
    ) -> Result<T, E>
    where
        E: From<diesel::result::Error>,
    {
        let actor = ActorContext::system("test:repo:workspaces");
        with_actor_bypass_context(conn, &actor, f)
    }

    fn fresh_workspace(conn: &mut DbConnection, slug: &str) -> Workspace {
        let record = NewWorkspace {
            uuid: Uuid::now_v7(),
            slug: slug.to_string(),
            name: format!("Workspace {slug}"),
            seat_limit: None,
        };
        as_admin(conn, |c| create_workspace(c, &record)).expect("create workspace")
    }

    #[test]
    fn identity_for_ids_returns_active_only() {
        let pool = setup_test_pool();
        let mut conn = pool.get().expect("conn");

        let live = fresh_workspace(&mut conn, "identlive");
        let gone = fresh_workspace(&mut conn, "identgone");
        as_admin(&mut conn, |c| archive_workspace(c, gone.id)).expect("archive");

        let rows = identity_for_ids(&mut conn, &[live.id, gone.id, -1]).expect("read");
        assert_eq!(
            rows,
            vec![(
                live.id,
                "identlive".to_string(),
                "Workspace identlive".to_string()
            )],
            "archived and unknown ids must be omitted"
        );
    }

    #[test]
    fn archive_sets_archived_at_and_hides_from_find() {
        let pool = setup_test_pool();
        let mut conn = pool.get().expect("conn");

        let ws = fresh_workspace(&mut conn, "archtest");
        assert!(ws.archived_at.is_none());

        let archived = as_admin(&mut conn, |c| archive_workspace(c, ws.id))
            .expect("archive")
            .expect("row updated");
        assert!(archived.archived_at.is_some());

        // find_by_slug filters archived rows out.
        let lookup = find_by_slug(&mut conn, "archtest").expect("find");
        assert!(
            lookup.is_none(),
            "archived workspace should not resolve by slug"
        );
    }

    #[test]
    fn restore_clears_archived_at() {
        let pool = setup_test_pool();
        let mut conn = pool.get().expect("conn");

        let ws = fresh_workspace(&mut conn, "restoretest");
        as_admin(&mut conn, |c| archive_workspace(c, ws.id)).expect("archive");

        let restored = as_admin(&mut conn, |c| restore_workspace(c, ws.id))
            .expect("restore")
            .expect("row updated");
        assert!(restored.archived_at.is_none());
        assert!(find_by_slug(&mut conn, "restoretest")
            .expect("find")
            .is_some());
    }

    #[test]
    fn rename_changes_name_only() {
        let pool = setup_test_pool();
        let mut conn = pool.get().expect("conn");

        let ws = fresh_workspace(&mut conn, "renametest");
        let renamed = as_admin(&mut conn, |c| {
            rename_workspace(c, ws.id, "New Display Name")
        })
        .expect("rename")
        .expect("row updated");
        assert_eq!(renamed.name, "New Display Name");
        assert_eq!(renamed.slug, ws.slug, "slug must not change");
    }

    #[test]
    fn hard_delete_refuses_unarchived_row() {
        let pool = setup_test_pool();
        let mut conn = pool.get().expect("conn");

        let ws = fresh_workspace(&mut conn, "noarchive");
        let n = as_admin(&mut conn, |c| hard_delete_workspace(c, ws.id, Utc::now()))
            .expect("hard-delete query");
        assert_eq!(n, 0, "active workspace must not be hard-deleted");
        assert!(find_by_id(&mut conn, ws.id).expect("find").is_some());
    }

    #[test]
    fn hard_delete_refuses_inside_grace_window() {
        let pool = setup_test_pool();
        let mut conn = pool.get().expect("conn");

        let ws = fresh_workspace(&mut conn, "gracewindow");
        as_admin(&mut conn, |c| archive_workspace(c, ws.id)).expect("archive");

        // Cutoff is 1 hour ago; the workspace was just archived, so
        // its archived_at is newer than the cutoff. Delete refuses.
        let cutoff = Utc::now() - chrono::Duration::hours(1);
        let n = as_admin(&mut conn, |c| hard_delete_workspace(c, ws.id, cutoff))
            .expect("hard-delete query");
        assert_eq!(n, 0, "freshly-archived workspace must survive cutoff");
    }

    #[test]
    fn hard_delete_succeeds_past_grace_window() {
        let pool = setup_test_pool();
        let mut conn = pool.get().expect("conn");

        let ws = fresh_workspace(&mut conn, "purgable");
        as_admin(&mut conn, |c| archive_workspace(c, ws.id)).expect("archive");

        // Cutoff is in the future, so every archived row qualifies.
        let cutoff = Utc::now() + chrono::Duration::hours(1);
        let n = as_admin(&mut conn, |c| hard_delete_workspace(c, ws.id, cutoff))
            .expect("hard-delete query");
        assert_eq!(n, 1);
        assert!(find_by_id(&mut conn, ws.id).expect("find").is_none());
    }

    #[test]
    fn hard_delete_tombstones_slug_so_it_cannot_be_reused() {
        let pool = setup_test_pool();
        let mut conn = pool.get().expect("conn");

        let ws = fresh_workspace(&mut conn, "reuseme");
        as_admin(&mut conn, |c| archive_workspace(c, ws.id)).expect("archive");
        let cutoff = Utc::now() + chrono::Duration::hours(1);
        let n = as_admin(&mut conn, |c| hard_delete_workspace(c, ws.id, cutoff))
            .expect("hard-delete query");
        assert_eq!(n, 1);

        // The cascade freed the slug from `workspaces`, but the tombstone
        // reserves it: recreating must fail as SlugTaken (P1.2), the same
        // outcome as an active collision, never a fresh resurrection.
        let record = NewWorkspace {
            uuid: Uuid::now_v7(),
            slug: "reuseme".to_string(),
            name: "Reuse Attempt".to_string(),
            seat_limit: None,
        };
        let err = as_admin(&mut conn, |c| create_workspace(c, &record))
            .expect_err("retired slug must not be reusable");
        assert!(
            matches!(err, CreateWorkspaceError::SlugTaken),
            "expected SlugTaken, got {err:?}"
        );
    }

    #[test]
    fn refused_hard_delete_does_not_tombstone_the_slug() {
        let pool = setup_test_pool();
        let mut conn = pool.get().expect("conn");

        // Archived but inside the grace window: the delete refuses, so
        // the slug must NOT be tombstoned (a later restore keeps it live).
        let ws = fresh_workspace(&mut conn, "stillmine");
        as_admin(&mut conn, |c| archive_workspace(c, ws.id)).expect("archive");
        let cutoff = Utc::now() - chrono::Duration::hours(1);
        let n = as_admin(&mut conn, |c| hard_delete_workspace(c, ws.id, cutoff))
            .expect("hard-delete query");
        assert_eq!(n, 0, "in-grace delete must refuse");

        let retired: i64 = retired_slugs::table
            .filter(retired_slugs::slug.eq("stillmine"))
            .count()
            .get_result(&mut conn)
            .expect("count retired");
        assert_eq!(retired, 0, "a refused delete must not reserve the slug");
    }

    #[test]
    fn list_pending_purge_filters_by_cutoff() {
        let pool = setup_test_pool();
        let mut conn = pool.get().expect("conn");

        let _active = fresh_workspace(&mut conn, "stayactive");
        let archived = fresh_workspace(&mut conn, "willpurge");
        as_admin(&mut conn, |c| archive_workspace(c, archived.id)).expect("archive");

        let pending = as_admin(&mut conn, |c| {
            list_workspaces_pending_purge(c, Utc::now() + chrono::Duration::hours(1))
        })
        .expect("list pending");
        assert!(pending.iter().any(|w| w.id == archived.id));
        assert!(
            !pending.iter().any(|w| w.slug == "stayactive"),
            "active workspaces must not appear in pending-purge list"
        );
    }

    #[test]
    fn list_workspaces_excludes_archived_by_default() {
        let pool = setup_test_pool();
        let mut conn = pool.get().expect("conn");

        let active = fresh_workspace(&mut conn, "listactive");
        let archived = fresh_workspace(&mut conn, "listarchived");
        as_admin(&mut conn, |c| archive_workspace(c, archived.id)).expect("archive");

        let visible = list_workspaces(&mut conn, false).expect("list");
        assert!(visible.iter().any(|w| w.id == active.id));
        assert!(!visible.iter().any(|w| w.id == archived.id));

        let all = list_workspaces(&mut conn, true).expect("list all");
        assert!(all.iter().any(|w| w.id == active.id));
        assert!(all.iter().any(|w| w.id == archived.id));
    }

    #[test]
    fn purge_grace_window_reads_env_var() {
        // Default when unset.
        std::env::remove_var("WORKSPACE_HARD_DELETE_GRACE_DAYS");
        assert_eq!(
            purge_grace_window(),
            Duration::from_secs(DEFAULT_HARD_DELETE_GRACE_DAYS * 86_400)
        );

        // Honors the override.
        std::env::set_var("WORKSPACE_HARD_DELETE_GRACE_DAYS", "7");
        assert_eq!(purge_grace_window(), Duration::from_secs(7 * 86_400));
        std::env::remove_var("WORKSPACE_HARD_DELETE_GRACE_DAYS");
    }

    // -----------------------------------------------------------------
    // Phase 4 W3: membership management tests
    // -----------------------------------------------------------------

    fn fresh_user(conn: &mut DbConnection, name: &str) -> Uuid {
        use crate::models::NewUser;
        use crate::schema::users;
        let new = NewUser {
            uuid: Uuid::new_v4(),
            name: name.to_string(),
            pronouns: None,
            avatar_url: None,
            banner_url: None,
            avatar_thumb: None,
            microsoft_uuid: None,
            mfa_secret: None,
            mfa_secret_kek_id: None,
            mfa_enabled: false,
            platform_role: None,
        };
        let user_uuid = new.uuid;
        as_admin(conn, |c| {
            diesel::insert_into(users::table).values(&new).execute(c)
        })
        .expect("insert user");
        user_uuid
    }

    #[test]
    fn list_memberships_for_user_returns_workspace_join() {
        let pool = setup_test_pool();
        let mut conn = pool.get().expect("conn");

        let user = fresh_user(&mut conn, "membership-test");
        let ws_a = fresh_workspace(&mut conn, "memship-a");
        let ws_b = fresh_workspace(&mut conn, "memship-b");
        let archived = fresh_workspace(&mut conn, "memship-archived");

        as_admin(&mut conn, |c| add_membership(c, ws_a.id, user, "admin")).expect("add a");
        as_admin(&mut conn, |c| add_membership(c, ws_b.id, user, "agent")).expect("add b");
        as_admin(&mut conn, |c| {
            add_membership(c, archived.id, user, "member")
        })
        .expect("add archived");
        as_admin(&mut conn, |c| archive_workspace(c, archived.id)).expect("archive");

        let rows =
            as_admin(&mut conn, |c| list_memberships_for_user(c, user)).expect("list memberships");
        let slugs: Vec<&str> = rows.iter().map(|(_, w)| w.slug.as_str()).collect();
        assert!(slugs.contains(&"memship-a"));
        assert!(slugs.contains(&"memship-b"));
        assert!(
            !slugs.contains(&"memship-archived"),
            "archived workspace must not appear in /me/workspaces switcher list"
        );
    }

    #[test]
    fn count_workspace_owners_reflects_promotion() {
        let pool = setup_test_pool();
        let mut conn = pool.get().expect("conn");
        let ws = fresh_workspace(&mut conn, "ownercount");
        let u1 = fresh_user(&mut conn, "owner1");
        let u2 = fresh_user(&mut conn, "owner2");

        as_admin(&mut conn, |c| add_membership(c, ws.id, u1, "owner")).expect("add owner");
        assert_eq!(
            as_admin(&mut conn, |c| count_workspace_owners(c, ws.id)).expect("count"),
            1
        );
        as_admin(&mut conn, |c| add_membership(c, ws.id, u2, "owner")).expect("add 2nd owner");
        assert_eq!(
            as_admin(&mut conn, |c| count_workspace_owners(c, ws.id)).expect("count"),
            2
        );
    }

    #[test]
    fn remove_membership_refuses_last_owner() {
        let pool = setup_test_pool();
        let mut conn = pool.get().expect("conn");
        let ws = fresh_workspace(&mut conn, "lastowner");
        let owner = fresh_user(&mut conn, "soleowner");
        as_admin(&mut conn, |c| add_membership(c, ws.id, owner, "owner")).expect("add owner");

        let n = as_admin(&mut conn, |c| remove_membership(c, ws.id, owner)).expect("remove");
        assert_eq!(n, 0, "must refuse to remove last owner");
        assert!(membership(&mut conn, ws.id, owner)
            .expect("probe")
            .is_some());
    }

    #[test]
    fn remove_membership_allows_owner_when_another_exists() {
        let pool = setup_test_pool();
        let mut conn = pool.get().expect("conn");
        let ws = fresh_workspace(&mut conn, "twoowners");
        let owner_a = fresh_user(&mut conn, "ownera");
        let owner_b = fresh_user(&mut conn, "ownerb");
        as_admin(&mut conn, |c| add_membership(c, ws.id, owner_a, "owner")).expect("add a");
        as_admin(&mut conn, |c| add_membership(c, ws.id, owner_b, "owner")).expect("add b");

        let n = as_admin(&mut conn, |c| remove_membership(c, ws.id, owner_a)).expect("remove");
        assert_eq!(n, 1);
        assert!(membership(&mut conn, ws.id, owner_a)
            .expect("probe")
            .is_none());
    }

    #[test]
    fn remove_membership_returns_zero_for_unknown_user() {
        let pool = setup_test_pool();
        let mut conn = pool.get().expect("conn");
        let ws = fresh_workspace(&mut conn, "noop-remove");
        let ghost = Uuid::new_v4();

        let n = as_admin(&mut conn, |c| remove_membership(c, ws.id, ghost)).expect("remove ghost");
        assert_eq!(n, 0);
    }

    #[test]
    fn update_membership_role_refuses_demoting_last_owner() {
        let pool = setup_test_pool();
        let mut conn = pool.get().expect("conn");
        let ws = fresh_workspace(&mut conn, "demote-lastowner");
        let owner = fresh_user(&mut conn, "soledemote");
        as_admin(&mut conn, |c| add_membership(c, ws.id, owner, "owner")).expect("add");

        let outcome = as_admin(&mut conn, |c| {
            update_membership_role(c, ws.id, owner, "admin")
        })
        .expect("update");
        assert!(matches!(outcome, UpdateMembershipRoleResult::LastOwner));
    }

    #[test]
    fn update_membership_role_allows_role_change_with_multiple_owners() {
        let pool = setup_test_pool();
        let mut conn = pool.get().expect("conn");
        let ws = fresh_workspace(&mut conn, "demote-ok");
        let a = fresh_user(&mut conn, "co-owner-a");
        let b = fresh_user(&mut conn, "co-owner-b");
        as_admin(&mut conn, |c| add_membership(c, ws.id, a, "owner")).expect("add a");
        as_admin(&mut conn, |c| add_membership(c, ws.id, b, "owner")).expect("add b");

        let outcome =
            as_admin(&mut conn, |c| update_membership_role(c, ws.id, a, "admin")).expect("update");
        match outcome {
            UpdateMembershipRoleResult::Updated(m) => assert_eq!(m.role, "admin"),
            other => panic!("expected Updated, got {other:?}"),
        }
    }

    #[test]
    fn update_membership_role_returns_not_found_for_missing_row() {
        let pool = setup_test_pool();
        let mut conn = pool.get().expect("conn");
        let ws = fresh_workspace(&mut conn, "update-missing");
        let ghost = Uuid::new_v4();

        let outcome = as_admin(&mut conn, |c| {
            update_membership_role(c, ws.id, ghost, "member")
        })
        .expect("update ghost");
        assert!(matches!(outcome, UpdateMembershipRoleResult::NotFound));
    }

    #[test]
    fn membership_changes_are_audit_logged() {
        use diesel::sql_types::{Integer, Nullable, Text, Uuid as SqlUuid};

        let pool = setup_test_pool();
        let mut conn = pool.get().expect("conn");
        let ws = fresh_workspace(&mut conn, "auditws").id;
        let target = fresh_user(&mut conn, "AuditTarget");
        let actor = Uuid::new_v4();

        // Run add / role-change / remove attributed to `actor` in `ws`,
        // the way the W3 handlers do (a user actor pinned to the
        // workspace). The tr_audit_workspace_members trigger should
        // capture one audit_log row per mutation.
        let act = ActorContext::user_at_workspace(actor, ws);
        with_actor_bypass_context::<_, diesel::result::Error>(&mut conn, &act, |c| {
            add_membership(c, ws, target, "member")?;
            update_membership_role(c, ws, target, "admin")?;
            remove_membership(c, ws, target)?;
            Ok(())
        })
        .expect("membership mutations");

        #[derive(QueryableByName, Debug)]
        struct AuditRow {
            #[diesel(sql_type = Text)]
            op: String,
            #[diesel(sql_type = Text)]
            pk_text: String,
            #[diesel(sql_type = Integer)]
            workspace_id: i32,
            #[diesel(sql_type = Nullable<SqlUuid>)]
            actor_uuid: Option<Uuid>,
            #[diesel(sql_type = Nullable<Text>)]
            changed: Option<String>,
        }

        let rows: Vec<AuditRow> = as_admin(&mut conn, |c| {
            diesel::sql_query(
                "SELECT op::text AS op, pk_text, workspace_id, actor_uuid, \
                 array_to_string(changed_cols, ',') AS changed \
                 FROM audit_log WHERE table_name = 'workspace_members' AND pk_text = $1 \
                 ORDER BY id",
            )
            .bind::<Text, _>(target.to_string())
            .load(c)
        })
        .expect("load audit rows");

        let ops: Vec<&str> = rows.iter().map(|r| r.op.as_str()).collect();
        assert_eq!(
            ops,
            vec!["I", "U", "D"],
            "one audit row per mutation, in order"
        );
        for r in &rows {
            assert_eq!(r.pk_text, target.to_string(), "pk is the member uuid");
            assert_eq!(r.workspace_id, ws, "audit row carries the row's workspace");
            assert_eq!(r.actor_uuid, Some(actor), "attributed to the acting admin");
        }
        // The update names the column that changed.
        assert!(
            rows[1].changed.as_deref().unwrap_or("").contains("role"),
            "role-change row should list role in changed_cols: {:?}",
            rows[1].changed
        );
    }

    // ── Staff seat cap (workspace_seat_limit migration + trigger) ──────────

    #[test]
    fn seat_limit_caps_staff_but_not_members() {
        let pool = setup_test_pool();
        let mut conn = pool.get().expect("conn");
        let ws = fresh_workspace(&mut conn, "seatcap");
        as_admin(&mut conn, |c| set_seat_limit(c, &ws.slug, Some(2))).expect("set cap");

        let (s1, s2, s3) = (
            fresh_user(&mut conn, "seat-s1"),
            fresh_user(&mut conn, "seat-s2"),
            fresh_user(&mut conn, "seat-s3"),
        );
        let m1 = fresh_user(&mut conn, "seat-m1");

        // Two staff fit the cap of 2.
        as_admin(&mut conn, |c| add_membership(c, ws.id, s1, "owner")).expect("owner");
        as_admin(&mut conn, |c| add_membership(c, ws.id, s2, "agent")).expect("agent");
        assert_eq!(
            as_admin(&mut conn, |c| count_staff_members(c, ws.id)).unwrap(),
            2
        );

        // End-user members don't count against the cap.
        as_admin(&mut conn, |c| add_membership(c, ws.id, m1, "member"))
            .expect("member is uncapped");

        // The third staff member trips the trigger.
        let err = as_admin(&mut conn, |c| add_membership(c, ws.id, s3, "agent")).unwrap_err();
        assert!(
            is_seat_limit_violation(&err),
            "expected seat-limit, got {err:?}"
        );
    }

    #[test]
    fn null_seat_limit_is_uncapped() {
        let pool = setup_test_pool();
        let mut conn = pool.get().expect("conn");
        let ws = fresh_workspace(&mut conn, "uncapped"); // seat_limit None
        for i in 0..6 {
            let u = fresh_user(&mut conn, &format!("unc-{i}"));
            as_admin(&mut conn, |c| add_membership(c, ws.id, u, "agent")).expect("uncapped add");
        }
        assert_eq!(
            as_admin(&mut conn, |c| count_staff_members(c, ws.id)).unwrap(),
            6
        );
    }

    #[test]
    fn promotion_to_staff_respects_seat_limit() {
        let pool = setup_test_pool();
        let mut conn = pool.get().expect("conn");
        let ws = fresh_workspace(&mut conn, "promote");
        as_admin(&mut conn, |c| set_seat_limit(c, &ws.slug, Some(1))).expect("set cap");
        let owner = fresh_user(&mut conn, "promo-owner");
        let member = fresh_user(&mut conn, "promo-member");
        as_admin(&mut conn, |c| add_membership(c, ws.id, owner, "owner")).expect("owner"); // 1 staff
        as_admin(&mut conn, |c| add_membership(c, ws.id, member, "member")).expect("member");

        // Promoting the member to a staff role would exceed the cap of 1.
        let err = as_admin(&mut conn, |c| {
            update_membership_role(c, ws.id, member, "agent")
        })
        .unwrap_err();
        assert!(
            is_seat_limit_violation(&err),
            "expected seat-limit, got {err:?}"
        );
    }

    #[test]
    fn set_seat_limit_sets_and_clears() {
        let pool = setup_test_pool();
        let mut conn = pool.get().expect("conn");
        let ws = fresh_workspace(&mut conn, "setcap");

        assert_eq!(
            as_admin(&mut conn, |c| set_seat_limit(c, &ws.slug, Some(5))).unwrap(),
            1
        );
        let lim: Option<i32> = as_admin(&mut conn, |c| {
            workspaces::table
                .find(ws.id)
                .select(workspaces::seat_limit)
                .first(c)
        })
        .unwrap();
        assert_eq!(lim, Some(5));

        as_admin(&mut conn, |c| set_seat_limit(c, &ws.slug, None)).unwrap();
        let lifted: Option<i32> = as_admin(&mut conn, |c| {
            workspaces::table
                .find(ws.id)
                .select(workspaces::seat_limit)
                .first(c)
        })
        .unwrap();
        assert_eq!(lifted, None, "lift clears the cap");

        // Unknown slug → no rows updated.
        assert_eq!(
            as_admin(&mut conn, |c| set_seat_limit(c, "no-such-slug", Some(1))).unwrap(),
            0
        );
    }
}
