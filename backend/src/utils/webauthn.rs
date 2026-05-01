//! WebAuthn/Passkey Utilities
//!
//! Provides WebAuthn configuration, credential storage, and challenge management
//! for passwordless authentication via passkeys.

use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use redis::AsyncCommands;
use std::env;
use url::Url;
use uuid::Uuid;
use base64::Engine;
use webauthn_rs::prelude::*;

use crate::db::DbConnection;
use crate::models::{NewPasskeyCredential, PasskeyCredential, PasskeyCredentialUpdate};
use crate::repository::passkey_credentials as repo;

use super::rate_limit::get_redis_url;

// =============================================================================
// Configuration
// =============================================================================

/// WebAuthn configuration from environment variables
pub struct WebAuthnConfig {
    pub rp_id: String,
    pub rp_name: String,
    pub rp_origin: Url,
}

impl WebAuthnConfig {
    /// Load WebAuthn configuration from environment variables
    /// In production, all WEBAUTHN_* variables are required.
    /// In development, defaults to localhost values.
    pub fn from_env() -> Result<Self> {
        let environment = env::var("ENVIRONMENT")
            .unwrap_or_else(|_| "development".to_string())
            .to_lowercase();

        let is_production = environment == "production";

        // In production, require explicit configuration
        let rp_id = match env::var("WEBAUTHN_RP_ID") {
            Ok(id) => id,
            Err(_) if is_production => {
                return Err(anyhow!(
                    "WEBAUTHN_RP_ID environment variable is required in production. \
                    Set it to your domain (e.g., 'example.com')"
                ));
            }
            Err(_) => {
                tracing::warn!(
                    "WEBAUTHN_RP_ID not set, defaulting to 'localhost'. \
                    This is insecure for production use."
                );
                "localhost".to_string()
            }
        };

        let rp_name = env::var("WEBAUTHN_RP_NAME")
            .unwrap_or_else(|_| "Nosdesk".to_string());

        let rp_origin_str = match env::var("WEBAUTHN_RP_ORIGIN") {
            Ok(origin) => origin,
            Err(_) if is_production => {
                return Err(anyhow!(
                    "WEBAUTHN_RP_ORIGIN environment variable is required in production. \
                    Set it to your full origin URL (e.g., 'https://example.com')"
                ));
            }
            Err(_) => {
                tracing::warn!(
                    "WEBAUTHN_RP_ORIGIN not set, defaulting to 'http://localhost:5173'. \
                    This is insecure for production use."
                );
                "http://localhost:5173".to_string()
            }
        };

        let rp_origin = Url::parse(&rp_origin_str)
            .map_err(|e| anyhow!("Invalid WEBAUTHN_RP_ORIGIN: {}", e))?;

        // Validate RP ID matches origin host in production
        if is_production {
            let origin_host = rp_origin.host_str().unwrap_or("");
            if !origin_host.ends_with(&rp_id) && origin_host != rp_id {
                return Err(anyhow!(
                    "WEBAUTHN_RP_ID '{}' does not match WEBAUTHN_RP_ORIGIN host '{}'. \
                    RP ID must be the origin's domain or a registrable suffix.",
                    rp_id,
                    origin_host
                ));
            }
        }

        tracing::info!(
            "WebAuthn configured: rp_id={}, rp_origin={}, environment={}",
            rp_id,
            rp_origin,
            environment
        );

        Ok(Self {
            rp_id,
            rp_name,
            rp_origin,
        })
    }

    /// Build a WebAuthn instance from this configuration
    pub fn build_webauthn(&self) -> Result<Webauthn> {
        let builder = WebauthnBuilder::new(&self.rp_id, &self.rp_origin)
            .map_err(|e| anyhow!("Failed to create WebAuthn builder: {:?}", e))?
            .rp_name(&self.rp_name);

        builder.build()
            .map_err(|e| anyhow!("Failed to build WebAuthn: {:?}", e))
    }
}

// Lazy static WebAuthn instance
lazy_static::lazy_static! {
    static ref WEBAUTHN_CONFIG: WebAuthnConfig = WebAuthnConfig::from_env()
        .expect("Failed to load WebAuthn configuration");

    pub static ref WEBAUTHN: Webauthn = WEBAUTHN_CONFIG.build_webauthn()
        .expect("Failed to build WebAuthn instance");
}

// =============================================================================
// Credential Storage
// =============================================================================
//
// Passkey credentials live in their own `passkey_credentials` table,
// one row per WebAuthn credential, keyed by the credential_id. The
// types below adapt that row shape to the in-memory representation
// the WebAuthn flow handlers want, plus convert from/to the
// `webauthn_rs::Passkey` type.

/// A stored passkey credential. Mirrors a row of `passkey_credentials`
/// with the `credential` JSONB rehydrated as a `Passkey`.
#[derive(Debug, Clone)]
pub struct StoredPasskeyCredential {
    pub id: String,
    pub name: String,
    pub credential: Passkey,
    pub transports: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub backup_eligible: bool,
    pub backup_state: bool,
}

impl StoredPasskeyCredential {
    fn from_row(row: PasskeyCredential) -> Result<Self> {
        let credential: Passkey = serde_json::from_value(row.credential)
            .map_err(|e| anyhow!("Failed to deserialize stored Passkey: {:?}", e))?;
        Ok(Self {
            id: row.credential_id,
            name: row.name,
            credential,
            transports: row.transports.into_iter().flatten().collect(),
            created_at: row.created_at,
            last_used_at: row.last_used_at,
            backup_eligible: row.backup_eligible,
            backup_state: row.backup_state,
        })
    }
}

/// All of a user's passkeys, loaded once and held for the duration
/// of a single request. Operations that mutate (add / rename / touch
/// last-used) write back to the table directly via the repo, this
/// type stays read-only.
#[derive(Debug, Clone, Default)]
pub struct UserPasskeyData {
    pub credentials: Vec<StoredPasskeyCredential>,
}

impl UserPasskeyData {
    pub fn find_credential(&self, credential_id: &str) -> Option<&StoredPasskeyCredential> {
        self.credentials.iter().find(|c| c.id == credential_id)
    }

    pub fn get_passkeys(&self) -> Vec<Passkey> {
        self.credentials.iter().map(|c| c.credential.clone()).collect()
    }
}

// =============================================================================
// User Passkey Operations
// =============================================================================

/// Load every credential for a user. Returns an empty bundle when
/// the user has none, never an error from "no rows found".
pub fn load_user_passkey_data(
    conn: &mut DbConnection,
    user_uuid: &Uuid,
) -> Result<UserPasskeyData> {
    let rows = repo::list_for_user(conn, user_uuid)
        .map_err(|e| anyhow!("Failed to load passkeys: {:?}", e))?;
    let credentials = rows
        .into_iter()
        .map(StoredPasskeyCredential::from_row)
        .collect::<Result<Vec<_>>>()?;
    Ok(UserPasskeyData { credentials })
}

/// Insert a newly-registered credential.
pub fn add_credential(
    conn: &mut DbConnection,
    user_uuid: &Uuid,
    credential: &StoredPasskeyCredential,
) -> Result<()> {
    let credential_json = serde_json::to_value(&credential.credential)
        .map_err(|e| anyhow!("Failed to serialize Passkey for storage: {:?}", e))?;
    let new = NewPasskeyCredential {
        user_uuid: *user_uuid,
        credential_id: credential.id.clone(),
        name: credential.name.clone(),
        credential: credential_json,
        transports: credential.transports.iter().map(|t| Some(t.clone())).collect(),
        backup_eligible: credential.backup_eligible,
        backup_state: credential.backup_state,
    };
    repo::create(conn, new)
        .map_err(|e| anyhow!("Failed to insert passkey: {:?}", e))?;
    Ok(())
}

/// Rename a credential. Scoped to user so a stolen credential_id
/// can't be used to rename another user's passkey. Returns
/// `Ok(false)` when no row matched (caller renders 404), `Ok(true)`
/// when the rename took effect.
pub fn rename_credential(
    conn: &mut DbConnection,
    user_uuid: &Uuid,
    credential_id: &str,
    new_name: &str,
) -> Result<bool> {
    let change = PasskeyCredentialUpdate {
        name: Some(new_name.to_string()),
        ..Default::default()
    };
    match repo::update_for_user(conn, user_uuid, credential_id, change) {
        Ok(_) => Ok(true),
        Err(diesel::result::Error::NotFound) => Ok(false),
        Err(e) => Err(anyhow!("Failed to rename passkey: {:?}", e)),
    }
}

/// Stamp `last_used_at` after a successful authentication.
/// Same not-found semantics as [`rename_credential`].
pub fn touch_last_used(
    conn: &mut DbConnection,
    user_uuid: &Uuid,
    credential_id: &str,
) -> Result<bool> {
    let change = PasskeyCredentialUpdate {
        name: None,
        last_used_at: Some(Some(Utc::now())),
    };
    match repo::update_for_user(conn, user_uuid, credential_id, change) {
        Ok(_) => Ok(true),
        Err(diesel::result::Error::NotFound) => Ok(false),
        Err(e) => Err(anyhow!("Failed to touch passkey last_used_at: {:?}", e)),
    }
}

/// Delete a credential. Returns true if a row was actually removed.
pub fn delete_credential(
    conn: &mut DbConnection,
    user_uuid: &Uuid,
    credential_id: &str,
) -> Result<bool> {
    let removed = repo::delete_for_user(conn, user_uuid, credential_id)
        .map_err(|e| anyhow!("Failed to delete passkey: {:?}", e))?;
    Ok(removed > 0)
}

/// Number of passkeys registered for a user. Caller-side helper for
/// the per-user cap; uses a count() query rather than loading rows.
pub fn get_passkey_count(conn: &mut DbConnection, user_uuid: &Uuid) -> Result<usize> {
    let n = repo::count_for_user(conn, user_uuid)
        .map_err(|e| anyhow!("Failed to count passkeys: {:?}", e))?;
    Ok(n as usize)
}

// =============================================================================
// Challenge Storage (Redis)
// =============================================================================

const CHALLENGE_TTL_SECONDS: u64 = 300; // 5 minutes

/// Store registration challenge state in Redis
pub async fn store_registration_state(
    user_uuid: &Uuid,
    state: &PasskeyRegistration,
) -> Result<()> {
    let redis_url = get_redis_url();
    let client = redis::Client::open(redis_url.as_str())?;
    let mut con = client.get_multiplexed_async_connection().await?;

    let key = format!("webauthn:reg_challenge:{user_uuid}");
    let state_json = serde_json::to_string(state)?;

    con.set_ex::<_, _, ()>(&key, state_json, CHALLENGE_TTL_SECONDS).await?;

    tracing::debug!("Stored registration challenge for user {}", user_uuid);
    Ok(())
}

/// Retrieve and delete registration challenge state from Redis
pub async fn get_registration_state(user_uuid: &Uuid) -> Result<PasskeyRegistration> {
    let redis_url = get_redis_url();
    let client = redis::Client::open(redis_url.as_str())?;
    let mut con = client.get_multiplexed_async_connection().await?;

    let key = format!("webauthn:reg_challenge:{user_uuid}");

    // Get and delete atomically
    let state_json: Option<String> = con.get_del(&key).await?;

    let state_json = state_json.ok_or_else(|| anyhow!("No registration challenge found"))?;
    let state: PasskeyRegistration = serde_json::from_str(&state_json)?;

    tracing::debug!("Retrieved registration challenge for user {}", user_uuid);
    Ok(state)
}

/// Store authentication challenge state in Redis
pub async fn store_authentication_state(
    email: &str,
    state: &PasskeyAuthentication,
) -> Result<()> {
    let redis_url = get_redis_url();
    let client = redis::Client::open(redis_url.as_str())?;
    let mut con = client.get_multiplexed_async_connection().await?;

    // Hash email for privacy
    let email_hash = hash_email(email);
    let key = format!("webauthn:auth_challenge:{email_hash}");
    let state_json = serde_json::to_string(state)?;

    con.set_ex::<_, _, ()>(&key, state_json, CHALLENGE_TTL_SECONDS).await?;

    tracing::debug!("Stored authentication challenge for email hash {}", email_hash);
    Ok(())
}

/// Retrieve and delete authentication challenge state from Redis
pub async fn get_authentication_state(email: &str) -> Result<PasskeyAuthentication> {
    let redis_url = get_redis_url();
    let client = redis::Client::open(redis_url.as_str())?;
    let mut con = client.get_multiplexed_async_connection().await?;

    let email_hash = hash_email(email);
    let key = format!("webauthn:auth_challenge:{email_hash}");

    // Get and delete atomically
    let state_json: Option<String> = con.get_del(&key).await?;

    let state_json = state_json.ok_or_else(|| anyhow!("No authentication challenge found"))?;
    let state: PasskeyAuthentication = serde_json::from_str(&state_json)?;

    tracing::debug!("Retrieved authentication challenge for email hash {}", email_hash);
    Ok(state)
}

/// Hash email for Redis key (privacy protection)
fn hash_email(email: &str) -> String {
    use ring::digest::{digest, SHA256};
    let hash = digest(&SHA256, email.to_lowercase().as_bytes());
    hex::encode(hash.as_ref())
}

/// Generate a unique session ID for discoverable authentication
pub fn generate_auth_session_id() -> String {
    use rand::Rng;
    let bytes: [u8; 16] = rand::thread_rng().gen();
    hex::encode(bytes)
}

/// Store discoverable authentication challenge state in Redis (by session ID)
/// Used for usernameless passkey login
pub async fn store_discoverable_auth_state(
    session_id: &str,
    state: &DiscoverableAuthentication,
) -> Result<()> {
    let redis_url = get_redis_url();
    let client = redis::Client::open(redis_url.as_str())?;
    let mut con = client.get_multiplexed_async_connection().await?;

    let key = format!("webauthn:discoverable_auth:{session_id}");
    let state_json = serde_json::to_string(state)?;

    con.set_ex::<_, _, ()>(&key, state_json, CHALLENGE_TTL_SECONDS).await?;

    tracing::debug!("Stored discoverable auth challenge for session {}", session_id);
    Ok(())
}

/// Retrieve and delete discoverable authentication challenge state from Redis
pub async fn get_discoverable_auth_state(session_id: &str) -> Result<DiscoverableAuthentication> {
    let redis_url = get_redis_url();
    let client = redis::Client::open(redis_url.as_str())?;
    let mut con = client.get_multiplexed_async_connection().await?;

    let key = format!("webauthn:discoverable_auth:{session_id}");

    // Get and delete atomically
    let state_json: Option<String> = con.get_del(&key).await?;

    let state_json = state_json.ok_or_else(|| anyhow!("No discoverable auth challenge found"))?;
    let state: DiscoverableAuthentication = serde_json::from_str(&state_json)?;

    tracing::debug!("Retrieved discoverable auth challenge for session {}", session_id);
    Ok(state)
}

// =============================================================================
// Utility Functions
// =============================================================================

/// Convert credential ID bytes to base64url string
pub fn credential_id_to_string(id: &CredentialID) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(id.as_ref())
}

/// Maximum number of passkeys allowed per user
pub const MAX_PASSKEYS_PER_USER: usize = 10;

/// Check if user can add more passkeys.
pub fn can_add_passkey(conn: &mut DbConnection, user_uuid: &Uuid) -> Result<bool> {
    Ok(get_passkey_count(conn, user_uuid)? < MAX_PASSKEYS_PER_USER)
}

/// Generate a default passkey name based on user agent
pub fn generate_passkey_name(user_agent: Option<&str>) -> String {
    if let Some(ua) = user_agent {
        let ua_lower = ua.to_lowercase();
        if ua_lower.contains("iphone") {
            "iPhone".to_string()
        } else if ua_lower.contains("ipad") {
            "iPad".to_string()
        } else if ua_lower.contains("mac") {
            if ua_lower.contains("safari") && !ua_lower.contains("chrome") {
                "Mac (Safari)".to_string()
            } else {
                "Mac".to_string()
            }
        } else if ua_lower.contains("windows") {
            "Windows".to_string()
        } else if ua_lower.contains("android") {
            "Android".to_string()
        } else if ua_lower.contains("linux") {
            "Linux".to_string()
        } else {
            "Security Key".to_string()
        }
    } else {
        "Passkey".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_email() {
        let hash1 = hash_email("test@example.com");
        let hash2 = hash_email("TEST@EXAMPLE.COM");
        assert_eq!(hash1, hash2); // Should be case-insensitive
        assert_eq!(hash1.len(), 64); // SHA-256 produces 32 bytes = 64 hex chars
    }

    #[test]
    fn test_generate_passkey_name() {
        assert_eq!(generate_passkey_name(Some("Mozilla/5.0 (iPhone; CPU iPhone OS 17_0")), "iPhone");
        assert_eq!(generate_passkey_name(Some("Mozilla/5.0 (Macintosh; Intel Mac OS X")), "Mac");
        assert_eq!(generate_passkey_name(Some("Mozilla/5.0 (Windows NT 10.0")), "Windows");
        assert_eq!(generate_passkey_name(None), "Passkey");
    }
}
