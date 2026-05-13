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
//! Search indexing and category seeding are likewise the
//! caller's responsibility: the web handler hits the search
//! service it already has on hand, and the CLI skips both
//! (it's running before the server is up, so the first server
//! start will pick up the new row via its normal startup paths).

use diesel::connection::Connection;
use diesel::prelude::*;
use diesel::sql_query;
use serde_json::json;
use thiserror::Error;
use uuid::Uuid;

use crate::db::DbConnection;
use crate::models::{User, UserEmail};

#[derive(Debug, Error)]
pub enum AdminSetupError {
    #[error("setup has already been completed — users exist in the system")]
    AlreadyComplete,
    #[error("email address already in use")]
    DuplicateEmail,
    #[error(transparent)]
    Db(#[from] diesel::result::Error),
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

    conn.transaction::<_, AdminSetupError, _>(|c| {
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
                    "role": user.role,
                    "pronouns": user.pronouns,
                    "avatar_url": user.avatar_url,
                    "avatar_thumb": user.avatar_thumb,
                }),
                groups: crate::sync::groups::workspace(),
                causation_id: None,
            },
        )?;

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
