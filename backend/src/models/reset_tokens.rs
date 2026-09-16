use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ===== RESET TOKENS MODELS =====

/// Generic reset tokens for password resets, MFA resets, and other temporary tokens
#[derive(Debug, Serialize, Deserialize, Identifiable, Queryable)]
#[diesel(table_name = crate::schema::reset_tokens)]
#[diesel(primary_key(token_hash))]
pub struct ResetToken {
    pub token_hash: String,
    pub user_uuid: Uuid,
    pub token_type: String,
    pub ip_address: Option<ipnetwork::IpNetwork>,
    pub user_agent: Option<String>,
    pub created_at: chrono::NaiveDateTime,
    pub expires_at: chrono::NaiveDateTime,
    pub used_at: Option<chrono::NaiveDateTime>,
    pub is_used: bool,
    pub metadata: Option<serde_json::Value>,
}

/// New reset token for creation
#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::reset_tokens)]
pub struct NewResetToken<'a> {
    pub token_hash: &'a str,
    pub user_uuid: Uuid,
    pub token_type: &'a str,
    pub ip_address: Option<ipnetwork::IpNetwork>,
    pub user_agent: Option<&'a str>,
    pub expires_at: chrono::DateTime<chrono::Utc>,
    pub metadata: Option<serde_json::Value>,
}
