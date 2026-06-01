//! Repository for the `user_recovery_codes` table.
//!
//! Decoupled from the JSONB-on-users design on 2026-05-31 (see
//! migration `2026-05-31-180000_decouple_user_recovery_codes`) so
//! consumption can be a single atomic Postgres statement
//! (`UPDATE … WHERE id = $1 AND used_at IS NULL RETURNING …`)
//! instead of an app-side read-modify-write of a JSON array.
//!
//! Hash semantics: the `code_hash` column stores opaque
//! application-side hashes (bcrypt today, possibly argon2id later
//! per `docs/auth-convergence.md`). This module never inspects the
//! hash — callers pass plaintext for verification, callers compute
//! the hash before calling `replace_all`. The two responsibilities
//! sit on the `utils::mfa` side so the hashing choice can change
//! without touching the repository surface.

use chrono::Utc;
use diesel::prelude::*;
use diesel::result::Error;
use uuid::Uuid;

use crate::db::DbConnection;
use crate::models::{NewUserRecoveryCode, UserRecoveryCode};
use crate::schema::user_recovery_codes;

/// Count the unused recovery codes a user currently holds. Used by
/// `mfa_status` and by the verify path to flag "running low; offer
/// regeneration" in the response. O(unused-count) via the partial
/// index on `(user_uuid) WHERE used_at IS NULL`.
pub fn count_unused(conn: &mut DbConnection, user_uuid: &Uuid) -> Result<i64, Error> {
    user_recovery_codes::table
        .filter(user_recovery_codes::user_uuid.eq(user_uuid))
        .filter(user_recovery_codes::used_at.is_null())
        .count()
        .get_result(conn)
}

/// Load every unused code for a user. Used by the verify path
/// (which then bcrypt-verifies each one in constant time before
/// consuming the matched id). Order is by `id` ascending so the
/// timing characteristics are stable across calls.
pub fn list_unused(
    conn: &mut DbConnection,
    user_uuid: &Uuid,
) -> Result<Vec<UserRecoveryCode>, Error> {
    user_recovery_codes::table
        .filter(user_recovery_codes::user_uuid.eq(user_uuid))
        .filter(user_recovery_codes::used_at.is_null())
        .order(user_recovery_codes::id.asc())
        .load(conn)
}

/// Atomically consume one specific recovery code by id. Returns
/// `Ok(true)` when the row was unused and is now marked used,
/// `Ok(false)` when no matching unused row exists (already used or
/// concurrent consumer beat us to it — Postgres row-level lock
/// resolves the race). Safe to call from multiple flows racing the
/// same code; only one wins.
// sync-audit-only: MFA recovery code consumption is auth-flow only; not observed by other clients
pub fn consume_by_id(conn: &mut DbConnection, id: i64) -> Result<bool, Error> {
    let rows_affected = diesel::update(
        user_recovery_codes::table
            .filter(user_recovery_codes::id.eq(id))
            .filter(user_recovery_codes::used_at.is_null()),
    )
    .set(user_recovery_codes::used_at.eq(Utc::now()))
    .execute(conn)?;
    Ok(rows_affected > 0)
}

/// Replace a user's full recovery-code set in one transaction:
/// delete all existing codes (used and unused), insert the
/// freshly-hashed batch. Used by enrol + regenerate flows. Atomic
/// per Postgres semantics — a failure rolls back so the user is
/// never left with zero codes mid-rotation.
// sync-audit-only: MFA enrol / regenerate is a private auth event; recovery code hashes never need to fan out to other workspace clients
pub fn replace_all(
    conn: &mut DbConnection,
    user_uuid: &Uuid,
    new_hashes: Vec<String>,
) -> Result<usize, Error> {
    conn.transaction(|conn| {
        diesel::delete(
            user_recovery_codes::table.filter(user_recovery_codes::user_uuid.eq(user_uuid)),
        )
        .execute(conn)?;

        if new_hashes.is_empty() {
            return Ok(0);
        }

        let rows: Vec<NewUserRecoveryCode> = new_hashes
            .into_iter()
            .map(|code_hash| NewUserRecoveryCode {
                user_uuid: *user_uuid,
                code_hash,
            })
            .collect();

        diesel::insert_into(user_recovery_codes::table)
            .values(&rows)
            .execute(conn)
    })
}

/// Drop every recovery code (used and unused) for a user. Used by
/// the `mfa_disable` flow to clear the recovery-code roster when
/// the second factor is removed.
// sync-audit-only: MFA disable wipes recovery codes; not observed by other clients
pub fn delete_all_for_user(conn: &mut DbConnection, user_uuid: &Uuid) -> Result<usize, Error> {
    diesel::delete(user_recovery_codes::table.filter(user_recovery_codes::user_uuid.eq(user_uuid)))
        .execute(conn)
}
