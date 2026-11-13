//! Connecting a Microsoft account to the signed-in user, end to end against a
//! fake Microsoft tenant: `POST /api/auth/oauth/connect` starts a PKCE flow
//! whose verifier and nonce ride in the `oauth_state` cookie, and the shared
//! callback exchanges the code with that verifier, verifies the ID token's
//! nonce and tenant, and links the identity to the account that started the
//! connect. An identity already linked to another account is refused, and
//! sign-in's authorize refuses to start a connect at all.
//!
//! Its own binary: the Microsoft settings and the Microsoft hosts are
//! process-wide. One test drives every case in turn for the same reason.

#![allow(clippy::expect_used)]

mod common;

use std::sync::{Arc, Mutex};

use actix_web::cookie::Cookie;
use actix_web::dev::Service;
use actix_web::http::{header, StatusCode};
use actix_web::{test, web, App, HttpMessage, HttpResponse};
use serde_json::json;
use uuid::Uuid;

use backend::models::{Claims, OAuthState};
use backend::utils::cookies::{cookie_name, OAUTH_STATE_COOKIE};

const TENANT: &str = "11111111-2222-3333-4444-555555555555";
const CLIENT: &str = "99999999-8888-7777-6666-555555555555";
/// The Microsoft object id the fake signs everyone in as.
const OID: &str = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";

/// The test key `src/utils/entra_token.rs` signs its fake Entra tokens with,
/// and its public modulus.
const KEY_PEM: &str = include_str!("../src/utils/entra_token_test_key.pem");
const KEY_N: &str = "nV5NyHAjEs5UNGwBsvAj-UdvIRILygB8oh30-rMdNRtaHlvS8d9TUsUW-ZpZgxvY3QaecaJsul95uVRofQEs4VlmOaSmo6DdrennQVdLWa6YYRY29Fu8oRy7rGeZXrsJ7urx5oV3wu8LqWv8Q7rU_8QmGVzGoj_zl0GMcBKoxJfIRQKfIVrkduMuFqa_UYz2-GuObwFpX2GtZEZKbaK2llLRufg0tAR7A7JPiL3CTYRnfyhv_n_HXQ2kjsHqM17l0b-og2wmkzjjWrHLvSDHx6jEqab_ELDcffdmZMTJkzEMgbVA1cz_0pFkv5UlvD7zVtwXHKW2brSIJL1cY3udlw";

/// What the fake tenant was told and will say.
#[derive(Default)]
struct Tenant {
    /// The `code_verifier` of each token request.
    verifiers: Vec<String>,
    /// The nonce the next ID token carries, as the authorize request named it.
    nonce: String,
}

/// A Microsoft tenant: discovery and keys for `<base>/<tenant>/v2.0`, a token
/// endpoint that records the verifier and answers with an RS256 ID token, and
/// Graph's `/v1.0/me`. Both hosts are this one server.
fn fake_microsoft() -> (actix_test::TestServer, String, Arc<Mutex<Tenant>>) {
    let tenant = Arc::new(Mutex::new(Tenant::default()));
    let shared = tenant.clone();
    let srv = actix_test::start(move || {
        let shared = shared.clone();
        let base =
            |req: &actix_web::HttpRequest| format!("http://{}", req.connection_info().host());
        App::new()
            .route(
                &format!("/{TENANT}/v2.0/.well-known/openid-configuration"),
                web::get().to(move |req: actix_web::HttpRequest| async move {
                    let base = base(&req);
                    HttpResponse::Ok().json(json!({
                        "issuer": format!("{base}/{TENANT}/v2.0"),
                        "authorization_endpoint": format!("{base}/{TENANT}/oauth2/v2.0/authorize"),
                        "token_endpoint": format!("{base}/{TENANT}/oauth2/v2.0/token"),
                        "jwks_uri": format!("{base}/{TENANT}/discovery/v2.0/keys"),
                        "response_types_supported": ["code"],
                        "subject_types_supported": ["pairwise"],
                        "id_token_signing_alg_values_supported": ["RS256"],
                    }))
                }),
            )
            .route(
                &format!("/{TENANT}/discovery/v2.0/keys"),
                web::get().to(|| async {
                    HttpResponse::Ok().json(json!({ "keys": [
                        { "kty": "RSA", "use": "sig", "kid": "k1", "n": KEY_N, "e": "AQAB" }
                    ]}))
                }),
            )
            .route(
                &format!("/{TENANT}/oauth2/v2.0/token"),
                web::post().to(
                    move |req: actix_web::HttpRequest, form: web::Form<Vec<(String, String)>>| {
                        let shared = shared.clone();
                        async move {
                            let verifier = form
                                .iter()
                                .find(|(k, _)| k == "code_verifier")
                                .map(|(_, v)| v.clone())
                                .unwrap_or_default();
                            let nonce = {
                                let mut tenant = shared.lock().expect("lock");
                                tenant.verifiers.push(verifier);
                                tenant.nonce.clone()
                            };
                            let now = chrono::Utc::now().timestamp();
                            let mut header =
                                jsonwebtoken::Header::new(jsonwebtoken::Algorithm::RS256);
                            header.kid = Some("k1".into());
                            let id_token = jsonwebtoken::encode(
                                &header,
                                &json!({
                                    "iss": format!("{}/{TENANT}/v2.0", base(&req)),
                                    "aud": CLIENT,
                                    "sub": OID,
                                    "oid": OID,
                                    "tid": TENANT,
                                    "nonce": nonce,
                                    "iat": now,
                                    "exp": now + 3600,
                                }),
                                &jsonwebtoken::EncodingKey::from_rsa_pem(KEY_PEM.as_bytes())
                                    .expect("test key"),
                            )
                            .expect("sign ID token");
                            HttpResponse::Ok().json(json!({
                                "token_type": "Bearer",
                                "access_token": "graph-access-token",
                                "expires_in": 3600,
                                "id_token": id_token,
                            }))
                        }
                    },
                ),
            )
            .route(
                "/v1.0/me",
                web::get().to(|| async {
                    HttpResponse::Ok().json(json!({
                        "id": OID,
                        "displayName": "Sam Connect",
                        "mail": "sam.connect@acme.test",
                        "userPrincipalName": "sam.connect@acme.test",
                    }))
                }),
            )
    });
    let base = format!("http://{}", srv.addr());
    (srv, base, tenant)
}

/// The value of `name` in an absolute URL's query.
fn query_param(location: &str, name: &str) -> String {
    url::Url::parse(location)
        .expect("absolute URL")
        .query_pairs()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.into_owned())
        .unwrap_or_else(|| panic!("{name} parameter in {location}"))
}

/// The PKCE S256 challenge for `verifier` (RFC 7636 §4.2).
fn s256(verifier: &str) -> String {
    use base64::Engine as _;
    use sha2::Digest as _;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(sha2::Sha256::digest(verifier))
}

fn decode_state(state: &str) -> OAuthState {
    jsonwebtoken::decode::<OAuthState>(
        state,
        &jsonwebtoken::DecodingKey::from_secret(backend::utils::jwt::JWT_SECRET.as_bytes()),
        &jsonwebtoken::Validation::default(),
    )
    .expect("state verifies")
    .claims
}

/// The account a Microsoft identity with `OID` is linked to, if any.
fn linked_account(pool: &common::TestPool) -> Option<Uuid> {
    backend::repository::user_auth_identities::find_user_by_identity(
        "microsoft",
        OID,
        &mut pool.get().expect("conn"),
    )
    .expect("identity lookup")
}

#[actix_web::test]
async fn a_microsoft_connect_links_the_identity_to_the_signed_in_account() {
    let (_microsoft, base, tenant) = fake_microsoft();
    backend::config_utils::override_microsoft_hosts_for_tests(&base, &base);
    std::env::set_var("MICROSOFT_CLIENT_ID", CLIENT);
    std::env::set_var("MICROSOFT_CLIENT_SECRET", "test-secret");
    std::env::set_var("MICROSOFT_TENANT_ID", TENANT);
    std::env::set_var(
        "MICROSOFT_REDIRECT_URI",
        "https://app.nosdesk.test/api/auth/oauth/callback",
    );
    common::ensure_test_keyring();

    let db = common::TestDb::new();
    let pool = db.pool_with_size(4);
    let sam = common::insert_user(&mut pool.get().expect("conn"), "Sam Connect");
    let other = common::insert_user(&mut pool.get().expect("conn"), "Other Person");

    // The signed-in person, as the cookie auth middleware leaves them.
    let signed_in: Arc<Mutex<Option<Uuid>>> = Arc::default();
    let as_user = signed_in.clone();
    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .wrap_fn(move |req, srv| {
                if let Some(uuid) = *as_user.lock().expect("lock") {
                    let now = chrono::Utc::now().timestamp();
                    req.extensions_mut().insert(Claims {
                        sub: uuid.to_string(),
                        name: "User".to_string(),
                        email: "user@example.com".to_string(),
                        platform_role: "user".to_string(),
                        scope: "full".to_string(),
                        sid: None,
                        workspace_uuid: None,
                        exp: (now + 3600) as usize,
                        iat: now as usize,
                        file_path: None,
                    });
                }
                srv.call(req)
            })
            .route(
                "/api/auth/oauth/authorize",
                web::post().to(backend::handlers::oauth_authorize),
            )
            .route(
                "/api/auth/oauth/connect",
                web::post().to(backend::handlers::oauth_connect),
            )
            .route(
                "/api/auth/oauth/callback",
                web::get().to(backend::handlers::oauth_callback),
            ),
    )
    .await;

    // Start a connect as `user`, asking to come back to `redirect_uri`, then
    // come back from Microsoft with a code. Checks what the start sends to
    // Microsoft and keeps in the cookie; returns where the callback lands.
    let connect = |user: Uuid, redirect_uri: &'static str| {
        let app = &app;
        let signed_in = signed_in.clone();
        let tenant = tenant.clone();
        let base = base.clone();
        async move {
            *signed_in.lock().expect("lock") = Some(user);
            let started = test::call_service(
                app,
                test::TestRequest::post()
                    .uri("/api/auth/oauth/connect")
                    .set_json(json!({ "provider_type": "microsoft", "redirect_uri": redirect_uri }))
                    .to_request(),
            )
            .await;
            assert_eq!(started.status(), StatusCode::OK);
            let cookie = started
                .response()
                .cookies()
                .find(|c| c.name() == cookie_name(OAUTH_STATE_COOKIE))
                .expect("oauth_state cookie")
                .value()
                .to_string();
            let body: serde_json::Value = test::read_body_json(started).await;
            let auth_url = body["auth_url"].as_str().expect("auth_url").to_string();

            // PKCE S256 and a nonce, both from the cookie; an ID token scope.
            assert!(
                auth_url.starts_with(&format!("{base}/{TENANT}/oauth2/v2.0/authorize?")),
                "{auth_url}"
            );
            let parts: Vec<&str> = cookie.split('.').collect();
            let [binding, verifier, nonce] = parts.as_slice() else {
                panic!("binding.verifier.nonce, got {} parts", parts.len());
            };
            assert_eq!(query_param(&auth_url, "code_challenge_method"), "S256");
            assert_eq!(query_param(&auth_url, "code_challenge"), s256(verifier));
            assert_eq!(query_param(&auth_url, "nonce"), *nonce);
            assert!(query_param(&auth_url, "scope")
                .split(' ')
                .any(|s| s == "openid"));

            // A connect for this account, whatever the caller asked for.
            let raw_state = query_param(&auth_url, "state");
            let state = decode_state(&raw_state);
            assert_eq!(state.binding.as_deref(), Some(*binding));
            assert_eq!(state.user_connection, Some(true));
            assert_eq!(state.pkce_verifier, None);
            let path = redirect_uri.split('?').next().expect("path");
            assert_eq!(state.redirect_uri, format!("{path}?user_uuid={user}"));

            // Microsoft sends the browser back; the callback is a top-level
            // navigation with no session of its own.
            *signed_in.lock().expect("lock") = None;
            tenant.lock().expect("lock").nonce = nonce.to_string();
            let landed = test::call_service(
                app,
                test::TestRequest::get()
                    .uri(&format!(
                        "/api/auth/oauth/callback?code=the-code&state={}",
                        urlencoding::encode(&raw_state)
                    ))
                    .cookie(Cookie::new(
                        cookie_name(OAUTH_STATE_COOKIE).into_owned(),
                        cookie.clone(),
                    ))
                    .to_request(),
            )
            .await;
            assert_eq!(landed.status(), StatusCode::FOUND);
            let verifiers = std::mem::take(&mut tenant.lock().expect("lock").verifiers);
            assert_eq!(
                verifiers,
                [verifier.to_string()],
                "the code is exchanged with the cookie's verifier"
            );
            landed
                .headers()
                .get(header::LOCATION)
                .and_then(|v| v.to_str().ok())
                .expect("Location")
                .to_string()
        }
    };

    // Sign-in's authorize starts no connect, signed in or not: the return
    // path, and the account a connect links to, would be the caller's.
    for (provider, user) in [
        ("microsoft", None),
        ("microsoft", Some(sam.uuid)),
        ("oidc", Some(sam.uuid)),
    ] {
        *signed_in.lock().expect("lock") = user;
        let refused = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/api/auth/oauth/authorize")
                .set_json(json!({
                    "provider_type": provider,
                    "user_connection": true,
                    "redirect_uri": format!("/profile/settings?user_uuid={}", other.uuid),
                }))
                .to_request(),
        )
        .await;
        assert_eq!(refused.status(), StatusCode::BAD_REQUEST, "{provider}");
        assert_eq!(
            refused.response().cookies().count(),
            0,
            "{provider}: no state cookie"
        );
        let body = String::from_utf8(test::read_body(refused).await.to_vec()).expect("utf8");
        assert!(
            !body.contains("auth_url") && !body.contains("state"),
            "{provider}: no flow started: {body}"
        );
    }
    *signed_in.lock().expect("lock") = None;
    assert!(tenant.lock().expect("lock").verifiers.is_empty());

    // Sam connects from the settings page: the identity is linked to Sam and
    // the browser goes back there.
    assert_eq!(linked_account(&pool), None);
    let landed = connect(sam.uuid, "/profile/settings").await;
    assert_eq!(landed, "/profile/settings?auth_success=true");
    assert_eq!(linked_account(&pool), Some(sam.uuid));

    // Someone else connecting the same Microsoft account is refused, back on
    // the page they started from, and Sam keeps it. The `user_uuid` they put
    // in their return path is replaced with their own.
    let landed = connect(
        other.uuid,
        "/profile/settings/security?user_uuid=00000000-0000-0000-0000-000000000000",
    )
    .await;
    assert_eq!(
        landed,
        "/profile/settings/security?auth_error=connect_failed"
    );
    assert_eq!(linked_account(&pool), Some(sam.uuid));
}
