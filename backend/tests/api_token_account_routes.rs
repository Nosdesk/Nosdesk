//! An API token never reaches an account, through the real route tree
//! (`startup::build_state` + `startup::configure_app`, as `boot_smoke` builds
//! them). Accounts are shared across workspaces and a token is one
//! workspace's automation, so every handler that reads or changes an account
//! carries `extractors::SessionOnly` and answers a token with the same
//! refusal, and the token still works the workspace's records.
//!
//! Its own binary because building the real state sets process env.

#![allow(clippy::expect_used)]

mod common;

use actix_web::dev::ServiceRequest;
use actix_web::{http::StatusCode, test, App};
use serde_json::{json, Value};

use backend::config::Config;
use backend::middleware::RequestLimiter;
use backend::startup::{build_state, configure_app, RequestLimiters};

fn limiter(redis: &str, prefix: &'static str) -> RequestLimiter {
    RequestLimiter::build(
        prefix,
        redis,
        10_000,
        std::time::Duration::from_secs(60),
        move |_: &ServiceRequest| Some(format!("{prefix}:tokens")),
    )
    .expect("build test limiter")
}

/// A user seated in workspace 1 (the one every self-hosted request is in).
fn seated(pool: &common::TestPool, name: &str, role: &str) -> backend::models::User {
    let user = common::insert_user(&mut pool.get().expect("conn"), name);
    diesel::sql_query("UPDATE users SET platform_role = 'user' WHERE uuid = $1")
        .bind::<diesel::sql_types::Uuid, _>(user.uuid)
        .execute(&mut pool.get().expect("conn"))
        .map(|_| ())
        .expect("plain account");
    common::seat(&mut pool.get().expect("conn"), 1, user.uuid, role);
    user
}

use diesel::RunQueryDsl;

#[actix_web::test]
async fn a_token_cannot_reach_account_routes() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let pool = db.pool_with_size(4);
    let redis = std::env::var("TEST_REDIS_URL").expect("TEST_REDIS_URL");
    std::env::set_var("REDIS_URL", &redis);
    std::env::set_var("NOSDESK_REGISTRY_URL", "");
    let config = Config::from_source(&|k| match k {
        "ENVIRONMENT" => Some("development".to_string()),
        "JWT_SECRET" => Some("0123456789abcdef0123456789abcdef01".to_string()),
        "REDIS_URL" => Some(redis.clone()),
        _ => None,
    })
    .expect("test config builds");
    let search_dir = std::env::temp_dir().join(format!(
        "nosdesk-token-routes-search-{}",
        std::process::id()
    ));
    std::env::set_var("SEARCH_INDEX_PATH", &search_dir);
    let limiters = RequestLimiters {
        public: limiter(&redis, "public"),
        public_form: limiter(&redis, "pubform"),
        auth: limiter(&redis, "auth"),
        frontend_logs: limiter(&redis, "felogs"),
        client_errors: limiter(&redis, "clienterr"),
    };
    let (state, _background) =
        build_state(&config, pool.clone(), limiters).expect("build_state succeeds");
    state.scheduler_shutdown.cancel();
    let ws = backend::middleware::WorkspaceContextConfig::initialise(&pool)
        .expect("workspace context initialises");
    let app = test::init_service(
        App::new().configure(|cfg| configure_app(cfg, &state, &pool, &ws, &config)),
    )
    .await;

    let holder = seated(&pool, "Token Holder", "admin");
    let agent = seated(&pool, "Token Agent", "agent");
    let own = common::mint_api_token(&mut pool.get().expect("conn"), &holder, "own");
    // A token an admin made for the agent, the agent acting through it.
    let made_for_agent = {
        let raw = format!("nsk_test_{}", uuid::Uuid::new_v4().simple());
        diesel::insert_into(backend::schema::api_tokens::table)
            .values(&backend::models::NewApiToken {
                token_hash: backend::repository::api_tokens::hash_token(&raw),
                token_prefix: backend::repository::api_tokens::get_token_prefix(&raw),
                user_uuid: agent.uuid,
                name: "for the agent".to_string(),
                scopes: Some(vec![Some("full".to_string())]),
                created_by: holder.uuid,
                expires_at: None,
                role_ceiling: Some("agent".to_string()),
                platform_role_ceiling: None,
            })
            .execute(&mut pool.get().expect("conn"))
            .expect("token for the agent");
        raw
    };

    let call = |bearer: String, req: test::TestRequest| {
        let req = req
            .insert_header(("Authorization", format!("Bearer {bearer}")))
            .to_request();
        let app = &app;
        async move {
            match test::try_call_service(app, req).await {
                Ok(resp) => {
                    let status = resp.status();
                    let body = test::read_body(resp).await;
                    (
                        status,
                        serde_json::from_slice::<Value>(&body).unwrap_or(Value::Null),
                    )
                }
                Err(e) => {
                    let resp = e.error_response();
                    let status = resp.status();
                    let body = actix_web::body::to_bytes(resp.into_body())
                        .await
                        .unwrap_or_default();
                    (
                        status,
                        serde_json::from_slice::<Value>(&body).unwrap_or(Value::Null),
                    )
                }
            }
        }
    };
    let me = holder.uuid;
    let refused = |what: &str, (status, body): (StatusCode, Value)| {
        assert_eq!(status, StatusCode::FORBIDDEN, "{what}: {body}");
        assert_eq!(
            body["error"],
            backend::extractors::SESSION_ONLY,
            "{what}: {body}"
        );
    };

    for (what, req) in [
        (
            "workspaces",
            test::TestRequest::get().uri("/api/me/workspaces"),
        ),
        (
            "identities",
            test::TestRequest::get().uri("/api/users/auth-identities"),
        ),
        (
            "emails",
            test::TestRequest::get().uri(&format!("/api/users/{me}/emails")),
        ),
        (
            "add an email",
            test::TestRequest::post()
                .uri(&format!("/api/users/{me}/emails"))
                .set_json(json!({ "email": "second@example.com" })),
        ),
        (
            "with emails",
            test::TestRequest::get().uri(&format!("/api/users/{me}/with-emails")),
        ),
        (
            "security info",
            test::TestRequest::get().uri(&format!("/api/users/{me}/security-info")),
        ),
        (
            "own profile",
            test::TestRequest::put()
                .uri(&format!("/api/users/{}", me.to_string().to_uppercase()))
                .set_json(json!({ "name": "Renamed by a token" })),
        ),
        (
            "notification preferences",
            test::TestRequest::put()
                .uri("/api/notifications/preferences")
                .set_json(json!({})),
        ),
        (
            "interrupt preferences",
            test::TestRequest::put()
                .uri("/api/notifications/interrupt-preferences")
                .set_json(json!({})),
        ),
        (
            "push device",
            test::TestRequest::post()
                .uri("/api/notifications/devices")
                .set_json(json!({})),
        ),
    ] {
        refused(what, call(own.clone(), req).await);
    }

    // An admin's token acting as the agent doesn't speak for them on their
    // own profile.
    refused(
        "profile fields for someone else",
        call(
            made_for_agent.clone(),
            test::TestRequest::put()
                .uri(&format!("/api/users/{}/profile-fields", agent.uuid))
                .set_json(json!({ "display_name": "Set by the admin", "custom_fields": {} })),
        )
        .await,
    );

    // The profile bundle loads for a token, without the account's addresses.
    let (status, body) = call(
        own.clone(),
        test::TestRequest::get().uri(&format!("/api/users/{me}/profile?include=emails")),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.get("emails").is_none_or(Value::is_null), "{body}");

    // The token still works the workspace's records. (A signed-in session
    // passing these routes is `it::admin_workspace_members`, which reads
    // `/api/me/workspaces` with one.)
    let (status, body) = call(own.clone(), test::TestRequest::get().uri("/api/tickets")).await;
    assert_eq!(status, StatusCode::OK, "token on tickets: {body}");
}
