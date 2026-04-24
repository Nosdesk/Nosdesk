//! Trust-chain resolution for a verified signature envelope.
//!
//! `signing.rs` handles the cryptographic primitive (is the signature
//! valid for this pubkey?). This module handles the policy question:
//! does the instance *trust* the pubkey that produced the valid
//! signature, and under which tier? Separate from the signing module
//! on purpose, since this layer touches the database and the
//! compiled-in root key whereas signing stays pure.

use crate::db::DbConnection;
use crate::repository::plugin_publishers;
use crate::services::plugins::signing::{self, sources, SignatureEnvelope, VerifiedArchive};

/// Fields a verified install writes to a `plugins` row. Shared by the
/// zip upload handler and filesystem provisioning so both paths stamp
/// identical provenance on the row.
#[derive(Debug, Clone)]
pub struct PluginSignerFields {
    pub trust_level: String,
    pub signer_pubkey: Option<String>,
    pub signer_source: Option<String>,
    pub signature_metadata: Option<serde_json::Value>,
}

impl PluginSignerFields {
    /// Build from a verified archive + its resolved tier.
    pub fn from_verified(verified: &VerifiedArchive, tier: &ResolvedTier) -> Self {
        Self {
            trust_level: tier.trust_level().to_string(),
            signer_pubkey: Some(verified.envelope.signer_pubkey.clone()),
            signer_source: Some(tier.signer_source().to_string()),
            signature_metadata: serde_json::to_value(&verified.envelope).ok(),
        }
    }

    /// Dev-mode unsigned install (debug builds only). No pubkey to
    /// anchor to; the `dev` source tag makes the row filterable in
    /// the admin UI.
    pub fn dev_mode() -> Self {
        Self {
            trust_level: "local".to_string(),
            signer_pubkey: None,
            signer_source: Some(sources::DEV.to_string()),
            signature_metadata: None,
        }
    }
}

/// Outcome of resolving a signature envelope against the instance's
/// configured roots.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedTier {
    /// Matches the compiled-in `NOSDESK_ROOT_PUBKEY`.
    Official,
    /// Matches a non-revoked entry in `plugin_trusted_publishers` with
    /// `tier = 'verified'`.
    Verified,
    /// Matches a non-revoked entry in `plugin_trusted_publishers` with
    /// `tier = 'community'`.
    Community,
    /// Matches this instance's `plugin_local_signing_key.pubkey`.
    Local,
}

impl ResolvedTier {
    /// The string stored in `plugins.trust_level`.
    pub fn trust_level(&self) -> &'static str {
        match self {
            ResolvedTier::Official => "official",
            ResolvedTier::Verified => "verified",
            ResolvedTier::Community => "community",
            ResolvedTier::Local => "local",
        }
    }

    /// The string stored in `plugins.signer_source`.
    pub fn signer_source(&self) -> &'static str {
        match self {
            ResolvedTier::Official => sources::NOSDESK_ROOT,
            ResolvedTier::Verified => sources::VERIFIED_PUBLISHER,
            ResolvedTier::Community => sources::COMMUNITY_PUBLISHER,
            ResolvedTier::Local => sources::LOCAL,
        }
    }
}

#[derive(Debug)]
pub enum TrustError {
    /// Pubkey didn't match any configured root.
    UntrustedSigner,
    /// Publisher exists but `revoked_at` is set.
    RevokedPublisher,
    /// The envelope's claimed `signer_source` tag (e.g.
    /// "verified-publisher") disagrees with the tier the verifier
    /// actually resolved the pubkey to. Defence in depth: the
    /// resolved tier is always authoritative, but a mismatch
    /// signals either a publisher misconfig or a tampering attempt
    /// and we'd rather reject than silently correct.
    SourceMismatch {
        claimed: String,
        resolved: &'static str,
    },
    Db(diesel::result::Error),
}

impl std::fmt::Display for TrustError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TrustError::UntrustedSigner => {
                write!(f, "signer pubkey is not registered as a trusted publisher")
            }
            TrustError::RevokedPublisher => {
                write!(f, "signer pubkey belongs to a revoked publisher")
            }
            TrustError::SourceMismatch { claimed, resolved } => write!(
                f,
                "envelope claims signer_source={claimed:?} but pubkey resolves to {resolved:?}"
            ),
            // Avoid leaking raw Diesel error text (column names,
            // parameter values) to API clients. The actual error
            // goes to the server log via the caller.
            TrustError::Db(_) => write!(f, "trust-chain lookup failed"),
        }
    }
}

impl std::error::Error for TrustError {}

impl From<diesel::result::Error> for TrustError {
    fn from(value: diesel::result::Error) -> Self {
        TrustError::Db(value)
    }
}

/// Walk the trust chain for a verified envelope. Callers must have
/// already confirmed the signature is valid via
/// [`signing::verify_archive`]; this only answers the policy question.
///
/// After resolving the tier, enforce that the envelope's *claimed*
/// `signer_source` matches. The claim is advisory (the resolved tier
/// is authoritative), but rejecting mismatches catches misconfigured
/// signers early and denies attackers a confusing state where the
/// envelope advertises one tier while the row stores another.
pub fn resolve(
    conn: &mut DbConnection,
    envelope: &SignatureEnvelope,
) -> Result<ResolvedTier, TrustError> {
    let tier = resolve_inner(conn, envelope)?;
    let expected = tier.signer_source();
    if envelope.signer_source != expected {
        return Err(TrustError::SourceMismatch {
            claimed: envelope.signer_source.clone(),
            resolved: expected,
        });
    }
    Ok(tier)
}

fn resolve_inner(
    conn: &mut DbConnection,
    envelope: &SignatureEnvelope,
) -> Result<ResolvedTier, TrustError> {
    if signing::is_nosdesk_root(&envelope.signer_pubkey) {
        return Ok(ResolvedTier::Official);
    }

    if let Some(row) = plugin_publishers::get_local_signing_key(conn)? {
        if row.pubkey == envelope.signer_pubkey {
            return Ok(ResolvedTier::Local);
        }
    }

    if let Some(publisher) = plugin_publishers::find_publisher_by_pubkey(conn, &envelope.signer_pubkey)? {
        if publisher.revoked_at.is_some() {
            return Err(TrustError::RevokedPublisher);
        }
        return match publisher.tier.as_str() {
            "verified" => Ok(ResolvedTier::Verified),
            "community" => Ok(ResolvedTier::Community),
            _ => Err(TrustError::UntrustedSigner),
        };
    }

    Err(TrustError::UntrustedSigner)
}
