use diesel::prelude::*;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::{setup_test_connection, TestFixtures};
    use crate::models::{UserRole, NewRefreshToken};
    use chrono::{Utc, Duration};

    #[test]
    fn create_and_get_valid_refresh_token() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "TokenUser", UserRole::User);

        let new_token = NewRefreshToken {
            token_hash: "testhash123".to_string(),
            user_uuid: user.uuid,
            expires_at: (Utc::now() + Duration::hours(1)).naive_utc(),
        };

        let created = create_refresh_token(&mut conn, new_token).unwrap();
        assert_eq!(created.token_hash, "testhash123");

        let fetched = get_valid_refresh_token(&mut conn, "testhash123").unwrap();
        assert_eq!(fetched.user_uuid, user.uuid);
    }

    #[test]
    fn revoke_refresh_token_makes_invalid() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "RevokeUser", UserRole::User);

        let new_token = NewRefreshToken {
            token_hash: "revokeme".to_string(),
            user_uuid: user.uuid,
            expires_at: (Utc::now() + Duration::hours(1)).naive_utc(),
        };

        create_refresh_token(&mut conn, new_token).unwrap();
        revoke_refresh_token(&mut conn, "revokeme").unwrap();

        let result = get_valid_refresh_token(&mut conn, "revokeme");
        assert!(result.is_err());
    }
}
