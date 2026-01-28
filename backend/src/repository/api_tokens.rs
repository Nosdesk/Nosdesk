//! API Token Repository
//!
//! Provides database operations for API tokens used for programmatic access.

use chrono::{Duration, Utc};
use diesel::prelude::*;
use ipnetwork::IpNetwork;
use rand::Rng;
use ring::digest::{Context, SHA256};
use uuid::Uuid;

use crate::db::DbConnection;
use crate::models::{ApiToken, ApiTokenCreatedResponse, ApiTokenInfo, NewApiToken};
use crate::schema::api_tokens;

/// API token prefix for identification
const TOKEN_PREFIX: &str = "nsk_";

/// Generate a cryptographically secure API token
/// Returns the full token: nsk_ + 32 random hex chars (36 chars total)
pub fn generate_api_token() -> String {
    let mut rng = rand::thread_rng();
    let token_bytes: [u8; 16] = rng.gen(); // 16 bytes = 32 hex chars
    format!("{}{}", TOKEN_PREFIX, hex::encode(token_bytes))
}

/// Hash a token using SHA-256
pub fn hash_token(token: &str) -> String {
    let mut context = Context::new(&SHA256);
    context.update(token.as_bytes());
    let digest = context.finish();
    hex::encode(digest.as_ref())
}

/// Extract the prefix from a token for display (first 8 chars)
pub fn get_token_prefix(token: &str) -> String {
    token.chars().take(8).collect()
}

/// Create a new API token
/// Returns the created token with the raw token value (only returned once!)
pub fn create_api_token(
    conn: &mut DbConnection,
    user_uuid: Uuid,
    name: String,
    created_by: Uuid,
    expires_in_days: Option<i64>,
    scopes: Option<Vec<String>>,
) -> Result<ApiTokenCreatedResponse, diesel::result::Error> {
    let raw_token = generate_api_token();
    let token_hash = hash_token(&raw_token);
    let token_prefix = get_token_prefix(&raw_token);

    let expires_at = expires_in_days.map(|days| (Utc::now() + Duration::days(days)).naive_utc());

    // Convert scopes to the database format (Option<Vec<Option<String>>>)
    let db_scopes: Option<Vec<Option<String>>> = scopes
        .map(|s| if s.is_empty() { vec!["full".to_string()] } else { s })
        .or(Some(vec!["full".to_string()]))
        .map(|v| v.into_iter().map(Some).collect());

    let new_token = NewApiToken {
        token_hash,
        token_prefix: token_prefix.clone(),
        user_uuid,
        name: name.clone(),
        scopes: db_scopes,
        created_by,
        expires_at,
    };

    let created: ApiToken = diesel::insert_into(api_tokens::table)
        .values(&new_token)
        .get_result(conn)?;

    Ok(ApiTokenCreatedResponse {
        uuid: created.uuid,
        token: raw_token,
        token_prefix,
        name,
        user_uuid,
        expires_at: created.expires_at,
    })
}

/// Get a valid API token by hash (not revoked, not expired)
pub fn get_valid_api_token(
    conn: &mut DbConnection,
    token_hash: &str,
) -> Result<ApiToken, diesel::result::Error> {
    let now = Utc::now().naive_utc();

    api_tokens::table
        .filter(api_tokens::token_hash.eq(token_hash))
        .filter(api_tokens::revoked_at.is_null())
        .filter(
            api_tokens::expires_at
                .is_null()
                .or(api_tokens::expires_at.gt(now)),
        )
        .first::<ApiToken>(conn)
}

/// Update last_used_at and last_used_ip for a token
pub fn update_token_last_used(
    conn: &mut DbConnection,
    token_id: i32,
    ip_address: Option<IpNetwork>,
) -> Result<usize, diesel::result::Error> {
    diesel::update(api_tokens::table.filter(api_tokens::id.eq(token_id)))
        .set((
            api_tokens::last_used_at.eq(Utc::now().naive_utc()),
            api_tokens::last_used_ip.eq(ip_address),
        ))
        .execute(conn)
}

/// List all API tokens (for admin view)
pub fn list_all_api_tokens(conn: &mut DbConnection) -> Result<Vec<ApiToken>, diesel::result::Error> {
    api_tokens::table
        .order(api_tokens::created_at.desc())
        .load::<ApiToken>(conn)
}

/// Get a token by UUID
pub fn get_api_token_by_uuid(
    conn: &mut DbConnection,
    token_uuid: Uuid,
) -> Result<ApiToken, diesel::result::Error> {
    api_tokens::table
        .filter(api_tokens::uuid.eq(token_uuid))
        .first::<ApiToken>(conn)
}

/// Revoke an API token by UUID (soft delete)
pub fn revoke_api_token(
    conn: &mut DbConnection,
    token_uuid: Uuid,
) -> Result<usize, diesel::result::Error> {
    diesel::update(api_tokens::table.filter(api_tokens::uuid.eq(token_uuid)))
        .set(api_tokens::revoked_at.eq(Utc::now().naive_utc()))
        .execute(conn)
}

/// Enrich API tokens with user names for display
pub fn enrich_tokens_with_users(
    conn: &mut DbConnection,
    tokens: Vec<ApiToken>,
) -> Result<Vec<ApiTokenInfo>, diesel::result::Error> {
    use crate::repository::get_user_by_uuid;

    let mut enriched = Vec::with_capacity(tokens.len());

    for token in tokens {
        let user_name = get_user_by_uuid(&token.user_uuid, conn)
            .map(|u| u.name)
            .unwrap_or_else(|_| "Unknown".to_string());

        let created_by_name = get_user_by_uuid(&token.created_by, conn)
            .map(|u| u.name)
            .unwrap_or_else(|_| "Unknown".to_string());

        // Convert scopes from Option<Vec<Option<String>>> to Vec<String>
        let scopes: Vec<String> = token
            .scopes
            .unwrap_or_default()
            .into_iter()
            .flatten()
            .collect();

        enriched.push(ApiTokenInfo {
            uuid: token.uuid,
            token_prefix: token.token_prefix,
            name: token.name,
            user_uuid: token.user_uuid,
            user_name,
            scopes,
            created_at: token.created_at,
            created_by_name,
            expires_at: token.expires_at,
            revoked_at: token.revoked_at,
            last_used_at: token.last_used_at,
        });
    }

    Ok(enriched)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::{setup_test_connection, TestFixtures};
    use crate::models::UserRole;

    // ── Pure logic tests ─────────────────────────────────────────

    #[test]
    fn generate_token_has_prefix_and_length() {
        let token = generate_api_token();
        assert!(token.starts_with("nsk_"));
        assert_eq!(token.len(), 36); // "nsk_" (4) + 32 hex chars
    }

    #[test]
    fn hash_token_is_deterministic() {
        let h1 = hash_token("nsk_abc123");
        let h2 = hash_token("nsk_abc123");
        assert_eq!(h1, h2);
    }

    #[test]
    fn hash_token_differs_for_different_input() {
        assert_ne!(hash_token("nsk_aaa"), hash_token("nsk_bbb"));
    }

    #[test]
    fn get_token_prefix_extracts_first_8() {
        assert_eq!(get_token_prefix("nsk_abcdef1234567890"), "nsk_abcd");
    }

    // ── DB-backed tests ──────────────────────────────────────────

    #[test]
    fn create_and_retrieve_api_token() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "tokuser", UserRole::Admin);

        let response = create_api_token(
            &mut conn, user.uuid, "My Token".into(), user.uuid, Some(30), None,
        ).unwrap();

        assert!(response.token.starts_with("nsk_"));
        assert_eq!(response.name, "My Token");

        // Can retrieve by hash
        let hash = hash_token(&response.token);
        let fetched = get_valid_api_token(&mut conn, &hash).unwrap();
        assert_eq!(fetched.name, "My Token");
    }

    #[test]
    fn revoked_token_is_not_valid() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "revuser", UserRole::Admin);

        let response = create_api_token(
            &mut conn, user.uuid, "Revoke Me".into(), user.uuid, None, None,
        ).unwrap();

        revoke_api_token(&mut conn, response.uuid).unwrap();

        let hash = hash_token(&response.token);
        assert!(get_valid_api_token(&mut conn, &hash).is_err());
    }

    #[test]
    fn expired_token_is_not_valid() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "expuser", UserRole::Admin);

        // Create a token that expires in 0 days (already expired)
        let response = create_api_token(
            &mut conn, user.uuid, "Expired".into(), user.uuid, Some(0), None,
        ).unwrap();

        let hash = hash_token(&response.token);
        // Token with 0-day expiry should already be expired
        assert!(get_valid_api_token(&mut conn, &hash).is_err());
    }

    #[test]
    fn default_scope_is_full() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "scopeuser", UserRole::Admin);

        let response = create_api_token(
            &mut conn, user.uuid, "Default Scope".into(), user.uuid, None, None,
        ).unwrap();

        let hash = hash_token(&response.token);
        let fetched = get_valid_api_token(&mut conn, &hash).unwrap();
        let scopes: Vec<String> = fetched.scopes.unwrap_or_default().into_iter().flatten().collect();
        assert!(scopes.contains(&"full".to_string()));
    }
}
