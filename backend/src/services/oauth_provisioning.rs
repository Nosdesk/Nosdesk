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

use diesel::result::Error as DieselError;
use tracing::{error, warn};

use crate::db::DbConnection;
use crate::models::{NewUserAuthIdentity, NewUserEmail, User, UserRole};
use crate::repository::{user_auth_identities, user_emails, users as users_repo, workspaces};
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
                    if let Err(e) = user_auth_identities::create_identity(new_identity, conn) {
                        warn!(
                            iss = %iss,
                            user_uuid = %user.uuid,
                            error = ?e,
                            "found user by email but failed to attach OIDC identity; \
                             proceeding without identity row"
                        );
                    } else {
                        // Mirror the OIDC-provided email into
                        // user_emails if it isn't already there.
                        // Matches the existing lazy path's
                        // diagnostic-on-error treatment.
                        ensure_email_linked(conn, &user, &iss, &email);
                    }
                    ProjectionOutcome::Existed(user)
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
                    let new_user =
                        NewUserBuilder::local_user(display_name, email.clone(), UserRole::User)
                            .build();
                    let user = users_repo::create_user(new_user, conn)
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
    // ON CONFLICT DO NOTHING in add_membership preserves the
    // existing role; re-projection never silently escalates or
    // downgrades. The handoff doc's "first-write-wins on role"
    // gotcha is enforced here.
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
