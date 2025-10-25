use diesel::prelude::*;
use uuid::Uuid;
use chrono::Utc;

use crate::db::DbConnection;
use crate::models::{RefreshToken, NewRefreshToken};
use crate::schema::refresh_tokens;

/// Create a new refresh token
pub fn create_refresh_token(
    conn: &mut DbConnection,
    new_token: NewRefreshToken,
) -> Result<RefreshToken, diesel::result::Error> {
    diesel::insert_into(refresh_tokens::table)
        .values(&new_token)
        .get_result(conn)
}

/// Get a refresh token by hash (and check if not revoked or expired)
pub fn get_valid_refresh_token(
    conn: &mut DbConnection,
    token_hash: &str,
) -> Result<RefreshToken, diesel::result::Error> {
    refresh_tokens::table
        .filter(refresh_tokens::token_hash.eq(token_hash))
        .filter(refresh_tokens::revoked_at.is_null())
        .filter(refresh_tokens::expires_at.gt(Utc::now().naive_utc()))
        .first::<RefreshToken>(conn)
}

/// Revoke a refresh token by hash
pub fn revoke_refresh_token(
    conn: &mut DbConnection,
    token_hash: &str,
) -> Result<usize, diesel::result::Error> {
    diesel::update(
        refresh_tokens::table.filter(refresh_tokens::token_hash.eq(token_hash))
    )
    .set(refresh_tokens::revoked_at.eq(Utc::now().naive_utc()))
    .execute(conn)
}

