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
                "role": user.role,
                "pronouns": user.pronouns,
                "avatar_url": user.avatar_url,
                "avatar_thumb": user.avatar_thumb,
                "deleted_at": user.deleted_at,
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

pub fn get_paginated_users(
    conn: &mut DbConnection,
    page: i64,
    page_size: i64,
    sort_field: Option<String>,
    sort_direction: Option<String>,
    search: Option<String>,
    role: Option<String>,
    deleted: DeletedFilter,
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
    let parsed_roles: Vec<UserRole> = match role.as_deref() {
        None => Vec::new(),
        Some("all") => Vec::new(),
        Some(s) => s
            .split(',')
            .map(|piece| piece.trim())
            .filter(|piece| !piece.is_empty())
            .filter_map(|piece| crate::utils::parse_role(piece).ok())
            .collect(),
    };

    // Count query with filters
    let mut count_query = users::table.into_boxed();
    if let Some(ref uuids) = search_uuids {
        count_query = count_query.filter(users::uuid.eq_any(uuids.clone()));
    }
    if !parsed_roles.is_empty() {
        count_query = count_query.filter(users::role.eq_any(parsed_roles.clone()));
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
    if !parsed_roles.is_empty() {
        query = query.filter(users::role.eq_any(parsed_roles.clone()));
    }
    query = match deleted {
        DeletedFilter::Active => query.filter(users::deleted_at.is_null()),
        DeletedFilter::Only => query.filter(users::deleted_at.is_not_null()),
        DeletedFilter::All => query,
    };
    query = match (sort_field.as_deref(), sort_direction.as_deref()) {
        (Some("name"), Some("asc")) => query.order(users::name.asc()),
        (Some("name"), _) => query.order(users::name.desc()),
        (Some("role"), Some("asc")) => query.order(users::role.asc()),
        (Some("role"), _) => query.order(users::role.desc()),
        _ => query.order(users::name.asc()),
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
// cross-codebase audit, see `docs/auth-convergence.md`).
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
            .filter(users::role.eq(crate::models::UserRole::Admin))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::UserRole;
    use crate::test_helpers::{setup_test_connection, TestFixtures};

    #[test]
    fn create_and_get_user_by_uuid() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "Alice Test", UserRole::Technician);

        let fetched = get_user_by_uuid(&user.uuid, &mut conn).unwrap();
        assert_eq!(fetched.name, "Alice Test");
        assert_eq!(fetched.role, UserRole::Technician);
    }

    #[test]
    fn get_user_by_name_test() {
        let mut conn = setup_test_connection();
        TestFixtures::create_user(&mut conn, "Bob Unique", UserRole::User);

        let fetched = get_user_by_name("Bob Unique", &mut conn).unwrap();
        assert_eq!(fetched.name, "Bob Unique");
    }

    #[test]
    fn get_users_returns_all() {
        let mut conn = setup_test_connection();
        let u1 = TestFixtures::create_user(&mut conn, "User A", UserRole::User);
        let u2 = TestFixtures::create_user(&mut conn, "User B", UserRole::Admin);

        let all = get_users(&mut conn).unwrap();
        let uuids: Vec<Uuid> = all.iter().map(|u| u.uuid).collect();
        assert!(uuids.contains(&u1.uuid));
        assert!(uuids.contains(&u2.uuid));
    }

    #[test]
    fn count_users_test() {
        let mut conn = setup_test_connection();
        let before = count_users(&mut conn).unwrap();
        TestFixtures::create_user(&mut conn, "Count1", UserRole::User);
        TestFixtures::create_user(&mut conn, "Count2", UserRole::User);
        let after = count_users(&mut conn).unwrap();
        assert_eq!(after, before + 2);
    }

    #[test]
    fn update_user_test() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "Before", UserRole::User);

        let update = UserUpdate {
            name: Some("After".to_string()),
            role: None,
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
        let user = TestFixtures::create_user(&mut conn, "ToDelete", UserRole::Admin);
        // Create a second admin so delete_user can reassign docs
        TestFixtures::create_user(&mut conn, "OtherAdmin", UserRole::Admin);
        let _ticket = TestFixtures::create_ticket(&mut conn, "ticket", Some(user.uuid), None);

        let deleted = purge_user(&user.uuid, &mut conn, None).unwrap();
        assert_eq!(deleted, 1);

        let result = get_user_by_uuid(&user.uuid, &mut conn);
        assert!(result.is_err());
    }

    #[test]
    fn get_users_by_uuids_test() {
        let mut conn = setup_test_connection();
        let u1 = TestFixtures::create_user(&mut conn, "Batch1", UserRole::User);
        let u2 = TestFixtures::create_user(&mut conn, "Batch2", UserRole::User);

        let results = get_users_by_uuids(&[u1.uuid, u2.uuid], &mut conn).unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn soft_delete_then_restore_round_trip() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "SoftRoundTrip", UserRole::User);
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
        let active = TestFixtures::create_user(&mut conn, "FilterActive", UserRole::User);
        let removed = TestFixtures::create_user(&mut conn, "FilterRemoved", UserRole::User);
        soft_delete_user(&removed.uuid, &mut conn).unwrap();

        let (rows, _total) = get_paginated_users(
            &mut conn,
            1,
            100,
            None,
            None,
            None,
            None,
            DeletedFilter::Active,
        )
        .unwrap();
        let uuids: Vec<Uuid> = rows.iter().map(|u| u.uuid).collect();
        assert!(uuids.contains(&active.uuid));
        assert!(!uuids.contains(&removed.uuid));
    }

    #[test]
    fn deleted_filter_only_returns_soft_deleted() {
        let mut conn = setup_test_connection();
        let active = TestFixtures::create_user(&mut conn, "OnlyActive", UserRole::User);
        let removed = TestFixtures::create_user(&mut conn, "OnlyRemoved", UserRole::User);
        soft_delete_user(&removed.uuid, &mut conn).unwrap();

        let (rows, _total) = get_paginated_users(
            &mut conn,
            1,
            100,
            None,
            None,
            None,
            None,
            DeletedFilter::Only,
        )
        .unwrap();
        let uuids: Vec<Uuid> = rows.iter().map(|u| u.uuid).collect();
        assert!(!uuids.contains(&active.uuid));
        assert!(uuids.contains(&removed.uuid));
    }

    #[test]
    fn list_users_pending_purge_respects_cutoff() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "PurgeCandidate", UserRole::User);
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
        TestFixtures::create_user(&mut conn, "PurgeWitness", UserRole::Admin);
        let user = TestFixtures::create_user(&mut conn, "PurgeMe", UserRole::User);

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
