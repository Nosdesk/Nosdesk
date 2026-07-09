//! WebAuthn/Passkey Utilities
//!
//! Provides WebAuthn configuration, credential storage, and challenge management
//! for passwordless authentication via passkeys.

use anyhow::{anyhow, Result};
use base64::Engine;
use chrono::{DateTime, Utc};
use redis::AsyncCommands;
use std::env;
use url::Url;
use uuid::Uuid;
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
        // Fail-closed: an unset / non-canonical ENVIRONMENT requires explicit
        // WEBAUTHN_RP_ID / WEBAUTHN_RP_ORIGIN rather than defaulting to the
        // insecure localhost values (matches config_utils::assume_production,
        // the single source of truth for hardened posture).
        let is_production = crate::config_utils::assume_production();

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

        let rp_name = env::var("WEBAUTHN_RP_NAME").unwrap_or_else(|_| "Nosdesk".to_string());

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

        let rp_origin =
            Url::parse(&rp_origin_str).map_err(|e| anyhow!("Invalid WEBAUTHN_RP_ORIGIN: {}", e))?;

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
            "WebAuthn configured: rp_id={}, rp_origin={}, production={}",
            rp_id,
            rp_origin,
            is_production
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

        builder
            .build()
            .map_err(|e| anyhow!("Failed to build WebAuthn: {:?}", e))
    }
}

// =============================================================================
// Per-request verifier (per-workspace RP)
// =============================================================================
//
// Hosted multi-tenant: RP ID + origin are the host the request is served on
// (`mercury.nosdesk.dev` or a tenant's custom domain), so passkeys are scoped
// per host/workspace and custom domains work without a shared RP ID (which
// would leak credentials across tenants). Self-hosted single-tenant: the
// env-configured RP (`WEBAUTHN_RP_ID` / `WEBAUTHN_RP_ORIGIN`). Building per
// request is cheap.

/// Build the WebAuthn verifier for the current request. In **hosted** mode the
/// RP ID/origin is the request's (validated) per-workspace host, so passkeys are
/// scoped per host / custom domain. In **self-hosted** mode it is the env config
/// (`WEBAUTHN_RP_ID` / `WEBAUTHN_RP_ORIGIN`). Returns an owned `Webauthn`; cheap
/// to construct per call. Errors surface per request (not a startup panic), so a
/// hosted deploy needs no `WEBAUTHN_RP_*` env at all.
///
/// The mode gate is essential: self-hosted requests **always** carry the
/// bootstrap `WorkspaceContext`, so gating on that alone would force the per-host
/// branch for every self-host deploy — deriving `https://{Host}` and thereby
/// ignoring `WEBAUTHN_RP_*` and breaking passkeys over plain HTTP / localhost /
/// behind a reverse proxy that rewrites Host.
pub fn webauthn_for_request(req: &actix_web::HttpRequest) -> Result<Webauthn> {
    use crate::middleware::workspace_context::DeploymentMode;
    if DeploymentMode::current() == DeploymentMode::Hosted {
        if let Some(host) = request_workspace_host(req) {
            return build_webauthn_for_host(&host);
        }
    }
    WebAuthnConfig::from_env()?.build_webauthn()
}

/// The host this request is actually served on (= the WebAuthn RP ID), when it
/// resolved to a hosted workspace. This is the **request host**, not the
/// workspace's canonical-preference host: `rp_origin` must equal the browser's
/// real origin, which differs from the canonical host when a workspace has a
/// custom domain but is reached via its subdomain. The `WorkspaceContext` gate
/// means the middleware already validated this host belongs to a workspace, and
/// the browser independently enforces RP-ID/origin matching. Mirrors OIDC's
/// `oauth_callback_redirect_uri`, the sibling request-origin concern.
fn request_workspace_host(req: &actix_web::HttpRequest) -> Option<String> {
    use actix_web::HttpMessage;
    // Gate: only build a per-workspace verifier for a resolved hosted workspace.
    // Drop the extensions borrow before reading headers.
    if req
        .extensions()
        .get::<crate::extractors::WorkspaceContext>()
        .is_none()
    {
        return None;
    }
    let host = req
        .headers()
        .get(actix_web::http::header::HOST)
        .and_then(|h| h.to_str().ok())?;
    let host = host.split(':').next().unwrap_or(host).trim().to_lowercase();
    (!host.is_empty()).then_some(host)
}

/// Build a verifier whose RP ID and origin are the workspace canonical `host`.
fn build_webauthn_for_host(host: &str) -> Result<Webauthn> {
    let rp_origin = Url::parse(&format!("https://{host}"))
        .map_err(|e| anyhow!("Invalid WebAuthn origin for host {host}: {e}"))?;
    let rp_name = env::var("WEBAUTHN_RP_NAME").unwrap_or_else(|_| "Nosdesk".to_string());
    WebauthnBuilder::new(host, &rp_origin)
        .map_err(|e| anyhow!("Failed to create WebAuthn builder: {e:?}"))?
        .rp_name(&rp_name)
        .build()
        .map_err(|e| anyhow!("Failed to build WebAuthn: {e:?}"))
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
        self.credentials
            .iter()
            .map(|c| c.credential.clone())
            .collect()
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
        transports: credential
            .transports
            .iter()
            .map(|t| Some(t.clone()))
            .collect(),
        backup_eligible: credential.backup_eligible,
        backup_state: credential.backup_state,
    };
    repo::create(conn, new).map_err(|e| anyhow!("Failed to insert passkey: {:?}", e))?;
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

/// Outcome of [`update_credential_post_auth`]. Returned so the
/// caller can emit a security event when the WebAuthn `backup_state`
/// flag flips (a possible clone-detection signal per WebAuthn L3
/// §6.1.3 — the credential may now be backed up to a different
/// ecosystem than the one that registered it).
#[derive(Debug, Default)]
pub struct CredentialPostAuthOutcome {
    /// `Some((previous, current))` when `backup_state` differed from
    /// the previously-stored value, `None` otherwise. The caller
    /// should record a `passkey_backup_state_changed` security
    /// event when present.
    pub backup_state_flip: Option<(bool, bool)>,
    /// Previously-stored sign counter; informational, useful for
    /// telemetry / audit. Caller can compare against the new value
    /// (`auth_result.counter()`) to confirm the bump landed.
    pub previous_sign_count: i64,
    /// New sign counter that was persisted.
    pub new_sign_count: i64,
}

/// Persist the WebAuthn `AuthenticationResult` back to the
/// credential row after a successful login. Writes the bumped sign
/// counter (both the denormalised `sign_count` column and the
/// counter embedded inside the `credential` JSONB blob so library
/// rehydration stays consistent), the current `backup_state` /
/// `backup_eligible` flags, and `last_used_at`.
///
/// Without this hook, the WebAuthn clone-detection property
/// (assertion counter must exceed the stored counter) is inoperative
/// — the library checks the counter on each authentication, but if
/// the stored value never advances past the registration baseline,
/// the check has no teeth. Calling this after every successful
/// `finish_passkey_authentication` /
/// `finish_discoverable_authentication` closes the gap.
///
/// Returns a [`CredentialPostAuthOutcome`] describing observable
/// state changes (currently: `backup_state` flips) so the handler
/// can emit the matching security event. Failure to find the
/// credential is mapped to `Ok(default)` rather than `Err` — the
/// credential lookup happened upstream during the ceremony so a
/// late-race delete is unusual, and dropping the post-auth update
/// shouldn't block an otherwise-successful login.
///
/// Counter regression (asserted counter ≤ stored) is enforced by
/// webauthn-rs inside `finish_*_authentication` itself; a
/// successful `AuthenticationResult` already implies the regression
/// check passed. We don't need to re-check here, only persist.
pub fn update_credential_post_auth(
    conn: &mut DbConnection,
    user_uuid: &Uuid,
    auth_result: &AuthenticationResult,
) -> Result<CredentialPostAuthOutcome> {
    // `cred_id()` returns `&HumanBinaryData` (raw bytes); the
    // `credential_id` column stores the canonical base64url form.
    // HumanBinaryData has no Display impl, so encode via the base64
    // crate directly.
    let credential_id =
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(auth_result.cred_id().as_ref());
    let row = match repo::find_by_credential_id(conn, &credential_id) {
        Ok(Some(row)) => row,
        Ok(None) => {
            tracing::warn!(
                user_uuid = %user_uuid,
                credential_id = %credential_id,
                "post-auth credential lookup returned no row; skipping update"
            );
            return Ok(CredentialPostAuthOutcome::default());
        }
        Err(e) => {
            return Err(anyhow!(
                "Failed to look up credential for post-auth update: {:?}",
                e
            ))
        }
    };

    // Defensive sanity check: the credential we're about to update
    // really belongs to the user we're authenticating. The handler
    // already establishes this via the credential→user lookup
    // earlier in the flow, but mismatches here would indicate a
    // larger flow break (or a races-with-delete-and-recreate
    // pathological case) and we'd rather refuse than corrupt.
    if row.user_uuid != *user_uuid {
        return Err(anyhow!(
            "post-auth credential user mismatch (stored {:?}, expected {:?})",
            row.user_uuid,
            user_uuid
        ));
    }

    let new_sign_count = auth_result.counter() as i64;
    let new_backup_state = auth_result.backup_state();
    let new_backup_eligible = auth_result.backup_eligible();
    let _ = new_backup_eligible; // tracked on the row at registration; not mutated here

    let backup_state_flip = if row.backup_state != new_backup_state {
        Some((row.backup_state, new_backup_state))
    } else {
        None
    };

    // Rewrite the embedded counter inside the `credential` JSONB so
    // a subsequent rehydration of the `Passkey` sees the bumped
    // value (the library's regression check reads it from there).
    // The blob's shape is `{"cred": {"counter": <u32>, ...}, ...}`
    // per webauthn-rs's Passkey serialisation; we touch only that
    // one field, leaving everything else byte-identical.
    let updated_credential_json = sync_counter_into_credential_blob(row.credential, new_sign_count);

    let change = PasskeyCredentialUpdate {
        name: None,
        last_used_at: Some(Some(Utc::now())),
        credential: Some(updated_credential_json),
        sign_count: Some(new_sign_count),
        backup_state: Some(new_backup_state),
        backup_state_changed_at: backup_state_flip.map(|_| Some(Utc::now())),
    };

    repo::update_for_user(conn, user_uuid, &credential_id, change)
        .map_err(|e| anyhow!("Failed to persist post-auth credential update: {:?}", e))?;

    Ok(CredentialPostAuthOutcome {
        backup_state_flip,
        previous_sign_count: row.sign_count,
        new_sign_count,
    })
}

/// Sync the bumped counter into the embedded `cred.counter` of the
/// stored credential JSONB. Best-effort: if the blob shape diverges
/// from what we expect (a manual schema change, a webauthn-rs
/// serialisation format bump), we return the original blob untouched
/// so the column-level `sign_count` still holds the authoritative
/// value and the next library rehydration of `Passkey` falls back
/// to whatever the blob says. The library's regression check then
/// degrades to "blob counter" but the security event for backup-
/// state flips still fires off the column.
fn sync_counter_into_credential_blob(
    mut blob: serde_json::Value,
    new_counter: i64,
) -> serde_json::Value {
    if let Some(cred) = blob.get_mut("cred").and_then(|c| c.as_object_mut()) {
        cred.insert(
            "counter".to_string(),
            serde_json::Value::Number(serde_json::Number::from(new_counter)),
        );
    }
    blob
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
pub async fn store_registration_state(user_uuid: &Uuid, state: &PasskeyRegistration) -> Result<()> {
    let redis_url = get_redis_url();
    let client = redis::Client::open(redis_url.as_str())?;
    let mut con = client.get_multiplexed_async_connection().await?;

    let key = format!("webauthn:reg_challenge:{user_uuid}");
    let state_json = serde_json::to_string(state)?;

    con.set_ex::<_, _, ()>(&key, state_json, CHALLENGE_TTL_SECONDS)
        .await?;

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
pub async fn store_authentication_state(email: &str, state: &PasskeyAuthentication) -> Result<()> {
    let redis_url = get_redis_url();
    let client = redis::Client::open(redis_url.as_str())?;
    let mut con = client.get_multiplexed_async_connection().await?;

    // Hash email for privacy
    let email_hash = hash_email(email);
    let key = format!("webauthn:auth_challenge:{email_hash}");
    let state_json = serde_json::to_string(state)?;

    con.set_ex::<_, _, ()>(&key, state_json, CHALLENGE_TTL_SECONDS)
        .await?;

    tracing::debug!(
        "Stored authentication challenge for email hash {}",
        email_hash
    );
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

    tracing::debug!(
        "Retrieved authentication challenge for email hash {}",
        email_hash
    );
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

    con.set_ex::<_, _, ()>(&key, state_json, CHALLENGE_TTL_SECONDS)
        .await?;

    tracing::debug!(
        "Stored discoverable auth challenge for session {}",
        session_id
    );
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

    tracing::debug!(
        "Retrieved discoverable auth challenge for session {}",
        session_id
    );
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
        assert_eq!(
            generate_passkey_name(Some("Mozilla/5.0 (iPhone; CPU iPhone OS 17_0")),
            "iPhone"
        );
        assert_eq!(
            generate_passkey_name(Some("Mozilla/5.0 (Macintosh; Intel Mac OS X")),
            "Mac"
        );
        assert_eq!(
            generate_passkey_name(Some("Mozilla/5.0 (Windows NT 10.0")),
            "Windows"
        );
        assert_eq!(generate_passkey_name(None), "Passkey");
    }

    // ---- sync_counter_into_credential_blob -------------------------
    //
    // Pure JSON-rewriter tests for the helper that keeps the embedded
    // `cred.counter` in lockstep with the denormalised `sign_count`
    // column. The library's `Passkey::deserialize` reads its counter
    // from this blob, so a divergence between the two would let a
    // rehydrated Passkey present a stale counter to the next
    // verification — re-opening the clone-detection gap this whole
    // change-set exists to close.

    #[test]
    fn sync_counter_bumps_existing_counter() {
        let blob = serde_json::json!({
            "cred": { "counter": 42, "cred_id": "abc", "user_verified": true },
            "other_field": "preserved",
        });
        let bumped = sync_counter_into_credential_blob(blob, 100);
        assert_eq!(bumped["cred"]["counter"], serde_json::json!(100));
        assert_eq!(bumped["cred"]["cred_id"], serde_json::json!("abc"));
        assert_eq!(bumped["cred"]["user_verified"], serde_json::json!(true));
        assert_eq!(bumped["other_field"], serde_json::json!("preserved"));
    }

    #[test]
    fn sync_counter_inserts_when_missing() {
        // Some `Passkey` shapes omit `counter` until the first
        // authentication. Insert rather than ignore so the next
        // rehydration sees the column-authoritative value.
        let blob = serde_json::json!({
            "cred": { "cred_id": "abc" },
        });
        let bumped = sync_counter_into_credential_blob(blob, 7);
        assert_eq!(bumped["cred"]["counter"], serde_json::json!(7));
    }

    #[test]
    fn sync_counter_leaves_unexpected_shape_alone() {
        // If the blob's shape diverges from what we expect (a manual
        // schema change, a webauthn-rs serialisation bump), we return
        // it untouched and let the column-level `sign_count` carry
        // the authoritative value. Better than corrupting the blob.
        let blob = serde_json::json!({ "no_cred_key": "here" });
        let bumped = sync_counter_into_credential_blob(blob.clone(), 999);
        assert_eq!(bumped, blob);

        let blob = serde_json::json!({ "cred": "not an object" });
        let bumped = sync_counter_into_credential_blob(blob.clone(), 999);
        assert_eq!(bumped, blob);

        let blob = serde_json::json!([1, 2, 3]);
        let bumped = sync_counter_into_credential_blob(blob.clone(), 999);
        assert_eq!(bumped, blob);
    }

    #[test]
    fn sync_counter_handles_zero() {
        // u32 → i64 round-trip at the boundary value. Some
        // authenticators don't implement a counter and always return
        // 0; the helper must accept that without losing information.
        let blob = serde_json::json!({ "cred": { "counter": 5 } });
        let bumped = sync_counter_into_credential_blob(blob, 0);
        assert_eq!(bumped["cred"]["counter"], serde_json::json!(0));
    }

    mod request_workspace_host {
        use super::super::request_workspace_host;
        use crate::extractors::WorkspaceContext;
        use actix_web::{test::TestRequest, HttpMessage, HttpRequest};

        fn req(host: &str, ctx: Option<WorkspaceContext>) -> HttpRequest {
            let req = TestRequest::default()
                .insert_header(("host", host))
                .to_http_request();
            if let Some(c) = ctx {
                req.extensions_mut().insert(c);
            }
            req
        }

        fn ctx(slug: &str, custom_domain: Option<&str>) -> WorkspaceContext {
            WorkspaceContext {
                workspace_id: 1,
                workspace_uuid: uuid::Uuid::nil(),
                slug: slug.to_string(),
                name: "Test".to_string(),
                custom_domain: custom_domain.map(str::to_string),
                organisation_id: None,
            }
        }

        #[test]
        fn uses_request_host_for_a_resolved_workspace() {
            let r = req("mercury.nosdesk.dev", Some(ctx("mercury", None)));
            assert_eq!(
                request_workspace_host(&r),
                Some("mercury.nosdesk.dev".to_string())
            );
        }

        #[test]
        fn uses_request_host_not_canonical_when_custom_domain_differs() {
            // Workspace has a custom domain but the browser is on the subdomain.
            // rp_origin must equal the browser origin, so the RP host is the
            // request host, NOT the canonical (custom-domain) host.
            let r = req(
                "mercury.nosdesk.dev",
                Some(ctx("mercury", Some("help.acme.com"))),
            );
            assert_eq!(
                request_workspace_host(&r),
                Some("mercury.nosdesk.dev".to_string())
            );
        }

        #[test]
        fn none_without_workspace_context() {
            // Self-hosted / unresolved: fall back to the env-configured RP.
            let r = req("mercury.nosdesk.dev", None);
            assert_eq!(request_workspace_host(&r), None);
        }

        #[test]
        fn strips_port_and_lowercases() {
            let r = req("Mercury.Nosdesk.Dev:8443", Some(ctx("mercury", None)));
            assert_eq!(
                request_workspace_host(&r),
                Some("mercury.nosdesk.dev".to_string())
            );
        }
    }
}
