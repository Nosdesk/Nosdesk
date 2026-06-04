//! Shared initial-admin creation flow.
//!
//! Both the web `setup_initial_admin` handler and the
//! `nosdesk-cli admin create` subcommand call this. Centralising
//! the transaction here means the AUD-005 advisory lock and the
//! `count(users) > 0` short-circuit can't drift between the two
//! call sites; whatever invariant one path enforces, the other
//! does automatically.
//!
//! The function does NOT consume the bootstrap token file — the
//! web handler does that as a belt-and-braces step after a
//! successful response (so a transaction-rollback path can't
//! invalidate the token), and the CLI doesn't need to because
//! shell access already implies file access.
//!
//! Search indexing remains the caller's responsibility: the web
//! handler hits the search service it already has on hand (post-
//! commit, so a rolled-back insert never orphans a tantivy doc),
//! and the CLI skips it (it runs before the server is up, so the
//! first server start picks up the new row via its normal startup
//! paths). Default-category seeding, by contrast, happens inside
//! this transaction so it inherits the bootstrap actor context (the
//! `ticket_categories` audit trigger needs `app.workspace_id` set)
//! and rolls back atomically with the admin if anything fails.

use diesel::prelude::*;
use diesel::sql_query;
use serde_json::json;
use thiserror::Error;
use uuid::Uuid;

use crate::db::DbConnection;
use crate::models::{User, UserEmail};
use crate::sync::actor::{ActorContext, BOOTSTRAP_WORKSPACE_ID};
use crate::sync::session::with_actor_context;

#[derive(Debug, Error)]
pub enum AdminSetupError {
    #[error("setup has already been completed; users exist in the system")]
    AlreadyComplete,
    #[error("email address already in use")]
    DuplicateEmail,
    #[error(transparent)]
    Db(#[from] diesel::result::Error),
}

/// Result of the env-var pre-seed path. Kept separate from
/// `AdminSetupError` because misconfigured env vars are a
/// startup-config problem (refuse to boot) whereas a downstream
/// DB error during seeding is a transient failure (worth a warn
/// but boot should continue).
#[derive(Debug, Error)]
pub enum EnvSeedError {
    #[error("INITIAL_ADMIN env var misconfigured: {0}")]
    Misconfigured(&'static str),
    #[error("failed to insert seeded admin: {0}")]
    AdminSetup(#[from] AdminSetupError),
}

/// Parameters for creating the initial admin. The caller is
/// responsible for validation (length, format, character set);
/// this function trusts what it's given. Email normalisation
/// happens here so both call sites agree on canonicalisation.
pub struct InitialAdminInput<'a> {
    pub name: &'a str,
    pub email: &'a str,
    pub password_hash: &'a str,
}

/// Holds the advisory lock for the lifetime of the transaction
/// so a concurrent `setup_initial_admin` can't slip in between
/// the count check and the inserts. Same arbitrary key the web
/// handler used pre-extraction (`0x4E4F44535F535450` =
/// "NODS_STP" in hex), kept identical to avoid invalidating
/// the existing AUD-005 invariant.
const SETUP_ADVISORY_LOCK_KEY: i64 = 0x4E4F44535F535450;

/// Create the initial admin user, the primary email row, and
/// the local auth identity in a single transaction. Returns the
/// inserted `User` and its primary `UserEmail` so callers can
/// use them without re-querying.
pub fn create_initial_admin(
    conn: &mut DbConnection,
    input: InitialAdminInput<'_>,
) -> Result<(User, UserEmail), AdminSetupError> {
    let (normalized_name, normalized_email) =
        crate::utils::normalization::normalize_user_data(input.name, input.email);
    let (new_user, primary_email) =
        crate::utils::NewUserBuilder::admin_user(normalized_name, normalized_email.clone())
            .build_with_email();

    // Bootstrap actor pins `app.workspace_id` to the bootstrap
    // workspace for the whole transaction, so the audited `users`
    // insert (and the folded category seed) satisfy the audit
    // trigger's NOT NULL workspace_id. `with_actor_context` opens the
    // transaction; the advisory lock + count short-circuit run inside
    // it exactly as before.
    let actor = ActorContext::bootstrap("admin_setup");
    with_actor_context(conn, &actor, |c| {
        sql_query(format!(
            "SELECT pg_advisory_xact_lock({SETUP_ADVISORY_LOCK_KEY})"
        ))
        .execute(c)?;

        if crate::repository::count_users(c)? > 0 {
            return Err(AdminSetupError::AlreadyComplete);
        }

        let user: User = diesel::insert_into(crate::schema::users::table)
            .values(&new_user)
            .get_result(c)?;

        // Item U: add bootstrap admin to the bootstrap workspace so
        // the 403 gate in cookie_auth_middleware finds a membership
        // row on first login. Workspace pinned explicitly because
        // this is the bootstrap path (no request context to drive the
        // GUC-backed column default).
        crate::repository::workspaces::add_membership(
            c,
            BOOTSTRAP_WORKSPACE_ID,
            user.uuid,
            "admin",
        )?;

        let user_email: UserEmail = diesel::insert_into(crate::schema::user_emails::table)
            .values(&crate::models::NewUserEmail {
                user_uuid: user.uuid,
                email: primary_email.clone(),
                email_type: "personal".to_string(),
                is_primary: true,
                is_verified: true,
                source: Some("manual".to_string()),
            })
            .get_result(c)?;

        #[derive(diesel::Insertable)]
        #[diesel(table_name = crate::schema::user_auth_identities)]
        struct NewLocalAuthIdentity<'a> {
            user_uuid: Uuid,
            provider_type: &'a str,
            external_id: &'a str,
            email: Option<&'a str>,
            password_hash: Option<&'a str>,
        }
        diesel::insert_into(crate::schema::user_auth_identities::table)
            .values(&NewLocalAuthIdentity {
                user_uuid: user.uuid,
                provider_type: "local",
                external_id: &normalized_email,
                email: Some(&normalized_email),
                password_hash: Some(input.password_hash),
            })
            .execute(c)?;

        crate::sync::emit::record(
            c,
            crate::sync::emit::SyncEmit {
                aggregate: crate::models::SyncAggregate::User,
                aggregate_id: user.uuid.to_string(),
                op: crate::models::SyncOp::Insert,
                event_type: "user.created",
                data: json!({
                    "uuid": user.uuid,
                    "name": user.name,
                    "email": user_email.email,
                    // Bootstrap admin: platform super-user with an admin
                    // membership in the bootstrap workspace (seeded
                    // below via add_membership).
                    "platform_role": "platform_admin",
                    "workspace_role": "admin",
                    "pronouns": user.pronouns,
                    "avatar_url": user.avatar_url,
                    "avatar_thumb": user.avatar_thumb,
                }),
                groups: crate::sync::groups::workspace(),
                causation_id: None,
            },
        )?;

        // Seed default ticket categories in the same transaction so
        // they inherit the bootstrap actor context and roll back with
        // the admin on failure. Idempotent: a no-op when categories
        // already exist (e.g. CLI seeded, then web setup re-runs).
        crate::repository::categories::seed_defaults_if_empty(c, Some(user.uuid))?;

        Ok((user, user_email))
    })
    .map_err(|e| match e {
        AdminSetupError::Db(db_err) => {
            let s = format!("{db_err:?}");
            if s.contains("duplicate") || s.contains("unique") {
                AdminSetupError::DuplicateEmail
            } else {
                AdminSetupError::Db(db_err)
            }
        }
        other => other,
    })
}

/// Pre-seed the initial admin from `INITIAL_ADMIN_*` env vars
/// (the Phase 3 GitOps / declarative-deploy path). Called once
/// at server boot before the bootstrap-token logic runs; if it
/// succeeds, the token machinery sees an existing user and goes
/// inert on its own.
///
/// Returns:
///   - `Ok(true)`  → an admin was created from env config
///   - `Ok(false)` → env vars not set, or users already exist
///   - `Err(_)`    → env vars set but unusable (boot should
///     surface this, then either refuse to start or warn and
///     fall through to the URL flow; main.rs picks the policy)
///
/// Required env vars:
///   - `INITIAL_ADMIN_EMAIL` — plaintext email address
///   - `INITIAL_ADMIN_PASSWORD_HASH` — bcrypt hash string
///     (starts with `$2a$` / `$2b$` / `$2y$`). Plaintext is
///     explicitly refused — the value would otherwise sit in
///     env files, container metadata, and process listings.
///     Use `nosdesk-cli secrets bcrypt-hash` to generate.
///
/// Optional:
///   - `INITIAL_ADMIN_NAME` — display name. Defaults to the
///     email's local-part when unset (operator can change it
///     from the UI after login).
pub fn seed_from_env(conn: &mut DbConnection) -> Result<bool, EnvSeedError> {
    let email = match std::env::var("INITIAL_ADMIN_EMAIL") {
        Ok(v) if !v.trim().is_empty() => v.trim().to_string(),
        _ => return Ok(false),
    };
    let password_hash = std::env::var("INITIAL_ADMIN_PASSWORD_HASH")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .ok_or(EnvSeedError::Misconfigured(
            "INITIAL_ADMIN_EMAIL is set but INITIAL_ADMIN_PASSWORD_HASH is not; \
             refusing to seed admin without a password",
        ))?;

    if !looks_like_bcrypt_hash(&password_hash) {
        return Err(EnvSeedError::Misconfigured(
            "INITIAL_ADMIN_PASSWORD_HASH must be a bcrypt hash starting with \
             $2a$, $2b$, or $2y$. Generate one with `nosdesk-cli secrets bcrypt-hash`.",
        ));
    }

    let name = std::env::var("INITIAL_ADMIN_NAME")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            email
                .split('@')
                .next()
                .filter(|s| !s.is_empty())
                .unwrap_or("admin")
                .to_string()
        });

    match create_initial_admin(
        conn,
        InitialAdminInput {
            name: &name,
            email: &email,
            password_hash: password_hash.trim(),
        },
    ) {
        Ok(_) => {
            tracing::warn!(
                "INITIAL_ADMIN_* env config seeded admin {email}. Consider \
                 unsetting these vars after verifying login; they're idempotent \
                 (users-exist short-circuit) but leaving secrets in env is best avoided."
            );
            Ok(true)
        }
        Err(AdminSetupError::AlreadyComplete) => {
            // Operator left the env vars set after first boot.
            // Idempotent skip — no harm, no log noise.
            Ok(false)
        }
        Err(e) => Err(EnvSeedError::AdminSetup(e)),
    }
}

/// Cheap surface check for the bcrypt PHC format. Real
/// verification happens at login time; this just guards against
/// plaintext slipping into the env.
fn looks_like_bcrypt_hash(s: &str) -> bool {
    let s = s.trim();
    // bcrypt hashes are 60 chars exactly: $2x$NN$<22 salt><31 hash>.
    // Allow a small range to be tolerant of future minor variants.
    let len_ok = (59..=72).contains(&s.len());
    let prefix_ok = s.starts_with("$2a$") || s.starts_with("$2b$") || s.starts_with("$2y$");
    len_ok && prefix_ok
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test guard: env-var manipulation isn't thread-safe in
    /// Rust's `std::env`. The serial mutex pins these tests to
    /// one-at-a-time even when `cargo test` parallelises the
    /// suite.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn with_env<F: FnOnce()>(pairs: &[(&str, Option<&str>)], f: F) {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let saved: Vec<_> = pairs
            .iter()
            .map(|(k, _)| (*k, std::env::var_os(k)))
            .collect();
        for (k, v) in pairs {
            match v {
                Some(val) => std::env::set_var(k, val),
                None => std::env::remove_var(k),
            }
        }
        f();
        for (k, v) in saved {
            match v {
                Some(val) => std::env::set_var(k, val),
                None => std::env::remove_var(k),
            }
        }
    }

    #[test]
    fn bcrypt_format_check_accepts_real_hashes() {
        // Three real bcrypt-shaped strings (variant prefixes).
        for h in [
            "$2a$12$abcdefghijklmnopqrstuuM5MLuvTAdEhfwjPGYvkMNDw7pYrkjFNW",
            "$2b$12$abcdefghijklmnopqrstuuM5MLuvTAdEhfwjPGYvkMNDw7pYrkjFNW",
            "$2y$12$abcdefghijklmnopqrstuuM5MLuvTAdEhfwjPGYvkMNDw7pYrkjFNW",
        ] {
            assert!(looks_like_bcrypt_hash(h), "should accept: {h}");
        }
    }

    #[test]
    fn bcrypt_format_check_rejects_plaintext_and_other_formats() {
        for bad in [
            "",
            "hunter2",
            "password",
            // argon2 (different algorithm; login can't verify these)
            "$argon2id$v=19$m=64,t=3,p=1$ZGVmZ2hpams$abc",
            // sha256 hex
            "5e884898da28047151d0e56f8dc6292773603d0d6aabbdd62a11ef721d1542d8",
            // bcrypt-shaped but too short
            "$2b$12$tooshort",
            // wrong prefix entirely
            "{bcrypt}$2b$12$valid-looking-rest-of-the-hash-string-here",
        ] {
            assert!(!looks_like_bcrypt_hash(bad), "should reject: {bad:?}");
        }
    }

    #[test]
    fn seed_returns_false_when_email_unset() {
        use crate::test_helpers::setup_test_connection;
        with_env(
            &[
                ("INITIAL_ADMIN_EMAIL", None),
                ("INITIAL_ADMIN_PASSWORD_HASH", None),
                ("INITIAL_ADMIN_NAME", None),
            ],
            || {
                let mut conn = setup_test_connection();
                let did_seed = seed_from_env(&mut conn).unwrap();
                assert!(!did_seed);
            },
        );
    }

    #[test]
    fn seed_refuses_when_email_set_but_hash_missing() {
        use crate::test_helpers::setup_test_connection;
        with_env(
            &[
                ("INITIAL_ADMIN_EMAIL", Some("admin@example.com")),
                ("INITIAL_ADMIN_PASSWORD_HASH", None),
            ],
            || {
                let mut conn = setup_test_connection();
                let err = seed_from_env(&mut conn).unwrap_err();
                assert!(
                    matches!(err, EnvSeedError::Misconfigured(_)),
                    "got: {err:?}"
                );
            },
        );
    }

    #[test]
    fn seed_refuses_plaintext_password_in_hash_var() {
        use crate::test_helpers::setup_test_connection;
        with_env(
            &[
                ("INITIAL_ADMIN_EMAIL", Some("admin@example.com")),
                ("INITIAL_ADMIN_PASSWORD_HASH", Some("hunter2")),
            ],
            || {
                let mut conn = setup_test_connection();
                let err = seed_from_env(&mut conn).unwrap_err();
                let msg = err.to_string();
                assert!(
                    msg.contains("bcrypt"),
                    "error message must point at bcrypt: {msg}"
                );
            },
        );
    }
}
