//! Trust-chain resolution for a verified signature envelope.
//!
//! `signing.rs` handles the cryptographic primitive (is the signature
//! valid for this pubkey?). This module handles the policy question:
//! does the instance *trust* the pubkey that produced the valid
//! signature, and under which tier? Separate from the signing module
//! on purpose, since this layer touches the database and the
//! compiled-in root key whereas signing stays pure.

use tracing::{error, info, warn};

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
    /// Resolved tier isn't in this instance's allowed-tier policy
    /// (e.g. a verified-publisher plugin on an instance that locks
    /// down to official+local until the Phase 4 sandbox ships).
    DisallowedTier {
        tier: &'static str,
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
            TrustError::DisallowedTier { tier } => write!(
                f,
                "plugin tier {tier:?} is not allowed on this instance \
                 (set NOSDESK_ALLOWED_PLUGIN_TIERS to enable it)"
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
        // Log the raw Diesel error before wrapping it in the
        // client-facing variant. The `Display` impl intentionally
        // returns a generic string so column names / input values
        // don't leak to API responses; the detail is captured here
        // where only the server log sees it.
        error!(error = %value, "Plugin trust-chain DB lookup failed");
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
///
/// Emits one tracing event per call with a stable `outcome` label
/// so log pipelines can build per-outcome counters / alerts (the
/// review's day-1 observability requirement for the verifier).
pub fn resolve(
    conn: &mut DbConnection,
    envelope: &SignatureEnvelope,
) -> Result<ResolvedTier, TrustError> {
    let result = resolve_logged(conn, envelope);
    log_resolve_outcome(envelope, &result);
    result
}

fn resolve_logged(
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
    enforce_tier_policy(&tier)?;
    Ok(tier)
}

/// Stable `outcome=` label per resolution outcome. Success cases
/// carry the resolved tier; error cases carry the rejection reason.
/// Kept as `&'static str` so log queries match exact strings.
fn outcome_label(result: &Result<ResolvedTier, TrustError>) -> &'static str {
    match result {
        Ok(ResolvedTier::Official) => "resolved_official",
        Ok(ResolvedTier::Verified) => "resolved_verified",
        Ok(ResolvedTier::Community) => "resolved_community",
        Ok(ResolvedTier::Local) => "resolved_local",
        Err(TrustError::UntrustedSigner) => "untrusted_signer",
        Err(TrustError::RevokedPublisher) => "revoked_publisher",
        Err(TrustError::SourceMismatch { .. }) => "source_mismatch",
        Err(TrustError::DisallowedTier { .. }) => "disallowed_tier",
        Err(TrustError::Db(_)) => "db_error",
    }
}

fn log_resolve_outcome(envelope: &SignatureEnvelope, result: &Result<ResolvedTier, TrustError>) {
    // Fingerprint the claimed pubkey for the log only. Decoding can
    // fail if the envelope is malformed, but at this point the
    // signing verifier has already accepted the bytes so it should
    // be well-formed; fall back to a stable placeholder otherwise.
    let fp = signing::decode_pubkey_fingerprint(&envelope.signer_pubkey)
        .unwrap_or_else(|| "(unparsed)".to_string());
    let outcome = outcome_label(result);
    match result {
        Ok(_) => info!(
            outcome,
            fingerprint = %fp,
            claimed_source = %envelope.signer_source,
            "Plugin trust resolved"
        ),
        Err(_) => warn!(
            outcome,
            fingerprint = %fp,
            claimed_source = %envelope.signer_source,
            "Plugin trust rejected"
        ),
    }
}

/// Reject tiers that the instance has disabled.
///
/// `NOSDESK_ALLOWED_PLUGIN_TIERS` (comma-separated `official` /
/// `verified` / `community` / `local`) overrides the default. The
/// shipping default in production is `"official,local"` —
/// `verified` and `community` plugins both run third-party code
/// in-process with full DOM access, which isn't safe until the
/// Phase 4 sandbox ships. Operators who need the wider tiers can
/// opt in explicitly; the choice stays visible in the env config.
fn enforce_tier_policy(tier: &ResolvedTier) -> Result<(), TrustError> {
    let allowed = allowed_tiers();
    if allowed.contains(tier.trust_level()) {
        return Ok(());
    }
    Err(TrustError::DisallowedTier {
        tier: tier.trust_level(),
    })
}

/// Default-deny posture: when the env var is unset, treat the
/// build profile as the deciding signal. Release binaries lock to
/// `official+local` until an operator says otherwise; debug
/// builds keep the developer-friendly full set.
fn allowed_tiers() -> std::collections::HashSet<&'static str> {
    use std::collections::HashSet;
    let raw = std::env::var("NOSDESK_ALLOWED_PLUGIN_TIERS").ok();
    let configured: Option<Vec<&str>> = raw.as_deref().map(|s| {
        s.split(',')
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .collect()
    });
    let from_env: HashSet<&'static str> = configured
        .map(|tokens| {
            tokens
                .into_iter()
                .filter_map(|t| match t {
                    "official" => Some("official"),
                    "verified" => Some("verified"),
                    "community" => Some("community"),
                    "local" => Some("local"),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default();
    if !from_env.is_empty() {
        return from_env;
    }
    if cfg!(debug_assertions) {
        ["official", "verified", "community", "local"]
            .into_iter()
            .collect()
    } else {
        ["official", "local"].into_iter().collect()
    }
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

    if let Some(publisher) =
        plugin_publishers::find_publisher_by_pubkey(conn, &envelope.signer_pubkey)?
    {
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
