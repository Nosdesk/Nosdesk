//! On hosted, "sign out all other sessions" also asks the control plane to end
//! the person's other remembered sign-ins, keeping the one this browser holds.
//! The helpdesk's own revocation stands whatever the control plane does, and
//! the response says whether the remembered sign-ins were ended. Signing out
//! of this browser likewise asks the control plane to end its remembered
//! sign-in.
//!
//! Self-hosted makes no such call (`tests/it/signout_others_self_hosted.rs`).
//!
//! A standalone binary rather than part of `tests/it`: the deployment mode and
//! the control-plane URL and key are process-wide environment. One test drives
//! every case in turn for the same reason.

#![allow(clippy::expect_used)]

mod common;

use std::sync::{Arc, Mutex};

use actix_web::dev::Service;
use actix_web::http::StatusCode;
use actix_web::test as http_test;
use actix_web::{web, App, HttpMessage, HttpRequest, HttpResponse};
use diesel::prelude::*;
use jsonwebtoken::{Algorithm, DecodingKey, Validation};
use serde_json::{json, Value};
use uuid::Uuid;

use backend::models::{Claims, NewActiveSession, NewUserAuthIdentity};
use backend::services::remembered_sign_ins;

const ISSUER: &str = "https://cp.test";

// Throwaway keypair, not a credential.
const TEST_PRIV: &str = "-----BEGIN PRIVATE KEY-----\n\
    MC4CAQAwBQYDK2VwBCIEIO6Su/YmjzEi0murpwXB/YjsQHnYIjRqJDJaxagBTQ88\n\
    -----END PRIVATE KEY-----\n";
const TEST_PUB: &str = "-----BEGIN PUBLIC KEY-----\n\
    MCowBQYDK2VwAyEAbQxmQHWB+LZXvtyh54SrZM41ptz/WroW9djdAx1HPZQ=\n\
    -----END PUBLIC KEY-----\n";

/// A control plane that answers `status` and records each bearer token, and
/// the path each came to.
#[derive(Default)]
struct FakeCp {
    status: Mutex<u16>,
    tokens: Mutex<Vec<Option<String>>>,
    paths: Mutex<Vec<String>>,
}

fn fake_cp(cp: Arc<FakeCp>) -> actix_test::TestServer {
    actix_test::start(move || {
        let answer = |cp: Arc<FakeCp>| {
            web::post().to(move |req: HttpRequest| {
                let cp = cp.clone();
                async move {
                    let token = req
                        .headers()
                        .get("authorization")
                        .and_then(|v| v.to_str().ok())
                        .and_then(|v| v.strip_prefix("Bearer "))
                        .map(str::to_owned);
                    cp.tokens.lock().unwrap().push(token);
                    cp.paths.lock().unwrap().push(req.path().to_string());
                    let status = *cp.status.lock().unwrap();
                    HttpResponse::build(StatusCode::from_u16(status).unwrap())
                        .json(json!({ "ended": 1 }))
                }
            })
        };
        App::new()
            .route(remembered_sign_ins::REVOKE_OTHERS_PATH, answer(cp.clone()))
            .route(remembered_sign_ins::END_PATH, answer(cp.clone()))
    })
}

fn id_token(sub: &str, sid: &str) -> String {
    use base64::Engine as _;
    let enc = base64::engine::general_purpose::URL_SAFE_NO_PAD;
    format!(
        "{}.{}.sig",
        enc.encode(br#"{"alg":"RS256"}"#),
        enc.encode(json!({ "iss": ISSUER, "sub": sub, "sid": sid }).to_string())
    )
}

fn session(conn: &mut backend::db::DbConnection, user: Uuid, id_token: Option<String>) -> Uuid {
    backend::repository::active_sessions::create_session(
        conn,
        NewActiveSession {
            user_uuid: user,
            device_name: None,
            ip_address: None,
            user_agent: None,
            location: None,
            expires_at: (chrono::Utc::now() + chrono::Duration::hours(1)).naive_utc(),
            oidc_id_token: id_token,
            confirmed_at: None,
        },
    )
    .expect("session")
    .session_id
}

fn live_sessions(conn: &mut backend::db::DbConnection, user: Uuid) -> Vec<Uuid> {
    use backend::schema::active_sessions;
    active_sessions::table
        .filter(active_sessions::user_uuid.eq(user))
        .select(active_sessions::session_id)
        .load(conn)
        .expect("sessions")
}

struct Person {
    user: Uuid,
    subject: String,
}

/// A hosted staff member: a helpdesk user with a control-plane identity.
fn person(conn: &mut backend::db::DbConnection) -> Person {
    person_under(conn, ISSUER)
}

/// As [`person`], with the identity stored under `issuer` as given.
fn person_under(conn: &mut backend::db::DbConnection, issuer: &str) -> Person {
    let user = common::insert_plain_user(conn, "Staff");
    let subject = Uuid::new_v4().to_string();
    backend::repository::user_auth_identities::create_identity(
        NewUserAuthIdentity {
            user_uuid: user,
            provider_type: issuer.to_string(),
            external_id: subject.clone(),
            email: None,
            metadata: None,
            password_hash: None,
            workspace_id: None,
        },
        conn,
    )
    .expect("identity");
    Person { user, subject }
}

fn claims(user: Uuid, sid: Uuid) -> Claims {
    Claims {
        sub: user.to_string(),
        name: "Staff".to_string(),
        email: String::new(),
        platform_role: "user".to_string(),
        scope: "full".to_string(),
        sid: Some(sid.to_string()),
        workspace_uuid: None,
        exp: (chrono::Utc::now().timestamp() + 3600) as usize,
        iat: chrono::Utc::now().timestamp() as usize,
    }
}

/// Sign out the others from `current`, having confirmed it's them (the step-up
/// every caller passes first); returns the response body.
async fn sign_out_others(pool: &backend::db::Pool, user: Uuid, current: Uuid) -> Value {
    backend::repository::active_sessions::record_confirmation(
        &mut pool.get().expect("conn"),
        &current,
        &user,
        chrono::Utc::now().naive_utc(),
        None,
    )
    .expect("confirm");
    let as_claims = claims(user, current);
    let app = http_test::init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .wrap_fn(move |req, srv| {
                req.extensions_mut().insert(as_claims.clone());
                srv.call(req)
            })
            .route(
                "/auth/sessions/others",
                web::delete().to(backend::handlers::revoke_all_other_sessions),
            ),
    )
    .await;
    let resp = http_test::call_service(
        &app,
        http_test::TestRequest::delete()
            .uri("/auth/sessions/others")
            .set_json(json!({}))
            .to_request(),
    )
    .await;
    assert_eq!(resp.status(), 200);
    http_test::read_body_json(resp).await
}

/// Sign out of `session` with its access cookie.
async fn sign_out(pool: &backend::db::Pool, user: Uuid, session: Uuid) {
    let access = jsonwebtoken::encode(
        &jsonwebtoken::Header::default(),
        &claims(user, session),
        &jsonwebtoken::EncodingKey::from_secret(backend::utils::jwt::JWT_SECRET.as_bytes()),
    )
    .expect("access token");
    let app = http_test::init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .route("/auth/logout", web::post().to(backend::handlers::logout)),
    )
    .await;
    let resp = http_test::call_service(
        &app,
        http_test::TestRequest::post()
            .uri("/auth/logout")
            .cookie(actix_web::cookie::Cookie::new(
                backend::utils::cookies::cookie_name(backend::utils::cookies::ACCESS_TOKEN_COOKIE),
                access,
            ))
            .to_request(),
    )
    .await;
    assert_eq!(resp.status(), 200);
}

fn verified_claims(token: &str) -> Value {
    let mut v = Validation::new(Algorithm::EdDSA);
    v.set_audience(&["nosdesk-cp"]);
    v.set_issuer(&["nosdesk-product"]);
    jsonwebtoken::decode::<Value>(
        token,
        &DecodingKey::from_ed_pem(TEST_PUB.as_bytes()).unwrap(),
        &v,
    )
    .expect("token verifies against the product public key")
    .claims
}

#[actix_web::test]
async fn on_hosted_signing_out_ends_remembered_sign_ins() {
    std::env::set_var("NOSDESK_DEPLOYMENT_MODE", "hosted");
    std::env::set_var("OIDC_ISSUER_URL", ISSUER);
    std::env::set_var(remembered_sign_ins::KEY_ENV, TEST_PRIV);
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let pool = db.pool_with_size(4);
    let cp = Arc::new(FakeCp::default());
    *cp.status.lock().unwrap() = 200;
    let server = fake_cp(cp.clone());
    std::env::set_var("NOSDESK_RELAY_URL", server.url(""));
    let conn = &mut pool.get().expect("conn");

    // The control plane ends them: this browser's sign-in is named to keep.
    let p = person(conn);
    let here = session(conn, p.user, Some(id_token(&p.subject, "sid-here")));
    session(conn, p.user, Some(id_token(&p.subject, "sid-elsewhere")));
    let body = sign_out_others(&pool, p.user, here).await;
    assert_eq!(body["revoked_count"], 1);
    assert_eq!(body["remembered_sign_ins_ended"], true);
    assert_eq!(live_sessions(conn, p.user), vec![here]);
    let token = {
        let tokens = cp.tokens.lock().unwrap();
        assert_eq!(tokens.len(), 1, "one call to the control plane");
        tokens[0].clone().expect("a bearer token")
    };
    let c = verified_claims(&token);
    assert_eq!(c["scope"], "product:signout");
    assert_eq!(c["sub"], p.subject.as_str());
    assert_eq!(c["keep_sid"], "sid-here");
    assert!(c["exp"].as_i64().unwrap() - c["iat"].as_i64().unwrap() <= 300);

    // An identity projected with a trailing slash on the issuer is the same
    // person.
    let p = person_under(conn, &format!("{ISSUER}/"));
    let here = session(conn, p.user, Some(id_token(&p.subject, "sid-slash")));
    session(conn, p.user, None);
    let body = sign_out_others(&pool, p.user, here).await;
    assert_eq!(body["remembered_sign_ins_ended"], true);
    let token = cp.tokens.lock().unwrap().last().cloned().flatten().unwrap();
    let c = verified_claims(&token);
    assert_eq!(c["sub"], p.subject.as_str());
    assert_eq!(c["keep_sid"], "sid-slash");

    // A session without an id_token names nothing to keep.
    let p = person(conn);
    let here = session(conn, p.user, None);
    session(conn, p.user, None);
    let body = sign_out_others(&pool, p.user, here).await;
    assert_eq!(body["remembered_sign_ins_ended"], true);
    let token = cp.tokens.lock().unwrap().last().cloned().flatten().unwrap();
    let c = verified_claims(&token);
    assert_eq!(c["sub"], p.subject.as_str());
    assert!(c.get("keep_sid").is_none());

    // The control plane fails, or predates the endpoint: the helpdesk's own
    // sessions still go, and the response says the others may stay.
    for status in [500, 404, 401] {
        *cp.status.lock().unwrap() = status;
        let p = person(conn);
        let here = session(conn, p.user, Some(id_token(&p.subject, "sid-here")));
        session(conn, p.user, None);
        let body = sign_out_others(&pool, p.user, here).await;
        assert_eq!(body["revoked_count"], 1, "status {status}");
        assert_eq!(body["remembered_sign_ins_ended"], false, "status {status}");
        assert_eq!(live_sessions(conn, p.user), vec![here], "status {status}");
    }

    // Unreachable.
    std::env::set_var("NOSDESK_RELAY_URL", "http://127.0.0.1:1");
    let p = person(conn);
    let here = session(conn, p.user, None);
    session(conn, p.user, None);
    let body = sign_out_others(&pool, p.user, here).await;
    assert_eq!(body["remembered_sign_ins_ended"], false);
    assert_eq!(live_sessions(conn, p.user), vec![here]);
    std::env::set_var("NOSDESK_RELAY_URL", server.url(""));

    // No control-plane identity: nothing to name, so no call.
    let calls = cp.tokens.lock().unwrap().len();
    let user = common::insert_plain_user(conn, "No identity");
    let here = session(conn, user, None);
    session(conn, user, None);
    let body = sign_out_others(&pool, user, here).await;
    assert_eq!(body["remembered_sign_ins_ended"], false);
    assert_eq!(live_sessions(conn, user), vec![here]);
    assert_eq!(cp.tokens.lock().unwrap().len(), calls);

    // No key: no call, same answer.
    *cp.status.lock().unwrap() = 200;
    std::env::remove_var(remembered_sign_ins::KEY_ENV);
    let p = person(conn);
    let here = session(conn, p.user, None);
    session(conn, p.user, None);
    let body = sign_out_others(&pool, p.user, here).await;
    assert_eq!(body["remembered_sign_ins_ended"], false);
    assert_eq!(live_sessions(conn, p.user), vec![here]);
    assert_eq!(cp.tokens.lock().unwrap().len(), calls);

    // Signing out of this browser ends its remembered sign-in, by name.
    std::env::set_var(remembered_sign_ins::KEY_ENV, TEST_PRIV);
    let p = person(conn);
    let here = session(conn, p.user, Some(id_token(&p.subject, "sid-this")));
    let elsewhere = session(conn, p.user, Some(id_token(&p.subject, "sid-other")));
    sign_out(&pool, p.user, here).await;
    assert_eq!(live_sessions(conn, p.user), vec![elsewhere]);
    assert_eq!(
        cp.paths.lock().unwrap().last().map(String::as_str),
        Some(remembered_sign_ins::END_PATH)
    );
    let token = cp.tokens.lock().unwrap().last().cloned().flatten().unwrap();
    let c = verified_claims(&token);
    assert_eq!(c["scope"], "product:signout-this");
    assert_eq!(c["sub"], p.subject.as_str());
    assert_eq!(c["sid"], "sid-this");
    assert!(c.get("keep_sid").is_none());

    // After confirming it's them at the control plane, the browser holds a new
    // remembered sign-in; signing out ends that one, not the first.
    let p = person(conn);
    let here = session(conn, p.user, Some(id_token(&p.subject, "sid-before")));
    backend::repository::active_sessions::record_confirmation(
        conn,
        &here,
        &p.user,
        chrono::Utc::now().naive_utc(),
        Some(&id_token(&p.subject, "sid-after-confirming")),
    )
    .expect("confirm");
    sign_out(&pool, p.user, here).await;
    let token = cp.tokens.lock().unwrap().last().cloned().flatten().unwrap();
    assert_eq!(verified_claims(&token)["sid"], "sid-after-confirming");

    // A session with no remembered sign-in asks for nothing.
    let calls = cp.tokens.lock().unwrap().len();
    let p = person(conn);
    let here = session(conn, p.user, None);
    sign_out(&pool, p.user, here).await;
    assert!(live_sessions(conn, p.user).is_empty());
    assert_eq!(cp.tokens.lock().unwrap().len(), calls);

    // The control plane failing leaves the sign-out itself whole.
    *cp.status.lock().unwrap() = 500;
    let p = person(conn);
    let here = session(conn, p.user, Some(id_token(&p.subject, "sid-this")));
    sign_out(&pool, p.user, here).await;
    assert!(live_sessions(conn, p.user).is_empty());
    assert_eq!(cp.tokens.lock().unwrap().len(), calls + 1);
}
