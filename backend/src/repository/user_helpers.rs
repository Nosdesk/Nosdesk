use crate::db::DbConnection;
use crate::models::{PlatformRole, User, UserEmail, WorkspaceRole};
use diesel::prelude::*;
use uuid::Uuid;

/// The user's `WorkspaceRole` in the CURRENT workspace — the one
/// `app.workspace_id` is scoped to — or `None` if they have no membership
/// there. `workspace_members` is RLS-isolated by workspace, so a caller
/// running in a workspace context (`TenantConn` / `with_actor_context` / a
/// workspace-pinned raw conn) gets the role in THAT workspace.
///
/// This replaced a hardcoded `workspace_id = 1` read, which returned `None`
/// for every non-bootstrap workspace under hosted multi-tenancy. An
/// unscoped caller now reads nothing rather than the wrong workspace's role
/// — fail-safe (least privilege), not a leak.
pub fn workspace_role(conn: &mut DbConnection, user_uuid: Uuid) -> Option<WorkspaceRole> {
    use crate::schema::workspace_members;
    workspace_members::table
        .filter(workspace_members::user_uuid.eq(user_uuid))
        .select(workspace_members::role)
        .first::<String>(conn)
        .ok()
        .map(|r| WorkspaceRole::from_db(&r))
}

/// Batched `workspace_role` for list/DTO assembly: one query for many users
/// instead of a per-row lookup. Scoped to the current workspace by RLS on
/// `workspace_members`, matching the singular `workspace_role`. Users with
/// no membership row are simply absent from the map.
pub fn workspace_roles_batch(
    user_uuids: &[Uuid],
    conn: &mut DbConnection,
) -> std::collections::HashMap<Uuid, WorkspaceRole> {
    use crate::schema::workspace_members;
    workspace_members::table
        .filter(workspace_members::user_uuid.eq_any(user_uuids))
        .select((workspace_members::user_uuid, workspace_members::role))
        .load::<(Uuid, String)>(conn)
        .unwrap_or_default()
        .into_iter()
        .map(|(uuid, role)| (uuid, WorkspaceRole::from_db(&role)))
        .collect()
}

/// True when `user` is a baseline, unprivileged account: platform
/// role `user` and no workspace role above `member` in the current
/// workspace. Privileged accounts (platform admin / audit reviewer,
/// or workspace agent/admin/owner) return false. Used by the guest
/// auto-provisioning paths so a drive-by submission can never reuse
/// or impersonate a staff account.
fn is_baseline_user(conn: &mut DbConnection, user: &User) -> bool {
    if PlatformRole::from_db(&user.platform_role) != PlatformRole::User {
        return false;
    }
    match workspace_role(conn, user.uuid) {
        Some(role) => !role.meets(WorkspaceRole::Agent),
        None => true,
    }
}

/// True when `user` may handle tickets (be assigned, see all
/// tickets): a platform admin, or a workspace agent/admin/owner in
/// the current workspace. The DB-side mirror of
/// `AuthContext::can_handle_tickets` for when only a `User` row is on
/// hand (assignee validation, SSE fan-out).
pub fn user_can_handle_tickets(conn: &mut DbConnection, user: &User) -> bool {
    PlatformRole::from_db(&user.platform_role).is_platform_admin()
        || workspace_role(conn, user.uuid).is_some_and(|r| r.meets(WorkspaceRole::Agent))
}

/// True when `user` is an administrator: a platform admin, or a
/// workspace admin/owner in the current workspace. Mirrors the old
/// `legacy_role_for_user(...) == "admin"` check used to guard
/// admin-account deletion.
pub fn user_is_admin(conn: &mut DbConnection, user: &User) -> bool {
    PlatformRole::from_db(&user.platform_role).is_platform_admin()
        || workspace_role(conn, user.uuid).is_some_and(|r| r.meets(WorkspaceRole::Admin))
}

/// Observer fired after a user record is successfully committed to the
/// database. Implementors react to user creation (e.g. the search
/// service maintains its index, audit logs append a row). The
/// repository helpers invoke this *after* the surrounding transaction
/// commits, so a rolled-back insert never fires the hook.
///
/// Defined here in the repository layer so the helpers don't have to
/// import any service module — the search service implements the
/// trait on its own type.
pub trait UserCreatedObserver: Send + Sync {
    fn user_created(&self, user: &User, primary_email: &str);
}

/// Get a user's primary email address
/// This is now the canonical way to get a user's email since users table no longer has email field
pub fn get_primary_email(user_uuid: &Uuid, conn: &mut DbConnection) -> Option<String> {
    use crate::schema::user_emails;

    user_emails::table
        .filter(user_emails::user_uuid.eq(user_uuid))
        .filter(user_emails::is_primary.eq(true))
        .select(user_emails::email)
        .first::<String>(conn)
        .ok()
}

/// Get user by email address (looks up in user_emails table)
/// SECURITY: Only matches PRIMARY emails - secondary emails cannot be used for login
/// This follows industry best practices (Google, Microsoft, GitHub, etc.)
/// NOTE: Email comparison is case-insensitive per RFC 5321
pub fn get_user_by_email(
    email: &str,
    conn: &mut DbConnection,
) -> Result<User, diesel::result::Error> {
    use crate::schema::{user_emails, users};

    users::table
        .inner_join(user_emails::table.on(users::uuid.eq(user_emails::user_uuid)))
        .filter(user_emails::email.ilike(email)) // Case-insensitive match
        .filter(user_emails::is_primary.eq(true)) // Only allow login with primary email
        .select(users::all_columns)
        .first::<User>(conn)
}

/// Create a user with their primary email atomically.
///
/// `role` is the legacy `UserRole` projection carried on the
/// emitted `user.created` sync event (the `users.role` column itself
/// was dropped in the W2 cleanup, so it's event-only now).
///
/// `workspace_role` is the per-workspace membership role written to
/// `workspace_members`. It's a separate, explicit parameter rather
/// than being derived from `role` because the two vocabularies don't
/// line up: `WorkspaceRole::Owner` has no `UserRole` equivalent, so
/// deriving the membership role from `UserRole` would make it
/// impossible to provision an owner. Callers that just want the
/// default mapping pass `WorkspaceRole::from_user_role(role)`.
pub fn create_user_with_email(
    new_user: crate::models::NewUser,
    workspace_role: WorkspaceRole,
    email: String,
    email_verified: bool,
    email_source: Option<String>,
    conn: &mut DbConnection,
    observer: Option<&dyn UserCreatedObserver>,
) -> Result<(User, UserEmail), diesel::result::Error> {
    use crate::models::{SyncAggregate, SyncOp};
    use crate::sync::emit::{self, SyncEmit};
    use crate::sync::groups;
    use serde_json::json;

    let result = conn.transaction::<_, diesel::result::Error, _>(|conn| {
        // Create user first
        let user: User = diesel::insert_into(crate::schema::users::table)
            .values(&new_user)
            .get_result(conn)?;

        // Add the workspace_members row that the Item U 403 gate
        // checks. workspace_id comes from the app.workspace_id GUC
        // via the column default - so callers must invoke this
        // function under `with_actor_context` (request handlers do
        // this automatically; bootstrap admin sets the GUC
        // explicitly first). The membership role is the caller-
        // supplied `workspace_role`; `ON CONFLICT DO NOTHING` makes
        // the grant first-write-wins so a re-entrant create never
        // escalates or downgrades an existing membership.
        diesel::sql_query(
            "INSERT INTO workspace_members (user_uuid, role) \
             VALUES ($1, $2) \
             ON CONFLICT (workspace_id, user_uuid) DO NOTHING",
        )
        .bind::<diesel::sql_types::Uuid, _>(user.uuid)
        .bind::<diesel::sql_types::Text, _>(workspace_role.as_str())
        .execute(conn)?;

        // Then create primary email
        let new_email = crate::models::NewUserEmail {
            user_uuid: user.uuid,
            email: email.clone(),
            email_type: "personal".to_string(),
            is_primary: true,
            is_verified: email_verified,
            source: email_source,
        };

        let user_email: UserEmail = diesel::insert_into(crate::schema::user_emails::table)
            .values(&new_email)
            .get_result(conn)?;

        // Emit the sync action carrying the user projection. The
        // primary email is the one we just inserted, so we don't
        // round-trip through `get_primary_email` — use it directly.
        // Same shape that `users::emit_user_event` writes for
        // updates / deletes; keep them in lockstep when fields are
        // added.
        emit::record(
            conn,
            SyncEmit {
                aggregate: SyncAggregate::User,
                aggregate_id: user.uuid.to_string(),
                op: SyncOp::Insert,
                event_type: "user.created",
                data: json!({
                    "uuid": user.uuid,
                    "name": user.name,
                    "email": user_email.email,
                    "platform_role": user.platform_role,
                    "workspace_role": workspace_role.as_str(),
                    "pronouns": user.pronouns,
                    "avatar_url": user.avatar_url,
                    "avatar_thumb": user.avatar_thumb,
                }),
                groups: groups::workspace(),
                causation_id: None,
            },
        )?;

        Ok((user, user_email))
    })?;

    // Fire the observer after the transaction commits so a rolled-
    // back insert never invokes downstream side effects. Optional
    // handle preserves the pure-DB unit tests in this module.
    if let Some(observer) = observer {
        observer.user_created(&result.0, &result.1.email);
    }

    Ok(result)
}

/// Source tag written to `user_emails.source` when an account is created by
/// a public guest-ticket submission. Used to distinguish "I'm a drive-by
/// reporter" accounts from real registered users.
pub const GUEST_EMAIL_SOURCE: &str = "guest_submission";

/// Outcome of [`find_or_create_guest_user`]. Separating "new" vs "reused"
/// vs "claimed by a real account" lets the caller decide whether to send a
/// fresh invitation email, skip it, or reject the submission entirely.
pub enum GuestUserResult {
    /// A new guest-origin account was just provisioned. Callers that send
    /// invitation/confirmation emails should only send on this variant so
    /// the same email isn't re-sent on every subsequent submission.
    Created(User),
    /// An existing guest-origin account was reused (same unverified email
    /// from a previous submission). No fresh invitation is needed — the
    /// original invitation (if any) is still valid or already used.
    Existing(User),
    /// Email is already registered to a verified or privileged account.
    /// The caller should reject the submission and ask the user to sign in
    /// rather than attaching the ticket to someone else's account.
    EmailClaimed,
}

/// Atomically find-or-create a requester account for a public guest ticket
/// submission.
///
/// **Reuse rule:** only UNVERIFIED, BASELINE end-user accounts (platform
/// `User`, workspace role below Agent) are reused. That covers anonymous guest
/// stubs as well as pre-created baseline members (the internal-IT pre-loaded
/// employee).
/// Any other match (verified email, admin, agent, audit reviewer, OAuth-linked)
/// returns [`GuestUserResult::EmailClaimed`].
///
/// **Concurrency:** the lookup and insert run inside a single DB transaction.
/// If a racing insert causes a unique-violation, the transaction retries the
/// lookup so the second caller gets the row the first caller just wrote.
pub fn find_or_create_guest_user(
    email: &str,
    name: &str,
    conn: &mut DbConnection,
    observer: Option<&dyn UserCreatedObserver>,
) -> Result<GuestUserResult, diesel::result::Error> {
    use diesel::result::{DatabaseErrorKind, Error as DieselError};

    conn.transaction::<_, DieselError, _>(|conn| {
        // 1. Look up existing user.
        if let Some(existing) = lookup_for_guest(email, conn)? {
            return Ok(existing);
        }

        // 2. Create a fresh guest account. `create_user_with_email`
        //    fires the observer for us, so reusing it here means the
        //    inbound-email auto-provisioning path (channels pipeline)
        //    notifies the search index without find_or_create_guest_user
        //    needing its own observer call.
        let new_user = crate::models::NewUser {
            uuid: Uuid::now_v7(),
            name: name.trim().to_string(),
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

        match create_user_with_email(
            new_user,
            WorkspaceRole::Member,
            email.to_string(),
            false,
            Some(GUEST_EMAIL_SOURCE.to_string()),
            conn,
            observer,
        ) {
            Ok((user, _)) => Ok(GuestUserResult::Created(user)),
            // Race: a concurrent request created the row between our lookup
            // and our insert. Look it up again under the same transaction.
            // A racing winner's row is an "existing" guest from our
            // perspective even if it was created moments ago — it means
            // the winner already triggered (or will trigger) the invitation
            // email, and we must not send a second one.
            Err(DieselError::DatabaseError(DatabaseErrorKind::UniqueViolation, _)) => {
                match lookup_for_guest(email, conn)? {
                    Some(result) => Ok(result),
                    // Extremely unlikely: unique violation but row vanished.
                    // Treat as claimed rather than loop forever.
                    None => Ok(GuestUserResult::EmailClaimed),
                }
            }
            Err(e) => Err(e),
        }
    })
}

/// Return the verified / privileged Nosdesk user that owns the given
/// email (case-insensitive match against ANY of the user's verified
/// emails — primary or secondary), if any.
///
/// "Verified or privileged" means either: the email row itself is
/// marked `is_verified = true`, OR the user holds a role above
/// [`"user"`] (technician / admin) on any of their emails.
///
/// Checking secondaries matters for the channel-pipeline
/// impersonation guard: a tech with `tech@yourco.com` primary and
/// `tech.alias@yourco.com` verified secondary should trigger the
/// tech-forward branch from either sending address, not just the
/// primary. A non-verified baseline-user row — typical of the guest-
/// submission auto-provisioning path — does NOT match.
pub fn find_verified_user_by_email(
    email: &str,
    conn: &mut DbConnection,
) -> Result<Option<User>, diesel::result::Error> {
    use crate::schema::{user_emails, users};

    let row: Option<(User, bool)> = users::table
        .inner_join(user_emails::table.on(users::uuid.eq(user_emails::user_uuid)))
        .filter(user_emails::email.ilike(email))
        .select((users::all_columns, user_emails::is_verified))
        .first::<(User, bool)>(conn)
        .optional()?;

    let Some((user, is_verified)) = row else {
        return Ok(None);
    };
    if is_verified {
        return Ok(Some(user));
    }
    // A guest auto-provisioned user lands as a baseline member with
    // platform_role = 'user'. Anything privileged (platform admin /
    // audit reviewer, or workspace agent+) must not be impersonated
    // via the guest path.
    if !is_baseline_user(conn, &user) {
        Ok(Some(user))
    } else {
        Ok(None)
    }
}

/// Internal helper: load the user by email and classify them as reusable or
/// claimed based on email-source + verification state.
fn lookup_for_guest(
    email: &str,
    conn: &mut DbConnection,
) -> Result<Option<GuestUserResult>, diesel::result::Error> {
    use crate::schema::{user_emails, users};

    // Fetch the user and their primary email row together so we can classify.
    let row: Option<(User, bool)> = users::table
        .inner_join(user_emails::table.on(users::uuid.eq(user_emails::user_uuid)))
        .filter(user_emails::email.ilike(email))
        .filter(user_emails::is_primary.eq(true))
        .select((users::all_columns, user_emails::is_verified))
        .first::<(User, bool)>(conn)
        .optional()?;

    let Some((user, is_verified)) = row else {
        return Ok(None);
    };

    // Reuse an existing account as the inbound/guest requester only when it is an
    // UNVERIFIED, BASELINE end-user (platform `User`, workspace role below Agent).
    // This covers anonymous guest stubs AND pre-created baseline members (the
    // internal-IT "pre-loaded employee" case, workstream F), where the email's
    // `source` is admin/import rather than a guest submission. The source no
    // longer gates reuse; the two guards below are the impersonation envelope:
    //   - `!is_verified`: never reuse an account that has proven email ownership.
    //     A verified user should sign in rather than have a spoofable `From`
    //     attributed to them.
    //   - `is_baseline_user`: never reuse an agent / admin / audit reviewer.
    // So a spoofed `From` can at most open a ticket as an unverified end-user,
    // the same trust the guest channel already extends to any sender address.
    let reusable = !is_verified && is_baseline_user(conn, &user);

    Ok(Some(if reusable {
        GuestUserResult::Existing(user)
    } else {
        GuestUserResult::EmailClaimed
    }))
}

/// Helper to get user with their primary email + preferences for
/// API responses. Flattens `user_preferences` columns back into
/// the response shape so the frontend doesn't need to know the
/// fields moved tables.
///
/// Falls back to `None` for preference fields if the row is
/// missing (shouldn't happen — the `trg_users_auto_create_preferences`
/// trigger ensures one exists per user — but the lookup is
/// best-effort so a corrupt DB doesn't 500 the whole `/auth/me`
/// flow).
pub fn get_user_with_primary_email(
    user: crate::models::User,
    conn: &mut DbConnection,
) -> crate::models::UserResponse {
    let primary_email = get_primary_email(&user.uuid, conn);
    let prefs = crate::repository::user_preferences::get(conn, user.uuid).ok();
    let workspace_role = workspace_role(conn, user.uuid);
    // O7: render the per-workspace persona name when set (RLS-scoped to the
    // active workspace), else the global control-plane name.
    let display_name = crate::repository::user_contact::display_name_overrides(conn, &[user.uuid])
        .ok()
        .and_then(|mut m| m.remove(&user.uuid))
        .unwrap_or(user.name);

    // Per-workspace avatar override, else the global (control-plane) avatar.
    // When overridden there is no per-workspace thumbnail, so clear the thumb
    // and let the UI fall back to the full avatar_url.
    let avatar_override = crate::repository::user_contact::avatar_url_overrides(conn, &[user.uuid])
        .ok()
        .and_then(|mut m| m.remove(&user.uuid));
    let (avatar_url, avatar_thumb) = match avatar_override {
        Some(url) => (Some(url), None),
        None => (user.avatar_url, user.avatar_thumb),
    };

    crate::models::UserResponse {
        uuid: user.uuid,
        name: display_name,
        email: primary_email,
        platform_role: PlatformRole::from_db(&user.platform_role),
        workspace_role,
        pronouns: user.pronouns,
        avatar_url,
        banner_url: user.banner_url,
        avatar_thumb,
        theme: prefs.as_ref().and_then(|p| p.theme.clone()),
        microsoft_uuid: user.microsoft_uuid,
        created_at: user.created_at,
        updated_at: user.updated_at,
        open_ticket_count: None,
        device_count: None,
        dashboard_layout: prefs.as_ref().and_then(|p| p.dashboard_layout.clone()),
        signature: prefs.as_ref().and_then(|p| p.signature.clone()),
        locale: prefs.as_ref().and_then(|p| p.locale.clone()),
        timezone: prefs.as_ref().and_then(|p| p.timezone.clone()),
        effective_locale: None,
        effective_timezone: None,
    }
}

/// Batch get primary emails for multiple users efficiently
/// Returns a HashMap of user_uuid -> email
pub fn get_primary_emails_batch(
    user_uuids: &[Uuid],
    conn: &mut DbConnection,
) -> std::collections::HashMap<Uuid, String> {
    use crate::schema::user_emails;

    let emails: Vec<(Uuid, String)> = user_emails::table
        .filter(user_emails::user_uuid.eq_any(user_uuids))
        .filter(user_emails::is_primary.eq(true))
        .select((user_emails::user_uuid, user_emails::email))
        .load::<(Uuid, String)>(conn)
        .unwrap_or_default();

    emails.into_iter().collect()
}

/// Helper to convert multiple users to UserResponses with their
/// emails AND preferences. Batches both queries so a 100-row
/// table load fires 3 SELECTs (users + emails + prefs) rather
/// than 1 + 2N.
pub fn get_users_with_primary_emails(
    users: Vec<crate::models::User>,
    conn: &mut DbConnection,
    workspace_id: i32,
) -> Vec<crate::models::UserResponse> {
    let user_uuids: Vec<Uuid> = users.iter().map(|u| u.uuid).collect();

    let email_map = get_primary_emails_batch(&user_uuids, conn);

    // Batch-fetch preferences keyed by user_uuid. Missing rows
    // (shouldn't happen given the auto-create trigger) fall
    // through to None on every preference field.
    let prefs_map: std::collections::HashMap<Uuid, crate::models::UserPreferences> =
        crate::repository::user_preferences::get_many(conn, &user_uuids)
            .unwrap_or_default()
            .into_iter()
            .map(|p| (p.user_uuid, p))
            .collect();

    // Batch the workspace_members role lookup so the per-row
    // legacy-role derivation doesn't do N+1 queries. The displayed role
    // is the caller's role in the request's workspace (passed in), so the
    // list shows correct per-workspace roles under hosted multi-tenancy.
    let workspace_role_map: std::collections::HashMap<Uuid, String> = {
        use crate::schema::workspace_members;
        workspace_members::table
            .filter(workspace_members::workspace_id.eq(workspace_id))
            .filter(workspace_members::user_uuid.eq_any(&user_uuids))
            .select((workspace_members::user_uuid, workspace_members::role))
            .load::<(Uuid, String)>(conn)
            .unwrap_or_default()
            .into_iter()
            .collect()
    };

    // O7: per-workspace persona overrides (RLS-scoped), applied over the global
    // name so rosters/pickers show the workspace persona when set.
    let persona_map = crate::repository::user_contact::display_name_overrides(conn, &user_uuids)
        .unwrap_or_default();

    users
        .into_iter()
        .map(|user| {
            let email = email_map.get(&user.uuid).cloned();
            let prefs = prefs_map.get(&user.uuid);
            let workspace_role = workspace_role_map
                .get(&user.uuid)
                .map(|r| WorkspaceRole::from_db(r));
            crate::models::UserResponse {
                uuid: user.uuid,
                name: persona_map.get(&user.uuid).cloned().unwrap_or(user.name),
                email,
                platform_role: PlatformRole::from_db(&user.platform_role),
                workspace_role,
                pronouns: user.pronouns,
                avatar_url: user.avatar_url,
                banner_url: user.banner_url,
                avatar_thumb: user.avatar_thumb,
                theme: prefs.and_then(|p| p.theme.clone()),
                microsoft_uuid: user.microsoft_uuid,
                created_at: user.created_at,
                updated_at: user.updated_at,
                open_ticket_count: None,
                device_count: None,
                dashboard_layout: prefs.and_then(|p| p.dashboard_layout.clone()),
                signature: prefs.and_then(|p| p.signature.clone()),
                locale: prefs.and_then(|p| p.locale.clone()),
                timezone: prefs.and_then(|p| p.timezone.clone()),
                effective_locale: None,
                effective_timezone: None,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::{setup_test_connection, TestFixtures};

    #[test]
    fn get_primary_email_returns_primary() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "emailuser", "user");
        TestFixtures::create_user_email(&mut conn, user.uuid, "primary@test.com", true);
        TestFixtures::create_user_email(&mut conn, user.uuid, "secondary@test.com", false);

        let email = get_primary_email(&user.uuid, &mut conn);
        assert_eq!(email, Some("primary@test.com".to_string()));
    }

    #[test]
    fn get_primary_email_returns_none_when_missing() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "noemail", "user");

        assert_eq!(get_primary_email(&user.uuid, &mut conn), None);
    }

    #[test]
    fn get_user_by_email_case_insensitive() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "ciuser", "user");
        TestFixtures::create_user_email(&mut conn, user.uuid, "alice@example.com", true);

        let found = get_user_by_email("ALICE@EXAMPLE.COM", &mut conn).unwrap();
        assert_eq!(found.uuid, user.uuid);
    }

    #[test]
    fn get_user_by_email_only_matches_primary() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "prionly", "user");
        TestFixtures::create_user_email(&mut conn, user.uuid, "real@test.com", true);
        TestFixtures::create_user_email(&mut conn, user.uuid, "secondary@test.com", false);

        assert!(get_user_by_email("secondary@test.com", &mut conn).is_err());
        assert!(get_user_by_email("real@test.com", &mut conn).is_ok());
    }

    #[test]
    fn create_user_with_email_atomic() {
        let mut conn = setup_test_connection();
        let new_user = crate::models::NewUser {
            uuid: Uuid::new_v4(),
            name: "Atomic".into(),
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

        let (user, email_record) = create_user_with_email(
            new_user,
            WorkspaceRole::Member,
            "atomic@test.com".into(),
            true,
            None,
            &mut conn,
            None,
        )
        .unwrap();

        assert_eq!(user.name, "Atomic");
        assert_eq!(email_record.email, "atomic@test.com");
        assert!(email_record.is_primary);
        assert!(email_record.is_verified);
    }

    #[test]
    fn batch_primary_emails() {
        let mut conn = setup_test_connection();
        let u1 = TestFixtures::create_user(&mut conn, "batch1", "user");
        let u2 = TestFixtures::create_user(&mut conn, "batch2", "user");
        TestFixtures::create_user_email(&mut conn, u1.uuid, "b1@test.com", true);
        TestFixtures::create_user_email(&mut conn, u2.uuid, "b2@test.com", true);

        let map = get_primary_emails_batch(&[u1.uuid, u2.uuid], &mut conn);
        assert_eq!(map.get(&u1.uuid), Some(&"b1@test.com".to_string()));
        assert_eq!(map.get(&u2.uuid), Some(&"b2@test.com".to_string()));
    }

    // ---- find_or_create_guest_user tests ----

    /// Insert a user_emails row with a specific `is_verified` and `source`.
    /// The `TestFixtures::create_user_email` helper defaults to
    /// `is_verified = true, source = None`, which is the wrong shape for
    /// guest-origin fixtures.
    fn insert_email(
        conn: &mut DbConnection,
        user_uuid: Uuid,
        email: &str,
        is_verified: bool,
        source: Option<&str>,
    ) {
        use crate::schema::user_emails;
        let new_email = crate::models::NewUserEmail {
            user_uuid,
            email: email.to_string(),
            email_type: "personal".into(),
            is_primary: true,
            is_verified,
            source: source.map(|s| s.to_string()),
        };
        diesel::insert_into(user_emails::table)
            .values(&new_email)
            .execute(conn)
            .expect("insert email");
    }

    #[test]
    fn find_or_create_guest_user_creates_when_email_is_new() {
        let mut conn = setup_test_connection();
        let result = find_or_create_guest_user("fresh@example.com", "Fresh User", &mut conn, None)
            .expect("should succeed");

        match result {
            GuestUserResult::Created(user) => {
                assert_eq!(user.name, "Fresh User");
                assert_eq!(
                    workspace_role(&mut conn, user.uuid),
                    Some(WorkspaceRole::Member)
                );
                // The matching email row should be unverified and tagged
                // with the guest source so the lookup classifies it as reusable.
                let email = get_user_by_email("fresh@example.com", &mut conn).unwrap();
                assert_eq!(email.uuid, user.uuid);
            }
            other => panic!("expected Created, got {:?}", std::mem::discriminant(&other)),
        }
    }

    #[test]
    fn find_or_create_guest_user_reuses_unverified_guest_origin_account() {
        let mut conn = setup_test_connection();
        let existing = TestFixtures::create_user(&mut conn, "Existing Guest", "user");
        insert_email(
            &mut conn,
            existing.uuid,
            "returning@example.com",
            false,
            Some(GUEST_EMAIL_SOURCE),
        );

        let result = find_or_create_guest_user("returning@example.com", "Anyone", &mut conn, None)
            .expect("should succeed");

        match result {
            GuestUserResult::Existing(user) => assert_eq!(user.uuid, existing.uuid),
            _ => panic!("expected Existing"),
        }
    }

    #[test]
    fn find_or_create_guest_user_rejects_verified_email() {
        let mut conn = setup_test_connection();
        let verified = TestFixtures::create_user(&mut conn, "Verified User", "user");
        insert_email(
            &mut conn,
            verified.uuid,
            "verified@example.com",
            true,
            Some(GUEST_EMAIL_SOURCE),
        );

        let result = find_or_create_guest_user("verified@example.com", "Attacker", &mut conn, None)
            .expect("should succeed");

        assert!(matches!(result, GuestUserResult::EmailClaimed));
    }

    #[test]
    fn find_or_create_guest_user_rejects_privileged_role_even_if_unverified() {
        // Paranoid safety net: an unverified *admin* email must never be
        // reusable by a guest submission.
        let mut conn = setup_test_connection();
        let admin = TestFixtures::create_user(&mut conn, "Admin", "admin");
        insert_email(
            &mut conn,
            admin.uuid,
            "admin@example.com",
            false,
            Some(GUEST_EMAIL_SOURCE),
        );

        let result = find_or_create_guest_user("admin@example.com", "Attacker", &mut conn, None)
            .expect("should succeed");

        assert!(matches!(result, GuestUserResult::EmailClaimed));
    }

    #[test]
    fn find_or_create_guest_user_reuses_unverified_baseline_member() {
        // Workstream F: an unverified, baseline (end-user / Member) account is
        // reusable as the inbound/guest requester regardless of how its email
        // was created — a pre-loaded internal-IT employee (source = admin /
        // import) or an invitation that was never accepted. Previously these
        // were dropped as EmailClaimed, silently losing the ticket. The
        // `!is_verified` + `is_baseline_user` guards (see the other tests) keep
        // verified and privileged accounts out.
        let mut conn = setup_test_connection();
        let member = TestFixtures::create_user(&mut conn, "Pre-loaded Employee", "user");
        insert_email(
            &mut conn,
            member.uuid,
            "employee@example.com",
            false,
            Some("admin_invitation"),
        );

        let result = find_or_create_guest_user("employee@example.com", "Someone", &mut conn, None)
            .expect("should succeed");

        match result {
            GuestUserResult::Existing(user) => assert_eq!(user.uuid, member.uuid),
            other => panic!(
                "expected Existing (reused baseline member), got {:?}",
                std::mem::discriminant(&other)
            ),
        }
    }

    #[test]
    fn find_or_create_guest_user_is_case_insensitive_on_lookup() {
        let mut conn = setup_test_connection();
        // First call creates.
        let r1 = find_or_create_guest_user("Alice@Example.com", "Alice", &mut conn, None).unwrap();
        let created_uuid = match r1 {
            GuestUserResult::Created(u) => u.uuid,
            _ => panic!("expected Created"),
        };

        // Second call with different casing hits the same account.
        let r2 =
            find_or_create_guest_user("ALICE@example.COM", "Alice Again", &mut conn, None).unwrap();
        match r2 {
            GuestUserResult::Existing(u) => assert_eq!(u.uuid, created_uuid),
            _ => panic!("expected Existing on repeat submission"),
        }
    }
}
