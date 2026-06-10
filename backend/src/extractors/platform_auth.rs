//! Platform-provisioning auth for the `/api/internal/v1/*` control-plane
//! surface.
//!
//! The hosted control plane (`~/dev/nosdesk-com`) signs a short-lived
//! EdDSA JWT and presents it as a `Bearer` token. Verification happens
//! against the operator-provided public key, with no DB lookup and no
//! `api_token` involvement:
//!
//!   * **404** if the instance isn't running in hosted mode
//!     (`NOSDESK_DEPLOYMENT_MODE != "hosted"`). Self-hosted instances
//!     don't expose this surface, so we don't even admit the route
//!     exists.
//!   * **401** if the server isn't configured to verify platform tokens
//!     (`PLATFORM_PUBLIC_KEY` / `PLATFORM_ISSUER` unset or unusable), or
//!     the presented token is missing / malformed / invalid.
//!
//! A valid token must be `EdDSA`-signed by the key matching
//! `PLATFORM_PUBLIC_KEY` (SPKI PEM), carry the configured `iss`, an
//! unexpired `exp`, and `scope == "platform:provision"`.
//!
//! ## Middleware + extractor split
//!
//! [`platform_auth_middleware`] is the gate: it verifies the token once
//! at the scope boundary (so it runs *before* the idempotency
//! middleware, and an unauthenticated request never touches the
//! idempotency cache) and stamps a [`PlatformVerified`] marker into the
//! request. The [`PlatformAuth`] extractor then just reads that marker,
//! giving handlers a compile-checked `_: PlatformAuth` contract with no
//! second verification. The extractor fails closed: if a handler is ever
//! mounted on a scope that the middleware doesn't wrap, the marker is
//! absent and the extractor rejects with 401.

use std::future::{ready, Ready};

use actix_web::body::MessageBody;
use actix_web::dev::{Payload, ServiceRequest, ServiceResponse};
use actix_web::middleware::Next;
use actix_web::{Error, FromRequest, HttpMessage, HttpRequest, HttpResponse, ResponseError};
use jsonwebtoken::{decode, Algorithm, DecodingKey, Validation};
use serde::Deserialize;
use serde_json::json;
use tracing::warn;

use crate::middleware::DeploymentMode;

/// The only scope a control-plane provisioning token may carry.
const PROVISION_SCOPE: &str = "platform:provision";

/// Claims we read off the token. Issuer and expiry are validated by
/// `jsonwebtoken` itself (see [`verify`]); we only pull `scope` out to
/// check it against [`PROVISION_SCOPE`].
#[derive(Debug, Deserialize)]
struct PlatformClaims {
    scope: String,
}

/// Zero-sized extractor: add `_: PlatformAuth` to a handler signature to
/// require a verified control-plane provisioning request. Successful
/// extraction is the only signal; there's nothing to read off it. The
/// actual verification is done once by [`platform_auth_middleware`]; the
/// extractor reads the [`PlatformVerified`] marker it leaves behind.
pub struct PlatformAuth;

/// Marker inserted into request extensions by [`platform_auth_middleware`]
/// once a request's platform JWT has been verified. Module-private: the
/// only producer is the middleware and the only consumer is the
/// [`PlatformAuth`] extractor.
#[derive(Clone, Copy)]
struct PlatformVerified;

#[derive(Debug)]
pub enum PlatformAuthError {
    /// Not a hosted deployment — surfaced as 404 so a self-hosted
    /// instance never reveals the provisioning route exists.
    NotHosted,
    /// Missing config, or a missing / malformed / invalid token. All
    /// collapse to 401 with a generic body so we don't leak which.
    Unauthorized,
}

impl std::fmt::Display for PlatformAuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotHosted => write!(f, "not found"),
            Self::Unauthorized => write!(f, "unauthorized"),
        }
    }
}

impl ResponseError for PlatformAuthError {
    fn error_response(&self) -> HttpResponse {
        match self {
            Self::NotHosted => HttpResponse::NotFound().finish(),
            Self::Unauthorized => HttpResponse::Unauthorized().json(json!({
                "error": "Unauthorized",
                "message": "invalid or missing platform credential"
            })),
        }
    }
}

impl FromRequest for PlatformAuth {
    type Error = PlatformAuthError;
    type Future = Ready<Result<Self, Self::Error>>;

    fn from_request(req: &HttpRequest, _payload: &mut Payload) -> Self::Future {
        // Read the marker the middleware left. We do NOT re-verify here:
        // the middleware is the single verification point. Absence of the
        // marker means the handler is mounted on a scope that isn't gated
        // by `platform_auth_middleware` — fail closed.
        let verified = req.extensions().get::<PlatformVerified>().is_some();
        ready(if verified {
            Ok(PlatformAuth)
        } else {
            warn!(
                path = %req.path(),
                "PlatformAuth extractor reached without platform verification; \
                 is the scope wrapped by platform_auth_middleware?"
            );
            Err(PlatformAuthError::Unauthorized)
        })
    }
}

/// Scope-level gate for the control-plane provisioning surface. Verifies
/// the platform JWT and, on success, stamps [`PlatformVerified`] into the
/// request before calling downstream layers. Wrap this **outside** the
/// idempotency middleware so authentication runs first and an
/// unauthenticated request never reaches the idempotency cache.
///
/// A verification failure renders the [`PlatformAuthError`] response
/// (404 on self-hosted, 401 otherwise) and short-circuits the chain.
pub async fn platform_auth_middleware(
    req: ServiceRequest,
    next: Next<impl MessageBody>,
) -> Result<ServiceResponse<impl MessageBody>, Error> {
    verify(req.request())?;
    req.extensions_mut().insert(PlatformVerified);
    next.call(req).await
}

/// Required env var, treated as unset when empty/whitespace.
fn required_env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn verify(req: &HttpRequest) -> Result<(), PlatformAuthError> {
    // (a) Self-hosted instances don't have this surface at all.
    if DeploymentMode::from_env() != DeploymentMode::Hosted {
        return Err(PlatformAuthError::NotHosted);
    }

    // (b) Verification material must be configured. Fail closed (401) if
    // not — including the issuer, which is part of the trust check.
    let public_key = required_env("PLATFORM_PUBLIC_KEY").ok_or_else(|| {
        warn!("PLATFORM_PUBLIC_KEY is not set; rejecting platform request");
        PlatformAuthError::Unauthorized
    })?;
    let issuer = required_env("PLATFORM_ISSUER").ok_or_else(|| {
        warn!("PLATFORM_ISSUER is not set; rejecting platform request");
        PlatformAuthError::Unauthorized
    })?;

    // (c) Pull and verify the Bearer token.
    let token = req
        .headers()
        .get("Authorization")
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "))
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .ok_or(PlatformAuthError::Unauthorized)?;

    verify_token(token, &public_key, &issuer)
}

/// Verify a presented JWT against the platform key + issuer. Pure (no
/// env, no request), so the trust check is unit-testable. Any failure
/// collapses to `Unauthorized`.
fn verify_token(token: &str, public_key_pem: &str, issuer: &str) -> Result<(), PlatformAuthError> {
    let key = DecodingKey::from_ed_pem(public_key_pem.as_bytes()).map_err(|e| {
        warn!(error = %e, "PLATFORM_PUBLIC_KEY is not a usable EdDSA public key");
        PlatformAuthError::Unauthorized
    })?;

    let mut validation = Validation::new(Algorithm::EdDSA);
    validation.validate_exp = true;
    validation.validate_aud = false; // provisioning tokens carry no audience
    validation.set_issuer(&[issuer]);
    validation.set_required_spec_claims(&["exp", "iss"]);

    let decoded = decode::<PlatformClaims>(token, &key, &validation).map_err(|e| {
        warn!(error = %e, "platform token failed verification");
        PlatformAuthError::Unauthorized
    })?;

    if decoded.claims.scope != PROVISION_SCOPE {
        warn!(
            scope = %decoded.claims.scope,
            "platform token carries the wrong scope"
        );
        return Err(PlatformAuthError::Unauthorized);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use jsonwebtoken::{encode, EncodingKey, Header};

    // Throwaway Ed25519 keypair, generated only for these tests. NOT the
    // production key.
    const TEST_PRIV: &str = "-----BEGIN PRIVATE KEY-----\n\
        MC4CAQAwBQYDK2VwBCIEIO6Su/YmjzEi0murpwXB/YjsQHnYIjRqJDJaxagBTQ88\n\
        -----END PRIVATE KEY-----\n";
    const TEST_PUB: &str = "-----BEGIN PUBLIC KEY-----\n\
        MCowBQYDK2VwAyEAbQxmQHWB+LZXvtyh54SrZM41ptz/WroW9djdAx1HPZQ=\n\
        -----END PUBLIC KEY-----\n";
    const TEST_ISS: &str = "https://control.test";

    #[derive(serde::Serialize)]
    struct MintClaims<'a> {
        iss: &'a str,
        scope: &'a str,
        exp: usize,
    }

    /// Sign a token with the test private key. `exp_offset` seconds from
    /// now (negative = already expired).
    fn mint(iss: &str, scope: &str, exp_offset: i64) -> String {
        let exp = (chrono::Utc::now().timestamp() + exp_offset).max(0) as usize;
        encode(
            &Header::new(Algorithm::EdDSA),
            &MintClaims { iss, scope, exp },
            &EncodingKey::from_ed_pem(TEST_PRIV.as_bytes()).expect("encode key"),
        )
        .expect("mint token")
    }

    #[test]
    fn accepts_valid_provision_token() {
        let token = mint(TEST_ISS, PROVISION_SCOPE, 300);
        assert!(verify_token(&token, TEST_PUB, TEST_ISS).is_ok());
    }

    #[test]
    fn rejects_wrong_issuer() {
        let token = mint("https://evil.test", PROVISION_SCOPE, 300);
        assert!(verify_token(&token, TEST_PUB, TEST_ISS).is_err());
    }

    #[test]
    fn rejects_wrong_scope() {
        let token = mint(TEST_ISS, "platform:other", 300);
        assert!(verify_token(&token, TEST_PUB, TEST_ISS).is_err());
    }

    #[test]
    fn rejects_expired_token() {
        let token = mint(TEST_ISS, PROVISION_SCOPE, -300);
        assert!(verify_token(&token, TEST_PUB, TEST_ISS).is_err());
    }

    #[test]
    fn rejects_tampered_signature() {
        let token = mint(TEST_ISS, PROVISION_SCOPE, 300);
        let last = token.chars().last().unwrap();
        let repl = if last == 'a' { 'b' } else { 'a' };
        let tampered = format!("{}{repl}", &token[..token.len() - 1]);
        assert!(verify_token(&tampered, TEST_PUB, TEST_ISS).is_err());
    }

    #[test]
    fn rejects_garbage_and_bad_key() {
        assert!(verify_token("not-a-jwt", TEST_PUB, TEST_ISS).is_err());
        let token = mint(TEST_ISS, PROVISION_SCOPE, 300);
        assert!(verify_token(&token, "not-a-pem", TEST_ISS).is_err());
    }
}
