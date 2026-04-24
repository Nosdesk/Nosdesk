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

use anyhow::{anyhow, Context, Result};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use ring::signature::{Ed25519KeyPair, KeyPair};
use tracing::{info, warn};

use crate::db::DbConnection;
use crate::models::NewLocalSigningKey;
use crate::repository::plugin_publishers;
use crate::services::plugins::signing;
use crate::utils::encryption;

/// AAD context bound into every ciphertext stored in
/// `plugin_local_signing_key.encrypted_sk`. Versioned so a future
/// scheme bump is a clean break. NEVER reuse this string for any
/// other encryption purpose, and don't change it without a
/// migration — changing it invalidates all existing ciphertexts.
const AAD_CONTEXT: &[u8] = b"nosdesk.plugin.local_sk.v1";

/// Return the existing local key's fingerprint + base64 pubkey, or
/// generate a fresh keypair, persist it encrypted, and return the
/// newly-created values. Idempotent: safe to call on every boot.
pub fn ensure_local_signing_key(conn: &mut DbConnection) -> Result<LocalKeyInfo> {
    if let Some(row) = plugin_publishers::get_local_signing_key(conn)
        .map_err(|e| anyhow!("load local signing key: {e}"))?
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

    let (pkcs8, pubkey) = signing::generate_keypair()
        .map_err(|e| anyhow!("generate local signing keypair: {e}"))?;
    let pubkey_b64 = BASE64.encode(&pubkey);
    let fingerprint = signing::fingerprint(&pubkey);

    let encrypted = encryption::encrypt_bytes_with_aad(&pkcs8, AAD_CONTEXT)
        .context("encrypt local signing key at rest")?;

    plugin_publishers::insert_local_signing_key(
        conn,
        NewLocalSigningKey {
            id: 1,
            pubkey: pubkey_b64.clone(),
            encrypted_sk: encrypted,
            fingerprint: fingerprint.clone(),
        },
    )
    .map_err(|e| anyhow!("insert local signing key: {e}"))?;

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

/// Load and decrypt the local signing keypair for use by the CLI
/// install path. Returns `None` if the key hasn't been bootstrapped
/// yet (which shouldn't happen in normal operation since startup
/// calls `ensure_local_signing_key`).
pub fn load_local_keypair(conn: &mut DbConnection) -> Result<Option<Ed25519KeyPair>> {
    let row = match plugin_publishers::get_local_signing_key(conn)
        .map_err(|e| anyhow!("load local signing key: {e}"))?
    {
        Some(r) => r,
        None => return Ok(None),
    };

    let pkcs8 = encryption::decrypt_bytes_with_aad(&row.encrypted_sk, AAD_CONTEXT)
        .context("decrypt local signing key")?;

    let keypair = Ed25519KeyPair::from_pkcs8(&pkcs8)
        .map_err(|e| anyhow!("parse local signing key pkcs8: {e:?}"))?;

    // Sanity-check the decrypted key matches the stored pubkey so a
    // corrupted or swapped ciphertext doesn't silently produce a key
    // that signs under a different pubkey than the UI advertises.
    let derived_pubkey = BASE64.encode(keypair.public_key().as_ref());
    if derived_pubkey != row.pubkey {
        return Err(anyhow!(
            "local signing key pubkey mismatch: stored row's pubkey does not match decrypted keypair"
        ));
    }

    Ok(Some(keypair))
}

#[derive(Debug, Clone)]
pub struct LocalKeyInfo {
    pub pubkey_b64: String,
    pub fingerprint: String,
    /// `true` if this call generated the key (first boot); `false` if
    /// it was already present.
    pub created: bool,
}
