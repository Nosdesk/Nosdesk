//! Portal sign-in with the workspace's own provider (requester SSO).
//!
//! `GET /api/portal/auth/sso` says whether the portal offers it (and the
//! button label). `GET /api/portal/auth/sso/start` sends the requester to the
//! provider with PKCE, a nonce and a CSRF state; those ride a short-lived,
//! HMAC-signed cookie (SameSite=Lax, so it survives the provider's top-level
//! redirect back). `GET /api/portal/auth/sso/callback` verifies the ID token and
//! signs the requester in.
//!
//! Who they are is `repository::requester_identities` (shared with the Teams
//! tab): an account linked to the provider's stable ids, else the email at a
//! listed domain, else a refusal that sends them back to the emailed sign-in
//! link.

use actix_web::cookie::{Cookie, SameSite};
use actix_web::{web, HttpMessage, HttpRequest, HttpResponse};
use base64::Engine as _;
use ring::hmac;
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::db::{DbConnection, Pool};
use crate::errors::ApiError;
use crate::extractors::WorkspaceContext;
use crate::handlers::portal::{mint_portal_session, portal_path};
use crate::models::WorkspaceIdentityProvider;
use crate::oidc::OidcAuthData;

const STATE_COOKIE: &str = "portal_sso";
/// How long the round trip to the provider may take.
const STATE_MINUTES: i64 = 10;
const KEY_LABEL: &[u8] = b"nosdesk-portal-sso-state-v1";

pub fn auth_config(cfg: &mut web::ServiceConfig) {
    cfg.route("/sso", web::get().to(sso_info))
        .route("/sso/start", web::get().to(sso_start))
        .route("/sso/callback", web::get().to(sso_callback));
}

#[derive(Serialize, Deserialize)]
struct State {
    csrf: String,
    workspace_id: i32,
    pkce_verifier: String,
    nonce: String,
    next: String,
    expires: i64,
}

fn key() -> Option<Vec<u8>> {
    let secret = std::env::var("JWT_SECRET").ok().filter(|s| !s.is_empty())?;
    let k = hmac::Key::new(hmac::HMAC_SHA256, secret.as_bytes());
    Some(hmac::sign(&k, KEY_LABEL).as_ref().to_vec())
}

fn seal(state: &State) -> Option<String> {
    let body =
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(serde_json::to_vec(state).ok()?);
    let tag = hmac::sign(&hmac::Key::new(hmac::HMAC_SHA256, &key()?), body.as_bytes());
    let sig = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(tag.as_ref());
    Some(format!("{body}.{sig}"))
}

fn open(value: &str) -> Option<State> {
    let (body, sig) = value.split_once('.')?;
    let sig = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(sig)
        .ok()?;
    hmac::verify(
        &hmac::Key::new(hmac::HMAC_SHA256, &key()?),
        body.as_bytes(),
        &sig,
    )
    .ok()?;
    let state: State = serde_json::from_slice(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(body)
            .ok()?,
    )
    .ok()?;
    (state.expires > chrono::Utc::now().timestamp()).then_some(state)
}

fn state_cookie(value: String, max_age_minutes: i64) -> Cookie<'static> {
    Cookie::build(crate::utils::cookies::cookie_name(STATE_COOKIE), value)
        .path("/")
        .http_only(true)
        .secure(crate::utils::cookies::auth_cookies_use_secure_flag())
        .same_site(SameSite::Lax)
        .max_age(actix_web::cookie::time::Duration::minutes(max_age_minutes))
        .finish()
}

fn back_to_login(reason: &str) -> HttpResponse {
    HttpResponse::Found()
        .cookie(state_cookie(String::new(), 0))
        .append_header((
            "Location",
            portal_path(&format!("/login?sso_error={reason}")),
        ))
        .finish()
}

/// The workspace's enabled provider, with its client secret.
fn load_provider(
    pool: &Pool,
    workspace_id: i32,
) -> Option<(WorkspaceIdentityProvider, String, Option<String>)> {
    crate::sync::session::run_in_workspace(
        pool,
        "background:portal_sso_provider",
        workspace_id,
        |conn| {
            let provider =
                crate::repository::workspace_identity_providers::get(conn)?.filter(|p| p.enabled);
            let origin = crate::utils::portal_ticket_link::portal_origin(conn, workspace_id);
            Ok::<_, diesel::result::Error>(provider.map(|p| (p, origin)))
        },
    )
    .ok()
    .flatten()
    .and_then(|(p, origin)| {
        let secret = crate::repository::workspace_identity_providers::client_secret(&p).ok()??;
        Some((p, secret, origin))
    })
}

fn redirect_uri(origin: &str) -> String {
    format!("{origin}{}", crate::handlers::requester_sso::CALLBACK_PATH)
}

/// `GET /api/portal/auth/sso`: whether to show "Sign in with …" on the portal.
pub async fn sso_info(req: HttpRequest, pool: web::Data<Pool>) -> HttpResponse {
    let off = || HttpResponse::Ok().json(json!({ "enabled": false }));
    let Some(ws) = req.extensions().get::<WorkspaceContext>().cloned() else {
        return off();
    };
    let workspace_id = ws.workspace_id;
    let found = crate::sync::session::run_in_workspace(
        &pool,
        "background:portal_sso_info",
        workspace_id,
        |conn| {
            let provider = crate::repository::workspace_identity_providers::get(conn)?;
            let origin = crate::utils::portal_ticket_link::portal_origin(conn, workspace_id);
            Ok::<_, diesel::result::Error>(provider.filter(|_| origin.is_some()))
        },
    );
    match found {
        Ok(Some(p)) if p.enabled && p.encrypted_client_secret.is_some() => {
            HttpResponse::Ok().json(json!({
                "enabled": true,
                "label": p.display_name,
                "kind": p.kind,
            }))
        }
        _ => off(),
    }
}

#[derive(Deserialize)]
pub struct StartQuery {
    /// A portal path to land on afterwards (`/tickets/12`).
    #[serde(default)]
    next: Option<String>,
}

/// `GET /api/portal/auth/sso/start`: off to the provider.
pub async fn sso_start(
    req: HttpRequest,
    pool: web::Data<Pool>,
    query: web::Query<StartQuery>,
) -> Result<HttpResponse, ApiError> {
    let Some(ws) = req.extensions().get::<WorkspaceContext>().cloned() else {
        return Err(ApiError::BadRequest("No workspace for this origin".into()));
    };
    let Some((provider, secret, Some(origin))) = load_provider(&pool, ws.workspace_id) else {
        return Ok(back_to_login("unavailable"));
    };
    let client = match crate::oidc::provider_client(
        &provider.issuer_url,
        &provider.client_id,
        &secret,
        &redirect_uri(&origin),
    )
    .await
    {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(error = %e, "portal sso: provider unavailable");
            return Ok(back_to_login("unavailable"));
        }
    };
    let csrf = Uuid::new_v4().to_string();
    let (url, auth) = crate::oidc::begin_provider_login(&client, csrf.clone());
    let next = query
        .next
        .as_deref()
        .filter(|p| p.starts_with("/tickets") && !p.contains("//"))
        .unwrap_or("/tickets")
        .to_string();
    let Some(sealed) = seal(&State {
        csrf,
        workspace_id: ws.workspace_id,
        pkce_verifier: auth.pkce_verifier,
        nonce: auth.nonce,
        next,
        expires: (chrono::Utc::now() + chrono::Duration::minutes(STATE_MINUTES)).timestamp(),
    }) else {
        return Ok(back_to_login("unavailable"));
    };
    Ok(HttpResponse::Found()
        .cookie(state_cookie(sealed, STATE_MINUTES))
        .append_header(("Location", url))
        .finish())
}

#[derive(Deserialize)]
pub struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

pub(crate) use crate::repository::requester_identities::Refused;

/// The keys requester SSO knows a person by: for Entra, the tenant and object
/// id first (shared with the Teams tab), then the provider's issuer + `sub`.
fn identity_keys(
    provider: &WorkspaceIdentityProvider,
    info: &crate::oidc::OidcUserInfo,
) -> Vec<crate::repository::requester_identities::IdentityKey> {
    use crate::repository::requester_identities::{entra_key, IdentityKey};
    let claim = |name: &str| info.raw_claims.get(name).and_then(|v| v.as_str());
    let mut keys = Vec::with_capacity(2);
    if provider.kind == "entra" {
        if let (Some(tid), Some(oid)) = (claim("tid"), claim("oid")) {
            keys.push(entra_key(tid, oid));
        }
    }
    keys.push(IdentityKey {
        provider_type: provider.issuer_url.clone(),
        external_id: info.sub.clone(),
    });
    keys
}

/// Resolve the provider's verified identity to a user (see
/// `repository::requester_identities`). Runs pinned to the workspace.
pub(crate) fn resolve_person(
    conn: &mut DbConnection,
    provider: &WorkspaceIdentityProvider,
    info: &crate::oidc::OidcUserInfo,
) -> Result<Result<Uuid, Refused>, diesel::result::Error> {
    let keys = identity_keys(provider, info);
    crate::repository::requester_identities::resolve(
        conn,
        provider.workspace_id,
        &provider.domains(),
        &crate::repository::requester_identities::Assertion {
            keys: &keys,
            email: info.email.as_deref(),
            email_verified: info.email_verified,
            name: info.name.as_deref(),
            via: "requester_sso",
        },
    )
}

/// `GET /api/portal/auth/sso/callback`: back from the provider.
pub async fn sso_callback(
    req: HttpRequest,
    pool: web::Data<Pool>,
    query: web::Query<CallbackQuery>,
) -> Result<HttpResponse, ApiError> {
    let Some(ws) = req.extensions().get::<WorkspaceContext>().cloned() else {
        return Err(ApiError::BadRequest("No workspace for this origin".into()));
    };
    if query.error.is_some() {
        return Ok(back_to_login("cancelled"));
    }
    let cookie_name = crate::utils::cookies::cookie_name(STATE_COOKIE);
    let Some(state) = req.cookie(&cookie_name).and_then(|c| open(c.value())) else {
        return Ok(back_to_login("expired"));
    };
    let (Some(code), Some(returned)) = (query.code.as_deref(), query.state.as_deref()) else {
        return Ok(back_to_login("failed"));
    };
    if state.workspace_id != ws.workspace_id
        || !constant_time_eq::constant_time_eq(returned.as_bytes(), state.csrf.as_bytes())
    {
        return Ok(back_to_login("failed"));
    }
    let Some((provider, secret, Some(origin))) = load_provider(&pool, ws.workspace_id) else {
        return Ok(back_to_login("unavailable"));
    };
    let info = match crate::oidc::provider_client(
        &provider.issuer_url,
        &provider.client_id,
        &secret,
        &redirect_uri(&origin),
    )
    .await
    {
        Ok(client) => {
            crate::oidc::finish_provider_login(
                &client,
                code,
                &OidcAuthData {
                    pkce_verifier: state.pkce_verifier.clone(),
                    nonce: state.nonce.clone(),
                },
            )
            .await
        }
        Err(e) => Err(e),
    };
    let info = match info {
        Ok(info) => info,
        Err(e) => {
            tracing::warn!(error = %e, "portal sso: sign-in failed");
            return Ok(back_to_login("failed"));
        }
    };

    let workspace_id = ws.workspace_id;
    let resolved = crate::sync::session::run_in_workspace(
        &pool,
        "background:portal_sso_resolve",
        workspace_id,
        |conn| resolve_person(conn, &provider, &info),
    );
    let user_uuid = match resolved {
        Ok(Ok(u)) => u,
        Ok(Err(Refused::Domain)) => return Ok(back_to_login("domain")),
        Err(e) => {
            tracing::error!(error = ?e, "portal sso: resolving the person failed");
            return Ok(back_to_login("failed"));
        }
    };
    let mut conn = pool
        .get()
        .map_err(|_| ApiError::Internal("Database connection failed".into()))?;
    let user = crate::repository::users::find_active_by_uuid(&user_uuid, &mut conn)
        .map_err(|_| ApiError::Unauthorized("That account isn't active".into()))?;
    let session = mint_portal_session(&user, ws.workspace_uuid, &req, &mut conn)?;
    Ok(HttpResponse::Found()
        .cookie(state_cookie(String::new(), 0))
        .cookie(session.access)
        .cookie(session.refresh)
        .cookie(session.csrf)
        .append_header(("Location", portal_path(&state.next)))
        .finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_round_trips_and_rejects_tampering_and_expiry() {
        std::env::set_var("JWT_SECRET", "test-portal-sso");
        let good = State {
            csrf: "c".into(),
            workspace_id: 1,
            pkce_verifier: "v".into(),
            nonce: "n".into(),
            next: "/tickets".into(),
            expires: chrono::Utc::now().timestamp() + 60,
        };
        let sealed = seal(&good).unwrap();
        assert_eq!(open(&sealed).map(|s| s.csrf), Some("c".into()));

        let (body, sig) = sealed.split_once('.').unwrap();
        let mut forged = body.to_string();
        forged.push('A');
        assert!(open(&format!("{forged}.{sig}")).is_none(), "tampered");

        let stale = State { expires: 0, ..good };
        assert!(open(&seal(&stale).unwrap()).is_none(), "expired");
    }

    fn info(sub: &str, email: Option<&str>, verified: Option<bool>) -> crate::oidc::OidcUserInfo {
        crate::oidc::OidcUserInfo {
            sub: sub.into(),
            email: email.map(str::to_string),
            email_verified: verified,
            name: Some("Sam".into()),
            preferred_username: None,
            given_name: None,
            family_name: None,
            picture: None,
            raw_claims: json!({}),
        }
    }

    #[test]
    fn people_are_keyed_on_the_subject_and_emails_trusted_only_at_listed_domains() {
        use crate::repository::workspace_identity_providers::{save, ProviderInput};
        let mut conn = crate::test_helpers::setup_test_connection();
        let provider = save(
            &mut conn,
            1,
            &ProviderInput {
                kind: "entra",
                display_name: "Microsoft",
                issuer_url: "https://login.microsoftonline.com/sso-test/v2.0",
                client_id: "client",
                client_secret: Some("s"),
                allowed_domains: &["sso-test.example".to_string()],
                enabled: true,
            },
        )
        .unwrap();

        let first = resolve_person(
            &mut conn,
            &provider,
            &info("s1", Some("Sam@SSO-test.example"), Some(true)),
        )
        .unwrap()
        .expect("a verified email at a listed domain signs in");
        // The subject wins over a changed email, even one at another domain.
        let again = resolve_person(
            &mut conn,
            &provider,
            &info("s1", Some("sam@elsewhere.example"), None),
        )
        .unwrap()
        .expect("a known subject signs in");
        assert_eq!(again, first);

        assert_eq!(
            resolve_person(
                &mut conn,
                &provider,
                &info("s2", Some("eve@elsewhere.example"), Some(true))
            )
            .unwrap(),
            Err(Refused::Domain),
        );
        assert_eq!(
            resolve_person(
                &mut conn,
                &provider,
                &info("s3", Some("kim@sso-test.example"), Some(false))
            )
            .unwrap(),
            Err(Refused::Domain),
            "an email the provider says is unverified is not trusted",
        );
        assert_eq!(
            resolve_person(&mut conn, &provider, &info("s4", None, None)).unwrap(),
            Err(Refused::Domain),
        );
        // A second subject with the first person's email links to the same account.
        let linked = resolve_person(
            &mut conn,
            &provider,
            &info("s5", Some("sam@sso-test.example"), None),
        )
        .unwrap()
        .unwrap();
        assert_eq!(linked, first);
    }
}
