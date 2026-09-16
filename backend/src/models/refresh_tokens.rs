use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Refresh token for JWT token rotation
#[derive(Debug, Serialize, Deserialize, Identifiable, Queryable)]
#[diesel(table_name = crate::schema::refresh_tokens)]
pub struct RefreshToken {
    pub id: i32,
    pub token_hash: String,
    pub user_uuid: Uuid,
    pub created_at: chrono::NaiveDateTime,
    pub expires_at: chrono::NaiveDateTime,
    pub revoked_at: Option<chrono::NaiveDateTime>,
    pub session_id: Option<Uuid>,
    pub family_id: Uuid,
    pub is_used: bool,
    pub used_at: Option<chrono::NaiveDateTime>,
    pub replaced_by_hash: Option<String>,
    pub grace_expires_at: Option<chrono::NaiveDateTime>,
    /// The realm this token was minted for. Compared for equality by the
    /// exchanging endpoint; see [`REFRESH_AUDIENCE_AGENT`].
    pub audience: String,
}

/// New refresh token for creation
#[derive(Debug, Insertable)]
#[diesel(table_name = crate::schema::refresh_tokens)]
pub struct NewRefreshToken {
    pub token_hash: String,
    pub user_uuid: Uuid,
    pub expires_at: chrono::NaiveDateTime,
    pub session_id: Option<Uuid>,
    pub family_id: Uuid,
    /// Which realm minted this token: [`REFRESH_AUDIENCE_AGENT`] or
    /// [`REFRESH_AUDIENCE_PORTAL`]. Required, with no default, so the compiler
    /// enumerates every mint site rather than letting a new one inherit
    /// whichever realm happens to be more privileged.
    pub audience: String,
}

/// A staff session: the agent app, and everything under `/api`.
pub const REFRESH_AUDIENCE_AGENT: &str = "agent";
/// A customer-portal session, established by magic link.
///
/// A portal credential must never be exchangeable for an agent one. The check
/// is string equality against the audience the exchanging endpoint serves, not
/// a parse with a fallback, so an unrecognised value fails closed.
pub const REFRESH_AUDIENCE_PORTAL: &str = "portal";
