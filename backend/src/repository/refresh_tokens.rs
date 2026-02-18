use diesel::prelude::*;
use chrono::Utc;
use uuid::Uuid;

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

/// Get a refresh token by hash (caller checks revocation/reuse)
pub fn get_refresh_token_by_hash(
    conn: &mut DbConnection,
    token_hash: &str,
) -> Result<RefreshToken, diesel::result::Error> {
    refresh_tokens::table
        .filter(refresh_tokens::token_hash.eq(token_hash))
        .filter(refresh_tokens::expires_at.gt(Utc::now().naive_utc()))
        .first::<RefreshToken>(conn)
}

/// Mark a token as used (token rotation with grace period)
pub fn mark_token_used(
    conn: &mut DbConnection,
    token_hash: &str,
    replacement_hash: &str,
    grace_until: chrono::NaiveDateTime,
) -> Result<usize, diesel::result::Error> {
    diesel::update(
        refresh_tokens::table.filter(refresh_tokens::token_hash.eq(token_hash))
    )
    .set((
        refresh_tokens::is_used.eq(true),
        refresh_tokens::used_at.eq(Utc::now().naive_utc()),
        refresh_tokens::replaced_by_hash.eq(replacement_hash),
        refresh_tokens::grace_expires_at.eq(grace_until),
    ))
    .execute(conn)
}

/// Revoke all tokens in a family (reuse detection)
pub fn revoke_token_family(
    conn: &mut DbConnection,
    family_id: &Uuid,
) -> Result<usize, diesel::result::Error> {
    diesel::update(
        refresh_tokens::table.filter(refresh_tokens::family_id.eq(family_id))
    )
    .set(refresh_tokens::revoked_at.eq(Utc::now().naive_utc()))
    .execute(conn)
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
    fn create_and_get_refresh_token() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "TokenUser", UserRole::User);

        let family = Uuid::new_v4();
        let new_token = NewRefreshToken {
            token_hash: "testhash123".to_string(),
            user_uuid: user.uuid,
            expires_at: (Utc::now() + Duration::hours(1)).naive_utc(),
            session_id: None,
            family_id: family,
        };

        let created = create_refresh_token(&mut conn, new_token).unwrap();
        assert_eq!(created.token_hash, "testhash123");
        assert_eq!(created.family_id, family);
        assert!(!created.is_used);

        let fetched = get_refresh_token_by_hash(&mut conn, "testhash123").unwrap();
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
            session_id: None,
            family_id: Uuid::new_v4(),
        };

        create_refresh_token(&mut conn, new_token).unwrap();
        revoke_refresh_token(&mut conn, "revokeme").unwrap();

        // Token still found (caller checks revoked_at), but it has revoked_at set
        let token = get_refresh_token_by_hash(&mut conn, "revokeme").unwrap();
        assert!(token.revoked_at.is_some());
    }

    #[test]
    fn mark_token_used_test() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "UsedUser", UserRole::User);

        let new_token = NewRefreshToken {
            token_hash: "useme".to_string(),
            user_uuid: user.uuid,
            expires_at: (Utc::now() + Duration::hours(1)).naive_utc(),
            session_id: None,
            family_id: Uuid::new_v4(),
        };

        create_refresh_token(&mut conn, new_token).unwrap();
        let grace = (Utc::now() + Duration::seconds(5)).naive_utc();
        mark_token_used(&mut conn, "useme", "newhash", grace).unwrap();

        let token = get_refresh_token_by_hash(&mut conn, "useme").unwrap();
        assert!(token.is_used);
        assert!(token.used_at.is_some());
        assert_eq!(token.replaced_by_hash.as_deref(), Some("newhash"));
    }

    #[test]
    fn revoke_token_family_test() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "FamilyUser", UserRole::User);

        let family = Uuid::new_v4();
        for hash in &["fam1", "fam2", "fam3"] {
            let new_token = NewRefreshToken {
                token_hash: hash.to_string(),
                user_uuid: user.uuid,
                expires_at: (Utc::now() + Duration::hours(1)).naive_utc(),
                session_id: None,
                family_id: family,
            };
            create_refresh_token(&mut conn, new_token).unwrap();
        }

        let revoked = revoke_token_family(&mut conn, &family).unwrap();
        assert_eq!(revoked, 3);
    }
}
