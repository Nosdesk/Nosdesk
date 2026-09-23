//! Enterprise license verification and the instance's current licence.
//!
//! A license is an EdDSA-signed JWT (same crypto as the platform-auth
//! provisioning tokens) minted by the control plane with a private key only
//! Nosdesk holds. The matching public keys are compiled into the binary
//! (`include_str!` below), NOT read from the environment: an operator can't
//! point verification at their own key without recompiling, so a license is
//! a genuine "Nosdesk signed this" artifact. As with any open-source gate it
//! is bypassable by patching the binary; that is accepted and out of scope.
//!
//! ## Where the licence comes from
//!
//! `NOSDESK_LICENSE_KEY` wins when set. Otherwise the licence stored in
//! `instance_settings` (pasted in the admin UI, or delivered by linking the
//! instance to Nosdesk Cloud). A stored licence is re-verified on every load,
//! so a database write can never grant more than a signed token does.
//!
//! The resolved state is swappable at runtime ([`install`], [`remove`],
//! [`reload`]); consumers read it per request through [`current`]. Expiry is
//! evaluated on read, so a licence that lapses while the server runs degrades
//! without a restart.
//!
//! Absent / malformed / expired / wrong-issuer license => Community edition.
//! A bad license never fails the server boot; it just doesn't grant Enterprise.
//!
//! The edition gates multi-workspace creation on self-hosted deployments
//! (see `handlers::admin_workspaces::create_workspace`): Community is capped
//! at a single workspace, Enterprise lifts the cap to the licensed count.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock, RwLock};

use base64::Engine as _;
use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tracing::{info, warn};

use crate::db::DbConnection;

/// Public halves of the license signing keypairs, baked into the binary and
/// selected by the token's `kid` header. One key today; a rotation release
/// adds the new key here beside the old one (see the CP licensing runbook).
/// The private halves are held only by Nosdesk.
const LICENSE_PUBLIC_KEYS: &[&str] = &[include_str!("../license_pubkey.pem")];

/// Issuer every valid license must carry.
pub const LICENSE_ISSUER: &str = "https://nosdesk.com";

/// Optional human-friendly token prefix. Stripped before JWT decoding.
pub const LICENSE_PREFIX: &str = "nsk_lic_";

/// Environment variable that, when set, overrides the stored licence.
pub const LICENSE_ENV: &str = "NOSDESK_LICENSE_KEY";

/// Workspace cap applied with no (valid) license.
pub const COMMUNITY_MAX_WORKSPACES: u32 = 1;

/// Feature keys this binary understands. v1.1 is an empty gate list (O1):
/// the claim is accepted, stored only for keys in this set, and nothing
/// is gated on them. Unknown keys are ignored, not rejected.
pub const KNOWN_FEATURES: &[&str] = &[];

/// JWT claims carried by a license token.
///
/// `jti` has no `#[serde(default)]` on purpose: a missing jti must fail
/// verification. `jsonwebtoken` 11's `required_spec_claims` only checks
/// `exp`/`sub`/`iss`/`aud`/`nbf` and silently skips anything else, so
/// listing `jti` there is a no-op; the struct field is the requirement.
#[derive(Debug, Serialize, Deserialize)]
pub struct LicenseClaims {
    /// Issuer; must equal [`LICENSE_ISSUER`].
    pub iss: String,
    /// Stable opaque customer id (UUID). Constant across reissues.
    pub sub: String,
    /// Display name of the licensee organisation.
    pub licensee: String,
    /// Per-issuance license id. Required; never empty.
    pub jti: String,
    /// Expiry (unix seconds). Validated.
    pub exp: i64,
    /// Issued-at (unix seconds).
    #[serde(default)]
    pub iat: i64,
    /// Maximum number of active workspaces this license permits.
    pub max_workspaces: u32,
    /// Entitlement keys. `#[serde(default)]` so older mental-model tokens
    /// without the claim still deserialize; unknown keys are dropped.
    #[serde(default)]
    pub features: Vec<String>,
}

/// Verified license details.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LicenseInfo {
    /// Stable customer id (the JWT `sub`). Survives reissues.
    pub customer_id: String,
    /// Display name (the JWT `licensee` claim).
    pub licensee: String,
    /// Per-issuance id (the JWT `jti`).
    pub license_id: String,
    pub max_workspaces: u32,
    pub expires_at: i64,
    /// Known feature keys only. Empty in v1.1.
    pub features: Vec<String>,
}

/// Why a token is not a usable licence. Static kinds, safe to log and to show
/// an operator; the token itself never is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum LicenseError {
    /// Not a licence at all: bad encoding, not a JWT, wrong algorithm.
    #[error("malformed")]
    Malformed,
    /// Signed by a key this binary does not carry (a newer rotation, or not
    /// Nosdesk's).
    #[error("unknown_key")]
    UnknownKey,
    #[error("bad_signature")]
    BadSignature,
    #[error("wrong_issuer")]
    WrongIssuer,
    /// Signed and from Nosdesk, but a required claim is missing or malformed.
    #[error("invalid_claims")]
    InvalidClaims,
    #[error("expired")]
    Expired,
    /// A licence is stored but could not be decrypted, typically because the
    /// encryption key it was written with is no longer loaded.
    #[error("unreadable")]
    Unreadable,
}

impl LicenseError {
    pub fn kind(self) -> &'static str {
        match self {
            Self::Malformed => "malformed",
            Self::UnknownKey => "unknown_key",
            Self::BadSignature => "bad_signature",
            Self::WrongIssuer => "wrong_issuer",
            Self::InvalidClaims => "invalid_claims",
            Self::Expired => "expired",
            Self::Unreadable => "unreadable",
        }
    }
}

/// The deployment's resolved edition.
#[derive(Debug, Clone)]
pub enum Edition {
    Community,
    Enterprise(LicenseInfo),
}

impl Edition {
    /// Maximum active workspaces permitted under this edition.
    pub fn max_workspaces(&self) -> u32 {
        match self {
            Edition::Community => COMMUNITY_MAX_WORKSPACES,
            Edition::Enterprise(info) => info.max_workspaces,
        }
    }

    pub fn is_enterprise(&self) -> bool {
        matches!(self, Edition::Enterprise(_))
    }

    /// Short edition slug for API/UI surfacing.
    pub fn name(&self) -> &'static str {
        match self {
            Edition::Community => "community",
            Edition::Enterprise(_) => "enterprise",
        }
    }

    pub fn license(&self) -> Option<&LicenseInfo> {
        match self {
            Edition::Community => None,
            Edition::Enterprise(info) => Some(info),
        }
    }

    /// Whether this edition carries `feature`. Community is always false.
    /// v1.1's known-key set is empty, so this is false for every live
    /// license; the helper is the mechanism, the gate list is policy.
    pub fn has_feature(&self, feature: &str) -> bool {
        match self {
            Edition::Community => false,
            Edition::Enterprise(info) => info.features.iter().any(|f| f == feature),
        }
    }
}

/// Process-wide [`Edition::has_feature`]. Handlers that already have an
/// `&Edition` should call the method.
pub fn has_feature(feature: &str) -> bool {
    current().has_feature(feature)
}

fn normalize_features(raw: Vec<String>) -> Vec<String> {
    raw.into_iter()
        .filter(|k| KNOWN_FEATURES.contains(&k.as_str()))
        .collect()
}

fn strip_prefix(token: &str) -> &str {
    let t = token.trim();
    t.strip_prefix(LICENSE_PREFIX).unwrap_or(t)
}

/// A public key and its `kid`: hex of the first 8 bytes of SHA-256 over the
/// raw 32-byte Ed25519 key, the same derivation the control plane stamps.
struct VerifyingKey {
    kid: String,
    key: DecodingKey,
}

impl VerifyingKey {
    fn from_pem(pem: &str) -> Option<Self> {
        let body: String = pem.lines().filter(|l| !l.starts_with("-----")).collect();
        let der = base64::engine::general_purpose::STANDARD
            .decode(body.trim())
            .ok()?;
        // SPKI for Ed25519 ends with the raw 32-byte key.
        let raw = der.get(der.len().checked_sub(32)?..)?;
        let digest = Sha256::digest(raw);
        Some(Self {
            kid: hex::encode(&digest[..8]),
            key: DecodingKey::from_ed_pem(pem.as_bytes()).ok()?,
        })
    }
}

fn embedded_keys() -> &'static [VerifyingKey] {
    static KEYS: OnceLock<Vec<VerifyingKey>> = OnceLock::new();
    KEYS.get_or_init(|| {
        LICENSE_PUBLIC_KEYS
            .iter()
            .filter_map(|pem| VerifyingKey::from_pem(pem))
            .collect()
    })
}

fn map_jwt_error(e: &jsonwebtoken::errors::Error) -> LicenseError {
    use jsonwebtoken::errors::ErrorKind as K;
    match e.kind() {
        K::InvalidSignature => LicenseError::BadSignature,
        K::InvalidIssuer => LicenseError::WrongIssuer,
        K::ExpiredSignature => LicenseError::Expired,
        K::MissingRequiredClaim(_) | K::InvalidSubject | K::Json(_) => LicenseError::InvalidClaims,
        _ => LicenseError::Malformed,
    }
}

/// Verify signature, issuer and claims, but NOT expiry, so an expired licence
/// can still be described to the operator. Callers decide what expiry means.
fn inspect_with(token: &str, keys: &[VerifyingKey]) -> Result<LicenseInfo, LicenseError> {
    let token = strip_prefix(token);
    let header = decode_header(token).map_err(|e| map_jwt_error(&e))?;
    if header.alg != Algorithm::EdDSA {
        return Err(LicenseError::Malformed);
    }
    let candidates: Vec<&VerifyingKey> = match &header.kid {
        Some(kid) => keys.iter().filter(|k| &k.kid == kid).collect(),
        None => keys.iter().collect(),
    };
    if candidates.is_empty() {
        return Err(LicenseError::UnknownKey);
    }

    let mut validation = Validation::new(Algorithm::EdDSA);
    validation.validate_exp = false;
    validation.validate_aud = false;
    validation.leeway = 0;
    validation.set_issuer(&[LICENSE_ISSUER]);
    // `sub` is a real registered claim so this check works. `jti` is not;
    // listing it here would be a silent no-op (see LicenseClaims).
    validation.set_required_spec_claims(&["exp", "iss", "sub"]);

    let mut last = LicenseError::BadSignature;
    for key in candidates {
        match decode::<LicenseClaims>(token, &key.key, &validation) {
            Ok(data) => {
                let c = data.claims;
                if c.jti.trim().is_empty() || c.licensee.trim().is_empty() {
                    return Err(LicenseError::InvalidClaims);
                }
                // `sub` must be the customer UUID. A display name here is a
                // mis-mint, and the meter would key on it.
                if uuid::Uuid::parse_str(&c.sub).is_err() {
                    return Err(LicenseError::InvalidClaims);
                }
                return Ok(LicenseInfo {
                    customer_id: c.sub,
                    licensee: c.licensee,
                    license_id: c.jti,
                    max_workspaces: c.max_workspaces,
                    expires_at: c.exp,
                    features: normalize_features(c.features),
                });
            }
            Err(e) => last = map_jwt_error(&e),
        }
    }
    Err(last)
}

fn check_expiry(info: LicenseInfo, now: i64) -> Result<LicenseInfo, LicenseError> {
    if info.expires_at <= now {
        Err(LicenseError::Expired)
    } else {
        Ok(info)
    }
}

/// Verify a license token against the given public key (SPKI PEM), expiry
/// included. Key-injectable so tests can sign with an ephemeral keypair.
pub fn verify_with_key(token: &str, public_key_pem: &str) -> Result<LicenseInfo, LicenseError> {
    let key = VerifyingKey::from_pem(public_key_pem).ok_or(LicenseError::UnknownKey)?;
    let info = inspect_with(token, std::slice::from_ref(&key))?;
    check_expiry(info, chrono::Utc::now().timestamp())
}

/// Verify a token against the embedded keys, expiry included.
pub fn verify(token: &str) -> Result<LicenseInfo, LicenseError> {
    let info = inspect_with(token, embedded_keys())?;
    check_expiry(info, chrono::Utc::now().timestamp())
}

// ---------------------------------------------------------------------------
// The instance's current licence
// ---------------------------------------------------------------------------

/// Where the current licence came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LicenseSource {
    None,
    /// `NOSDESK_LICENSE_KEY`. Read-only in the UI.
    Env,
    /// Pasted into the admin UI.
    Pasted,
    /// Delivered by linking the instance to Nosdesk Cloud.
    Linked,
}

impl LicenseSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Env => "env",
            Self::Pasted => "pasted",
            Self::Linked => "linked",
        }
    }

    fn from_stored(s: &str) -> Self {
        match s {
            "linked" => Self::Linked,
            _ => Self::Pasted,
        }
    }
}

/// The instance's licence as last resolved. Cheap to clone (the token is
/// shared); never serialised, so the token cannot leak through a response.
#[derive(Clone)]
pub struct LicenseState {
    pub source: LicenseSource,
    token: Option<Arc<str>>,
    /// Signature, issuer and claims verified. May be expired; see [`Self::edition`].
    pub info: Option<LicenseInfo>,
    /// Why a present licence could not be used, as of evaluation. Read it
    /// through [`Self::error`], which also accounts for expiry since then.
    error: Option<LicenseError>,
    /// Bumped on every change, so holders of derived state (the relay's
    /// cached token) can tell the licence moved under them.
    pub generation: u64,
}

impl std::fmt::Debug for LicenseState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LicenseState")
            .field("source", &self.source)
            .field("info", &self.info)
            .field("error", &self.error)
            .field("generation", &self.generation)
            .finish_non_exhaustive()
    }
}

impl LicenseState {
    fn evaluate(token: Option<String>, source: LicenseSource, generation: u64) -> Self {
        Self::evaluate_with(token, source, generation, embedded_keys())
    }

    fn evaluate_with(
        token: Option<String>,
        source: LicenseSource,
        generation: u64,
        keys: &[VerifyingKey],
    ) -> Self {
        let Some(token) = token.filter(|t| !t.trim().is_empty()) else {
            return Self {
                source: LicenseSource::None,
                token: None,
                info: None,
                error: None,
                generation,
            };
        };
        let (info, error) = match inspect_with(&token, keys) {
            Ok(info) => {
                let expired = info.expires_at <= chrono::Utc::now().timestamp();
                (Some(info), expired.then_some(LicenseError::Expired))
            }
            Err(e) => (None, Some(e)),
        };
        Self {
            source,
            token: Some(Arc::from(strip_prefix(&token))),
            info,
            error,
            generation,
        }
    }

    /// Why the licence grants nothing right now, if it doesn't. Expiry is
    /// checked live, so a licence that lapsed after it was loaded reads as
    /// expired without a reload.
    pub fn error(&self) -> Option<LicenseError> {
        self.error.or_else(|| {
            self.info
                .as_ref()
                .filter(|i| i.expires_at <= chrono::Utc::now().timestamp())
                .map(|_| LicenseError::Expired)
        })
    }

    /// The edition this licence grants right now. Expiry is checked here, not
    /// at load, so a licence that lapses mid-run stops granting at `exp`.
    pub fn edition(&self) -> Edition {
        match &self.info {
            Some(info) if info.expires_at > chrono::Utc::now().timestamp() => {
                Edition::Enterprise(info.clone())
            }
            _ => Edition::Community,
        }
    }

    /// The token to present to the cloud relay, when there is one worth
    /// presenting. Expired licences are still presented: the relay honours its
    /// own grace window, and deciding that here would duplicate its policy.
    pub fn relay_credential(&self) -> Option<Arc<str>> {
        self.info.as_ref().and(self.token.clone())
    }
}

static GENERATION: AtomicU64 = AtomicU64::new(1);

fn env_token() -> Option<String> {
    std::env::var(LICENSE_ENV)
        .ok()
        .filter(|s| !s.trim().is_empty())
}

/// Whether `NOSDESK_LICENSE_KEY` is set, which makes the UI read-only.
pub fn env_managed() -> bool {
    env_token().is_some()
}

fn cell() -> &'static RwLock<Arc<LicenseState>> {
    static STATE: OnceLock<RwLock<Arc<LicenseState>>> = OnceLock::new();
    STATE.get_or_init(|| {
        let state = LicenseState::evaluate(env_token(), LicenseSource::Env, 0);
        log_state(&state);
        RwLock::new(Arc::new(state))
    })
}

fn log_state(state: &LicenseState) {
    match (&state.info, state.error) {
        (Some(info), None) => info!(
            source = state.source.as_str(),
            customer_id = %info.customer_id,
            licensee = %info.licensee,
            license_id = %info.license_id,
            max_workspaces = info.max_workspaces,
            expires_at = info.expires_at,
            "Enterprise license verified"
        ),
        (_, Some(e)) => warn!(
            source = state.source.as_str(),
            error_kind = e.kind(),
            "license failed verification; running as Community edition"
        ),
        (None, None) => {}
    }
}

fn swap(next: LicenseState) {
    log_state(&next);
    *cell().write().unwrap_or_else(|p| p.into_inner()) = Arc::new(next);
}

/// The current licence state.
pub fn state() -> Arc<LicenseState> {
    cell().read().unwrap_or_else(|p| p.into_inner()).clone()
}

/// The edition the current licence grants right now.
pub fn current() -> Edition {
    state().edition()
}

/// Re-resolve from the environment and the stored licence. Called at boot
/// once the database is up, and periodically so a licence installed through
/// another replica reaches this one. Returns whether anything changed.
pub fn reload(conn: &mut DbConnection) -> bool {
    use crate::repository::instance_settings;

    let next = match env_token() {
        Some(t) => (Some(t), LicenseSource::Env, false),
        None => match instance_settings::get(conn) {
            Ok(Some(row)) => {
                let source = row
                    .license_source
                    .as_deref()
                    .map(LicenseSource::from_stored)
                    .unwrap_or(LicenseSource::None);
                match instance_settings::decrypt_license(&row) {
                    Ok(t) => (t, source, false),
                    // Say so rather than keep a stale licence or fall silently
                    // to Community: the page shows the stored key as
                    // unreadable, and pasting it again fixes it.
                    Err(e) => {
                        warn!(error = %e, "stored license could not be decrypted");
                        (None, source, true)
                    }
                }
            }
            Ok(None) => (None, LicenseSource::None, false),
            Err(e) => {
                warn!(error = %e, "instance_settings read failed; keeping the current license");
                return false;
            }
        },
    };
    let (token, source, unreadable) = next;

    let now = state();
    let unchanged = now.source == source
        && now.token.as_deref() == token.as_deref().map(strip_prefix)
        && (now.error == Some(LicenseError::Unreadable)) == unreadable;
    if unchanged {
        return false;
    }
    let generation = GENERATION.fetch_add(1, Ordering::Relaxed);
    swap(if unreadable {
        LicenseState {
            source,
            token: None,
            info: None,
            error: Some(LicenseError::Unreadable),
            generation,
        }
    } else {
        LicenseState::evaluate(token, source, generation)
    });
    true
}

#[derive(Debug, thiserror::Error)]
pub enum InstallError {
    /// `NOSDESK_LICENSE_KEY` is set; the stored licence would be ignored.
    #[error("the license is managed by NOSDESK_LICENSE_KEY")]
    EnvManaged,
    #[error("license rejected: {0}")]
    Invalid(LicenseError),
    #[error(transparent)]
    Storage(#[from] crate::repository::instance_settings::InstanceSettingsError),
}

/// Verify, store and activate a licence. Rejects expired tokens: installing
/// one would change nothing but the display.
pub fn install(
    conn: &mut DbConnection,
    token: &str,
    source: LicenseSource,
    installed_by: Option<uuid::Uuid>,
) -> Result<LicenseInfo, InstallError> {
    if env_managed() {
        return Err(InstallError::EnvManaged);
    }
    let info = verify(token).map_err(InstallError::Invalid)?;
    let stored_source = match source {
        LicenseSource::Linked => "linked",
        _ => "pasted",
    };
    let token = strip_prefix(token);
    crate::repository::instance_settings::set_license(conn, token, stored_source, installed_by)?;
    swap(LicenseState::evaluate(
        Some(token.to_string()),
        LicenseSource::from_stored(stored_source),
        GENERATION.fetch_add(1, Ordering::Relaxed),
    ));
    Ok(info)
}

/// Remove the stored licence; the instance returns to Community.
pub fn remove(conn: &mut DbConnection) -> Result<(), InstallError> {
    if env_managed() {
        return Err(InstallError::EnvManaged);
    }
    crate::repository::instance_settings::clear_license(conn)
        .map_err(|e| InstallError::Storage(e.into()))?;
    swap(LicenseState::evaluate(
        None,
        LicenseSource::None,
        GENERATION.fetch_add(1, Ordering::Relaxed),
    ));
    Ok(())
}

/// Whether a self-serve admin workspace create is within the edition's cap.
///
/// Gated purely on the resolved edition (Community = 1 active workspace;
/// Enterprise = the licensed count), NOT on `NOSDESK_DEPLOYMENT_MODE`. The cap
/// used to be skipped whenever the mode was `hosted`, but the mode is an env
/// var, so a self-hoster could flip it and create unlimited workspaces with no
/// license. Hosted deployments provision through the control-plane
/// `/api/internal` surface (which is 404'd off self-hosted and is authoritative
/// for its own billing), not this self-serve path, so applying the cap here in
/// every mode is safe and closes the bypass.
pub fn workspace_creation_allowed(edition: &Edition, active_workspaces: u64) -> bool {
    active_workspaces < u64::from(edition.max_workspaces())
}

#[cfg(test)]
mod tests {
    use super::*;
    use jsonwebtoken::{encode, EncodingKey, Header};

    // Throwaway Ed25519 keypair for these tests only. NOT the production key.
    const TEST_PRIV: &str = "-----BEGIN PRIVATE KEY-----\n\
        MC4CAQAwBQYDK2VwBCIEIO6Su/YmjzEi0murpwXB/YjsQHnYIjRqJDJaxagBTQ88\n\
        -----END PRIVATE KEY-----\n";
    const TEST_PUB: &str = "-----BEGIN PUBLIC KEY-----\n\
        MCowBQYDK2VwAyEAbQxmQHWB+LZXvtyh54SrZM41ptz/WroW9djdAx1HPZQ=\n\
        -----END PUBLIC KEY-----\n";

    const TEST_CUSTOMER: &str = "550e8400-e29b-41d4-a716-446655440000";

    fn mint(iss: &str, max_workspaces: u32, exp_offset: i64) -> String {
        mint_with(
            iss,
            TEST_CUSTOMER,
            "Acme Corp",
            "lic_test_1",
            max_workspaces,
            exp_offset,
            vec![],
        )
    }

    fn mint_with(
        iss: &str,
        sub: &str,
        licensee: &str,
        jti: &str,
        max_workspaces: u32,
        exp_offset: i64,
        features: Vec<&str>,
    ) -> String {
        #[derive(Serialize)]
        struct Mint<'a> {
            iss: &'a str,
            sub: &'a str,
            licensee: &'a str,
            jti: &'a str,
            iat: i64,
            exp: i64,
            max_workspaces: u32,
            features: Vec<&'a str>,
        }
        let now = chrono::Utc::now().timestamp();
        let claims = Mint {
            iss,
            sub,
            licensee,
            jti,
            iat: now,
            exp: now + exp_offset,
            max_workspaces,
            features,
        };
        encode(
            &Header::new(Algorithm::EdDSA),
            &claims,
            &EncodingKey::from_ed_pem(TEST_PRIV.as_bytes()).expect("encode key"),
        )
        .expect("mint license")
    }

    #[test]
    fn valid_license_verifies() {
        let token = mint(LICENSE_ISSUER, 10, 3600);
        let info = verify_with_key(&token, TEST_PUB).expect("valid license");
        assert_eq!(info.customer_id, TEST_CUSTOMER);
        assert_eq!(info.licensee, "Acme Corp");
        assert_eq!(info.license_id, "lic_test_1");
        assert_eq!(info.max_workspaces, 10);
        assert!(info.features.is_empty());
    }

    #[test]
    fn prefix_is_stripped() {
        let token = format!("{LICENSE_PREFIX}{}", mint(LICENSE_ISSUER, 5, 3600));
        assert!(verify_with_key(&token, TEST_PUB).is_ok());
    }

    #[test]
    fn expired_license_rejected() {
        let token = mint(LICENSE_ISSUER, 10, -3600);
        assert!(verify_with_key(&token, TEST_PUB).is_err());
    }

    #[test]
    fn wrong_issuer_rejected() {
        let token = mint("https://evil.example", 10, 3600);
        assert!(verify_with_key(&token, TEST_PUB).is_err());
    }

    #[test]
    fn tampered_token_rejected() {
        let mut token = mint(LICENSE_ISSUER, 10, 3600);
        // Flip a character in the payload segment.
        let mid = token.len() / 2;
        let b = token.as_bytes()[mid];
        let repl = if b == b'A' { 'B' } else { 'A' };
        token.replace_range(mid..mid + 1, &repl.to_string());
        assert!(verify_with_key(&token, TEST_PUB).is_err());
    }

    fn mint_with_kid(kid: Option<&str>, exp_offset: i64) -> String {
        #[derive(Serialize)]
        struct Mint<'a> {
            iss: &'a str,
            sub: &'a str,
            licensee: &'a str,
            jti: &'a str,
            exp: i64,
            max_workspaces: u32,
        }
        let mut header = Header::new(Algorithm::EdDSA);
        header.kid = kid.map(str::to_string);
        encode(
            &header,
            &Mint {
                iss: LICENSE_ISSUER,
                sub: TEST_CUSTOMER,
                licensee: "Acme Corp",
                jti: "lic_kid",
                exp: chrono::Utc::now().timestamp() + exp_offset,
                max_workspaces: 4,
            },
            &EncodingKey::from_ed_pem(TEST_PRIV.as_bytes()).expect("encode key"),
        )
        .expect("mint")
    }

    fn test_key() -> VerifyingKey {
        VerifyingKey::from_pem(TEST_PUB).expect("test key")
    }

    /// The control plane stamps `kid` as the first 8 bytes of SHA-256 over the
    /// raw key; the product must derive the same value or every CP-issued
    /// licence would read as signed by an unknown key.
    #[test]
    fn embedded_kid_matches_the_control_plane_derivation() {
        let keys = embedded_keys();
        assert_eq!(keys.len(), LICENSE_PUBLIC_KEYS.len());
        assert_eq!(keys[0].kid, "c9cb9e8b445ddf00");
    }

    #[test]
    fn matching_kid_verifies_and_absent_kid_falls_back() {
        let kid = test_key().kid;
        assert!(inspect_with(&mint_with_kid(Some(&kid), 3600), &[test_key()]).is_ok());
        assert!(inspect_with(&mint_with_kid(None, 3600), &[test_key()]).is_ok());
    }

    #[test]
    fn unknown_kid_is_rejected_as_such() {
        let err = inspect_with(
            &mint_with_kid(Some("0000000000000000"), 3600),
            &[test_key()],
        )
        .expect_err("unknown kid");
        assert_eq!(err, LicenseError::UnknownKey);
    }

    #[test]
    fn errors_name_the_fault() {
        assert_eq!(
            verify_with_key("not-a-token", TEST_PUB).expect_err("garbage"),
            LicenseError::Malformed
        );
        assert_eq!(
            verify_with_key(&mint("https://evil.example", 1, 3600), TEST_PUB).expect_err("iss"),
            LicenseError::WrongIssuer
        );
        assert_eq!(
            verify_with_key(&mint(LICENSE_ISSUER, 1, -60), TEST_PUB).expect_err("exp"),
            LicenseError::Expired
        );
    }

    /// An expired licence still describes itself (so the admin page can say
    /// whose licence lapsed and when) but grants nothing.
    #[test]
    fn expired_state_keeps_info_and_grants_community() {
        let state = LicenseState::evaluate_with(
            Some(mint_with_kid(None, -60)),
            LicenseSource::Pasted,
            7,
            &[test_key()],
        );
        assert_eq!(state.error(), Some(LicenseError::Expired));
        assert_eq!(state.info.as_ref().map(|i| i.max_workspaces), Some(4));
        assert!(!state.edition().is_enterprise());
        assert!(state.relay_credential().is_some());
    }

    #[test]
    fn valid_state_grants_enterprise_and_strips_the_prefix() {
        let token = format!("{LICENSE_PREFIX}{}", mint_with_kid(None, 3600));
        let state =
            LicenseState::evaluate_with(Some(token), LicenseSource::Linked, 2, &[test_key()]);
        assert!(state.error().is_none());
        assert_eq!(state.edition().max_workspaces(), 4);
        let cred = state.relay_credential().expect("credential");
        assert!(!cred.starts_with(LICENSE_PREFIX));
    }

    #[test]
    fn unverifiable_state_offers_no_relay_credential() {
        let state = LicenseState::evaluate_with(
            Some("garbage".into()),
            LicenseSource::Pasted,
            3,
            &[test_key()],
        );
        assert_eq!(state.error(), Some(LicenseError::Malformed));
        assert!(state.relay_credential().is_none());
        assert!(!state.edition().is_enterprise());
    }

    #[test]
    fn community_cap_is_one() {
        assert_eq!(Edition::Community.max_workspaces(), 1);
        assert!(!Edition::Community.is_enterprise());
    }

    fn enterprise(max: u32) -> Edition {
        Edition::Enterprise(LicenseInfo {
            customer_id: TEST_CUSTOMER.into(),
            licensee: "Acme Corp".into(),
            license_id: "lic_test_1".into(),
            max_workspaces: max,
            expires_at: 0,
            features: vec![],
        })
    }

    #[test]
    fn missing_jti_is_rejected() {
        #[derive(Serialize)]
        struct NoJti<'a> {
            iss: &'a str,
            sub: &'a str,
            licensee: &'a str,
            iat: i64,
            exp: i64,
            max_workspaces: u32,
        }
        let now = chrono::Utc::now().timestamp();
        let token = encode(
            &Header::new(Algorithm::EdDSA),
            &NoJti {
                iss: LICENSE_ISSUER,
                sub: TEST_CUSTOMER,
                licensee: "Acme Corp",
                iat: now,
                exp: now + 3600,
                max_workspaces: 10,
            },
            &EncodingKey::from_ed_pem(TEST_PRIV.as_bytes()).expect("encode key"),
        )
        .expect("mint");
        assert!(verify_with_key(&token, TEST_PUB).is_err());
    }

    #[test]
    fn empty_jti_is_rejected() {
        let token = mint_with(
            LICENSE_ISSUER,
            TEST_CUSTOMER,
            "Acme Corp",
            "",
            10,
            3600,
            vec![],
        );
        assert!(verify_with_key(&token, TEST_PUB).is_err());
    }

    #[test]
    fn display_name_as_sub_is_rejected() {
        let token = mint_with(
            LICENSE_ISSUER,
            "Acme Corp",
            "Acme Corp",
            "lic_test_1",
            10,
            3600,
            vec![],
        );
        assert!(verify_with_key(&token, TEST_PUB).is_err());
    }

    #[test]
    fn unknown_features_are_dropped() {
        let token = mint_with(
            LICENSE_ISSUER,
            TEST_CUSTOMER,
            "Acme Corp",
            "lic_test_1",
            10,
            3600,
            // Deliberately keys that will never enter KNOWN_FEATURES, so this
            // test does not start failing the day a real feature is added.
            vec!["not-a-key", "also-not-a-key"],
        );
        let info = verify_with_key(&token, TEST_PUB).expect("valid license");
        assert!(info.features.is_empty());
        let edition = Edition::Enterprise(info);
        assert!(!edition.has_feature("not-a-key"));
    }

    #[test]
    fn community_has_no_features() {
        assert!(!Edition::Community.has_feature("scim"));
    }

    #[test]
    fn creation_allowed_respects_edition_count_cap() {
        // Community: one active workspace, then capped.
        assert!(workspace_creation_allowed(&Edition::Community, 0));
        assert!(!workspace_creation_allowed(&Edition::Community, 1));
        assert!(!workspace_creation_allowed(&Edition::Community, 2));

        // Enterprise: up to the licensed count.
        let ent = enterprise(3);
        assert!(workspace_creation_allowed(&ent, 2));
        assert!(!workspace_creation_allowed(&ent, 3));
    }
}
