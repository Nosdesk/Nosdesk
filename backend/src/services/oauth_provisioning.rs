//! Shared user-provisioning core for OIDC / OAuth flows.
//!
//! Two callers exercise the same code path:
//!
//!  * **Lazy** — `handlers::auth_providers::find_or_create_oauth_user`,
//!    invoked when an OIDC user logs in for the first time. The
//!    provider hands us a user_info JSON; we either link to an
//!    existing local user or mint a fresh one.
//!
//!  * **Eager** — `handlers::internal_workspaces::upsert_projected_user`
//!    (M5 Task 4), invoked by the control plane during workspace
//!    provisioning. The owner's `users` row is created **before**
//!    they ever log in, so the `workspace_members` FK has a target
//!    by the time the provisioning sequence finishes.
//!
//! Both paths share the same resolution rules:
//!
//!  1. Find a `user_auth_identities` row matching `(provider_type,
//!     external_id)` — for OIDC this is `(iss, sub)`, the stable
//!     identity per D7 of the convergence doc.
//!  2. On miss, fall back to looking up by email. If found, attach
//!     a fresh identity row to the existing user. This covers
//!     "operator created the user manually, OIDC now signing in"
//!     and "two providers share the same user" cases without
//!     duplicating the local user row.
//!  3. On both misses, mint a fresh local user, identity, and
//!     primary email row.
//!  4. Either way, ensure the workspace membership row exists. The
//!     repo's `add_membership` uses `ON CONFLICT DO NOTHING` so
//!     re-projecting an existing membership is a no-op — the role
//!     on the existing row is preserved (no silent escalation /
//!     downgrade; see handoff doc Task 4 gotcha).
//!
//! The function returns whether the local user was newly minted
//! (`created = true`) vs found via either lookup (`created = false`)
//! so the eager endpoint can pick its 201/200 response code and
//! the lazy caller can ignore the bool.

use diesel::result::{DatabaseErrorKind, Error as DieselError};
use tracing::error;

use crate::db::DbConnection;
use crate::models::{NewUserAuthIdentity, NewUserEmail, PlatformRole, User, WorkspaceRole};
use crate::repository::{
    user_auth_identities, user_emails, user_helpers, users as users_repo, workspaces,
};
use crate::utils::user::NewUserBuilder;

/// Inputs to [`find_or_create_projected_user`]. Both callers build
/// one of these and hand it off; the function has no side
/// dependencies on the request context.
pub struct ProjectedUserInput {
    /// OIDC `iss` claim. Mapped onto `user_auth_identities.provider_type`.
    pub iss: String,
    /// OIDC `sub` claim — the provider-stable user identifier.
    /// Mapped onto `user_auth_identities.external_id`.
    pub sub: String,
    pub email: String,
    /// Display name. Required for new-user creation; for the
    /// existing-by-identity path we use whatever's already on the
    /// users row (no rename here).
    pub name: Option<String>,
    /// Workspace membership role to grant: `"owner"`, `"admin"`,
    /// or `"member"`. ON CONFLICT DO NOTHING semantics mean
    /// re-projecting an existing membership preserves the prior
    /// role; this value only takes effect on first grant.
    pub role: String,
    /// Workspace to grant membership in. Resolved by the caller
    /// (the eager endpoint looks up by slug; the lazy path threads
    /// it from the request's WorkspaceContext).
    pub workspace_id: i32,
    /// Optional password hash to stash on the identity row. The
    /// lazy OIDC path generates a random one and stores it (the
    /// identity carries it for legacy password-fallback support);
    /// the eager path passes `None` since the eager-projected user
    /// authenticates exclusively via the OIDC flow once they log in.
    pub password_hash: Option<String>,
    /// Optional metadata blob to stash on the identity row. Lazy
    /// path stuffs the provider's user_info JSON; eager passes
    /// `None` (the control plane already knows what it sent).
    pub metadata: Option<serde_json::Value>,
}

/// Outcome of [`find_or_create_projected_user`].
pub enum ProjectionOutcome {
    /// The local user row was minted in this call (returns 201
    /// from the eager handler).
    Created(User),
    /// The local user already existed — either by `(iss, sub)`
    /// identity match or by email fallback. The eager handler
    /// returns 200 in this case.
    Existed(User),
}

impl ProjectionOutcome {
    pub fn into_user(self) -> User {
        match self {
            Self::Created(u) | Self::Existed(u) => u,
        }
    }
    pub fn is_created(&self) -> bool {
        matches!(self, Self::Created(_))
    }
}

/// Resolve a user from an OIDC identity, creating one if needed,
/// and ensure they're a member of the target workspace. See the
/// module docs for the four-step lookup order.
pub fn find_or_create_projected_user(
    conn: &mut DbConnection,
    input: ProjectedUserInput,
) -> Result<ProjectionOutcome, String> {
    let ProjectedUserInput {
        iss,
        sub,
        email,
        name,
        role,
        workspace_id,
        password_hash,
        metadata,
    } = input;

    // --- 1. find by (iss, sub) ---
    let outcome = match user_auth_identities::find_user_by_identity(&iss, &sub, conn) {
        Ok(Some(user_uuid)) => {
            let user = users_repo::find_active_by_uuid(&user_uuid, conn)
                .map_err(|e| format!("identity {iss}/{sub} resolved a user that's gone: {e:?}"))?;
            ProjectionOutcome::Existed(user)
        }
        Ok(None) => {
            // --- 2. fall back by email; attach identity if found ---
            match users_repo::get_user_by_email(&email, conn) {
                Ok(user) => {
                    let new_identity = NewUserAuthIdentity {
                        user_uuid: user.uuid,
                        provider_type: iss.clone(),
                        external_id: sub.clone(),
                        email: Some(email.clone()),
                        metadata: metadata.clone(),
                        password_hash: password_hash.clone(),
                    };
                    // Step 1 found no identity under (iss, sub), so a
                    // UniqueViolation here means a SEPARATE identity row
                    // already owns (iss, sub) and points at a different
                    // user. Silently linking would route the projected
                    // workspace member to user A while OIDC login would
                    // resolve (iss, sub) to user B at line 123, so B
                    // would log in with no membership and access would
                    // be broken with no failure surfaced. Surface as a
                    // hard error instead. The transient case (any other
                    // DieselError) also returns Err so the control
                    // plane retries via the idempotency key rather than
                    // being told `created: false` for a row we did not
                    // actually link.
                    match user_auth_identities::create_identity(new_identity, conn) {
                        Ok(_) => {
                            // Mirror the OIDC-provided email into
                            // user_emails if it isn't already there.
                            ensure_email_linked(conn, &user, &iss, &email);
                            ProjectionOutcome::Existed(user)
                        }
                        Err(DieselError::DatabaseError(DatabaseErrorKind::UniqueViolation, _)) => {
                            return Err(format!(
                                "OIDC identity ({iss}, {sub}) is already attached to a \
                                 different user; refusing to silently link to the \
                                 email-matched user {user_uuid}",
                                user_uuid = user.uuid,
                            ));
                        }
                        Err(e) => {
                            return Err(format!(
                                "attach OIDC identity to email-matched user: {e:?}"
                            ));
                        }
                    }
                }
                Err(_) => {
                    // --- 3. create fresh user + identity + email ---
                    let display_name = name.clone().unwrap_or_else(|| {
                        // Fallback for callers that didn't send a
                        // name. Email local-part is a reasonable
                        // best-guess; the operator can rename
                        // later.
                        email
                            .split('@')
                            .next()
                            .unwrap_or(email.as_str())
                            .to_string()
                    });
                    // A brand-new OIDC user has no platform privileges;
                    // their workspace role comes from the projection's
                    // requested `role`, set explicitly below.
                    let new_user =
                        NewUserBuilder::local_user(display_name, email.clone(), PlatformRole::User)
                            .build();
                    // Mint via the sync-wired helper so the OIDC address
                    // lands as the user's PRIMARY email in user_emails.
                    // get_user_by_email (step 2 above) and the MFA /
                    // password-reset flows resolve only the primary
                    // email, so a user minted without one would miss the
                    // email fallback on the next projection and get a
                    // duplicate row. The low-level users_repo::create_user
                    // writes only the users table and is vestigial for
                    // exactly this reason. Address is provider-verified,
                    // so seed it verified; source records the issuer.
                    //
                    // The membership role is the projection's requested
                    // `role` (e.g. `owner`), passed as an explicit
                    // WorkspaceRole. Deriving it from `user_role` would
                    // wrongly write `member` for an owner-projection,
                    // since `UserRole` has no `Owner`.
                    let (user, _email) = user_helpers::create_user_with_email(
                        new_user,
                        WorkspaceRole::from_db(&role),
                        email.clone(),
                        true,
                        Some(iss.clone()),
                        conn,
                        None,
                    )
                    .map_err(|e| format!("create_user: {e:?}"))?;

                    let new_identity = NewUserAuthIdentity {
                        user_uuid: user.uuid,
                        provider_type: iss.clone(),
                        external_id: sub.clone(),
                        email: Some(email),
                        metadata,
                        password_hash,
                    };
                    if let Err(e) = user_auth_identities::create_identity(new_identity, conn) {
                        return Err(format!("created user but failed to attach identity: {e:?}"));
                    }
                    ProjectionOutcome::Created(user)
                }
            }
        }
        Err(e) => return Err(format!("find_user_by_identity: {e:?}")),
    };

    // --- 4. ensure workspace_members row exists ---
    // For a freshly-created user, create_user_with_email already wrote
    // the membership with the requested role above, so this is an
    // idempotent no-op. For an EXISTING user (matched by identity or
    // email) it grants membership in this workspace if they lacked
    // one. Either way `add_membership` uses ON CONFLICT DO NOTHING, so
    // re-projection never silently escalates or downgrades an existing
    // role: the handoff doc's "first-write-wins on role" gotcha. The
    // sanctioned way to CORRECT a wrong role afterwards is the W3
    // role-change endpoint (`PATCH /api/workspace/members/{uuid}` or the
    // operator console), which updates the row and is audit-logged by the
    // tr_audit_workspace_members trigger; re-projection is not a
    // correction path by design.
    let user_uuid = match &outcome {
        ProjectionOutcome::Created(u) | ProjectionOutcome::Existed(u) => u.uuid,
    };
    workspaces::add_membership(conn, workspace_id, user_uuid, &role)
        .map_err(|e| format!("add workspace membership: {e:?}"))?;

    Ok(outcome)
}

/// Insert a `user_emails` row mirroring the OIDC-provided address
/// when the user doesn't already have it. Best-effort: errors are
/// logged and swallowed because the auth flow is more important
/// than a perfectly-populated email index.
fn ensure_email_linked(conn: &mut DbConnection, user: &User, provider_type: &str, email: &str) {
    use crate::schema::user_emails as ue;
    use diesel::RunQueryDsl;

    match user_emails::find_user_by_any_email(conn, email) {
        Err(DieselError::NotFound) => {
            let row = NewUserEmail {
                user_uuid: user.uuid,
                email: email.to_lowercase(),
                email_type: "work".to_string(),
                is_primary: false,
                is_verified: true,
                source: Some(provider_type.to_string()),
            };
            if let Err(e) = diesel::insert_into(ue::table).values(&row).execute(conn) {
                error!(
                    provider = %provider_type,
                    user_uuid = %user.uuid,
                    error = ?e,
                    "failed to mirror OIDC email into user_emails; user can log in but \
                     email-based flows (MFA, password reset) will not find them",
                );
            }
        }
        Ok(_) => {
            // Already linked, nothing to do.
        }
        Err(e) => {
            error!(
                provider = %provider_type,
                user_uuid = %user.uuid,
                error = ?e,
                "failed to look up OIDC email; skipping mirror insert",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::setup_test_connection;
    use diesel::prelude::*;

    /// Closes the eager-then-lazy half of the (iss, sub) byte-identical
    /// invariant at the service layer: project a user via the eager
    /// endpoint's input, then call the same service with a lazy-style
    /// input (same iss/sub, plus password_hash + metadata as the OIDC
    /// callback would supply). The second call must hit step 1
    /// (find_by_identity) and return `Existed` with the same user row,
    /// not mint a duplicate. Operator misconfiguration of
    /// `auth_providers.provider_type` is NOT covered here (see
    /// `docs/m5-integration-contract.md`).
    #[test]
    fn lazy_login_finds_eagerly_projected_user() {
        let mut conn = setup_test_connection();
        let iss = "https://api.nosdesk.com/";
        let sub = format!("owner-{}", uuid::Uuid::new_v4());
        let email = format!("owner+{}@acme.example", uuid::Uuid::new_v4());

        let eager = ProjectedUserInput {
            iss: iss.to_string(),
            sub: sub.clone(),
            email: email.clone(),
            name: Some("Owner One".to_string()),
            role: "owner".to_string(),
            workspace_id: 1,
            password_hash: None,
            metadata: None,
        };
        let first = find_or_create_projected_user(&mut conn, eager).expect("eager project");
        assert!(first.is_created(), "eager call must mint the user");
        let first_uuid = first.into_user().uuid;

        let lazy = ProjectedUserInput {
            iss: iss.to_string(),
            sub: sub.clone(),
            email: email.clone(),
            name: Some("Owner One renamed by IdP".to_string()),
            role: "owner".to_string(),
            workspace_id: 1,
            password_hash: Some("$2b$12$placeholder".to_string()),
            metadata: Some(serde_json::json!({ "source": "lazy_oidc_callback" })),
        };
        let second = find_or_create_projected_user(&mut conn, lazy).expect("lazy project");
        assert!(
            !second.is_created(),
            "lazy call must resolve the existing user via (iss, sub), not mint a second"
        );
        assert_eq!(
            second.into_user().uuid,
            first_uuid,
            "(iss, sub) lookup must return the same user uuid the eager call minted"
        );
    }

    /// Same shape as above but asserts the trailing slash matters: a
    /// lazy lookup with `iss` minus the trailing slash misses, falls
    /// to the email-fallback branch, and (since this test reuses the
    /// already-attached email) attaches a SECOND identity row to the
    /// same user. The user uuid stays the same because of the email
    /// match, but the duplicate identity row is the symptom an operator
    /// misconfiguring `auth_providers.provider_type` would see. Locks
    /// in the byte-identicality requirement: a separate row, not a
    /// silent reuse, is what diverges on issuer drift.
    #[test]
    fn iss_trailing_slash_drift_produces_separate_identity_row() {
        let mut conn = setup_test_connection();
        let iss_canonical = "https://api.nosdesk.com/";
        let iss_drifted = "https://api.nosdesk.com"; // no trailing slash
        let sub = format!("owner-{}", uuid::Uuid::new_v4());
        let email = format!("owner+{}@acme.example", uuid::Uuid::new_v4());

        let eager = ProjectedUserInput {
            iss: iss_canonical.to_string(),
            sub: sub.clone(),
            email: email.clone(),
            name: Some("Owner Two".to_string()),
            role: "owner".to_string(),
            workspace_id: 1,
            password_hash: None,
            metadata: None,
        };
        let first = find_or_create_projected_user(&mut conn, eager).expect("eager project");
        let first_uuid = first.into_user().uuid;

        let drifted = ProjectedUserInput {
            iss: iss_drifted.to_string(),
            sub: sub.clone(),
            email: email.clone(),
            name: Some("Owner Two".to_string()),
            role: "owner".to_string(),
            workspace_id: 1,
            password_hash: Some("$2b$12$placeholder".to_string()),
            metadata: None,
        };
        let second =
            find_or_create_projected_user(&mut conn, drifted).expect("drifted-iss project");
        assert_eq!(
            second.into_user().uuid,
            first_uuid,
            "email fallback resolves to the same user when the address matches"
        );

        // Two identity rows now: one per iss value. The canonical row
        // is what an OIDC login with byte-identical iss would hit;
        // the drifted row is what a misconfigured `provider_type`
        // would write. Asserting two rows locks in that the drift
        // produced divergent state, not a silent merge.
        use crate::schema::user_auth_identities::dsl as i;
        let count: i64 = i::user_auth_identities
            .filter(i::user_uuid.eq(first_uuid))
            .filter(i::external_id.eq(&sub))
            .count()
            .get_result(&mut conn)
            .expect("count identities");
        assert_eq!(
            count, 2,
            "drifted iss must store a separate identity row, not reuse the canonical one"
        );
    }
}
