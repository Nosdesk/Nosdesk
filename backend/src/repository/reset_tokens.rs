use diesel::prelude::*;
use chrono::{DateTime, Utc};
use uuid::Uuid;
use crate::db::DbConnection;
use crate::models::ResetToken;
use crate::schema::reset_tokens;
use crate::utils::reset_tokens::{TokenType, ResetTokenUtils};

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
    let new_token = crate::models::NewResetToken {
        token_hash,
        user_uuid,
        token_type,
        ip_address,
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

/// Delete expired tokens (cleanup job)
pub fn delete_expired_tokens(conn: &mut DbConnection) -> QueryResult<usize> {
    diesel::delete(reset_tokens::table.filter(reset_tokens::expires_at.lt(Utc::now())))
        .execute(conn)
}

/// Delete all tokens for a user of a specific type
pub fn delete_user_tokens_by_type(
    conn: &mut DbConnection,
    user_uuid_value: Uuid,
    token_type_value: &str,
) -> QueryResult<usize> {
    diesel::delete(
        reset_tokens::table
            .filter(reset_tokens::user_uuid.eq(user_uuid_value))
            .filter(reset_tokens::token_type.eq(token_type_value))
    )
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

/// Validate and consume a reset token
/// Returns Ok(user_uuid) if token is valid, unused, and not expired
pub fn validate_and_consume_token(
    conn: &mut DbConnection,
    raw_token: &str,
    expected_token_type: &str,
) -> Result<Uuid, String> {
    // Hash the raw token to look it up
    let token_hash_value = ResetTokenUtils::hash_token(raw_token);

    // Find the token
    let token = find_token_by_hash(conn, &token_hash_value)
        .map_err(|_| "Invalid or expired token".to_string())?;

    // Verify token type
    if token.token_type != expected_token_type {
        return Err("Invalid token type".to_string());
    }

    // Check if already used
    if token.is_used {
        return Err("Token has already been used".to_string());
    }

    // Check if expired (convert NaiveDateTime to DateTime<Utc>)
    let expires_at_utc = DateTime::<Utc>::from_naive_utc_and_offset(token.expires_at, Utc);
    if ResetTokenUtils::is_token_expired(expires_at_utc) {
        return Err("Token has expired".to_string());
    }

    // Mark as used
    mark_token_as_used(conn, &token_hash_value)
        .map_err(|_| "Failed to mark token as used".to_string())?;

    Ok(token.user_uuid)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    // Note: These tests require a test database connection
    // They are here as documentation of the expected behavior
}
