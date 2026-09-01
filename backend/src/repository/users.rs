use diesel::prelude::*;
use diesel::result::Error;
use serde_json::json;
use uuid::Uuid;

use crate::db::DbConnection;
use crate::models::*;
use crate::schema::*;
use crate::sync::emit::{self, SyncEmit};
use crate::sync::groups;

/// Default grace window between soft-delete and permanent purge.
/// Salesforce never hard-deletes; Google releases after 20 days;
/// Atlassian steers admins to "deactivate". Thirty days is a
/// middle ground that gives an admin a forgiving recovery window
/// without keeping personal data around indefinitely (GDPR-style
/// concerns when a user has explicitly asked for erasure are
/// handled by the "Permanently delete" admin action that bypasses
/// the window).
const DEFAULT_PURGE_GRACE_DAYS: i64 = 30;

/// Configurable grace window via `NOSDESK_USER_PURGE_GRACE_DAYS`.
/// Returns the [`Duration`](chrono::Duration) the retention worker
/// uses to decide which soft-deleted users to purge. Out-of-range
/// values (zero, negative, over a year) fall back to the default
/// rather than masking a config typo.
pub fn purge_grace_window() -> chrono::Duration {
    let days = std::env::var("NOSDESK_USER_PURGE_GRACE_DAYS")
        .ok()
        .and_then(|s| s.trim().parse::<i64>().ok())
        .filter(|n| *n > 0 && *n <= 365)
        .unwrap_or(DEFAULT_PURGE_GRACE_DAYS);
    chrono::Duration::days(days)
}

/// Emit a `user.updated` sync_action carrying the projection the
/// frontend's `useReference('user', uuid)` consumes. Called from
/// inside the same transaction as the SQL write so the event row
/// appears atomically with the changed row. Pulls the primary
/// email from `user_emails` since the canonical address lives
/// outside the `users` table.
fn emit_user_event(
    conn: &mut DbConnection,
    user: &User,
    op: SyncOp,
    event_type: &'static str,
) -> QueryResult<()> {
    let email =
        crate::repository::user_helpers::get_primary_email(&user.uuid, conn).unwrap_or_default();
    let workspace_role = crate::repository::user_helpers::workspace_role(conn, user.uuid)
        .map(|r| r.as_str().to_string());
    // Personal dashboard layout lives in `user_preferences`; carry it
    // so a user's own sessions sync the arrangement live through the
    // pool. Tolerant fetch — on delete the prefs row may have already
    // cascaded, which is fine (the layout is irrelevant then).
    let dashboard_layout = crate::repository::user_preferences::get(conn, user.uuid)
        .ok()
        .and_then(|p| p.dashboard_layout);
    emit::record(
        conn,
        SyncEmit {
            aggregate: SyncAggregate::User,
            aggregate_id: user.uuid.to_string(),
            op,
            event_type,
            data: json!({
                "uuid": user.uuid,
                "name": user.name,
                "email": email,
                "platform_role": user.platform_role,
                "workspace_role": workspace_role,
                "pronouns": user.pronouns,
                "avatar_url": user.avatar_url,
                "avatar_thumb": user.avatar_thumb,
                "deleted_at": user.deleted_at,
                "dashboard_layout": dashboard_layout,
            }),
            groups: groups::workspace(),
            causation_id: None,
        },
    )?;
    Ok(())
}

/// Observer fired after a user's row is updated. Mirrors
/// `UserCreatedObserver` so the search service can keep its index
/// in sync with name / email / role changes regardless of which
/// handler made the edit.
pub trait UserUpdatedObserver: Send + Sync {
    fn user_updated(&self, user: &User, primary_email: Option<&str>);
}

/// Observer fired after a user is deleted. Implementor removes the
/// user from the search index so the row doesn't haunt search
/// results after removal.
pub trait UserDeletedObserver: Send + Sync {
    fn user_deleted(&self, user_uuid: &Uuid);
}

// User repository functions
pub fn get_users(conn: &mut DbConnection) -> Result<Vec<User>, Error> {
    users::table.order_by(users::name.asc()).load::<User>(conn)
}

// Get paginated users with filtering and sorting
/// Filter on the soft-delete state when paginating users. Default
/// is `Active` so every existing call site naturally hides
/// soft-deleted rows from active surfaces (mention search,
/// assignee pickers, the default admin list). The admin "Deleted
/// users" view passes [`DeletedFilter::Only`] to flip the
/// condition; debugging surfaces can pass [`DeletedFilter::All`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeletedFilter {
    Active,
    Only,
    All,
}

impl DeletedFilter {
    /// Parse the `deleted=` query string value. Unrecognised input
    /// falls back to `Active` so a typo doesn't accidentally
    /// expose soft-deleted rows.
    pub fn from_query(value: Option<&str>) -> Self {
        match value.map(str::trim).map(str::to_lowercase).as_deref() {
            Some("deleted") | Some("only") => DeletedFilter::Only,
            Some("all") => DeletedFilter::All,
            _ => DeletedFilter::Active,
        }
    }
}

/// Which People population the caller wants: the staff Team or the end-user
/// Requesters. Absent = the combined directory (no population filter).
#[derive(Clone, Copy)]
pub enum Population {
    Team,
    Requesters,
}

impl Population {
    /// Parse the `population=` query value; unknown/absent = None (no filter).
    pub fn from_query(value: Option<&str>) -> Option<Self> {
        match value.map(str::trim).map(str::to_lowercase).as_deref() {
            Some("team") => Some(Population::Team),
            Some("requesters") | Some("requester") => Some(Population::Requesters),
            _ => None,
        }
    }
}

/// The single definition of "this row is a requester (end-user)": platform role
/// `user` and a workspace membership of `member` (or none). Team is its
/// negation, so the split lives in one place. `workspace_id` is an i32, so the
/// interpolation is injection-safe (same idiom as the role filter).
fn requester_sql(workspace_id: i32) -> String {
    format!(
        "(users.platform_role = 'user' \
         AND COALESCE((SELECT role FROM workspace_members \
                       WHERE workspace_id = {workspace_id} AND user_uuid = users.uuid), \
                      'member') = 'member')"
    )
}

#[allow(clippy::too_many_arguments)]
pub fn get_paginated_users(
    conn: &mut DbConnection,
    page: i64,
    page_size: i64,
    sort_field: Option<String>,
    sort_direction: Option<String>,
    search: Option<String>,
    role: Option<String>,
    population: Option<Population>,
    deleted: DeletedFilter,
    workspace_id: i32,
) -> Result<(Vec<User>, i64), Error> {
    use crate::schema::user_emails;

    // Resolve search UUIDs once (if search is active)
    let search_uuids: Option<Vec<Uuid>> = match search.as_deref() {
        Some(term) if !term.is_empty() => {
            let pattern = format!("%{}%", term.to_lowercase());
            Some(
                users::table
                    .left_join(user_emails::table.on(user_emails::user_uuid.eq(users::uuid)))
                    .select(users::uuid)
                    .filter(
                        users::name
                            .ilike(pattern.clone())
                            .or(user_emails::email.ilike(pattern)),
                    )
                    .distinct()
                    .load::<Uuid>(conn)?,
            )
        }
        _ => None,
    };

    // Parse role filter — accepts a single role ("admin") or a
    // comma-separated set ("admin,technician") so the assignee
    // picker can hit the eligible-staff set in one request instead
    // of one request per role. "all" stays the no-filter sentinel.
    let parsed_roles: Vec<String> = match role.as_deref() {
        None => Vec::new(),
        Some("all") => Vec::new(),
        Some(s) => s
            .split(',')
            .map(|piece| piece.trim().to_lowercase())
            .filter(|piece| !piece.is_empty())
            .filter(|piece| crate::utils::parse_roles(piece).is_ok())
            .collect(),
    };

    // Build the post-W2 role filter as raw SQL. The legacy
    // `users.role` column is gone; "effective role" derives from
    // `users.platform_role` + the caller's `workspace_members.role` in
    // the request's workspace. One OR-clause per requested legacy role;
    // an empty result keeps the filter off entirely. `workspace_id` is an
    // i32, so interpolating it into the subquery is injection-safe.
    let role_sql_filter: Option<String> = if parsed_roles.is_empty() {
        None
    } else {
        let mut parts: Vec<String> = Vec::new();
        let mut any = false;
        for r in &parsed_roles {
            match r.as_str() {
                "admin" => {
                    parts.push(format!(
                        "(users.platform_role = 'platform_admin' \
                         OR (SELECT role FROM workspace_members \
                             WHERE workspace_id = {workspace_id} AND user_uuid = users.uuid) \
                            IN ('owner', 'admin'))"
                    ));
                    any = true;
                }
                "technician" => {
                    parts.push(format!(
                        "(users.platform_role <> 'platform_admin' \
                         AND (SELECT role FROM workspace_members \
                              WHERE workspace_id = {workspace_id} AND user_uuid = users.uuid) \
                             = 'agent')"
                    ));
                    any = true;
                }
                "user" => {
                    parts.push(format!(
                        "(users.platform_role <> 'platform_admin' \
                         AND COALESCE((SELECT role FROM workspace_members \
                                       WHERE workspace_id = {workspace_id} AND user_uuid = users.uuid), \
                                      'member') = 'member')"
                    ));
                    any = true;
                }
                // audit_reviewer lives on users.platform_role.
                "audit_reviewer" => {
                    parts.push("users.platform_role = 'audit_reviewer'".to_string());
                    any = true;
                }
                _ => {}
            }
        }
        if any {
            Some(parts.join(" OR "))
        } else {
            // Caller asked for only audit_reviewer (no matches) —
            // force an empty result.
            Some("false".to_string())
        }
    };

    // Coarse People split: Team = staff, Requesters = end-users. One predicate
    // defines a requester; Team is its negation.
    let population_sql: Option<String> = population.map(|p| match p {
        Population::Requesters => requester_sql(workspace_id),
        Population::Team => format!("NOT {}", requester_sql(workspace_id)),
    });

    // CASE-rank used by sort-by-role: same tier ordering as the
    // derived projection (admin < technician < user < other), scoped to
    // the request's workspace.
    let role_rank_sql: String = format!(
        "CASE \
        WHEN users.platform_role = 'platform_admin' THEN 0 \
        WHEN (SELECT role FROM workspace_members \
              WHERE workspace_id = {workspace_id} AND user_uuid = users.uuid) \
             IN ('owner', 'admin') THEN 1 \
        WHEN (SELECT role FROM workspace_members \
              WHERE workspace_id = {workspace_id} AND user_uuid = users.uuid) \
             = 'agent' THEN 2 \
        ELSE 3 END"
    );

    // Sort-only correlated subqueries for the three fields that aren't
    // `users` columns. The handler enriches each page with the primary
    // email and the ticket / asset counts *after* LIMIT, so without
    // these the list could only ever be ordered by name and role.
    //
    // Each fragment deliberately mirrors the predicates of its
    // counterpart in `handlers::users` (`get_open_ticket_counts_batch`
    // / `get_device_counts_batch` / `get_primary_emails_batch`) — same
    // joins, same filters, and likewise no explicit workspace
    // predicate, leaving that to RLS on the same connection. If the
    // ORDER BY counted a different set than the displayed number, the
    // column would sort into an order the numbers appear to contradict.
    //
    // Static SQL, and `workspace_id` is an i32, so there is nothing
    // interpolated that could be injected. Same idiom as
    // `role_rank_sql` above.
    const OPEN_TICKET_COUNT_SQL: &str = "(SELECT COUNT(*) FROM tickets t \
         JOIN workflow_states ws ON ws.id = t.workflow_state_id \
         WHERE t.assignee_uuid = users.uuid \
           AND ws.category IN ('triage', 'backlog', 'active', 'in_review'))";
    const DEVICE_COUNT_SQL: &str = "(SELECT COUNT(*) FROM assets a \
         WHERE a.primary_user_uuid = users.uuid)";
    const PRIMARY_EMAIL_SQL: &str = "(SELECT ue.email FROM user_emails ue \
         WHERE ue.user_uuid = users.uuid AND ue.is_primary LIMIT 1)";

    // Count query with filters
    let mut count_query = users::table.into_boxed();
    if let Some(ref uuids) = search_uuids {
        count_query = count_query.filter(users::uuid.eq_any(uuids.clone()));
    }
    if let Some(ref filter) = role_sql_filter {
        count_query = count_query.filter(diesel::dsl::sql::<diesel::sql_types::Bool>(filter));
    }
    if let Some(ref filter) = population_sql {
        count_query = count_query.filter(diesel::dsl::sql::<diesel::sql_types::Bool>(filter));
    }
    count_query = match deleted {
        DeletedFilter::Active => count_query.filter(users::deleted_at.is_null()),
        DeletedFilter::Only => count_query.filter(users::deleted_at.is_not_null()),
        DeletedFilter::All => count_query,
    };
    let total: i64 = count_query.count().get_result(conn)?;

    // Data query with same filters + sort + pagination
    let mut query = users::table.into_boxed();
    if let Some(ref uuids) = search_uuids {
        query = query.filter(users::uuid.eq_any(uuids.clone()));
    }
    if let Some(ref filter) = population_sql {
        query = query.filter(diesel::dsl::sql::<diesel::sql_types::Bool>(filter));
    }
    if let Some(ref filter) = role_sql_filter {
        query = query.filter(diesel::dsl::sql::<diesel::sql_types::Bool>(filter));
    }
    query = match deleted {
        DeletedFilter::Active => query.filter(users::deleted_at.is_null()),
        DeletedFilter::Only => query.filter(users::deleted_at.is_not_null()),
        DeletedFilter::All => query,
    };
    query = match (sort_field.as_deref(), sort_direction.as_deref()) {
        (Some("name"), Some("asc")) => query
            .order(users::name.asc())
            .then_order_by(users::uuid.asc()),
        (Some("name"), _) => query
            .order(users::name.desc())
            .then_order_by(users::uuid.asc()),
        (Some("role"), Some("asc")) => query
            .order(diesel::dsl::sql::<diesel::sql_types::Integer>(&role_rank_sql).asc())
            .then_order_by(users::uuid.asc()),
        (Some("role"), _) => query
            .order(diesel::dsl::sql::<diesel::sql_types::Integer>(&role_rank_sql).desc())
            .then_order_by(users::uuid.asc()),
        (Some("created_at"), Some("asc")) => query
            .order(users::created_at.asc())
            .then_order_by(users::uuid.asc()),
        (Some("created_at"), _) => query
            .order(users::created_at.desc())
            .then_order_by(users::uuid.asc()),
        // A user with no primary email row sorts last either way,
        // rather than leading an A-Z sort with a blank cell.
        (Some("email"), Some("asc")) => query
            .order(
                diesel::dsl::sql::<diesel::sql_types::Nullable<diesel::sql_types::Text>>(
                    PRIMARY_EMAIL_SQL,
                )
                .asc()
                .nulls_last(),
            )
            .then_order_by(users::uuid.asc()),
        (Some("email"), _) => query
            .order(
                diesel::dsl::sql::<diesel::sql_types::Nullable<diesel::sql_types::Text>>(
                    PRIMARY_EMAIL_SQL,
                )
                .desc()
                .nulls_last(),
            )
            .then_order_by(users::uuid.asc()),
        (Some("open_ticket_count"), Some("asc")) => query
            .order(diesel::dsl::sql::<diesel::sql_types::BigInt>(OPEN_TICKET_COUNT_SQL).asc())
            .then_order_by(users::uuid.asc()),
        (Some("open_ticket_count"), _) => query
            .order(diesel::dsl::sql::<diesel::sql_types::BigInt>(OPEN_TICKET_COUNT_SQL).desc())
            .then_order_by(users::uuid.asc()),
        (Some("device_count"), Some("asc")) => query
            .order(diesel::dsl::sql::<diesel::sql_types::BigInt>(DEVICE_COUNT_SQL).asc())
            .then_order_by(users::uuid.asc()),
        (Some("device_count"), _) => query
            .order(diesel::dsl::sql::<diesel::sql_types::BigInt>(DEVICE_COUNT_SQL).desc())
            .then_order_by(users::uuid.asc()),
        _ => query
            .order(users::name.asc())
            .then_order_by(users::uuid.asc()),
    };

    let offset = (page - 1) * page_size;
    let results = query.offset(offset).limit(page_size).load::<User>(conn)?;
    Ok((results, total))
}

// Note: get_user_by_id removed - users table now uses UUID as primary key
// Use get_user_by_uuid instead.
//
// **Active-vs-any semantics.** `get_user_by_uuid` returns the row
// regardless of `deleted_at`. Use it ONLY for non-auth-gated paths
// that need to render or reference soft-deleted users (audit log,
// comment history, ticket assignee/requester display, admin
// "view-deleted" surfaces).
//
// For any path where the user's row drives an *authentication
// decision* — login, MFA verify, JWT validation, API token auth,
// passkey ceremony, password reset, OAuth callback — call
// [`find_active_by_uuid`] below instead. Otherwise a user
// soft-deleted between credential enrolment and the next auth
// attempt can still authenticate (F2C.2 H4 finding from the
// cross-codebase audit).
pub fn get_user_by_uuid(uuid: &Uuid, conn: &mut DbConnection) -> Result<User, Error> {
    users::table.find(uuid).first::<User>(conn)
}

/// Fetch a user row only if the account is active (i.e. `deleted_at
/// IS NULL`). The active-only variant of [`get_user_by_uuid`].
///
/// Returns `Error::NotFound` for both "row doesn't exist" and
/// "row exists but is soft-deleted" — callers in auth paths
/// should treat both as "this user can't authenticate" without
/// distinguishing (don't leak deletion state in error messages
/// or response timing). The fail-closed direction.
///
/// **Use this in every auth gate.** The sibling repo's F2C audit
/// flagged "soft-deleted user can still authenticate via passkey"
/// as HIGH because the passkey login path looked up the user
/// without filtering `deleted_at`. The whole class of bug —
/// password reset, MFA verify, OAuth callback completion — has
/// the same shape.
pub fn find_active_by_uuid(uuid: &Uuid, conn: &mut DbConnection) -> Result<User, Error> {
    users::table
        .find(uuid)
        .filter(users::deleted_at.is_null())
        .first::<User>(conn)
}

// This function now delegates to user_helpers module since email is in user_emails table
pub fn get_user_by_email(email: &str, conn: &mut DbConnection) -> Result<User, Error> {
    crate::repository::user_helpers::get_user_by_email(email, conn)
}

pub fn get_user_by_name(name: &str, conn: &mut DbConnection) -> Result<User, Error> {
    users::table
        .filter(users::name.eq(name))
        .first::<User>(conn)
}

// sync-audit-only: Vestigial low-level helper: handlers all use `user_helpers::create_user_with_email` (which IS sync-wired). Kept here so a future stray caller still passes the lint, but new code should reach for the wired helper
pub fn create_user(user: NewUser, conn: &mut DbConnection) -> Result<User, Error> {
    diesel::insert_into(users::table)
        .values(user)
        .get_result(conn)
}

// sync-audit-only: vestigial low-level helper; handlers use sync-wired user_helpers
pub fn update_user(
    user_uuid: &Uuid,
    user: UserUpdate,
    conn: &mut DbConnection,
    observer: Option<&dyn UserUpdatedObserver>,
) -> Result<User, Error> {
    // Wrap the UPDATE + sync emit in a single transaction so a
    // crash between the two never leaves the row updated without
    // a corresponding sync_actions event (or vice versa).
    let result: User = conn.transaction::<User, Error, _>(|conn| {
        let updated: User = diesel::update(users::table.find(user_uuid))
            .set(user)
            .get_result(conn)?;
        emit_user_event(conn, &updated, SyncOp::Update, "user.updated")?;
        Ok(updated)
    })?;

    if let Some(observer) = observer {
        // Fetch the primary email for the index doc; best-effort,
        // don't fail the update if the lookup misses.
        let primary_email = crate::repository::user_helpers::get_primary_email(user_uuid, conn);
        observer.user_updated(&result, primary_email.as_deref());
    }

    Ok(result)
}

/// Soft-delete a user: stamp `deleted_at`, emit `user.updated`,
/// and return the updated row. The row stays in the table so
/// historical references (tickets, audit log, plugin installs)
/// keep resolving; the active-user query filter hides it from
/// login + mention search + assignee pickers.
///
/// Restorable via [`restore_user`] until the retention worker
/// invokes [`purge_user`] after the grace window.
pub fn soft_delete_user(user_uuid: &Uuid, conn: &mut DbConnection) -> Result<User, Error> {
    conn.transaction::<User, Error, _>(|conn| {
        let now = chrono::Utc::now().naive_utc();
        let updated: User = diesel::update(users::table.find(user_uuid))
            .set((users::deleted_at.eq(Some(now)), users::updated_at.eq(now)))
            .get_result(conn)?;
        // emit::record fires inside emit_user_event.
        emit_user_event(conn, &updated, SyncOp::Update, "user.soft_deleted")?;
        Ok(updated)
    })
}

/// Inverse of [`soft_delete_user`]: clears `deleted_at` and emits
/// `user.updated`. The user becomes visible to all active-user
/// surfaces again. Cached sessions stay revoked (the auth gate
/// invalidated them on soft-delete); the restored user must
/// re-authenticate.
pub fn restore_user(user_uuid: &Uuid, conn: &mut DbConnection) -> Result<User, Error> {
    conn.transaction::<User, Error, _>(|conn| {
        let now = chrono::Utc::now().naive_utc();
        let updated: User = diesel::update(users::table.find(user_uuid))
            .set((
                users::deleted_at.eq::<Option<chrono::NaiveDateTime>>(None),
                users::updated_at.eq(now),
            ))
            .get_result(conn)?;
        // emit::record fires inside emit_user_event.
        emit_user_event(conn, &updated, SyncOp::Update, "user.restored")?;
        Ok(updated)
    })
}

// sync-audit-only: read-only query for the retention worker
/// Load every soft-deleted user whose `deleted_at` is older than
/// `before`. The retention worker calls this once per cron tick
/// to find rows it should hand to [`purge_user`].
pub fn list_users_pending_purge(
    conn: &mut DbConnection,
    before: chrono::NaiveDateTime,
) -> Result<Vec<User>, Error> {
    users::table
        .filter(users::deleted_at.is_not_null())
        .filter(users::deleted_at.lt(before))
        .order(users::deleted_at.asc())
        .load::<User>(conn)
}

// sync-audit-only: vestigial low-level helper; handlers use sync-wired user_helpers
/// Hard-delete a user and every row that FK-references them. Only
/// the retention worker (after the grace window) and the
/// registration-rollback path in auth.rs should call this. Admin
/// "Delete" buttons route through [`soft_delete_user`] instead.
///
/// Renamed from `delete_user` in the soft-delete rollout so it's
/// obvious at call sites which semantics are being requested.
pub fn purge_user(
    user_uuid: &Uuid,
    conn: &mut DbConnection,
    observer: Option<&dyn UserDeletedObserver>,
) -> Result<usize, Error> {
    use crate::schema::{
        article_contents, assets, attachments, comments, documentation_pages,
        documentation_revisions, linked_tickets, project_tickets, projects, sync_history,
        ticket_assets, tickets, user_auth_identities, user_emails,
    };

    // Start a transaction to ensure all-or-nothing deletion
    conn.transaction::<_, Error, _>(|conn| {
        // === Phase 1: Handle RESTRICT constraints ===
        // Tables with ON DELETE RESTRICT require explicit deletion/reassignment first

        // 1a. Delete all comments by this user
        diesel::delete(comments::table.filter(comments::user_uuid.eq(user_uuid))).execute(conn)?;

        // 1b. Update tickets where user is requester (RESTRICT) - set to NULL
        diesel::update(tickets::table.filter(tickets::requester_uuid.eq(user_uuid)))
            .set(tickets::requester_uuid.eq::<Option<Uuid>>(None))
            .execute(conn)?;

        // 1c. Update documentation_pages created_by/last_edited_by (RESTRICT)
        // Reassign to first available admin
        let first_admin: Option<User> = users::table
            .into_boxed()
            .filter(users::platform_role.eq("platform_admin"))
            .filter(users::uuid.ne(user_uuid))
            .first(conn)
            .optional()?;

        if let Some(admin) = first_admin {
            // Reassign documentation to another admin
            diesel::update(
                documentation_pages::table.filter(documentation_pages::created_by.eq(user_uuid)),
            )
            .set(documentation_pages::created_by.eq(admin.uuid))
            .execute(conn)?;

            diesel::update(
                documentation_pages::table
                    .filter(documentation_pages::last_edited_by.eq(user_uuid)),
            )
            .set(documentation_pages::last_edited_by.eq(admin.uuid))
            .execute(conn)?;

            diesel::update(
                documentation_revisions::table
                    .filter(documentation_revisions::created_by.eq(user_uuid)),
            )
            .set(documentation_revisions::created_by.eq(admin.uuid))
            .execute(conn)?;
        }
        // Note: If no other admin exists, delete fails due to FK constraint
        // Intentional - at least one admin must own documentation

        // === Phase 2: Handle SET NULL constraints ===
        // These tables have ON DELETE SET NULL but handled explicitly for clarity

        // 2a. Devices
        diesel::update(assets::table.filter(assets::primary_user_uuid.eq(user_uuid)))
            .set(assets::primary_user_uuid.eq::<Option<Uuid>>(None))
            .execute(conn)?;
        diesel::update(assets::table.filter(assets::created_by.eq(user_uuid)))
            .set(assets::created_by.eq::<Option<Uuid>>(None))
            .execute(conn)?;

        // 2b. Tickets (assignee, created_by, closed_by)
        diesel::update(tickets::table.filter(tickets::assignee_uuid.eq(user_uuid)))
            .set(tickets::assignee_uuid.eq::<Option<Uuid>>(None))
            .execute(conn)?;
        diesel::update(tickets::table.filter(tickets::created_by.eq(user_uuid)))
            .set(tickets::created_by.eq::<Option<Uuid>>(None))
            .execute(conn)?;
        diesel::update(tickets::table.filter(tickets::closed_by.eq(user_uuid)))
            .set(tickets::closed_by.eq::<Option<Uuid>>(None))
            .execute(conn)?;

        // 2c. Projects
        diesel::update(projects::table.filter(projects::created_by.eq(user_uuid)))
            .set(projects::created_by.eq::<Option<Uuid>>(None))
            .execute(conn)?;
        diesel::update(projects::table.filter(projects::owner_uuid.eq(user_uuid)))
            .set(projects::owner_uuid.eq::<Option<Uuid>>(None))
            .execute(conn)?;

        // 2d. Attachments
        diesel::update(attachments::table.filter(attachments::uploaded_by.eq(user_uuid)))
            .set(attachments::uploaded_by.eq::<Option<Uuid>>(None))
            .execute(conn)?;

        // 2e. Linked tickets
        diesel::update(linked_tickets::table.filter(linked_tickets::created_by.eq(user_uuid)))
            .set(linked_tickets::created_by.eq::<Option<Uuid>>(None))
            .execute(conn)?;

        // 2f. Project tickets
        diesel::update(project_tickets::table.filter(project_tickets::created_by.eq(user_uuid)))
            .set(project_tickets::created_by.eq::<Option<Uuid>>(None))
            .execute(conn)?;

        // 2g. Ticket devices
        diesel::update(ticket_assets::table.filter(ticket_assets::created_by.eq(user_uuid)))
            .set(ticket_assets::created_by.eq::<Option<Uuid>>(None))
            .execute(conn)?;

        // 2h. Article contents
        diesel::update(article_contents::table.filter(article_contents::created_by.eq(user_uuid)))
            .set(article_contents::created_by.eq::<Option<Uuid>>(None))
            .execute(conn)?;
        diesel::update(article_contents::table.filter(article_contents::updated_by.eq(user_uuid)))
            .set(article_contents::updated_by.eq::<Option<Uuid>>(None))
            .execute(conn)?;

        // 2i. Sync history
        diesel::update(sync_history::table.filter(sync_history::initiated_by.eq(user_uuid)))
            .set(sync_history::initiated_by.eq::<Option<Uuid>>(None))
            .execute(conn)?;

        // 2j. User auth identities created_by
        diesel::update(
            user_auth_identities::table.filter(user_auth_identities::created_by.eq(user_uuid)),
        )
        .set(user_auth_identities::created_by.eq::<Option<Uuid>>(None))
        .execute(conn)?;

        // 2k. User emails created_by
        diesel::update(user_emails::table.filter(user_emails::created_by.eq(user_uuid)))
            .set(user_emails::created_by.eq::<Option<Uuid>>(None))
            .execute(conn)?;

        // === Phase 3: Delete CASCADE tables explicitly (for clarity) ===
        // These would be deleted automatically by CASCADE, but explicit is clearer

        // 3a. Delete user auth identities
        diesel::delete(
            user_auth_identities::table.filter(user_auth_identities::user_uuid.eq(user_uuid)),
        )
        .execute(conn)?;

        // 3b. Delete user emails
        diesel::delete(user_emails::table.filter(user_emails::user_uuid.eq(user_uuid)))
            .execute(conn)?;

        // === Phase 4: Delete the user ===
        // Capture the row before delete so the sync emit carries
        // the projection (name / role / avatar) for clients that
        // want to display "Foo Bar (deleted)" in historical
        // contexts. After the row is gone we'd only have the uuid.
        let user_row: User = users::table.find(user_uuid).first(conn)?;
        let deleted_count = diesel::delete(users::table.find(user_uuid)).execute(conn)?;

        if deleted_count > 0 {
            emit_user_event(conn, &user_row, SyncOp::Delete, "user.deleted")?;
        }

        Ok(deleted_count)
    })
    .inspect(|count| {
        if *count > 0 {
            if let Some(observer) = observer {
                observer.user_deleted(user_uuid);
            }
        }
    })
}

// Batch get users by UUIDs
pub fn get_users_by_uuids(uuids: &[Uuid], conn: &mut DbConnection) -> Result<Vec<User>, Error> {
    users::table
        .filter(users::uuid.eq_any(uuids))
        .order_by(users::name.asc())
        .load::<User>(conn)
}

/// Batch-fetch users into a `uuid -> User` map, the batched counterpart to
/// per-row `get_user_by_uuid` in list/DTO assembly. Duplicate or missing
/// input uuids are harmless (the DB returns each user once; absent uuids
/// simply aren't in the map).
pub fn get_user_map_by_uuids(
    uuids: &[Uuid],
    conn: &mut DbConnection,
) -> Result<std::collections::HashMap<Uuid, User>, Error> {
    Ok(get_users_by_uuids(uuids, conn)?
        .into_iter()
        .map(|u| (u.uuid, u))
        .collect())
}

/// Like [`get_user_map_by_uuids`] but overlays each user's PER-WORKSPACE persona
/// override (O7) onto `User.name` AND `User.avatar_url`, so callers that render
/// users in the active workspace show the persona name and avatar (e.g.
/// "Warehouse Labourer at Foodcare") rather than the global control-plane
/// record. RLS scopes the override to the active workspace; users without a
/// persona keep their global values. Use this for display surfaces (assignee /
/// author / roster); use the plain map when you need the canonical global
/// record (audit, notifications).
pub fn get_user_map_by_uuids_with_persona(
    uuids: &[Uuid],
    conn: &mut DbConnection,
) -> Result<std::collections::HashMap<Uuid, User>, Error> {
    let mut map = get_user_map_by_uuids(uuids, conn)?;
    let overrides = crate::repository::user_contact::persona_overrides(conn, uuids)?;
    for (uuid, persona) in overrides {
        if let Some(user) = map.get_mut(&uuid) {
            if let Some(name) = persona.display_name {
                user.name = name;
            }
            if let Some(avatar) = persona.avatar_url {
                user.avatar_url = Some(avatar);
                // No per-workspace thumbnail; clear the global one so the UI
                // uses the full override image.
                user.avatar_thumb = None;
            }
        }
    }
    Ok(map)
}

// Count total users in the database (for onboarding check)
pub fn count_users(conn: &mut DbConnection) -> Result<i64, Error> {
    users::table.count().get_result(conn)
}

// sync-audit-only: User MFA mutations — sensitive fields, not in the sync user projection. Coverage lives in security_events / audit_log
/// Update user MFA fields by UUID
pub fn update_user_mfa(
    uuid: &Uuid,
    mfa_update: UserMfaUpdate,
    conn: &mut DbConnection,
) -> Result<User, Error> {
    diesel::update(users::table.filter(users::uuid.eq(uuid)))
        .set(mfa_update)
        .get_result(conn)
}

// sync-audit-only: User MFA mutations — sensitive fields, not in the sync user projection. Coverage lives in security_events / audit_log
/// Wipe MFA enrolment for a user: clear the TOTP secret and backup
/// codes, flip `mfa_enabled` off, and bump `updated_at`. Used by
/// the CLI admin lockout-recovery path — the user re-enrols on
/// their next login. `UserMfaUpdate` uses `Option<T>`, which for a
/// nullable column would be ambiguous between "leave alone" and
/// "set NULL"; we write the clearing SQL directly to avoid the
/// ambiguity.
pub fn clear_user_mfa(conn: &mut DbConnection, user_uuid: &Uuid) -> Result<usize, Error> {
    // Recovery codes have moved to `user_recovery_codes`; caller
    // wipes them via `repository::user_recovery_codes::delete_all_for_user`
    // alongside (or before) this call. The two writes don't need a
    // shared transaction — losing recovery codes after MFA is
    // already off is benign.
    // The CHECK constraint
    // `(mfa_secret IS NULL) = (mfa_secret_kek_id IS NULL)` requires we
    // clear both columns together.
    diesel::update(users::table.filter(users::uuid.eq(user_uuid)))
        .set((
            users::mfa_secret.eq::<Option<Vec<u8>>>(None),
            users::mfa_secret_kek_id.eq::<Option<i16>>(None),
            users::mfa_enabled.eq(false),
            users::updated_at.eq(chrono::Utc::now().naive_utc()),
        ))
        .execute(conn)
}

// sync-audit-only: password_changed_at is a security timestamp on the audited users row (covered by tr_audit_users); no sync aggregate subscribes
/// Stamp `password_changed_at` on the audited `users` row after a
/// password change. Must run inside actor + workspace context (the
/// audit trigger reads `app.workspace_id`); the caller supplies the
/// transaction. Centralizes the identical write in the local, admin,
/// invitation-accept, and password-reset flows.
pub fn set_password_changed_at(
    conn: &mut DbConnection,
    user_uuid: &Uuid,
    at: chrono::NaiveDateTime,
) -> Result<usize, Error> {
    diesel::update(users::table.find(user_uuid))
        .set(users::password_changed_at.eq(at))
        .execute(conn)
}

/// Outcome of [`set_user_roles`].
#[derive(Debug, PartialEq, Eq)]
pub enum SetUserRolesOutcome {
    /// Both the platform role and the workspace membership role were written.
    Applied,
    /// A product-initiated change touching a control-plane-owned staff seat in
    /// hosted; refused, nothing written. Hand off to the control plane.
    ExternallyManaged,
}

// sync-audit-only: platform_role + workspace_members role changes are recorded by tr_audit_users and tr_audit_workspace_members; no sync aggregate subscribes to a role change
/// Rewrite a user's two-axis W2 role: `platform_role` on the audited
/// `users` row and their `workspace_members.role` for `workspace_id`.
/// Both writes must run inside actor + workspace context (the audit
/// trigger reads `app.workspace_id`); the caller supplies the
/// transaction. Replaces the duplicated inline single/bulk set-role
/// SQL in `handlers::users`, same two updates, but scoped by an
/// explicit `workspace_id` argument instead of the `app.workspace_id`
/// GUC read.
pub fn set_user_roles(
    conn: &mut DbConnection,
    workspace_id: i32,
    user_uuid: Uuid,
    platform_role: &str,
    workspace_role: &str,
    authority: crate::repository::workspaces::SeatWriteAuthority,
) -> Result<SetUserRolesOutcome, Error> {
    // Gate BEFORE any write: a product-initiated change touching a
    // control-plane-owned staff seat (current OR new role is staff) in hosted
    // is refused, matching update_membership_role.
    let current =
        crate::repository::workspaces::get_membership_role(conn, workspace_id, user_uuid)?
            .unwrap_or_default();
    if authority.refuses_change(&current, workspace_role) {
        return Ok(SetUserRolesOutcome::ExternallyManaged);
    }
    diesel::update(users::table.find(user_uuid))
        .set((
            users::platform_role.eq(platform_role),
            users::updated_at.eq(chrono::Utc::now().naive_utc()),
        ))
        .execute(conn)?;
    diesel::update(
        workspace_members::table
            .filter(workspace_members::workspace_id.eq(workspace_id))
            .filter(workspace_members::user_uuid.eq(user_uuid)),
    )
    .set(workspace_members::role.eq(workspace_role))
    .execute(conn)?;
    Ok(SetUserRolesOutcome::Applied)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::{setup_test_connection, TestFixtures};

    #[test]
    fn create_and_get_user_by_uuid() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "Alice Test", "technician");

        let fetched = get_user_by_uuid(&user.uuid, &mut conn).unwrap();
        assert_eq!(fetched.name, "Alice Test");
        assert_eq!(
            crate::repository::user_helpers::workspace_role(&mut conn, fetched.uuid),
            Some(crate::models::WorkspaceRole::Agent)
        );
    }

    #[test]
    fn get_user_by_name_test() {
        let mut conn = setup_test_connection();
        TestFixtures::create_user(&mut conn, "Bob Unique", "user");

        let fetched = get_user_by_name("Bob Unique", &mut conn).unwrap();
        assert_eq!(fetched.name, "Bob Unique");
    }

    #[test]
    fn get_users_returns_all() {
        let mut conn = setup_test_connection();
        let u1 = TestFixtures::create_user(&mut conn, "User A", "user");
        let u2 = TestFixtures::create_user(&mut conn, "User B", "admin");

        let all = get_users(&mut conn).unwrap();
        let uuids: Vec<Uuid> = all.iter().map(|u| u.uuid).collect();
        assert!(uuids.contains(&u1.uuid));
        assert!(uuids.contains(&u2.uuid));
    }

    #[test]
    fn count_users_test() {
        let mut conn = setup_test_connection();
        let before = count_users(&mut conn).unwrap();
        TestFixtures::create_user(&mut conn, "Count1", "user");
        TestFixtures::create_user(&mut conn, "Count2", "user");
        let after = count_users(&mut conn).unwrap();
        assert_eq!(after, before + 2);
    }

    #[test]
    fn update_user_test() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "Before", "user");

        let update = UserUpdate {
            name: Some("After".to_string()),
            pronouns: None,
            avatar_url: None,
            banner_url: None,
            avatar_thumb: None,
            microsoft_uuid: None,
            updated_at: None,
        };

        let updated = update_user(&user.uuid, update, &mut conn, None).unwrap();
        assert_eq!(updated.name, "After");
    }

    #[test]
    fn delete_user_test() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "ToDelete", "admin");
        // Create a second admin so delete_user can reassign docs
        TestFixtures::create_user(&mut conn, "OtherAdmin", "admin");
        let _ticket = TestFixtures::create_ticket(&mut conn, "ticket", Some(user.uuid), None);

        let deleted = purge_user(&user.uuid, &mut conn, None).unwrap();
        assert_eq!(deleted, 1);

        let result = get_user_by_uuid(&user.uuid, &mut conn);
        assert!(result.is_err());
    }

    #[test]
    fn get_users_by_uuids_test() {
        let mut conn = setup_test_connection();
        let u1 = TestFixtures::create_user(&mut conn, "Batch1", "user");
        let u2 = TestFixtures::create_user(&mut conn, "Batch2", "user");

        let results = get_users_by_uuids(&[u1.uuid, u2.uuid], &mut conn).unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn soft_delete_then_restore_round_trip() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "SoftRoundTrip", "user");
        assert!(user.deleted_at.is_none());

        let deleted = soft_delete_user(&user.uuid, &mut conn).unwrap();
        assert!(deleted.deleted_at.is_some());
        // Row stays in the table.
        let fetched = get_user_by_uuid(&user.uuid, &mut conn).unwrap();
        assert!(fetched.deleted_at.is_some());

        let restored = restore_user(&user.uuid, &mut conn).unwrap();
        assert!(restored.deleted_at.is_none());
        let fetched_after = get_user_by_uuid(&user.uuid, &mut conn).unwrap();
        assert!(fetched_after.deleted_at.is_none());
    }

    #[test]
    fn deleted_filter_active_excludes_soft_deleted() {
        let mut conn = setup_test_connection();
        let active = TestFixtures::create_user(&mut conn, "FilterActive", "user");
        let removed = TestFixtures::create_user(&mut conn, "FilterRemoved", "user");
        soft_delete_user(&removed.uuid, &mut conn).unwrap();

        let (rows, _total) = get_paginated_users(
            &mut conn,
            1,
            100,
            None,
            None,
            None,
            None,
            None,
            DeletedFilter::Active,
            crate::sync::actor::BOOTSTRAP_WORKSPACE_ID,
        )
        .unwrap();
        let uuids: Vec<Uuid> = rows.iter().map(|u| u.uuid).collect();
        assert!(uuids.contains(&active.uuid));
        assert!(!uuids.contains(&removed.uuid));
    }

    #[test]
    fn deleted_filter_only_returns_soft_deleted() {
        let mut conn = setup_test_connection();
        let active = TestFixtures::create_user(&mut conn, "OnlyActive", "user");
        let removed = TestFixtures::create_user(&mut conn, "OnlyRemoved", "user");
        soft_delete_user(&removed.uuid, &mut conn).unwrap();

        let (rows, _total) = get_paginated_users(
            &mut conn,
            1,
            100,
            None,
            None,
            None,
            None,
            None,
            DeletedFilter::Only,
            crate::sync::actor::BOOTSTRAP_WORKSPACE_ID,
        )
        .unwrap();
        let uuids: Vec<Uuid> = rows.iter().map(|u| u.uuid).collect();
        assert!(!uuids.contains(&active.uuid));
        assert!(uuids.contains(&removed.uuid));
    }

    // The People split: Team = staff (here an agent), Requesters = end-users
    // (a member). Both have platform_role `user`, so this exercises the
    // membership half of requester_sql, the core of the split.
    #[test]
    fn population_filter_splits_team_from_requesters() {
        let mut conn = setup_test_connection();
        let ws = crate::sync::actor::BOOTSTRAP_WORKSPACE_ID;
        let staff = TestFixtures::create_user(&mut conn, "PopStaff", "technician");
        let requester = TestFixtures::create_user(&mut conn, "PopRequester", "user");

        let mut ids = |pop| -> Vec<Uuid> {
            get_paginated_users(
                &mut conn,
                1,
                100,
                None,
                None,
                None,
                None,
                Some(pop),
                DeletedFilter::Active,
                ws,
            )
            .unwrap()
            .0
            .iter()
            .map(|u| u.uuid)
            .collect()
        };

        let team = ids(Population::Team);
        assert!(team.contains(&staff.uuid), "Team includes staff");
        assert!(!team.contains(&requester.uuid), "Team excludes requesters");

        let requesters = ids(Population::Requesters);
        assert!(
            requesters.contains(&requester.uuid),
            "Requesters includes end-users"
        );
        assert!(
            !requesters.contains(&staff.uuid),
            "Requesters excludes staff"
        );
    }

    #[test]
    fn list_users_pending_purge_respects_cutoff() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "PurgeCandidate", "user");
        soft_delete_user(&user.uuid, &mut conn).unwrap();

        // Cutoff in the past: nothing eligible yet.
        let cutoff_before = chrono::Utc::now().naive_utc() - chrono::Duration::days(1);
        let none = list_users_pending_purge(&mut conn, cutoff_before).unwrap();
        assert!(!none.iter().any(|u| u.uuid == user.uuid));

        // Cutoff in the future: the row qualifies (deleted_at < cutoff).
        let cutoff_after = chrono::Utc::now().naive_utc() + chrono::Duration::days(1);
        let pending = list_users_pending_purge(&mut conn, cutoff_after).unwrap();
        assert!(pending.iter().any(|u| u.uuid == user.uuid));
    }

    #[test]
    fn purge_after_soft_delete_removes_row() {
        let mut conn = setup_test_connection();
        // Second admin so purge_user can reassign docs if the test
        // fixture ever adds any. Mirrors the existing delete test.
        TestFixtures::create_user(&mut conn, "PurgeWitness", "admin");
        let user = TestFixtures::create_user(&mut conn, "PurgeMe", "user");

        soft_delete_user(&user.uuid, &mut conn).unwrap();
        let removed = purge_user(&user.uuid, &mut conn, None).unwrap();
        assert_eq!(removed, 1);
        assert!(get_user_by_uuid(&user.uuid, &mut conn).is_err());
    }

    #[test]
    fn purge_grace_window_defaults_to_thirty_days() {
        // The default is sticky: tests run with no NOSDESK_USER_PURGE_GRACE_DAYS
        // set, so the worker uses 30 days. If this drifts, the
        // helpdesk-industry-default story we wrote into the plan breaks.
        // Set explicitly in case some other test polluted the env.
        std::env::remove_var("NOSDESK_USER_PURGE_GRACE_DAYS");
        assert_eq!(purge_grace_window().num_days(), 30);
    }
}
