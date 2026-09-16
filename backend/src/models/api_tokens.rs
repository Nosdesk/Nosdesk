use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ===== API TOKEN MODELS =====

/// API token for programmatic access (stored in database)
#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable)]
#[diesel(table_name = crate::schema::api_tokens)]
pub struct ApiToken {
    pub id: i32,
    pub uuid: Uuid,
    pub token_hash: String,
    pub token_prefix: String,
    pub user_uuid: Uuid,
    pub name: String,
    pub scopes: Option<Vec<Option<String>>>,
    pub created_at: chrono::NaiveDateTime,
    pub created_by: Uuid,
    pub expires_at: Option<chrono::NaiveDateTime>,
    pub revoked_at: Option<chrono::NaiveDateTime>,
    pub last_used_at: Option<chrono::NaiveDateTime>,
    pub last_used_ip: Option<ipnetwork::IpNetwork>,
    pub workspace_id: i32,
}

/// New API token for insertion. All tokens are user-bound; the
/// control-plane provisioning surface authenticates with an EdDSA JWT
/// (see `extractors::PlatformAuth`), not an api_token.
#[derive(Debug, Insertable)]
#[diesel(table_name = crate::schema::api_tokens)]
pub struct NewApiToken {
    pub token_hash: String,
    pub token_prefix: String,
    pub user_uuid: Uuid,
    pub name: String,
    pub scopes: Option<Vec<Option<String>>>,
    pub created_by: Uuid,
    pub expires_at: Option<chrono::NaiveDateTime>,
}

/// Request to create a new API token
#[derive(Debug, Deserialize)]
pub struct CreateApiTokenRequest {
    pub name: String,
    pub user_uuid: Uuid,
    #[serde(default)]
    pub expires_in_days: Option<i64>,
    #[serde(default)]
    pub scopes: Option<Vec<String>>,
}

/// Response when an API token is created (includes the raw token - only shown once!)
#[derive(Debug, Serialize)]
pub struct ApiTokenCreatedResponse {
    pub uuid: Uuid,
    pub token: String,
    pub token_prefix: String,
    pub name: String,
    pub user_uuid: Uuid,
    pub expires_at: Option<chrono::NaiveDateTime>,
}

/// API token info for listing (no sensitive data)
#[derive(Debug, Serialize)]
pub struct ApiTokenInfo {
    pub uuid: Uuid,
    pub token_prefix: String,
    pub name: String,
    pub user_uuid: Uuid,
    pub user_name: String,
    pub scopes: Vec<String>,
    pub created_at: chrono::NaiveDateTime,
    pub created_by_name: String,
    pub expires_at: Option<chrono::NaiveDateTime>,
    pub revoked_at: Option<chrono::NaiveDateTime>,
    pub last_used_at: Option<chrono::NaiveDateTime>,
}
