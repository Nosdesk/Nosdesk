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
use crate::schema::{workspace_members, workspaces};

/// Returned by [`create_workspace`] so the caller can distinguish a
/// slug-collision from other DB failures without parsing error
/// strings.
#[derive(Debug)]
pub enum CreateWorkspaceError {
    /// The requested slug is already taken by another (active OR
    /// tombstoned) workspace row. UNIQUE on `workspaces.slug`
    /// enforces this at the DB layer; we surface it as a typed
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
/// per the locked-decision in `docs/m5-product-side-handoff.md` (the
/// product owns workspace identity; the control plane mirrors).
/// `plan` is omitted so the DB default (`'free'`) applies.
pub fn create_workspace(
    conn: &mut DbConnection,
    record: &NewWorkspace,
) -> Result<Workspace, CreateWorkspaceError> {
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

// sync-pending-wire: emit a WorkspaceMember sync_action when that aggregate + the Phase 4 W3 membership lifecycle handlers land; today no aggregate variant exists and workspace_members has no audit_log trigger, so the grant rides along with the user.created emit in the same create_user_with_email txn
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
pub fn archive_workspace(
    conn: &mut DbConnection,
    id: i32,
) -> QueryResult<Option<Workspace>> {
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
pub fn restore_workspace(
    conn: &mut DbConnection,
    id: i32,
) -> QueryResult<Option<Workspace>> {
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
pub fn hard_delete_workspace(
    conn: &mut DbConnection,
    id: i32,
    cutoff: DateTime<Utc>,
) -> QueryResult<usize> {
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
pub fn count_workspace_owners(
    conn: &mut DbConnection,
    workspace_id: i32,
) -> QueryResult<i64> {
    workspace_members::table
        .filter(workspace_members::workspace_id.eq(workspace_id))
        .filter(workspace_members::role.eq("owner"))
        .count()
        .get_result(conn)
}

// sync-audit-only: workspace_members lifecycle is operator-side; emit comes in Phase 4 W3 once the WorkspaceMember aggregate ships
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

// sync-audit-only: workspace_members lifecycle is operator-side
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
        };
        as_admin(conn, |c| create_workspace(c, &record)).expect("create workspace")
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
        assert!(lookup.is_none(), "archived workspace should not resolve by slug");
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
        assert!(find_by_slug(&mut conn, "restoretest").expect("find").is_some());
    }

    #[test]
    fn rename_changes_name_only() {
        let pool = setup_test_pool();
        let mut conn = pool.get().expect("conn");

        let ws = fresh_workspace(&mut conn, "renametest");
        let renamed = as_admin(&mut conn, |c| rename_workspace(c, ws.id, "New Display Name"))
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
            diesel::insert_into(users::table)
                .values(&new)
                .execute(c)
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
        as_admin(&mut conn, |c| add_membership(c, archived.id, user, "member"))
            .expect("add archived");
        as_admin(&mut conn, |c| archive_workspace(c, archived.id)).expect("archive");

        let rows = as_admin(&mut conn, |c| list_memberships_for_user(c, user))
            .expect("list memberships");
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

        let n = as_admin(&mut conn, |c| remove_membership(c, ws.id, owner))
            .expect("remove");
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

        let n = as_admin(&mut conn, |c| remove_membership(c, ws.id, owner_a))
            .expect("remove");
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

        let n = as_admin(&mut conn, |c| remove_membership(c, ws.id, ghost))
            .expect("remove ghost");
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

        let outcome = as_admin(&mut conn, |c| {
            update_membership_role(c, ws.id, a, "admin")
        })
        .expect("update");
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
}
