//! Signing out ends the server session whichever credential the client still
//! holds: the web access cookie (live or past its 15 minutes), the refresh
//! cookie alone, or the mobile bearer. With none, it still clears cookies.
//! Only the caller's own session ends.

#![allow(clippy::expect_used)]

use actix_web::cookie::Cookie;
use actix_web::http::StatusCode;
use actix_web::{test, web, App};
use jsonwebtoken::{encode, EncodingKey, Header};

use backend::models::Claims;
use backend::utils::cookies::{cookie_name, ACCESS_TOKEN_COOKIE, REFRESH_TOKEN_COOKIE};

/// A logout request carrying one of a signed-in client's credentials.
type LogoutWith<'a> = Box<dyn Fn(&SignedIn) -> test::TestRequest + 'a>;

struct SignedIn {
    sid: uuid::Uuid,
    access: String,
    refresh: String,
}

fn sign_in(pool: &crate::common::TestPool, user: &backend::models::User) -> SignedIn {
    let mut conn = pool.get().expect("conn");
    let session = backend::repository::active_sessions::create_session(
        &mut conn,
        backend::models::NewActiveSession {
            user_uuid: user.uuid,
            device_name: Some("logout-test".into()),
            ip_address: None,
            user_agent: None,
            location: None,
            expires_at: (chrono::Utc::now() + chrono::Duration::days(7)).naive_utc(),
            oidc_id_token: None,
            confirmed_at: None,
        },
    )
    .expect("session");
    let (_, tokens) = backend::utils::jwt::helpers::create_login_response(
        user.clone(),
        &session.session_id,
        &uuid::Uuid::new_v4(),
        &mut conn,
    )
    .expect("login tokens");
    SignedIn {
        sid: session.session_id,
        access: tokens.access_token,
        refresh: tokens.refresh_token,
    }
}

/// A session token for `sid` that ran out an hour ago.
fn expired_access(user: &backend::models::User, sid: uuid::Uuid) -> String {
    let now = chrono::Utc::now().timestamp() as usize;
    let claims = Claims {
        sub: user.uuid.to_string(),
        name: user.name.clone(),
        email: String::new(),
        platform_role: user.platform_role.clone(),
        scope: "full".to_string(),
        sid: Some(sid.to_string()),
        workspace_uuid: None,
        exp: now - 3600,
        iat: now - 3600 - 15 * 60,
    };
    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(backend::utils::jwt::JWT_SECRET.as_bytes()),
    )
    .expect("token")
}

fn live(pool: &crate::common::TestPool, sid: uuid::Uuid) -> bool {
    let mut conn = pool.get().expect("conn");
    backend::repository::active_sessions::get_session_by_session_id(&mut conn, &sid).is_ok()
}

fn logout() -> test::TestRequest {
    test::TestRequest::post().uri("/api/auth/logout")
}

#[actix_web::test]
async fn logout_ends_the_session_from_any_credential_it_holds() {
    crate::common::ensure_test_keyring();
    let db = crate::common::TestDb::new();
    let pool = db.pool_with_size(4);
    let user = crate::common::insert_user(&mut pool.get().expect("conn"), "Leaving Agent");
    let app = test::init_service(App::new().app_data(web::Data::new(pool.clone())).route(
        "/api/auth/logout",
        web::post().to(backend::handlers::logout),
    ))
    .await;

    let bystander = sign_in(&pool, &user);

    let cases: [(&str, LogoutWith<'_>); 4] = [
        (
            "access cookie",
            Box::new(|s| {
                logout().cookie(Cookie::new(
                    cookie_name(ACCESS_TOKEN_COOKIE),
                    s.access.clone(),
                ))
            }),
        ),
        (
            "expired access cookie",
            Box::new(|s| {
                logout().cookie(Cookie::new(
                    cookie_name(ACCESS_TOKEN_COOKIE),
                    expired_access(&user, s.sid),
                ))
            }),
        ),
        (
            "refresh cookie only",
            Box::new(|s| {
                logout().cookie(Cookie::new(
                    cookie_name(REFRESH_TOKEN_COOKIE),
                    s.refresh.clone(),
                ))
            }),
        ),
        (
            "mobile bearer",
            Box::new(|s| logout().insert_header(("Authorization", format!("Bearer {}", s.access)))),
        ),
    ];

    for (shape, request) in cases {
        let session = sign_in(&pool, &user);
        assert!(live(&pool, session.sid));
        let res = test::call_service(&app, request(&session).to_request()).await;
        assert_eq!(res.status(), StatusCode::OK, "{shape}");
        assert!(!live(&pool, session.sid), "{shape}: the session ended");
    }

    assert!(
        live(&pool, bystander.sid),
        "another session of the same person stays"
    );

    let res = test::call_service(&app, logout().to_request()).await;
    assert_eq!(
        res.status(),
        StatusCode::OK,
        "no credential still signs out"
    );
    assert!(
        res.response()
            .cookies()
            .any(|c| c.name() == cookie_name(ACCESS_TOKEN_COOKIE)),
        "and expires the cookies"
    );
}

#[actix_web::test]
async fn a_forged_token_ends_nothing() {
    crate::common::ensure_test_keyring();
    let db = crate::common::TestDb::new();
    let pool = db.pool_with_size(4);
    let user = crate::common::insert_user(&mut pool.get().expect("conn"), "Targeted Agent");
    let app = test::init_service(App::new().app_data(web::Data::new(pool.clone())).route(
        "/api/auth/logout",
        web::post().to(backend::handlers::logout),
    ))
    .await;
    let session = sign_in(&pool, &user);

    let mut forged = expired_access(&user, session.sid);
    forged.push('x');
    let req = logout().cookie(Cookie::new(cookie_name(ACCESS_TOKEN_COOKIE), forged));
    let res = test::call_service(&app, req.to_request()).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert!(
        live(&pool, session.sid),
        "a token that fails its signature ends nothing"
    );
}
