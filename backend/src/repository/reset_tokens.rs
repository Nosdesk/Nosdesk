use crate::db::DbConnection;
use crate::models::ResetToken;
use crate::schema::reset_tokens;
use crate::utils::reset_tokens::ResetTokenUtils;
use chrono::{DateTime, Utc};
use diesel::prelude::*;
use uuid::Uuid;

// sync-audit-only: Sessions / auth tokens (covered by security_events)
/// Create a new reset token in the database
pub fn create_reset_token(
    conn: &mut DbConnection,
    token_hash: &str,
    user_uuid: Uuid,
    token_type: &str,
    ip_address: Option<&str>,
    user_agent: Option<&str>,
    expires_at: DateTime<Utc>,
    metadata: Option<serde_json::Value>,
) -> QueryResult<ResetToken> {
    // Convert IP address string to IpNetwork
    let ip_network = ip_address.and_then(|ip_str| ip_str.parse().ok());

    let new_token = crate::models::NewResetToken {
        token_hash,
        user_uuid,
        token_type,
        ip_address: ip_network,
        user_agent,
        expires_at,
        metadata,
    };

    diesel::insert_into(reset_tokens::table)
        .values(&new_token)
        .get_result(conn)
}

/// Find a reset token by its hash
pub fn find_token_by_hash(
    conn: &mut DbConnection,
    token_hash_value: &str,
) -> QueryResult<ResetToken> {
    reset_tokens::table
        .filter(reset_tokens::token_hash.eq(token_hash_value))
        .first(conn)
}

// sync-audit-only: Sessions / auth tokens (covered by security_events)
/// Mark a token as used
pub fn mark_token_as_used(
    conn: &mut DbConnection,
    token_hash_value: &str,
) -> QueryResult<ResetToken> {
    diesel::update(reset_tokens::table.filter(reset_tokens::token_hash.eq(token_hash_value)))
        .set((
            reset_tokens::used_at.eq(Some(Utc::now())),
            reset_tokens::is_used.eq(true),
        ))
        .get_result(conn)
}

/// A user's live tokens of `token_type`: unused and unexpired, newest first.
/// The portal sign-in code is checked against these.
pub fn live_tokens(
    conn: &mut DbConnection,
    user: Uuid,
    token_type: &str,
) -> QueryResult<Vec<ResetToken>> {
    reset_tokens::table
        .filter(reset_tokens::user_uuid.eq(user))
        .filter(reset_tokens::token_type.eq(token_type))
        .filter(reset_tokens::is_used.eq(false))
        .filter(reset_tokens::expires_at.gt(Utc::now()))
        .order(reset_tokens::created_at.desc())
        .load(conn)
}

// sync-audit-only: Sessions / auth tokens (covered by security_events)
/// Claim a token by hash if it is still unused and unexpired, in one UPDATE,
/// so two concurrent uses can't both succeed. `None` when it was already spent.
pub fn claim_unused(
    conn: &mut DbConnection,
    token_hash_value: &str,
) -> QueryResult<Option<ResetToken>> {
    diesel::update(
        reset_tokens::table
            .filter(reset_tokens::token_hash.eq(token_hash_value))
            .filter(reset_tokens::is_used.eq(false))
            .filter(reset_tokens::expires_at.gt(Utc::now())),
    )
    .set((
        reset_tokens::used_at.eq(Some(Utc::now())),
        reset_tokens::is_used.eq(true),
    ))
    .get_result(conn)
    .optional()
}

// sync-audit-only: Sessions / auth tokens (covered by security_events)
/// Replace a token's metadata (the sign-in code's attempt counter).
pub fn set_metadata(
    conn: &mut DbConnection,
    token_hash_value: &str,
    metadata: serde_json::Value,
) -> QueryResult<usize> {
    diesel::update(reset_tokens::table.filter(reset_tokens::token_hash.eq(token_hash_value)))
        .set(reset_tokens::metadata.eq(Some(metadata)))
        .execute(conn)
}

/// Count tokens for a user created within a time window (for rate limiting)
pub fn count_recent_tokens(
    conn: &mut DbConnection,
    user_uuid_value: Uuid,
    token_type_value: &str,
    since: DateTime<Utc>,
) -> QueryResult<i64> {
    reset_tokens::table
        .filter(reset_tokens::user_uuid.eq(user_uuid_value))
        .filter(reset_tokens::token_type.eq(token_type_value))
        .filter(reset_tokens::created_at.gt(since))
        .count()
        .get_result(conn)
}

// sync-audit-only: Sessions / auth tokens (covered by security_events)
/// Invalidate all tokens of a specific type for a user
/// Used when resending invitations to invalidate old tokens
pub fn invalidate_tokens_by_type(
    conn: &mut DbConnection,
    user_uuid_value: Uuid,
    token_type_value: &str,
) -> QueryResult<usize> {
    diesel::update(
        reset_tokens::table
            .filter(reset_tokens::user_uuid.eq(user_uuid_value))
            .filter(reset_tokens::token_type.eq(token_type_value))
            .filter(reset_tokens::is_used.eq(false)),
    )
    .set((
        reset_tokens::is_used.eq(true),
        reset_tokens::used_at.eq(Some(Utc::now())),
    ))
    .execute(conn)
}

// sync-audit-only: auth flow mirror of mark_token_as_used; covered by security_events
/// Validate and consume a reset token atomically.
///
/// AUD-012: replaces a non-atomic check-then-update with one
/// SQL `UPDATE ... WHERE is_used = false ... RETURNING user_uuid`.
/// Two concurrent requests carrying the same token used to both
/// see `is_used = false` and both proceed to set passwords, mark
/// emails verified, etc. With the atomic update only one
/// `UPDATE` ever sees `is_used = false`; the other gets an empty
/// RETURNING and fails with the same generic error a missing or
/// expired token would produce.
///
/// All failure modes collapse to "Invalid or expired token." A
/// caller that distinguishes "wrong type" from "already used"
/// from "expired" leaks state about which tokens are alive,
/// which is useless given the 256-bit token entropy but still
/// worth not leaking.
pub fn validate_and_consume_token(
    conn: &mut DbConnection,
    raw_token: &str,
    expected_token_type: &str,
) -> Result<Uuid, String> {
    validate_and_consume_token_with_metadata(conn, raw_token, expected_token_type)
        .map(|(user_uuid, _)| user_uuid)
}

// sync-audit-only: auth flow mirror of mark_token_as_used; covered by security_events
/// As [`validate_and_consume_token`], also returning the token's `metadata`.
///
/// Email confirmation needs it: `reset_tokens` is user-scoped, and a user may
/// hold several unverified addresses, so which address a token confirms is
/// recorded in `metadata` rather than being derivable from its subject.
///
/// One implementation on purpose. The single-use guarantee is the `is_used =
/// false` predicate inside this one UPDATE, so a second claim path would be a
/// second chance to get atomicity wrong.
pub fn validate_and_consume_token_with_metadata(
    conn: &mut DbConnection,
    raw_token: &str,
    expected_token_type: &str,
) -> Result<(Uuid, Option<serde_json::Value>), String> {
    let token_hash_value = ResetTokenUtils::hash_token(raw_token);
    let now = Utc::now();

    let claimed: Option<(Uuid, Option<serde_json::Value>)> = diesel::update(
        reset_tokens::table
            .filter(reset_tokens::token_hash.eq(&token_hash_value))
            .filter(reset_tokens::token_type.eq(expected_token_type))
            .filter(reset_tokens::is_used.eq(false))
            .filter(reset_tokens::expires_at.gt(now)),
    )
    .set((
        reset_tokens::is_used.eq(true),
        reset_tokens::used_at.eq(Some(now)),
    ))
    .returning((reset_tokens::user_uuid, reset_tokens::metadata))
    .get_result(conn)
    .optional()
    .map_err(|e| format!("Failed to claim token: {e}"))?;

    claimed.ok_or_else(|| "Invalid or expired token".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::{setup_test_connection, TestFixtures};
    use crate::utils::reset_tokens::TokenType;

    fn create_invitation_token(conn: &mut DbConnection, user_uuid: Uuid) -> String {
        let issued = crate::utils::reset_tokens::ResetTokenUtils::create_reset_token(
            user_uuid,
            TokenType::Invitation,
        );
        create_reset_token(
            conn,
            &issued.token_hash,
            user_uuid,
            TokenType::Invitation.as_str(),
            None,
            None,
            issued.expires_at,
            None,
        )
        .expect("seed token row");
        issued.raw_token
    }

    #[test]
    fn validate_and_consume_token_succeeds_once() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "alice", "user");
        let raw = create_invitation_token(&mut conn, user.uuid);

        let claimed = validate_and_consume_token(&mut conn, &raw, TokenType::Invitation.as_str())
            .expect("first consume must succeed");
        assert_eq!(claimed, user.uuid);
    }

    #[test]
    fn second_consume_of_same_token_fails() {
        // The atomic UPDATE pattern: the second caller's
        // `WHERE is_used = false` clause filters the row out, so
        // RETURNING comes back empty and we surface the same
        // generic error a never-existed token would produce.
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "bob", "user");
        let raw = create_invitation_token(&mut conn, user.uuid);

        validate_and_consume_token(&mut conn, &raw, TokenType::Invitation.as_str())
            .expect("first consume succeeds");
        let second = validate_and_consume_token(&mut conn, &raw, TokenType::Invitation.as_str());
        assert!(second.is_err(), "second consume must fail");
        assert_eq!(
            second.unwrap_err(),
            "Invalid or expired token",
            "error message must not distinguish 'already used' from 'never existed'",
        );
    }

    #[test]
    fn wrong_token_type_fails_with_generic_error() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "carol", "user");
        let raw = create_invitation_token(&mut conn, user.uuid);

        // The token exists and is fresh, but the caller's
        // expected type doesn't match. The atomic UPDATE filters
        // on token_type so the row is unchanged and the response
        // is the same generic error.
        let err = validate_and_consume_token(&mut conn, &raw, TokenType::PasswordReset.as_str())
            .expect_err("wrong token type must fail");
        assert_eq!(err, "Invalid or expired token");

        // Confirm the row was NOT consumed — a subsequent call
        // with the correct type still succeeds.
        validate_and_consume_token(&mut conn, &raw, TokenType::Invitation.as_str())
            .expect("token remains consumable with the right type");
    }

    #[test]
    fn nonexistent_token_fails_with_generic_error() {
        let mut conn = setup_test_connection();
        let err = validate_and_consume_token(
            &mut conn,
            "not-a-real-token",
            TokenType::Invitation.as_str(),
        )
        .expect_err("missing token must fail");
        assert_eq!(err, "Invalid or expired token");
    }
}
