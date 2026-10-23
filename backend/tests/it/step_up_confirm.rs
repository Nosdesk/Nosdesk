//! `/api/auth/step-up`: the step-up dialog's status and confirm endpoints.
//! A confirmation covers the session that made it, for the window, and
//! nothing else; an account with nothing to type is offered no field (it
//! confirms through its provider or passkey, covered elsewhere).

#![allow(clippy::expect_used)]

use actix_web::dev::Service;
use actix_web::test as http_test;
use actix_web::{web, App, HttpMessage};
use serde_json::{json, Value};
use uuid::Uuid;

use backend::models::{Claims, NewActiveSession};

const PASSWORD: &str = "correct horse battery staple";

fn session(conn: &mut backend::db::DbConnection, user: Uuid) -> Uuid {
    backend::repository::active_sessions::create_session(
        conn,
        NewActiveSession {
            user_uuid: user,
            device_name: None,
            ip_address: None,
            user_agent: None,
            location: None,
            expires_at: (chrono::Utc::now() + chrono::Duration::hours(1)).naive_utc(),
            oidc_id_token: None,
            confirmed_at: None,
        },
    )
    .expect("session")
    .session_id
}

fn give_local_password(conn: &mut backend::db::DbConnection, user: Uuid) {
    backend::repository::user_auth_identities::create_local_identity(
        backend::models::NewUserAuthIdentity {
            user_uuid: user,
            provider_type: "local".to_string(),
            external_id: user.to_string(),
            email: None,
            metadata: None,
            password_hash: Some(bcrypt::hash(PASSWORD, 4).expect("hash")),
            workspace_id: None,
        },
        conn,
    )
    .expect("local identity");
}

/// Call the step-up endpoints as `user` on session `sid`.
async fn call(
    pool: &backend::db::Pool,
    user: Uuid,
    sid: Uuid,
    req: http_test::TestRequest,
) -> (u16, Value) {
    let claims = Claims {
        sub: user.to_string(),
        name: "Agent".to_string(),
        email: String::new(),
        platform_role: "user".to_string(),
        scope: "full".to_string(),
        sid: Some(sid.to_string()),
        workspace_uuid: None,
        exp: (chrono::Utc::now().timestamp() + 3600) as usize,
        iat: chrono::Utc::now().timestamp() as usize,
    };
    let app = http_test::init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .wrap_fn(move |req, srv| {
                req.extensions_mut().insert(claims.clone());
                srv.call(req)
            })
            .route(
                "/auth/step-up",
                web::get().to(backend::handlers::step_up::status),
            )
            .route(
                "/auth/step-up",
                web::post().to(backend::handlers::step_up::confirm),
            ),
    )
    .await;
    let resp = http_test::call_service(&app, req.uri("/auth/step-up").to_request()).await;
    let status = resp.status().as_u16();
    let body = http_test::read_body(resp).await;
    (status, serde_json::from_slice(&body).unwrap_or(Value::Null))
}

fn status() -> http_test::TestRequest {
    http_test::TestRequest::get()
}

fn confirm(body: Value) -> http_test::TestRequest {
    http_test::TestRequest::post().set_json(body)
}

#[actix_web::test]
async fn a_password_confirms_this_session_for_the_window() {
    crate::common::ensure_test_keyring();
    let db = crate::common::TestDb::new();
    let pool = db.pool_with_size(4);
    let conn = &mut pool.get().expect("conn");
    let user = crate::common::insert_plain_user(conn, "Agent");
    give_local_password(conn, user);
    let here = session(conn, user);
    let elsewhere = session(conn, user);

    let (code, body) = call(&pool, user, here, status()).await;
    assert_eq!(code, 200);
    assert_eq!(body["methods"], json!(["password"]));
    assert_eq!(body["confirmed_until"], Value::Null);
    assert_eq!(body["window_minutes"], 10);

    for wrong in [
        json!({}),
        json!({ "password": "" }),
        json!({ "password": "nope" }),
    ] {
        let (code, _) = call(&pool, user, here, confirm(wrong.clone())).await;
        assert_eq!(code, 400, "{wrong}");
    }
    let (code, _) = call(&pool, user, here, status()).await;
    assert_eq!(code, 200);

    let (code, body) = call(&pool, user, here, confirm(json!({ "password": PASSWORD }))).await;
    assert_eq!(code, 200);
    let until: chrono::DateTime<chrono::Utc> =
        serde_json::from_value(body["confirmed_until"].clone()).expect("confirmed_until");
    let left = until - chrono::Utc::now();
    assert!(left > chrono::Duration::minutes(9) && left <= chrono::Duration::minutes(10));

    let (_, body) = call(&pool, user, here, status()).await;
    assert!(
        body["confirmed_until"].is_string(),
        "this session is confirmed"
    );
    let (_, body) = call(&pool, user, elsewhere, status()).await;
    assert_eq!(
        body["confirmed_until"],
        Value::Null,
        "another session of the same person is not"
    );
}

#[actix_web::test]
async fn an_account_with_nothing_to_type_is_offered_no_field() {
    crate::common::ensure_test_keyring();
    let db = crate::common::TestDb::new();
    let pool = db.pool_with_size(4);
    let conn = &mut pool.get().expect("conn");
    let user = crate::common::insert_plain_user(conn, "SSO only");
    let here = session(conn, user);

    let (code, body) = call(&pool, user, here, status()).await;
    assert_eq!(code, 200);
    assert_eq!(body["methods"], json!([]));
    let (code, _) = call(&pool, user, here, confirm(json!({}))).await;
    assert_eq!(code, 400, "an empty credential confirms nothing");
    let (code, _) = call(&pool, user, here, confirm(json!({ "mfa_code": "123456" }))).await;
    assert_eq!(code, 400, "a code without MFA confirms nothing");
}

#[test]
fn a_confirmation_keeps_a_new_provider_token_and_never_moves_back() {
    crate::common::ensure_test_keyring();
    let db = crate::common::TestDb::new();
    let pool = db.pool_with_size(2);
    let conn = &mut pool.get().expect("conn");
    let user = crate::common::insert_plain_user(conn, "Agent");
    let other = crate::common::insert_plain_user(conn, "Someone else");
    let here = session(conn, user);
    let now = chrono::Utc::now().naive_utc();
    let repo = |conn: &mut backend::db::DbConnection, who: Uuid, at, token| {
        backend::repository::active_sessions::record_confirmation(conn, &here, &who, at, token)
            .expect("record")
    };

    assert_eq!(repo(conn, other, now, None), 0, "someone else's session");
    assert_eq!(repo(conn, user, now, Some("token-1")), 1);
    let earlier = now - chrono::Duration::minutes(5);
    assert_eq!(
        repo(conn, user, earlier, Some("token-0")),
        0,
        "never backwards"
    );
    let row = backend::repository::active_sessions::get_session_by_session_id(conn, &here)
        .expect("session");
    assert_eq!(row.oidc_id_token.as_deref(), Some("token-1"));
    assert_eq!(
        backend::repository::active_sessions::confirmed_at(conn, &here, &user).expect("read"),
        Some(now)
    );
}
