//! Plugin signing and verification primitives.
//!
//! Scope:
//!   - Canonical digest of a zip archive's contents, stable across
//!     repackagings that preserve the file set + bytes.
//!   - Ed25519 sign / verify over that digest.
//!   - Signature envelope serialisation for the `nosdesk-signature.json`
//!     entry embedded in the archive.
//!
//! No DB access, no filesystem work beyond what `zip` does on its
//! byte slice. Callers (provisioning, upload handler, CLI) compose
//! this with their own trust-chain resolution. That keeps the crypto
//! surface small and the unit tests pure.

use std::io::Read;

use ring::digest::{Context, SHA256};
use ring::rand::SystemRandom;
use ring::signature::{Ed25519KeyPair, KeyPair, UnparsedPublicKey, ED25519};
use serde::{Deserialize, Serialize};

/// Filename of the signature envelope inside a signed plugin zip.
/// Kept as a const so sign and verify agree on the one file that
/// must NOT contribute to the canonical digest.
pub const SIGNATURE_FILE: &str = "nosdesk-signature.json";

/// Current envelope schema version. Bump on breaking changes to
/// the signed fields; verifiers refuse unknown versions but
/// accept any version in [`SUPPORTED_ENVELOPE_VERSIONS`] so older
/// signed plugins keep verifying through schema transitions.
///
/// v1 -> v2 (2026-05-19): added `not_before`/`not_after` to the
/// envelope and the canonical signed bytes, so the signature
/// binds the time window when present. Existing v1 envelopes
/// continue to verify without expiry enforcement.
pub const ENVELOPE_VERSION: u8 = 2;

/// Versions a verifier will accept. Producers always emit
/// [`ENVELOPE_VERSION`]; the verifier widens to the historical
/// list so a customer's already-installed plugins don't break on
/// upgrade.
pub const SUPPORTED_ENVELOPE_VERSIONS: &[u8] = &[1, 2];

/// Hard cap on any single decompressed archive entry. Plugin bundles
/// are expected in the tens of KB; 1 MB is headroom for future minified
/// frameworks without letting one entry exhaust the worker.
pub const MAX_ENTRY_SIZE: u64 = 1024 * 1024;

/// Hard cap on total decompressed bytes across all entries in an
/// archive. Stops zip bombs: the HTTP layer caps the ciphertext at
/// 2 MB but deflate ratios of 1000:1 are achievable.
pub const MAX_TOTAL_SIZE: u64 = 8 * 1024 * 1024;

/// Hard cap on the signature envelope JSON before it's handed to
/// serde. The envelope is pulled from attacker-controlled bytes
/// *before* any signature check, so give the parser a small, fixed
/// budget. A realistic envelope is a few hundred bytes.
pub const MAX_ENVELOPE_SIZE: usize = 64 * 1024;

/// Hard cap on the raw signed-zip blob size, enforced at every
/// boundary that accepts one (HTTP upload handler, filesystem
/// provisioning). Decouples outer-file limits from inner decompressed
/// limits; an attacker can't feed in a 2 GB ciphertext just to force
/// us to scan it.
pub const MAX_ARCHIVE_SIZE: usize = 2 * 1024 * 1024;

/// Base64 Ed25519 pubkey of the Nosdesk root signing key, baked in at
/// compile time via the `NOSDESK_ROOT_PUBKEY` env var. `None` in local
/// dev builds where the env var is unset, in which case official-tier
/// installs are refused at runtime. Never fall back to a hard-coded
/// key, since that weakens the trust root for anyone who forgets to
/// set the env.
pub fn root_pubkey() -> Option<&'static str> {
    option_env!("NOSDESK_ROOT_PUBKEY")
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

/// `true` if `pubkey_b64` matches the compiled-in Nosdesk root pubkey.
/// Returns `false` when no root is configured, which correctly fails
/// closed for any attempted official-tier install.
pub fn is_nosdesk_root(pubkey_b64: &str) -> bool {
    matches!(root_pubkey(), Some(root) if root == pubkey_b64)
}

/// Labels for the authority chain a signer's pubkey resolved through.
/// Stored in the envelope and echoed into `plugins.signer_source` at
/// install time so dashboards can group by trust tier.
pub mod sources {
    pub const NOSDESK_ROOT: &str = "nosdesk-root";
    pub const VERIFIED_PUBLISHER: &str = "verified-publisher";
    pub const COMMUNITY_PUBLISHER: &str = "community-publisher";
    pub const LOCAL: &str = "local";
    /// Dev-mode installs that skipped signature verification entirely.
    /// Never written into a signature envelope; used only as a
    /// `plugins.signer_source` value so the admin UI can flag them.
    pub const DEV: &str = "dev";
}

/// The `nosdesk-signature.json` envelope that lives inside a signed
/// plugin zip. All fields are signed except `signature` itself (the
/// signature is over `signed_digest` concatenated with a context
/// string, see [`canonical_sign_input`]).
///
/// `deny_unknown_fields` stops attackers smuggling extra keys into
/// `signature_metadata` (which we persist verbatim and render in the
/// admin UI) and keeps v2 forward-compat bumps noisy instead of
/// silently parsed.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SignatureEnvelope {
    pub version: u8,
    /// Always `"ed25519"` for now; placeholder for future alg rotation.
    pub algorithm: String,
    /// Base64-encoded 32-byte Ed25519 public key.
    pub signer_pubkey: String,
    /// Which authority chain the signer claims — verifiers still
    /// check the pubkey against the real chain, this is just a hint
    /// for UI + log lines.
    pub signer_source: String,
    /// RFC 3339 timestamp the signature was produced. Not used for
    /// validation (we don't enforce freshness yet); captured for
    /// audit.
    pub signed_at: String,
    /// Hex-encoded SHA-256 of the canonical archive digest. Verifier
    /// recomputes this from the zip's non-signature entries and
    /// refuses the archive if it doesn't match.
    pub signed_digest: String,
    /// Optional RFC 3339 timestamp before which the signature is
    /// not valid. v2 only; v1 envelopes omit. When set the verifier
    /// refuses the archive if the current time is earlier than
    /// `not_before`. Bound by the signature: removing or rewriting
    /// the field invalidates the envelope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub not_before: Option<String>,
    /// Optional RFC 3339 timestamp after which the signature is no
    /// longer valid. v2 only; v1 envelopes omit. When set the
    /// verifier refuses the archive if the current time is later
    /// than `not_after`. Bound by the signature like `not_before`.
    /// Lets publishers ship signatures with a known shelf life so
    /// a stolen key can only abuse it within the window the key
    /// owner intended.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub not_after: Option<String>,
    /// Base64-encoded 64-byte Ed25519 signature.
    pub signature: String,
}

/// Result of successful verification — the envelope, the canonical
/// digest that was signed, and the inner file set. Callers then
/// resolve the pubkey against whichever authority chain is relevant
/// to their install path.
#[derive(Debug)]
pub struct VerifiedArchive {
    pub envelope: SignatureEnvelope,
    pub digest_hex: String,
    pub files: Vec<ArchiveEntry>,
}

/// A single file extracted from the archive, minus the signature
/// envelope itself. Exposed so install handlers can read
/// `manifest.json` / `bundle.js` without re-parsing the zip.
#[derive(Debug, Clone)]
pub struct ArchiveEntry {
    pub name: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug)]
pub enum SigningError {
    ArchiveFormat(String),
    /// Two zip entries share a name. The canonical digest covers
    /// both but downstream `ZipArchive::by_name` would only see one
    /// of them, a classic parser-differential. Refuse the archive.
    DuplicateEntry(String),
    /// A single entry exceeds `MAX_ENTRY_SIZE` or the sum of entries
    /// exceeds `MAX_TOTAL_SIZE` — i.e. zip-bomb territory.
    DecompressedTooLarge,
    /// The `nosdesk-signature.json` blob itself is too large to be
    /// worth parsing before authentication.
    EnvelopeTooLarge,
    MissingSignature,
    MalformedEnvelope(String),
    UnsupportedVersion(u8),
    UnsupportedAlgorithm(String),
    TamperedArchive,
    BadSignature,
    InvalidPubkey,
    InvalidSignatureField,
    /// `not_before` / `not_after` malformed: must be RFC 3339.
    MalformedValidity(String),
    /// Current time is before the envelope's `not_before`. The
    /// signature is real, it just isn't active yet.
    SignatureNotYetValid {
        not_before: String,
    },
    /// Current time is past the envelope's `not_after`. The
    /// signature was valid at issue, the publisher's intent was a
    /// shorter shelf life than now.
    SignatureExpired {
        not_after: String,
    },
    KeyGen(String),
}

impl std::fmt::Display for SigningError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ArchiveFormat(m) => write!(f, "archive is not a readable zip: {m}"),
            Self::DuplicateEntry(n) => write!(f, "archive contains duplicate entry {n:?}"),
            Self::DecompressedTooLarge => write!(
                f,
                "decompressed archive exceeds size limits (entry {MAX_ENTRY_SIZE} / total {MAX_TOTAL_SIZE} bytes)"
            ),
            Self::EnvelopeTooLarge => {
                write!(f, "signature envelope exceeds {MAX_ENVELOPE_SIZE} bytes")
            }
            Self::MissingSignature => write!(
                f,
                "archive is missing {SIGNATURE_FILE}; unsigned archives must go through the CLI with --dev-mode"
            ),
            Self::MalformedEnvelope(m) => write!(f, "signature envelope is malformed: {m}"),
            Self::UnsupportedVersion(v) => {
                write!(f, "unsupported envelope version {v} (expected {ENVELOPE_VERSION})")
            }
            Self::UnsupportedAlgorithm(a) => write!(f, "unsupported algorithm: {a}"),
            Self::TamperedArchive => {
                write!(f, "signed_digest does not match recomputed archive digest")
            }
            Self::BadSignature => write!(f, "ed25519 signature verification failed"),
            Self::InvalidPubkey => write!(f, "signer_pubkey is not a valid base64 32-byte value"),
            Self::InvalidSignatureField => {
                write!(f, "signature field is not a valid base64 64-byte value")
            }
            Self::MalformedValidity(m) => {
                write!(f, "envelope not_before/not_after is not RFC 3339: {m}")
            }
            Self::SignatureNotYetValid { not_before } => {
                write!(f, "signature not yet valid (not_before={not_before})")
            }
            Self::SignatureExpired { not_after } => {
                write!(f, "signature expired (not_after={not_after})")
            }
            Self::KeyGen(m) => write!(f, "failed to generate signing key: {m}"),
        }
    }
}

impl std::error::Error for SigningError {}

/// Compute the canonical digest of a set of archive entries, suitable
/// for signing. The digest is stable under zip re-packaging that
/// preserves filenames and bytes: we sort entries by name and hash
/// `name_len(u32-be) || name || content_len(u64-be) || content` for
/// each, feeding every entry's per-entry hash into a final SHA-256.
/// The signature envelope itself is NOT included — it'd be circular.
pub fn canonical_digest(entries: &[ArchiveEntry]) -> [u8; 32] {
    let mut sorted: Vec<&ArchiveEntry> = entries
        .iter()
        .filter(|e| e.name != SIGNATURE_FILE)
        .collect();
    sorted.sort_by(|a, b| a.name.cmp(&b.name));

    let mut outer = Context::new(&SHA256);
    for entry in sorted {
        let name_bytes = entry.name.as_bytes();
        outer.update(&(name_bytes.len() as u32).to_be_bytes());
        outer.update(name_bytes);
        outer.update(&(entry.bytes.len() as u64).to_be_bytes());
        outer.update(&entry.bytes);
    }
    let out = outer.finish();
    let mut result = [0u8; 32];
    result.copy_from_slice(out.as_ref());
    result
}

/// The exact byte sequence an Ed25519 signature covers. A
/// domain-separator prefix keeps future uses of the same key (e.g.
/// registry-index signing via `nosdesk-registry-v1:`) from
/// accidentally producing cross-protocol signature confusion.
///
/// Shape per version:
///
/// - v1: `b"nosdesk-plugin-v1:" || digest_hex`
/// - v2: `b"nosdesk-plugin-v2:" || digest_hex || b"|" || not_before
///   || b"|" || not_after`, where missing expiry fields are
///   represented as empty strings. Including them in the signed
///   bytes binds the time window to the signature, so an attacker
///   can't strip / forge expiry without invalidating the envelope.
///
/// Unknown versions are rejected by the caller before reaching
/// here; this function is internal-only.
fn canonical_sign_input(
    version: u8,
    digest_hex: &str,
    not_before: Option<&str>,
    not_after: Option<&str>,
) -> Vec<u8> {
    let mut out = Vec::with_capacity(96 + digest_hex.len());
    match version {
        1 => {
            out.extend_from_slice(b"nosdesk-plugin-v1:");
            out.extend_from_slice(digest_hex.as_bytes());
        }
        _ => {
            // Defaults to v2 shape for any version we accept that
            // isn't v1. SUPPORTED_ENVELOPE_VERSIONS is the gate;
            // canonical_sign_input never runs on a rejected version.
            out.extend_from_slice(b"nosdesk-plugin-v2:");
            out.extend_from_slice(digest_hex.as_bytes());
            out.push(b'|');
            out.extend_from_slice(not_before.unwrap_or("").as_bytes());
            out.push(b'|');
            out.extend_from_slice(not_after.unwrap_or("").as_bytes());
        }
    }
    out
}

/// Pull every file out of a zip blob into in-memory `ArchiveEntry`s.
///
/// Enforces both per-entry (`MAX_ENTRY_SIZE`) and total-archive
/// (`MAX_TOTAL_SIZE`) decompressed-byte budgets to shut down zip
/// bombs: the HTTP layer only caps ciphertext, and deflate ratios
/// over 1000:1 are achievable. `ZipFile::size()` is untrusted header
/// metadata, so we both reject oversized headers up-front AND cap
/// the actual read via `Read::take`.
///
/// Rejects archives with duplicate entry names, where the canonical
/// digest and later name-based lookups would disagree.
pub fn read_archive(bytes: &[u8]) -> Result<Vec<ArchiveEntry>, SigningError> {
    let reader = std::io::Cursor::new(bytes);
    let mut zip =
        zip::ZipArchive::new(reader).map_err(|e| SigningError::ArchiveFormat(e.to_string()))?;
    let mut entries = Vec::with_capacity(zip.len());
    let mut seen = std::collections::HashSet::with_capacity(zip.len());
    let mut total: u64 = 0;
    for i in 0..zip.len() {
        let file = zip
            .by_index(i)
            .map_err(|e| SigningError::ArchiveFormat(e.to_string()))?;
        if !file.is_file() {
            continue;
        }
        let name = file.name().to_string();
        if !seen.insert(name.clone()) {
            return Err(SigningError::DuplicateEntry(name));
        }

        // Header-declared size sanity check. An honest archive with a
        // >1MB entry is already unusual for a plugin, so refuse early
        // without even starting the decompressor.
        if file.size() > MAX_ENTRY_SIZE {
            return Err(SigningError::DecompressedTooLarge);
        }

        // Cap `with_capacity` independently of the header hint and
        // bound the read itself through `take`. If the actual content
        // exceeds the limit the read stops at MAX_ENTRY_SIZE + 1 and
        // we reject; never trust `file.size()` for memory bounds.
        let capacity = file.size().min(MAX_ENTRY_SIZE) as usize;
        let mut buf = Vec::with_capacity(capacity);
        let read = file
            .take(MAX_ENTRY_SIZE + 1)
            .read_to_end(&mut buf)
            .map_err(|e| SigningError::ArchiveFormat(e.to_string()))?;
        if read as u64 > MAX_ENTRY_SIZE {
            return Err(SigningError::DecompressedTooLarge);
        }

        total = total
            .checked_add(read as u64)
            .ok_or(SigningError::DecompressedTooLarge)?;
        if total > MAX_TOTAL_SIZE {
            return Err(SigningError::DecompressedTooLarge);
        }

        entries.push(ArchiveEntry { name, bytes: buf });
    }
    Ok(entries)
}

/// Look up a file by name inside a verified archive. Case-sensitive,
/// exact match. Returns the bytes the signature actually covers.
/// Callers should prefer this over re-opening the raw zip: it
/// guarantees installs consume the same bytes that verification did.
pub fn find_entry<'a>(files: &'a [ArchiveEntry], name: &str) -> Option<&'a [u8]> {
    files
        .iter()
        .find(|e| e.name == name)
        .map(|e| e.bytes.as_slice())
}

/// Full verification: read the archive, pull the envelope, recompute
/// the digest, verify the Ed25519 signature, and hand back the
/// verified archive for the caller to resolve against an authority
/// chain. Returns `VerifiedArchive` with the pubkey still in the
/// envelope — trust resolution is NOT this module's job.
///
/// Every call emits exactly one tracing event with a stable
/// `outcome` field so log aggregation can build per-outcome
/// counters without parsing the human-readable message.
pub fn verify_archive(bytes: &[u8]) -> Result<VerifiedArchive, SigningError> {
    let result = (|| -> Result<VerifiedArchive, SigningError> {
        let entries = read_archive(bytes)?;
        verify_entries(entries)
    })();
    log_verify_outcome(&result);
    result
}

/// Stable `outcome=` label per `SigningError` variant. Kept as a
/// `&'static str` so log queries can match exact strings without
/// worrying about Display drift; the human-facing message lives in
/// [`SigningError::fmt`].
fn outcome_label(err: &SigningError) -> &'static str {
    match err {
        SigningError::ArchiveFormat(_) => "archive_format",
        SigningError::DuplicateEntry(_) => "duplicate_entry",
        SigningError::DecompressedTooLarge => "decompressed_too_large",
        SigningError::EnvelopeTooLarge => "envelope_too_large",
        SigningError::MissingSignature => "missing_signature",
        SigningError::MalformedEnvelope(_) => "malformed_envelope",
        SigningError::UnsupportedVersion(_) => "unsupported_version",
        SigningError::UnsupportedAlgorithm(_) => "unsupported_algorithm",
        SigningError::TamperedArchive => "tampered_archive",
        SigningError::BadSignature => "bad_signature",
        SigningError::InvalidPubkey => "invalid_pubkey",
        SigningError::InvalidSignatureField => "invalid_signature_field",
        SigningError::MalformedValidity(_) => "malformed_validity",
        SigningError::SignatureNotYetValid { .. } => "signature_not_yet_valid",
        SigningError::SignatureExpired { .. } => "signature_expired",
        SigningError::KeyGen(_) => "key_gen",
    }
}

fn log_verify_outcome(result: &Result<VerifiedArchive, SigningError>) {
    match result {
        Ok(v) => {
            // Decode + fingerprint for the log only; if the pubkey
            // string is malformed the verifier would have errored
            // before reaching here, so default is safe.
            let fp = decode_pubkey_fingerprint(&v.envelope.signer_pubkey).unwrap_or_default();
            tracing::info!(
                outcome = "verified",
                signer_source = %v.envelope.signer_source,
                fingerprint = %fp,
                "Plugin signature verified"
            );
        }
        Err(e) => {
            tracing::warn!(
                outcome = outcome_label(e),
                error = %e,
                "Plugin signature rejected"
            );
        }
    }
}

/// Verify a pre-materialised set of archive entries. Used when the
/// source isn't a zip: the filesystem provisioner walks a plugin
/// directory into `ArchiveEntry` values and feeds them through this
/// path so zip and directory installs share identical trust logic.
pub fn verify_entries(entries: Vec<ArchiveEntry>) -> Result<VerifiedArchive, SigningError> {
    let envelope_bytes = entries
        .iter()
        .find(|e| e.name == SIGNATURE_FILE)
        .ok_or(SigningError::MissingSignature)?
        .bytes
        .clone();

    // The envelope is parsed from attacker-controlled bytes *before*
    // any signature check. Give serde a small, fixed budget so a
    // hostile file can't exhaust CPU or stack before we reject it.
    if envelope_bytes.len() > MAX_ENVELOPE_SIZE {
        return Err(SigningError::EnvelopeTooLarge);
    }

    let envelope: SignatureEnvelope = serde_json::from_slice(&envelope_bytes)
        .map_err(|e| SigningError::MalformedEnvelope(e.to_string()))?;

    if !SUPPORTED_ENVELOPE_VERSIONS.contains(&envelope.version) {
        return Err(SigningError::UnsupportedVersion(envelope.version));
    }
    if envelope.algorithm != "ed25519" {
        return Err(SigningError::UnsupportedAlgorithm(envelope.algorithm));
    }
    // v1 has no expiry concept; refuse v1 envelopes that smuggle
    // the v2 fields. Otherwise a publisher could ship an envelope
    // claiming version=1 (so the verifier uses v1 signed bytes
    // shape, ignoring expiry) while UI tools render the expiry
    // claim as if it were authoritative.
    if envelope.version == 1 && (envelope.not_before.is_some() || envelope.not_after.is_some()) {
        return Err(SigningError::MalformedEnvelope(
            "v1 envelope must not contain not_before/not_after".into(),
        ));
    }

    let digest = canonical_digest(&entries);
    let digest_hex = hex::encode(digest);
    if digest_hex != envelope.signed_digest {
        return Err(SigningError::TamperedArchive);
    }

    let pubkey_bytes = base64_decode(&envelope.signer_pubkey)
        .ok()
        .filter(|v| v.len() == 32)
        .ok_or(SigningError::InvalidPubkey)?;
    let sig_bytes = base64_decode(&envelope.signature)
        .ok()
        .filter(|v| v.len() == 64)
        .ok_or(SigningError::InvalidSignatureField)?;

    let sign_bytes = canonical_sign_input(
        envelope.version,
        &digest_hex,
        envelope.not_before.as_deref(),
        envelope.not_after.as_deref(),
    );
    let pubkey = UnparsedPublicKey::new(&ED25519, pubkey_bytes);
    pubkey
        .verify(&sign_bytes, &sig_bytes)
        .map_err(|_| SigningError::BadSignature)?;

    // Signature is valid; now enforce the time window. Parsing the
    // RFC 3339 strings AFTER signature verification keeps the
    // expensive expiry-checking cheap when the archive was bogus
    // anyway, and the time window is a *policy* check rather than
    // a cryptographic one.
    enforce_validity_window(&envelope)?;

    Ok(VerifiedArchive {
        envelope,
        digest_hex,
        files: entries,
    })
}

/// Refuse the envelope if the current time is outside any expiry
/// window the envelope advertises. Either bound being absent means
/// "no constraint on that side"; both absent means "no window
/// enforcement" (the default for v1 envelopes and for v2 envelopes
/// whose publisher didn't opt into expiry).
fn enforce_validity_window(envelope: &SignatureEnvelope) -> Result<(), SigningError> {
    let now = chrono::Utc::now();
    if let Some(s) = &envelope.not_before {
        let nb = chrono::DateTime::parse_from_rfc3339(s)
            .map_err(|e| SigningError::MalformedValidity(format!("not_before: {e}")))?;
        if now < nb.with_timezone(&chrono::Utc) {
            return Err(SigningError::SignatureNotYetValid {
                not_before: s.clone(),
            });
        }
    }
    if let Some(s) = &envelope.not_after {
        let na = chrono::DateTime::parse_from_rfc3339(s)
            .map_err(|e| SigningError::MalformedValidity(format!("not_after: {e}")))?;
        if now > na.with_timezone(&chrono::Utc) {
            return Err(SigningError::SignatureExpired {
                not_after: s.clone(),
            });
        }
    }
    Ok(())
}

/// Sign a set of archive entries, producing the envelope to embed.
/// Callers typically pass the decoded archive without any prior
/// signature entry (or with one, which `canonical_digest` ignores)
/// and then write the returned envelope as `SIGNATURE_FILE` back
/// into the zip.
///
/// Produces an envelope at [`ENVELOPE_VERSION`] (currently v2) with
/// no expiry constraint. Use [`sign_entries_with_validity`] to set
/// `not_before` / `not_after`.
pub fn sign_entries(
    entries: &[ArchiveEntry],
    signing_key: &Ed25519KeyPair,
    signer_source: &str,
) -> SignatureEnvelope {
    sign_entries_with_validity(entries, signing_key, signer_source, None, None)
}

/// Sign with an explicit validity window. `not_before` and
/// `not_after` are RFC 3339 strings; pass `None` for either bound
/// to leave it open. Both fields are included in the canonical
/// signed bytes so an attacker can't strip them after the fact.
pub fn sign_entries_with_validity(
    entries: &[ArchiveEntry],
    signing_key: &Ed25519KeyPair,
    signer_source: &str,
    not_before: Option<String>,
    not_after: Option<String>,
) -> SignatureEnvelope {
    let digest = canonical_digest(entries);
    let digest_hex = hex::encode(digest);
    let sign_bytes = canonical_sign_input(
        ENVELOPE_VERSION,
        &digest_hex,
        not_before.as_deref(),
        not_after.as_deref(),
    );
    let signature = signing_key.sign(&sign_bytes);
    SignatureEnvelope {
        version: ENVELOPE_VERSION,
        algorithm: "ed25519".into(),
        signer_pubkey: base64_encode(signing_key.public_key().as_ref()),
        signer_source: signer_source.into(),
        signed_at: chrono::Utc::now().to_rfc3339(),
        signed_digest: digest_hex,
        not_before,
        not_after,
        signature: base64_encode(signature.as_ref()),
    }
}

/// Decode a base64 pubkey from an envelope and return its
/// fingerprint, or `None` if decoding fails. Used by the trust
/// resolver and verify outcome logging; centralised here so
/// `base64_decode` can stay module-private.
pub fn decode_pubkey_fingerprint(b64_pubkey: &str) -> Option<String> {
    base64_decode(b64_pubkey).ok().map(|b| fingerprint(&b))
}

/// Compute the fingerprint shown in admin UIs for a pubkey. First 8
/// bytes of SHA-256 over the raw pubkey, lowercase hex. Short enough
/// to read, long enough to pin a key against casual confusion.
pub fn fingerprint(pubkey: &[u8]) -> String {
    let mut ctx = Context::new(&SHA256);
    ctx.update(pubkey);
    let out = ctx.finish();
    hex::encode(&out.as_ref()[..8])
}

/// Generate a fresh Ed25519 keypair. Returns the PKCS8-encoded secret
/// blob (ring's storage format) and the raw public key bytes. The
/// caller persists the PKCS8 blob encrypted at rest.
pub fn generate_keypair() -> Result<(Vec<u8>, Vec<u8>), SigningError> {
    let rng = SystemRandom::new();
    let pkcs8 =
        Ed25519KeyPair::generate_pkcs8(&rng).map_err(|e| SigningError::KeyGen(format!("{e:?}")))?;
    let keypair = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref())
        .map_err(|e| SigningError::KeyGen(format!("{e:?}")))?;
    Ok((
        pkcs8.as_ref().to_vec(),
        keypair.public_key().as_ref().to_vec(),
    ))
}

// ---------- base64 shim ----------
//
// Kept private to this module to avoid yet another top-level
// encoding dep; `base64` is already transitively available via
// webauthn, but using `ring`'s raw byte APIs and a tiny shim means
// this file has exactly the deps it declares (`ring` + `serde` +
// `hex` + `chrono`), and any reviewer can see the base64 round-trip
// without chasing a crate.

fn base64_encode(bytes: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn base64_decode(s: &str) -> Result<Vec<u8>, base64::DecodeError> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.decode(s)
}

// ---------- Tests ----------

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::write::FileOptions;

    fn rng_keypair() -> Ed25519KeyPair {
        let rng = SystemRandom::new();
        let pkcs8 = Ed25519KeyPair::generate_pkcs8(&rng).unwrap();
        Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap()
    }

    fn make_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut buf = Vec::new();
        {
            let cursor = std::io::Cursor::new(&mut buf);
            let mut zip = zip::ZipWriter::new(cursor);
            let options = FileOptions::default();
            for (name, bytes) in entries {
                zip.start_file(*name, options).unwrap();
                zip.write_all(bytes).unwrap();
            }
            zip.finish().unwrap();
        }
        buf
    }

    /// Take an unsigned zip, produce a signed version by embedding the
    /// computed envelope as `SIGNATURE_FILE`.
    fn embed_envelope(zip_bytes: &[u8], envelope: &SignatureEnvelope) -> Vec<u8> {
        let mut entries = read_archive(zip_bytes).unwrap();
        entries.retain(|e| e.name != SIGNATURE_FILE);
        entries.push(ArchiveEntry {
            name: SIGNATURE_FILE.into(),
            bytes: serde_json::to_vec(envelope).unwrap(),
        });
        let borrowed: Vec<(&str, &[u8])> = entries
            .iter()
            .map(|e| (e.name.as_str(), e.bytes.as_slice()))
            .collect();
        make_zip(&borrowed)
    }

    #[test]
    fn canonical_digest_is_order_independent() {
        let a = vec![
            ArchiveEntry {
                name: "manifest.json".into(),
                bytes: b"{\"name\":\"x\"}".to_vec(),
            },
            ArchiveEntry {
                name: "bundle.js".into(),
                bytes: b"export const a = 1;".to_vec(),
            },
        ];
        let b = vec![
            ArchiveEntry {
                name: "bundle.js".into(),
                bytes: b"export const a = 1;".to_vec(),
            },
            ArchiveEntry {
                name: "manifest.json".into(),
                bytes: b"{\"name\":\"x\"}".to_vec(),
            },
        ];
        assert_eq!(canonical_digest(&a), canonical_digest(&b));
    }

    #[test]
    fn canonical_digest_ignores_signature_file() {
        let base = vec![ArchiveEntry {
            name: "manifest.json".into(),
            bytes: b"{}".to_vec(),
        }];
        let mut with_sig = base.clone();
        with_sig.push(ArchiveEntry {
            name: SIGNATURE_FILE.into(),
            bytes: b"whatever bytes".to_vec(),
        });
        assert_eq!(canonical_digest(&base), canonical_digest(&with_sig));
    }

    #[test]
    fn roundtrip_sign_then_verify() {
        let kp = rng_keypair();
        let zip = make_zip(&[
            (
                "manifest.json",
                b"{\"name\":\"hello\",\"displayName\":\"Hello\",\"version\":\"0.1.0\"}",
            ),
            ("bundle.js", b"export default {};"),
        ]);
        let entries = read_archive(&zip).unwrap();
        let envelope = sign_entries(&entries, &kp, sources::LOCAL);
        let signed = embed_envelope(&zip, &envelope);

        let verified = verify_archive(&signed).unwrap();
        assert_eq!(verified.envelope.signer_source, sources::LOCAL);
        assert_eq!(verified.envelope.algorithm, "ed25519");
        assert_eq!(verified.envelope.version, ENVELOPE_VERSION);
        let manifest = verified
            .files
            .iter()
            .find(|f| f.name == "manifest.json")
            .unwrap();
        assert!(manifest.bytes.starts_with(b"{\"name\":\"hello\""));
    }

    #[test]
    fn verify_rejects_missing_signature() {
        let zip = make_zip(&[("manifest.json", b"{}")]);
        match verify_archive(&zip) {
            Err(SigningError::MissingSignature) => {}
            other => panic!("expected MissingSignature, got {other:?}"),
        }
    }

    #[test]
    fn verify_rejects_tampered_file() {
        let kp = rng_keypair();
        let zip = make_zip(&[
            ("manifest.json", b"{\"name\":\"hello\"}"),
            ("bundle.js", b"export default {};"),
        ]);
        let entries = read_archive(&zip).unwrap();
        let envelope = sign_entries(&entries, &kp, sources::LOCAL);
        let mut signed_entries = entries.clone();
        signed_entries.push(ArchiveEntry {
            name: SIGNATURE_FILE.into(),
            bytes: serde_json::to_vec(&envelope).unwrap(),
        });
        // Replace bundle.js bytes AFTER signing → digest mismatch.
        for e in signed_entries.iter_mut() {
            if e.name == "bundle.js" {
                e.bytes = b"export default { tampered: true };".to_vec();
            }
        }
        let borrowed: Vec<(&str, &[u8])> = signed_entries
            .iter()
            .map(|e| (e.name.as_str(), e.bytes.as_slice()))
            .collect();
        let tampered = make_zip(&borrowed);

        match verify_archive(&tampered) {
            Err(SigningError::TamperedArchive) => {}
            other => panic!("expected TamperedArchive, got {other:?}"),
        }
    }

    #[test]
    fn verify_rejects_bad_signature() {
        let kp = rng_keypair();
        let other_kp = rng_keypair();
        let zip = make_zip(&[("manifest.json", b"{\"name\":\"hello\"}")]);
        let entries = read_archive(&zip).unwrap();
        let mut envelope = sign_entries(&entries, &kp, sources::LOCAL);
        // Keep the digest valid, but claim `other_kp`'s pubkey →
        // signature was computed by `kp` but the envelope says
        // `other_kp`, so ED25519 verify fails.
        envelope.signer_pubkey = base64_encode(other_kp.public_key().as_ref());
        let signed = embed_envelope(&zip, &envelope);
        match verify_archive(&signed) {
            Err(SigningError::BadSignature) => {}
            other => panic!("expected BadSignature, got {other:?}"),
        }
    }

    #[test]
    fn verify_rejects_envelope_with_wrong_digest_field() {
        let kp = rng_keypair();
        let zip = make_zip(&[("manifest.json", b"{\"name\":\"hello\"}")]);
        let entries = read_archive(&zip).unwrap();
        let mut envelope = sign_entries(&entries, &kp, sources::LOCAL);
        envelope.signed_digest = "00".repeat(32);
        let signed = embed_envelope(&zip, &envelope);
        match verify_archive(&signed) {
            Err(SigningError::TamperedArchive) => {}
            other => panic!("expected TamperedArchive, got {other:?}"),
        }
    }

    #[test]
    fn verify_rejects_unknown_version() {
        let kp = rng_keypair();
        let zip = make_zip(&[("manifest.json", b"{}")]);
        let entries = read_archive(&zip).unwrap();
        let mut envelope = sign_entries(&entries, &kp, sources::LOCAL);
        envelope.version = 99;
        let signed = embed_envelope(&zip, &envelope);
        match verify_archive(&signed) {
            Err(SigningError::UnsupportedVersion(99)) => {}
            other => panic!("expected UnsupportedVersion(99), got {other:?}"),
        }
    }

    #[test]
    fn fingerprint_is_stable_and_short() {
        let kp = rng_keypair();
        let fp = fingerprint(kp.public_key().as_ref());
        assert_eq!(fp.len(), 16); // 8 bytes hex
        assert_eq!(fp, fingerprint(kp.public_key().as_ref()));
    }

    #[test]
    fn generate_keypair_produces_usable_keys() {
        let (pkcs8, pub_bytes) = generate_keypair().unwrap();
        assert_eq!(pub_bytes.len(), 32);
        let kp = Ed25519KeyPair::from_pkcs8(&pkcs8).unwrap();
        assert_eq!(kp.public_key().as_ref(), &pub_bytes[..]);
    }

    #[test]
    fn verify_rejects_oversized_envelope() {
        // An envelope that parses as valid JSON but exceeds the
        // pre-authentication size cap. Must be refused before serde
        // even runs. Padding with harmless whitespace keeps the
        // structure valid JSON while inflating size.
        let kp = rng_keypair();
        let zip = make_zip(&[("manifest.json", b"{}")]);
        let entries = read_archive(&zip).unwrap();
        let envelope = sign_entries(&entries, &kp, sources::LOCAL);
        let mut envelope_bytes = serde_json::to_vec(&envelope).unwrap();
        envelope_bytes.extend(std::iter::repeat_n(b' ', MAX_ENVELOPE_SIZE + 1));
        let bloated = make_zip(&[("manifest.json", b"{}"), (SIGNATURE_FILE, &envelope_bytes)]);
        match verify_archive(&bloated) {
            Err(SigningError::EnvelopeTooLarge) => {}
            other => panic!("expected EnvelopeTooLarge, got {other:?}"),
        }
    }

    #[test]
    fn verify_rejects_envelope_with_unknown_field() {
        // An envelope carrying a smuggled field. With
        // `deny_unknown_fields` on `SignatureEnvelope`, deserialise
        // must refuse rather than silently keeping the payload
        // where `signature_metadata` could render it into the UI.
        let kp = rng_keypair();
        let zip = make_zip(&[("manifest.json", b"{}")]);
        let entries = read_archive(&zip).unwrap();
        let envelope = sign_entries(&entries, &kp, sources::LOCAL);
        let mut as_json = serde_json::to_value(&envelope).unwrap();
        as_json
            .as_object_mut()
            .unwrap()
            .insert("evil".into(), serde_json::Value::String("xss".into()));
        let signed = make_zip(&[
            ("manifest.json", b"{}"),
            (
                SIGNATURE_FILE,
                serde_json::to_vec(&as_json).unwrap().as_slice(),
            ),
        ]);
        match verify_archive(&signed) {
            Err(SigningError::MalformedEnvelope(_)) => {}
            other => panic!("expected MalformedEnvelope, got {other:?}"),
        }
    }

    #[test]
    fn read_archive_rejects_oversized_entry() {
        // One entry larger than MAX_ENTRY_SIZE. Make a zip with a
        // single huge entry and confirm read_archive refuses.
        // MAX_ENTRY_SIZE is 1 MB; build just above it.
        let big = vec![b'x'; (MAX_ENTRY_SIZE as usize) + 1];
        let zip = make_zip(&[("oversized.bin", &big)]);
        match read_archive(&zip) {
            Err(SigningError::DecompressedTooLarge) => {}
            other => panic!("expected DecompressedTooLarge, got {other:?}"),
        }
    }

    #[test]
    fn verify_accepts_signature_inside_validity_window() {
        // not_before in the past, not_after in the future; the
        // verifier should accept the archive without complaint.
        let kp = rng_keypair();
        let zip = make_zip(&[("manifest.json", b"{}")]);
        let entries = read_archive(&zip).unwrap();
        let past = (chrono::Utc::now() - chrono::Duration::hours(1)).to_rfc3339();
        let future = (chrono::Utc::now() + chrono::Duration::hours(1)).to_rfc3339();
        let envelope =
            sign_entries_with_validity(&entries, &kp, sources::LOCAL, Some(past), Some(future));
        let signed = embed_envelope(&zip, &envelope);
        let verified = verify_archive(&signed).unwrap();
        assert!(verified.envelope.not_before.is_some());
        assert!(verified.envelope.not_after.is_some());
    }

    #[test]
    fn verify_rejects_expired_signature() {
        let kp = rng_keypair();
        let zip = make_zip(&[("manifest.json", b"{}")]);
        let entries = read_archive(&zip).unwrap();
        let past = (chrono::Utc::now() - chrono::Duration::hours(1)).to_rfc3339();
        let envelope =
            sign_entries_with_validity(&entries, &kp, sources::LOCAL, None, Some(past.clone()));
        let signed = embed_envelope(&zip, &envelope);
        match verify_archive(&signed) {
            Err(SigningError::SignatureExpired { not_after }) => {
                assert_eq!(not_after, past);
            }
            other => panic!("expected SignatureExpired, got {other:?}"),
        }
    }

    #[test]
    fn verify_rejects_signature_not_yet_valid() {
        let kp = rng_keypair();
        let zip = make_zip(&[("manifest.json", b"{}")]);
        let entries = read_archive(&zip).unwrap();
        let future = (chrono::Utc::now() + chrono::Duration::hours(1)).to_rfc3339();
        let envelope =
            sign_entries_with_validity(&entries, &kp, sources::LOCAL, Some(future.clone()), None);
        let signed = embed_envelope(&zip, &envelope);
        match verify_archive(&signed) {
            Err(SigningError::SignatureNotYetValid { not_before }) => {
                assert_eq!(not_before, future);
            }
            other => panic!("expected SignatureNotYetValid, got {other:?}"),
        }
    }

    #[test]
    fn verify_rejects_stripped_expiry_field() {
        // Sign with not_after set, then remove the field from the
        // envelope post-hoc. The canonical_sign_input for v2 binds
        // not_after into the signed bytes, so removing it should
        // produce BadSignature, not silently extend the lifetime.
        let kp = rng_keypair();
        let zip = make_zip(&[("manifest.json", b"{}")]);
        let entries = read_archive(&zip).unwrap();
        let future = (chrono::Utc::now() + chrono::Duration::hours(1)).to_rfc3339();
        let mut envelope =
            sign_entries_with_validity(&entries, &kp, sources::LOCAL, None, Some(future));
        envelope.not_after = None;
        let signed = embed_envelope(&zip, &envelope);
        match verify_archive(&signed) {
            Err(SigningError::BadSignature) => {}
            other => panic!("expected BadSignature, got {other:?}"),
        }
    }

    #[test]
    fn verify_rejects_v1_envelope_with_validity_fields() {
        // A v1 envelope MUST NOT carry not_before/not_after, since
        // v1 canonical_sign_input doesn't include them in the
        // signed bytes. Without this gate a signer could ship a v1
        // envelope with claimed expiry that the verifier silently
        // ignored.
        let kp = rng_keypair();
        let zip = make_zip(&[("manifest.json", b"{}")]);
        let entries = read_archive(&zip).unwrap();
        let mut envelope = sign_entries(&entries, &kp, sources::LOCAL);
        envelope.version = 1;
        envelope.not_after = Some("2099-01-01T00:00:00Z".into());
        let signed = embed_envelope(&zip, &envelope);
        match verify_archive(&signed) {
            Err(SigningError::MalformedEnvelope(_)) => {}
            other => panic!("expected MalformedEnvelope, got {other:?}"),
        }
    }
}
