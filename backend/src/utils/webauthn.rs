//! WebAuthn/Passkey Utilities
//!
//! Provides WebAuthn configuration, credential storage, and challenge management
//! for passwordless authentication via passkeys.

use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use std::env;
use url::Url;
use uuid::Uuid;
use base64::Engine;
use webauthn_rs::prelude::*;

use crate::db::DbConnection;
use crate::models::User;
use crate::repository;

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
// Credential Storage Types (JSONB)
// =============================================================================

/// A stored passkey credential in the user's passkey_credentials JSONB
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredPasskeyCredential {
    /// Base64URL-encoded credential ID
    pub id: String,
    /// User-friendly name for this passkey
    pub name: String,
    /// The full Passkey credential from webauthn-rs (serialized)
    #[serde(with = "passkey_serde")]
    pub credential: Passkey,
    /// Supported transports (internal, usb, nfc, ble, hybrid)
    pub transports: Vec<String>,
    /// When this passkey was registered
    pub created_at: DateTime<Utc>,
    /// When this passkey was last used for authentication
    pub last_used_at: Option<DateTime<Utc>>,
    /// Whether this credential is backup eligible
    pub backup_eligible: bool,
    /// Whether this credential is currently backed up
    pub backup_state: bool,
}

/// Custom serde module for Passkey serialization
mod passkey_serde {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use webauthn_rs::prelude::Passkey;

    pub fn serialize<S>(passkey: &Passkey, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        // Serialize to JSON value first, then to the target format
        let json = serde_json::to_value(passkey)
            .map_err(serde::ser::Error::custom)?;
        json.serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Passkey, D::Error>
    where
        D: Deserializer<'de>,
    {
        let json = serde_json::Value::deserialize(deserializer)?;
        serde_json::from_value(json).map_err(serde::de::Error::custom)
    }
}

/// User's complete passkey data structure stored in JSONB
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UserPasskeyData {
    pub credentials: Vec<StoredPasskeyCredential>,
}

impl UserPasskeyData {
    /// Create empty passkey data
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self { credentials: vec![] }
    }

    /// Add a new credential
    pub fn add_credential(&mut self, credential: StoredPasskeyCredential) {
        self.credentials.push(credential);
    }

    /// Remove a credential by ID
    pub fn remove_credential(&mut self, credential_id: &str) -> bool {
        let len_before = self.credentials.len();
        self.credentials.retain(|c| c.id != credential_id);
        self.credentials.len() < len_before
    }

    /// Find a credential by ID
    pub fn find_credential(&self, credential_id: &str) -> Option<&StoredPasskeyCredential> {
        self.credentials.iter().find(|c| c.id == credential_id)
    }

    /// Find a credential by ID (mutable)
    pub fn find_credential_mut(&mut self, credential_id: &str) -> Option<&mut StoredPasskeyCredential> {
        self.credentials.iter_mut().find(|c| c.id == credential_id)
    }

    /// Get all Passkey credentials for authentication
    pub fn get_passkeys(&self) -> Vec<Passkey> {
        self.credentials.iter().map(|c| c.credential.clone()).collect()
    }

    /// Update a credential's last_used_at timestamp after successful authentication
    #[allow(dead_code)]
    pub fn update_last_used(&mut self, credential_id: &str) {
        if let Some(cred) = self.find_credential_mut(credential_id) {
            cred.last_used_at = Some(Utc::now());
        }
    }
}

// =============================================================================
// User Passkey Operations
// =============================================================================

/// Get user's passkey data from the database
pub fn get_user_passkey_data(user: &User) -> UserPasskeyData {
    user.passkey_credentials
        .as_ref()
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

/// Save user's passkey data to the database
pub fn save_user_passkey_data(
    conn: &mut DbConnection,
    user_uuid: &Uuid,
    data: &UserPasskeyData,
) -> Result<()> {
    let json_value = serde_json::to_value(data)?;
    repository::update_user_passkey_credentials(conn, user_uuid, Some(json_value))
        .map_err(|e| anyhow!("Failed to save passkey data: {:?}", e))?;
    Ok(())
}

/// Check if user has any passkeys registered
#[allow(dead_code)]
pub fn user_has_passkeys(user: &User) -> bool {
    !get_user_passkey_data(user).credentials.is_empty()
}

/// Get the number of passkeys registered for a user
pub fn get_passkey_count(user: &User) -> usize {
    get_user_passkey_data(user).credentials.len()
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

/// Parse credential ID from base64url string
#[allow(dead_code)]
pub fn credential_id_from_string(s: &str) -> Result<CredentialID> {
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(s)
        .map_err(|e| anyhow!("Invalid credential ID: {}", e))?;
    Ok(CredentialID::from(bytes))
}

/// Maximum number of passkeys allowed per user
pub const MAX_PASSKEYS_PER_USER: usize = 10;

/// Check if user can add more passkeys
pub fn can_add_passkey(user: &User) -> bool {
    get_passkey_count(user) < MAX_PASSKEYS_PER_USER
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
