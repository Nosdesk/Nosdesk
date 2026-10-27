//! Password sign-in routes check the login lockout before the password, and
//! refuse with 503 when its counter can't be read outside development.
//! `/api/auth/mfa-login` counts wrong passwords and wrong codes against it.
//!
//! Standalone: it sets REDIS_URL and ENVIRONMENT, which the handlers read from
//! the process environment. Needs the sandbox Postgres and `TEST_REDIS_URL`.

#![allow(clippy::expect_used)]

mod common;

use actix_web::{test, web, App};
use diesel::prelude::*;
use serde_json::{json, Value};
use uuid::Uuid;

use backend::models::{NewUser, User};

const PASSWORD: &str = "correct horse battery staple";

/// The tests here change process-wide environment variables.
static ENV: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn seed_user(conn: &mut PgConnection, email: &str, mfa: bool) -> User {
    use backend::schema::{user_auth_identities, user_emails, users};
    let uuid = Uuid::new_v4();
    let (mfa_secret, mfa_secret_kek_id) = if mfa {
        let (blob, kek) =
            backend::utils::mfa::encrypt_mfa_secret("JBSWY3DPEHPK3PXP", &uuid).expect("encrypt");
        (Some(blob), Some(kek))
    } else {
        (None, None)
    };
    let user: User = diesel::insert_into(users::table)
        .values(&NewUser {
            uuid,
            name: "Sign-in test".to_string(),
            pronouns: None,
            avatar_url: None,
            banner_url: None,
            avatar_thumb: None,
            microsoft_uuid: None,
            mfa_secret,
            mfa_secret_kek_id,
            mfa_enabled: mfa,
            platform_role: None,
        })
        .get_result(conn)
        .expect("insert user");
    diesel::insert_into(user_emails::table)
        .values((
            user_emails::user_uuid.eq(uuid),
            user_emails::email.eq(email),
            user_emails::email_type.eq("personal"),
            user_emails::is_primary.eq(true),
            user_emails::is_verified.eq(true),
        ))
        .execute(conn)
        .expect("insert email");
    diesel::insert_into(user_auth_identities::table)
        .values((
            user_auth_identities::user_uuid.eq(uuid),
            user_auth_identities::provider_type.eq("local"),
            user_auth_identities::external_id.eq(uuid.to_string()),
            user_auth_identities::password_hash.eq(bcrypt::hash(PASSWORD, 4).expect("hash")),
        ))
        .execute(conn)
        .expect("insert local identity");
    user
}

fn fresh_email() -> String {
    format!("lockout-{}@example.com", Uuid::new_v4().simple())
}

macro_rules! app {
    ($pool:expr) => {
        test::init_service(
            App::new()
                .app_data(web::Data::new($pool.clone()))
                .service(web::scope("/api/auth").configure(backend::handlers::auth::config)),
        )
        .await
    };
}

/// POSTs `body` from one client address; answers (status, JSON body).
macro_rules! post {
    ($app:expr, $path:expr, $body:expr) => {{
        let req = test::TestRequest::post()
            .uri($path)
            .peer_addr("203.0.113.9:4000".parse().expect("addr"))
            .set_json($body)
            .to_request();
        let resp = test::call_service(&$app, req).await;
        let status = resp.status().as_u16();
        let bytes = test::read_body(resp).await;
        (
            status,
            serde_json::from_slice::<Value>(&bytes).unwrap_or(Value::Null),
        )
    }};
}

/// Each route's body, with the given password.
fn bodies(email: &str, password: &str) -> Vec<(&'static str, Value)> {
    vec![
        (
            "/api/auth/mfa-login",
            json!({"email": email, "password": password, "mfa_token": "000000"}),
        ),
        (
            "/api/auth/mfa-setup-login",
            json!({"email": email, "password": password}),
        ),
        (
            "/api/auth/mfa-enable-login",
            json!({"email": email, "password": password, "token": "000000"}),
        ),
        (
            "/api/auth/passkey-setup-login/start",
            json!({"email": email, "password": password}),
        ),
        (
            "/api/auth/passkey-setup-login/finish",
            json!({
                "email": email, "password": password, "id": "x", "rawId": "x",
                "response": {}, "type": "public-key",
            }),
        ),
    ]
}

#[actix_web::test]
async fn an_unreadable_lockout_refuses_sign_in_before_the_password() {
    let _env = ENV.lock().await;
    common::ensure_test_keyring();
    // Nothing listens on port 1. ENVIRONMENT unset assumes production.
    std::env::set_var("REDIS_URL", "redis://127.0.0.1:1");
    std::env::remove_var("ENVIRONMENT");

    let db = common::TestDb::new();
    let pool = db.pool_with_size(4);
    let email = fresh_email();
    seed_user(&mut db.conn(), &email, true);
    let app = app!(pool);

    for password in [PASSWORD, "wrong password"] {
        for (path, body) in bodies(&email, password) {
            let (status, body) = post!(app, path, body);
            assert_eq!(status, 503, "{path}: {body}");
            let error = body["error"].as_str().unwrap_or_default();
            assert!(
                error.starts_with("Authentication service temporarily unavailable"),
                "{path}: {body}"
            );
        }
    }
}

#[actix_web::test]
async fn wrong_mfa_codes_lock_the_account_for_any_password() {
    let _env = ENV.lock().await;
    common::ensure_test_keyring();
    std::env::set_var(
        "REDIS_URL",
        std::env::var("TEST_REDIS_URL").expect("TEST_REDIS_URL"),
    );
    std::env::remove_var("ENVIRONMENT");

    let db = common::TestDb::new();
    let pool = db.pool_with_size(4);
    let email = fresh_email();
    seed_user(&mut db.conn(), &email, true);
    let app = app!(pool);
    let attempt =
        |password: &str| json!({"email": email, "password": password, "mfa_token": "000000"});

    // The fifth wrong code locks.
    for n in 1..=4 {
        let (status, body) = post!(app, "/api/auth/mfa-login", attempt(PASSWORD));
        assert_eq!(status, 400, "attempt {n}: {body}");
    }
    let (status, body) = post!(app, "/api/auth/mfa-login", attempt(PASSWORD));
    assert_eq!(status, 429, "{body}");

    let (right, right_body) = post!(app, "/api/auth/mfa-login", attempt(PASSWORD));
    let (wrong, wrong_body) = post!(app, "/api/auth/mfa-login", attempt("wrong password"));
    assert_eq!(right, 429, "{right_body}");
    assert_eq!(wrong, 429, "{wrong_body}");
    assert_eq!(right_body["code"], wrong_body["code"]);
}

#[actix_web::test]
async fn wrong_passwords_lock_mfa_login() {
    let _env = ENV.lock().await;
    common::ensure_test_keyring();
    std::env::set_var(
        "REDIS_URL",
        std::env::var("TEST_REDIS_URL").expect("TEST_REDIS_URL"),
    );
    std::env::remove_var("ENVIRONMENT");

    let db = common::TestDb::new();
    let pool = db.pool_with_size(4);
    let email = fresh_email();
    seed_user(&mut db.conn(), &email, true);
    let app = app!(pool);
    let body = json!({"email": email, "password": "wrong password", "mfa_token": "000000"});

    for n in 1..=4 {
        let (status, resp) = post!(app, "/api/auth/mfa-login", body.clone());
        assert_eq!(status, 401, "attempt {n}: {resp}");
    }
    let (status, resp) = post!(app, "/api/auth/mfa-login", body);
    assert_eq!(status, 429, "{resp}");
}
