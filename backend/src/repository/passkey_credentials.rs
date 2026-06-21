//! Passkey credential storage. One row per WebAuthn credential,
//! keyed by `credential_id`. Replaces the JSONB-blob-on-users design
//! that forced full table scans on every login and lost concurrent
//! adds via read-modify-write.

use diesel::prelude::*;
use diesel::result::Error;
use uuid::Uuid;

use crate::db::DbConnection;
use crate::models::{NewPasskeyCredential, PasskeyCredential, PasskeyCredentialUpdate};
use crate::schema::passkey_credentials;

/// All passkeys for a user, ordered by creation time so the UI shows
/// oldest first. Empty vec when the user has none.
pub fn list_for_user(
    conn: &mut DbConnection,
    user_uuid: &Uuid,
) -> Result<Vec<PasskeyCredential>, Error> {
    passkey_credentials::table
        .filter(passkey_credentials::user_uuid.eq(user_uuid))
        .order(passkey_credentials::created_at.asc())
        .load(conn)
}

/// Find a credential by its base64url credential ID. Used by the
/// login flow to identify which user owns an asserted credential.
pub fn find_by_credential_id(
    conn: &mut DbConnection,
    credential_id: &str,
) -> Result<Option<PasskeyCredential>, Error> {
    passkey_credentials::table
        .filter(passkey_credentials::credential_id.eq(credential_id))
        .first(conn)
        .optional()
}

// sync-audit-only: Sessions / auth tokens (covered by security_events)
pub fn create(
    conn: &mut DbConnection,
    new: NewPasskeyCredential,
) -> Result<PasskeyCredential, Error> {
    diesel::insert_into(passkey_credentials::table)
        .values(&new)
        .get_result(conn)
}

// sync-audit-only: Sessions / auth tokens (covered by security_events)
/// Update mutable fields (rename, last_used_at touch). Returns the
/// updated row, or `Error::NotFound` if no such credential exists
/// for that user.
pub fn update_for_user(
    conn: &mut DbConnection,
    user_uuid: &Uuid,
    credential_id: &str,
    change: PasskeyCredentialUpdate,
) -> Result<PasskeyCredential, Error> {
    diesel::update(
        passkey_credentials::table
            .filter(passkey_credentials::user_uuid.eq(user_uuid))
            .filter(passkey_credentials::credential_id.eq(credential_id)),
    )
    .set(change)
    .get_result(conn)
}

// sync-audit-only: Sessions / auth tokens (covered by security_events)
/// Delete a single credential owned by a user. Scoped to user_uuid
/// so an admin can't accidentally delete another user's credential
/// by ID alone. Returns the number of rows deleted (0 or 1).
pub fn delete_for_user(
    conn: &mut DbConnection,
    user_uuid: &Uuid,
    credential_id: &str,
) -> Result<usize, Error> {
    diesel::delete(
        passkey_credentials::table
            .filter(passkey_credentials::user_uuid.eq(user_uuid))
            .filter(passkey_credentials::credential_id.eq(credential_id)),
    )
    .execute(conn)
}

// sync-audit-only: Sessions / auth tokens (covered by security_events)
/// Delete every passkey credential owned by a user. Used by the
/// locked-out-admin CLI recovery (`admin clear-passkeys`) when a
/// passkey is blocking login from a non-secure-context origin and the
/// user can't complete the WebAuthn challenge. Returns the number of
/// credentials removed.
pub fn delete_all_for_user(conn: &mut DbConnection, user_uuid: &Uuid) -> Result<usize, Error> {
    diesel::delete(passkey_credentials::table.filter(passkey_credentials::user_uuid.eq(user_uuid)))
        .execute(conn)
}

/// Number of credentials a user has. Used for the per-user cap
/// check before registration.
pub fn count_for_user(conn: &mut DbConnection, user_uuid: &Uuid) -> Result<i64, Error> {
    passkey_credentials::table
        .filter(passkey_credentials::user_uuid.eq(user_uuid))
        .count()
        .get_result(conn)
}
