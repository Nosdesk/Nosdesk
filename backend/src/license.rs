//! Enterprise license verification.
//!
//! A license is an EdDSA-signed JWT (same crypto as the platform-auth
//! provisioning tokens) minted offline with the private signing key that
//! only Nosdesk holds. The matching public key is compiled into the binary
//! (`include_str!` below), NOT read from the environment: an operator can't
//! point verification at their own key without recompiling, so a license is
//! a genuine "Nosdesk signed this" artifact. As with any open-source gate it
//! is bypassable by patching the binary; that is accepted and out of scope.
//!
//! Absent / malformed / expired / wrong-issuer license ⇒ Community edition.
//! A bad license never fails the server boot; it just doesn't grant Enterprise.
//!
//! The edition gates multi-workspace creation on self-hosted deployments
//! (see `handlers::admin_workspaces::create_workspace`): Community is capped
//! at a single workspace, Enterprise lifts the cap to the licensed count.

use std::sync::OnceLock;

use jsonwebtoken::{decode, Algorithm, DecodingKey, Validation};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

/// Public half of the license signing keypair, baked into the binary.
/// The private half is held only by Nosdesk (never committed; see
/// `backend/license_private.pem`, gitignored).
const LICENSE_PUBLIC_KEY: &str = include_str!("../license_pubkey.pem");

/// Issuer every valid license must carry.
pub const LICENSE_ISSUER: &str = "https://nosdesk.com";

/// Optional human-friendly token prefix. Stripped before JWT decoding.
pub const LICENSE_PREFIX: &str = "nsk_lic_";

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
/// listing `jti` there is a no-op — the struct field is the requirement.
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
#[derive(Debug, Clone)]
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
/// `&Edition` should call the method; this is for the boot-cached current.
pub fn has_feature(feature: &str) -> bool {
    current().has_feature(feature)
}

fn missing_claim(name: &str) -> jsonwebtoken::errors::Error {
    jsonwebtoken::errors::ErrorKind::MissingRequiredClaim(name.to_string()).into()
}

fn normalize_features(raw: Vec<String>) -> Vec<String> {
    raw.into_iter()
        .filter(|k| KNOWN_FEATURES.iter().any(|known| *known == k.as_str()))
        .collect()
}

/// Verify a license token against the given public key (SPKI PEM). Pure and
/// key-injectable so tests can sign with an ephemeral keypair. Production code
/// calls [`current`], which uses the embedded key.
pub fn verify_with_key(
    token: &str,
    public_key_pem: &str,
) -> Result<LicenseInfo, jsonwebtoken::errors::Error> {
    let token = token
        .trim()
        .strip_prefix(LICENSE_PREFIX)
        .unwrap_or_else(|| token.trim());

    let key = DecodingKey::from_ed_pem(public_key_pem.as_bytes())?;
    let mut validation = Validation::new(Algorithm::EdDSA);
    validation.validate_exp = true;
    validation.validate_aud = false;
    validation.set_issuer(&[LICENSE_ISSUER]);
    // `sub` is a real registered claim so this check works. `jti` is not
    // — listing it here would be a silent no-op (see LicenseClaims).
    validation.set_required_spec_claims(&["exp", "iss", "sub"]);

    let data = decode::<LicenseClaims>(token, &key, &validation)?;
    let c = data.claims;
    if c.jti.trim().is_empty() {
        return Err(missing_claim("jti"));
    }
    if c.licensee.trim().is_empty() {
        return Err(missing_claim("licensee"));
    }
    // `sub` is present (set_required_spec_claims enforces that) but must be a
    // UUID. That is a malformed claim, not a missing one — an operator who
    // mis-mints sees only a warn line, so the error has to name the real fault.
    if uuid::Uuid::parse_str(&c.sub).is_err() {
        return Err(jsonwebtoken::errors::ErrorKind::InvalidSubject.into());
    }
    Ok(LicenseInfo {
        customer_id: c.sub,
        licensee: c.licensee,
        license_id: c.jti,
        max_workspaces: c.max_workspaces,
        expires_at: c.exp,
        features: normalize_features(c.features),
    })
}

/// Resolve the edition from `NOSDESK_LICENSE_KEY` against the embedded key.
fn load_from_env() -> Edition {
    match std::env::var("NOSDESK_LICENSE_KEY") {
        Ok(raw) if !raw.trim().is_empty() => match verify_with_key(&raw, LICENSE_PUBLIC_KEY) {
            Ok(info) => {
                info!(
                    customer_id = %info.customer_id,
                    licensee = %info.licensee,
                    license_id = %info.license_id,
                    max_workspaces = info.max_workspaces,
                    expires_at = info.expires_at,
                    "Enterprise license verified"
                );
                Edition::Enterprise(info)
            }
            Err(e) => {
                warn!(error = %e, "NOSDESK_LICENSE_KEY failed verification; running as Community edition");
                Edition::Community
            }
        },
        _ => Edition::Community,
    }
}

/// The process-wide edition, resolved once on first call and cached.
pub fn current() -> &'static Edition {
    static EDITION: OnceLock<Edition> = OnceLock::new();
    EDITION.get_or_init(load_from_env)
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
