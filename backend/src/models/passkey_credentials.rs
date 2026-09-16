use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ===== PASSKEY CREDENTIAL MODELS =====

/// One row of `passkey_credentials`. Each WebAuthn credential is
/// stored as its own row with a unique `credential_id`, replacing
/// the earlier JSONB-blob-on-users design.
#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable, Selectable)]
#[diesel(table_name = crate::schema::passkey_credentials)]
pub struct PasskeyCredential {
    pub id: Uuid,
    pub user_uuid: Uuid,
    pub credential_id: String,
    pub name: String,
    pub credential: serde_json::Value,
    pub transports: Vec<Option<String>>,
    pub backup_eligible: bool,
    pub backup_state: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub last_used_at: Option<chrono::DateTime<chrono::Utc>>,
    /// WebAuthn sign counter (u32 in the spec, stored as i8/BIGINT to
    /// fit Postgres's signed integer types). Bumped on every
    /// successful authentication by `webauthn::update_credential_post_auth`.
    /// The counter inside `credential` JSONB is kept in lockstep so
    /// the library's regression check works on rehydrated `Passkey`.
    pub sign_count: i64,
    /// Set when the WebAuthn `backup_state` flag flips between
    /// authentications (e.g. credential synced to a new ecosystem).
    /// Pairs with the `passkey_backup_state_changed` security event.
    pub backup_state_changed_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Insertable)]
#[diesel(table_name = crate::schema::passkey_credentials)]
pub struct NewPasskeyCredential {
    pub user_uuid: Uuid,
    pub credential_id: String,
    pub name: String,
    pub credential: serde_json::Value,
    pub transports: Vec<Option<String>>,
    pub backup_eligible: bool,
    pub backup_state: bool,
    // `sign_count` defaults to 0 at the column level; no need to set
    // explicitly on insert. A freshly-registered credential's first
    // authentication will bump it past 0 (or stay at 0 for the
    // counter-less authenticator case, per WebAuthn §6.1.1).
}

/// Subset for updating mutable fields. Two clusters:
///   - `name` — user-initiated rename.
///   - `last_used_at`, `credential`, `sign_count`, `backup_state`,
///     `backup_state_changed_at` — written together by the post-auth
///     hook (`webauthn::update_credential_post_auth`) so the
///     denormalised columns and the JSONB blob never disagree.
#[derive(Debug, Default, AsChangeset)]
#[diesel(table_name = crate::schema::passkey_credentials)]
pub struct PasskeyCredentialUpdate {
    pub name: Option<String>,
    pub last_used_at: Option<Option<chrono::DateTime<chrono::Utc>>>,
    /// Full JSONB blob — written when the embedded counter changes
    /// so the library's `Passkey` deserialisation sees the current
    /// value.
    pub credential: Option<serde_json::Value>,
    /// Denormalised sign counter for fast clone-detection queries.
    pub sign_count: Option<i64>,
    /// Current backup state from the most recent assertion; flips
    /// here drive the `passkey_backup_state_changed` security event.
    pub backup_state: Option<bool>,
    /// Stamped at the moment a backup_state flip is observed.
    pub backup_state_changed_at: Option<Option<chrono::DateTime<chrono::Utc>>>,
}
