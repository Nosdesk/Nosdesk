//! Hosted only: ending a person's remembered sign-ins at the control plane.
//!
//! On Nosdesk Cloud the control plane remembers a web sign-in for 8 hours, and
//! that remembered sign-in carries to every Nosdesk web client, the helpdesk
//! included. Ending the helpdesk's own sessions is therefore not enough for
//! "sign out all other sessions": another browser that still holds a
//! remembered sign-in signs straight back in without a password. So after the
//! helpdesk revokes its own sessions, it asks the control plane to end the
//! person's other remembered sign-ins, keeping the one this browser holds.
//!
//! ## The request
//!
//! `POST {NOSDESK_RELAY_URL}/api/product/v1/sessions/revoke-others`, no body,
//! with a bearer token signed by `PRODUCT_JWT_PRIVATE_KEY` (Ed25519 PKCS#8
//! PEM; the control plane holds the public half). The token is minted for
//! this one request and lives 60 seconds; it names the person (`sub`, their
//! control-plane subject) and the remembered sign-in to keep (`keep_sid`).
//!
//! The browser's remembered sign-in is the Hydra login session its id_token
//! names in the `sid` claim. The helpdesk keeps that id_token on the session
//! row for RP-initiated logout, so no extra state is needed.
//!
//! ## When it can't
//!
//! The helpdesk's own revocation never depends on this call. Without the key,
//! without a control-plane identity, or when the control plane fails or
//! predates the endpoint (404), the caller reports that other browsers may
//! stay signed in for up to 8 hours.

use std::time::Duration;

use jsonwebtoken::{Algorithm, EncodingKey, Header};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::db::DbConnection;
use crate::middleware::DeploymentMode;

/// The private key the helpdesk signs these requests with.
pub const KEY_ENV: &str = "PRODUCT_JWT_PRIVATE_KEY";
/// The control-plane endpoint, relative to the Nosdesk Cloud API.
pub const REVOKE_OTHERS_PATH: &str = "/api/product/v1/sessions/revoke-others";
/// What the control plane checks the token against.
pub const ISSUER: &str = "nosdesk-product";
pub const AUDIENCE: &str = "nosdesk-cp";
pub const SCOPE: &str = "product:signout";
/// Token lifetime. The control plane refuses anything over 300 seconds.
const TOKEN_TTL_SECS: i64 = 60;
/// The person is waiting on a modal; the helpdesk's revocation is already done.
const TIMEOUT: Duration = Duration::from_secs(5);

/// What became of the request to end the other remembered sign-ins.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// The control plane ended them.
    Ended,
    /// `PRODUCT_JWT_PRIVATE_KEY` is unset or unusable.
    NotConfigured,
    /// The person has no control-plane identity here.
    NoIdentity,
    /// The control plane was unreachable, refused, or predates the endpoint.
    Failed,
}

impl Outcome {
    pub fn ended(self) -> bool {
        self == Outcome::Ended
    }

    /// Static discriminator for logs and the security event.
    pub fn kind(self) -> &'static str {
        match self {
            Self::Ended => "ended",
            Self::NotConfigured => "not_configured",
            Self::NoIdentity => "no_identity",
            Self::Failed => "failed",
        }
    }
}

#[derive(Serialize)]
struct Claims<'a> {
    iss: &'static str,
    aud: &'static str,
    scope: &'static str,
    sub: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    keep_sid: Option<&'a str>,
    iat: i64,
    exp: i64,
}

/// The signing key from `PRODUCT_JWT_PRIVATE_KEY`, or `None` when unset or
/// unusable.
fn signing_key() -> Option<EncodingKey> {
    let pem = std::env::var(KEY_ENV)
        .ok()
        .filter(|s| !s.trim().is_empty())?;
    match EncodingKey::from_ed_pem(pem.as_bytes()) {
        Ok(key) => Some(key),
        Err(e) => {
            tracing::error!(error = %e, "{KEY_ENV} is not a usable Ed25519 private key");
            None
        }
    }
}

/// Mint the token for one request: end `subject`'s remembered sign-ins
/// except `keep_sid`.
pub fn mint(
    key: &EncodingKey,
    subject: &str,
    keep_sid: Option<&str>,
    now: i64,
) -> Result<String, jsonwebtoken::errors::Error> {
    jsonwebtoken::encode(
        &Header::new(Algorithm::EdDSA),
        &Claims {
            iss: ISSUER,
            aud: AUDIENCE,
            scope: SCOPE,
            sub: subject,
            keep_sid,
            iat: now,
            exp: now + TOKEN_TTL_SECS,
        },
        key,
    )
}

#[derive(Deserialize)]
struct IdTokenClaims {
    sub: Option<String>,
    sid: Option<String>,
}

/// The `sid` an id_token carries, when its `sub` is `subject`.
///
/// No signature check: the id_token was verified at sign-in and this reads the
/// copy the helpdesk stored on its own session row. The `sub` match keeps a
/// token from another identity from naming a sign-in to keep.
pub fn id_token_sid(id_token: &str, subject: &str) -> Option<String> {
    use base64::Engine as _;
    let payload = id_token.split('.').nth(1)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload.trim_end_matches('='))
        .ok()?;
    let claims: IdTokenClaims = serde_json::from_slice(&bytes).ok()?;
    if claims.sub.as_deref() != Some(subject) {
        return None;
    }
    claims.sid.filter(|s| !s.is_empty())
}

/// The person's control-plane subject, and the remembered sign-in of the
/// session asking (when it has one).
///
/// The subject comes from the person's control-plane identity (the OIDC
/// identity under the configured issuer), not from the session, so a session
/// without an id_token still ends every other remembered sign-in.
pub fn resolve(
    conn: &mut DbConnection,
    user_uuid: &Uuid,
    current_session: Option<Uuid>,
) -> Option<(String, Option<String>)> {
    let issuer = crate::handlers::auth_providers::oidc_identity_issuer();
    let subject = crate::repository::user_auth_identities::get_user_identities(user_uuid, conn)
        .ok()?
        .into_iter()
        .find(|i| i.provider_type == issuer && i.workspace_id.is_none())
        .map(|i| i.external_id)?;
    let keep_sid = current_session
        .and_then(|sid| {
            crate::repository::active_sessions::get_session_by_session_id(conn, &sid).ok()
        })
        .and_then(|s| s.oidc_id_token)
        .and_then(|t| id_token_sid(&t, &subject));
    Some((subject, keep_sid))
}

/// Send one request to the control plane at `base_url`.
pub async fn send(base_url: &str, token: &str) -> Outcome {
    let Some(http) = crate::services::notifications::channels::relay_client::cloud_http() else {
        return Outcome::Failed;
    };
    let url = format!("{}{REVOKE_OTHERS_PATH}", base_url.trim_end_matches('/'));
    match http
        .post(url)
        .bearer_auth(token)
        .timeout(TIMEOUT)
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => Outcome::Ended,
        Ok(resp) => {
            // 404: a control plane deployed before this endpoint.
            tracing::warn!(
                status = resp.status().as_u16(),
                "control plane did not end the other remembered sign-ins"
            );
            Outcome::Failed
        }
        Err(e) => {
            tracing::warn!(error = %e.without_url(), "control plane unreachable ending remembered sign-ins");
            Outcome::Failed
        }
    }
}

/// End `user_uuid`'s remembered sign-ins at the control plane, except the one
/// `current_session` holds. `None` outside hosted mode, where there are none.
pub async fn end_others(
    conn: &mut DbConnection,
    user_uuid: &Uuid,
    current_session: Option<Uuid>,
) -> Option<Outcome> {
    if DeploymentMode::current() != DeploymentMode::Hosted {
        return None;
    }
    let Some(key) = signing_key() else {
        tracing::warn!("{KEY_ENV} unset; other remembered sign-ins were not ended");
        return Some(Outcome::NotConfigured);
    };
    let Some((subject, keep_sid)) = resolve(conn, user_uuid, current_session) else {
        tracing::warn!(%user_uuid, "no control-plane identity; other remembered sign-ins were not ended");
        return Some(Outcome::NoIdentity);
    };
    let token = match mint(
        &key,
        &subject,
        keep_sid.as_deref(),
        chrono::Utc::now().timestamp(),
    ) {
        Ok(t) => t,
        Err(e) => {
            tracing::error!(error = %e, "minting the control-plane sign-out token failed");
            return Some(Outcome::Failed);
        }
    };
    let base_url = crate::services::notifications::channels::relay_client::cloud_base_url();
    Some(send(&base_url, &token).await)
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine as _;
    use jsonwebtoken::{DecodingKey, Validation};

    // Throwaway keypair, not a credential.
    const TEST_PRIV: &str = "-----BEGIN PRIVATE KEY-----\n\
        MC4CAQAwBQYDK2VwBCIEIO6Su/YmjzEi0murpwXB/YjsQHnYIjRqJDJaxagBTQ88\n\
        -----END PRIVATE KEY-----\n";
    const TEST_PUB: &str = "-----BEGIN PUBLIC KEY-----\n\
        MCowBQYDK2VwAyEAbQxmQHWB+LZXvtyh54SrZM41ptz/WroW9djdAx1HPZQ=\n\
        -----END PUBLIC KEY-----\n";

    fn id_token(claims: serde_json::Value) -> String {
        let enc = base64::engine::general_purpose::URL_SAFE_NO_PAD;
        format!(
            "{}.{}.sig",
            enc.encode(br#"{"alg":"RS256"}"#),
            enc.encode(claims.to_string())
        )
    }

    #[test]
    fn the_token_names_the_person_and_the_sign_in_to_keep() {
        let key = EncodingKey::from_ed_pem(TEST_PRIV.as_bytes()).unwrap();
        let now = chrono::Utc::now().timestamp();
        let token = mint(&key, "sub-1", Some("sid-1"), now).unwrap();
        let mut v = Validation::new(Algorithm::EdDSA);
        v.set_audience(&[AUDIENCE]);
        v.set_issuer(&[ISSUER]);
        let claims = jsonwebtoken::decode::<serde_json::Value>(
            &token,
            &DecodingKey::from_ed_pem(TEST_PUB.as_bytes()).unwrap(),
            &v,
        )
        .unwrap()
        .claims;
        assert_eq!(claims["scope"], SCOPE);
        assert_eq!(claims["sub"], "sub-1");
        assert_eq!(claims["keep_sid"], "sid-1");
        assert_eq!(claims["exp"].as_i64().unwrap() - now, TOKEN_TTL_SECS);

        let none = mint(&key, "sub-1", None, now).unwrap();
        let claims = jsonwebtoken::decode::<serde_json::Value>(
            &none,
            &DecodingKey::from_ed_pem(TEST_PUB.as_bytes()).unwrap(),
            &v,
        )
        .unwrap()
        .claims;
        assert!(claims.get("keep_sid").is_none());
    }

    #[test]
    fn the_sid_is_read_only_from_the_same_persons_id_token() {
        let t = id_token(serde_json::json!({ "sub": "sub-1", "sid": "sid-1" }));
        assert_eq!(id_token_sid(&t, "sub-1").as_deref(), Some("sid-1"));
        assert_eq!(id_token_sid(&t, "sub-2"), None);
        let no_sid = id_token(serde_json::json!({ "sub": "sub-1" }));
        assert_eq!(id_token_sid(&no_sid, "sub-1"), None);
        let empty_sid = id_token(serde_json::json!({ "sub": "sub-1", "sid": "" }));
        assert_eq!(id_token_sid(&empty_sid, "sub-1"), None);
        assert_eq!(id_token_sid("garbage", "sub-1"), None);
        assert_eq!(id_token_sid("a.!!!.c", "sub-1"), None);
    }
}
