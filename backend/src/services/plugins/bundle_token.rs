//! Short-lived, single-bundle, workspace-scoped token authorizing a cross-origin
//! (cookieless) fetch of one plugin's bundle from the sandbox runtime.
//!
//! The sandbox iframe runs on a separate/opaque origin and sends no cookies, so
//! it cannot use the session. This token is the auth instead: the authenticated
//! app mints it for a plugin the caller may load; the sandbox `/bundle` route
//! verifies it, pins the workspace, and reads the bundle under RLS. It is scoped
//! to one workspace + one plugin + one bundle hash and lives ~60s, so a leak
//! serves at most that single (already-installed) bundle, briefly.

use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

use crate::utils::jwt::JWT_SECRET;

/// Fixed discriminator: kept as belt-and-suspenders, but no longer the only thing
/// separating a bundle token from a session token — see `bundle_key`.
const TYP: &str = "plugin_bundle";
const TTL_SECS: u64 = 60;
/// Clock-skew tolerance on `exp`. Small: mint + verify run on the same backend
/// (or a clock-synced cluster), and the whole point of the ~60s TTL is a tight
/// replay window, so the previous 30s (half the lifetime) is dropped to 5s.
const LEEWAY_SECS: u64 = 5;

/// Domain-separation label for the bundle-token signing key. Bumping the suffix
/// rotates every outstanding token.
const BUNDLE_KEY_LABEL: &[u8] = b"nosdesk-plugin-bundle-token-v1";

/// A dedicated HS256 key for bundle tokens, DERIVED from `JWT_SECRET` via
/// `HMAC-SHA256(JWT_SECRET, label)`. So a session token (signed with the raw
/// `JWT_SECRET`) can't be verified as a bundle token and vice-versa, and leaking
/// the bundle key doesn't reveal `JWT_SECRET`. No new operator config: the key is
/// derived, not a separate env var.
fn bundle_key() -> &'static [u8] {
    static KEY: OnceLock<Vec<u8>> = OnceLock::new();
    KEY.get_or_init(|| {
        let k = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, JWT_SECRET.as_bytes());
        ring::hmac::sign(&k, BUNDLE_KEY_LABEL).as_ref().to_vec()
    })
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PluginBundleClaims {
    pub typ: String,
    pub workspace_id: i32,
    pub plugin_uuid: Uuid,
    /// Pins the token to a bundle version; a rotated bundle invalidates it.
    pub bundle_hash: String,
    pub exp: u64,
    pub iat: u64,
}

#[derive(Debug)]
pub enum BundleTokenError {
    Time,
    Encode(jsonwebtoken::errors::Error),
    Decode(jsonwebtoken::errors::Error),
    WrongType,
}

impl std::fmt::Display for BundleTokenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Time => write!(f, "system time error"),
            Self::Encode(e) => write!(f, "encode error: {e}"),
            Self::Decode(e) => write!(f, "decode error: {e}"),
            Self::WrongType => write!(f, "token is not a plugin_bundle token"),
        }
    }
}

pub fn mint(
    workspace_id: i32,
    plugin_uuid: Uuid,
    bundle_hash: &str,
) -> Result<String, BundleTokenError> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| BundleTokenError::Time)?
        .as_secs();
    let claims = PluginBundleClaims {
        typ: TYP.to_string(),
        workspace_id,
        plugin_uuid,
        bundle_hash: bundle_hash.to_string(),
        exp: now + TTL_SECS,
        iat: now,
    };
    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(bundle_key()),
    )
    .map_err(BundleTokenError::Encode)
}

pub fn verify(token: &str) -> Result<PluginBundleClaims, BundleTokenError> {
    let mut v = Validation::new(Algorithm::HS256);
    v.validate_exp = true;
    v.validate_aud = false; // our tokens carry no audience
    v.leeway = LEEWAY_SECS;
    let data = decode::<PluginBundleClaims>(token, &DecodingKey::from_secret(bundle_key()), &v)
        .map_err(BundleTokenError::Decode)?;
    if data.claims.typ != TYP {
        return Err(BundleTokenError::WrongType);
    }
    Ok(data.claims)
}

/// The token's fixed lifetime (seconds), so the mint endpoint can tell the
/// client how soon to load.
pub const fn ttl_secs() -> u64 {
    TTL_SECS
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set_secret() {
        // JWT_SECRET is a lazy_static reading env; ensure it's present for tests.
        if std::env::var("JWT_SECRET").is_err() {
            std::env::set_var("JWT_SECRET", "test-secret-for-bundle-token");
        }
    }

    #[test]
    fn roundtrips_and_carries_scope() {
        set_secret();
        let uuid = Uuid::now_v7();
        let t = mint(42, uuid, "abc123").expect("mint");
        let c = verify(&t).expect("verify");
        assert_eq!(c.workspace_id, 42);
        assert_eq!(c.plugin_uuid, uuid);
        assert_eq!(c.bundle_hash, "abc123");
        assert_eq!(c.typ, TYP);
        assert!(c.exp > c.iat);
    }

    #[test]
    fn rejects_wrong_typ() {
        set_secret();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let forged = PluginBundleClaims {
            typ: "full".to_string(), // pretend to be a session-ish token
            workspace_id: 1,
            plugin_uuid: Uuid::now_v7(),
            bundle_hash: "x".to_string(),
            exp: now + 60,
            iat: now,
        };
        // Sign with the bundle key so the signature passes and the `typ` guard is
        // what rejects it.
        let t = encode(
            &Header::default(),
            &forged,
            &EncodingKey::from_secret(bundle_key()),
        )
        .unwrap();
        assert!(matches!(verify(&t), Err(BundleTokenError::WrongType)));
    }

    #[test]
    fn rejects_token_signed_with_session_key() {
        // Key separation: a token signed with the raw JWT_SECRET (a session token)
        // must not verify as a bundle token, even with a valid `typ`.
        set_secret();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let claims = PluginBundleClaims {
            typ: TYP.to_string(),
            workspace_id: 1,
            plugin_uuid: Uuid::now_v7(),
            bundle_hash: "x".to_string(),
            exp: now + 60,
            iat: now,
        };
        let t = encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(JWT_SECRET.as_bytes()),
        )
        .unwrap();
        assert!(matches!(verify(&t), Err(BundleTokenError::Decode(_))));
    }

    #[test]
    fn rejects_tampered_signature() {
        set_secret();
        let mut t = mint(1, Uuid::now_v7(), "h").expect("mint");
        t.push('x'); // corrupt the signature segment
        assert!(verify(&t).is_err());
    }
}
