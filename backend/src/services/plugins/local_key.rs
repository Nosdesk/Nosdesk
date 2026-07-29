//! Instance-local plugin signing key management.
//!
//! On first boot we generate an Ed25519 keypair and persist the PKCS8
//! private blob encrypted at rest. AES-256-GCM shares the master key
//! with MFA secrets, so we bind a context string into the AEAD's AAD
//! (`AAD_CONTEXT` below): a `plugin_local_signing_key.encrypted_sk`
//! blob won't decrypt under the MFA context and vice versa, giving a
//! hard cross-purpose separation even while the key is shared.
//!
//! The public key becomes the anchor for the `local` trust tier:
//! plugins signed with this key are accepted by the CLI install path
//! on this instance only.

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use tracing::{info, warn};

use crate::db::DbConnection;
use crate::models::NewLocalSigningKey;
use crate::repository::plugin_publishers;
use crate::services::plugins::signing;
use crate::utils::encryption;

/// Failure modes for local signing key bootstrap. Typed (rather
/// than `anyhow::Error`) so callers can distinguish cause without
/// pattern-matching on Display strings.
#[derive(Debug)]
pub enum LocalKeyError {
    DbLoad(diesel::result::Error),
    DbInsert(diesel::result::Error),
    Generate(String),
    Encrypt(String),
    Decrypt(String),
}

impl std::fmt::Display for LocalKeyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DbLoad(e) => write!(f, "load local signing key: {e}"),
            Self::DbInsert(e) => write!(f, "insert local signing key: {e}"),
            Self::Generate(m) => write!(f, "generate local signing keypair: {m}"),
            Self::Encrypt(m) => write!(f, "encrypt local signing key at rest: {m}"),
            Self::Decrypt(m) => write!(f, "decrypt local signing key: {m}"),
        }
    }
}

impl std::error::Error for LocalKeyError {}

/// AAD context bound into every ciphertext stored in
/// `plugin_local_signing_key.encrypted_sk`. Versioned so a future
/// scheme bump is a clean break. NEVER reuse this string for any
/// other encryption purpose, and don't change it without a
/// migration — changing it invalidates all existing ciphertexts.
const AAD_CONTEXT: &[u8] = b"nosdesk.plugin.local_sk.v1";

/// Return the existing local key's fingerprint + base64 pubkey, or
/// generate a fresh keypair, persist it encrypted, and return the
/// newly-created values. Idempotent: safe to call on every boot.
pub fn ensure_local_signing_key(conn: &mut DbConnection) -> Result<LocalKeyInfo, LocalKeyError> {
    if let Some(row) =
        plugin_publishers::get_local_signing_key(conn).map_err(LocalKeyError::DbLoad)?
    {
        info!(
            fingerprint = %row.fingerprint,
            "Plugin local signing key loaded"
        );
        return Ok(LocalKeyInfo {
            pubkey_b64: row.pubkey,
            fingerprint: row.fingerprint,
            created: false,
        });
    }

    let (pkcs8, pubkey) =
        signing::generate_keypair().map_err(|e| LocalKeyError::Generate(e.to_string()))?;
    let pubkey_b64 = BASE64.encode(&pubkey);
    let fingerprint = signing::fingerprint(&pubkey);

    let kr = encryption::keyring();
    let encrypted = kr
        .encrypt(&pkcs8, AAD_CONTEXT)
        .map_err(|e| LocalKeyError::Encrypt(e.to_string()))?;
    let kek_id = kr.current_version() as i16;

    plugin_publishers::insert_local_signing_key(
        conn,
        NewLocalSigningKey {
            id: 1,
            pubkey: pubkey_b64.clone(),
            encrypted_sk: encrypted,
            encrypted_sk_kek_id: kek_id,
            fingerprint: fingerprint.clone(),
        },
    )
    .map_err(LocalKeyError::DbInsert)?;

    warn!(
        fingerprint = %fingerprint,
        "Generated new plugin local signing key. Record this fingerprint, it pins the CLI-install trust root for this instance."
    );

    Ok(LocalKeyInfo {
        pubkey_b64,
        fingerprint,
        created: true,
    })
}

#[derive(Debug, Clone)]
pub struct LocalKeyInfo {
    pub pubkey_b64: String,
    pub fingerprint: String,
    /// `true` if this call generated the key (first boot); `false` if
    /// it was already present.
    pub created: bool,
}

/// Load and decrypt the instance's local signing key as PKCS8 bytes, for signing
/// a plugin at the `local` tier (the CLI `sign --local` path). Generates the key
/// first on a fresh instance so signing works immediately. The bytes are the raw
/// PKCS8 an `Ed25519KeyPair` is built from; they are secret and zeroized on drop.
pub fn load_local_signing_pkcs8(
    conn: &mut DbConnection,
) -> Result<zeroize::Zeroizing<Vec<u8>>, LocalKeyError> {
    ensure_local_signing_key(conn)?;
    let row = plugin_publishers::get_local_signing_key(conn)
        .map_err(LocalKeyError::DbLoad)?
        .ok_or(LocalKeyError::DbLoad(diesel::result::Error::NotFound))?;
    encryption::keyring()
        .decrypt(&row.encrypted_sk, AAD_CONTEXT)
        .map_err(|e| LocalKeyError::Decrypt(e.to_string()))
}
