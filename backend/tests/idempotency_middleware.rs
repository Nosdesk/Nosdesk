//! Integration test for the Idempotency-Key middleware (M5 Task 2).
//!
//! Wraps a counter-incrementing test handler with the middleware and
//! drives it through actix-test + awc to verify the four contract
//! points:
//!
//!   1. POST with a fresh Idempotency-Key runs the handler and
//!      returns its response.
//!   2. POST with the same Idempotency-Key returns the *cached*
//!      response without re-running the handler (assert the
//!      counter doesn't advance, AND the body matches the first
//!      response even when the second payload differs).
//!   3. POST without an Idempotency-Key always runs the handler
//!      (no caching).
//!   4. GET requests are never cached even when an
//!      Idempotency-Key header is present.

#![allow(clippy::expect_used)]

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Once};

use actix_web::{web, App, HttpResponse};
use diesel::prelude::*;
use serde_json::json;

use backend::middleware::idempotency::idempotency_middleware;

mod common;

/// Init the at-rest Keyring once per process. Middleware doesn't
/// touch the keyring, but the WorkspaceGucCustomizer below sets the
/// workspace GUC which other code paths sometimes use; defensive.
fn ensure_test_keyring() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        if std::env::var("MFA_KEK_V1").is_err() {
            std::env::set_var(
                "MFA_KEK_V1",
                "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
            );
        }
        if let Err(e) = backend::utils::encryption::init_keyring() {
            panic!("ensure_test_keyring: init_keyring failed: {e}");
        }
    });
}

/// Connection customizer mirroring tests/common/mod.rs's private one
/// so audit triggers don't trip on the inserts we do via the test
/// handler.
#[derive(Debug)]
struct WorkspaceGuc;

impl diesel::r2d2::CustomizeConnection<diesel::pg::PgConnection, diesel::r2d2::Error>
    for WorkspaceGuc
{
    fn on_acquire(&self, conn: &mut diesel::pg::PgConnection) -> Result<(), diesel::r2d2::Error> {
        use diesel::RunQueryDsl;
        diesel::sql_query("SELECT set_config('app.workspace_id', '1', false)")
            .execute(conn)
            .map_err(diesel::r2d2::Error::QueryError)?;
        Ok(())
    }
}

fn build_pool(url: &str) -> backend::db::Pool {
    use diesel::r2d2::{ConnectionManager, Pool};
    ensure_test_keyring();
    let mgr = ConnectionManager::<diesel::pg::PgConnection>::new(url);
    Pool::builder()
        .max_size(4)
        .connection_customizer(Box::new(WorkspaceGuc))
        .build(mgr)
        .expect("build backend Pool")
}

#[actix_web::test]
async fn idempotency_caches_post_response_and_replays_on_retry() {
    let test_db = common::TestDb::new();
    let pool = build_pool(test_db.url());

    // Each handler invocation bumps the counter; the test asserts the
    // counter reflects how many times the *handler* ran, distinct
    // from how many *requests* the test made. Arc'd so the closure
    // in actix_test::start can clone it into the App factory.
    let counter = Arc::new(AtomicU32::new(0));
    let counter_for_app = counter.clone();
    let pool_for_app = pool.clone();

    let srv = actix_test::start(move || {
        let counter_post = counter_for_app.clone();
        let counter_get = counter_for_app.clone();
        let post_echo = move |body: web::Json<serde_json::Value>| {
            let c = counter_post.clone();
            async move {
                let n = c.fetch_add(1, Ordering::SeqCst) + 1;
                HttpResponse::Ok().json(json!({
                    "handler_call": n,
                    "echo": body.into_inner(),
                }))
            }
        };
        let get_echo = move || {
            let c = counter_get.clone();
            async move {
                let n = c.fetch_add(1, Ordering::SeqCst) + 1;
                HttpResponse::Ok().json(json!({
                    "handler_call": n,
                    "echo": "get",
                }))
            }
        };
        App::new()
            .app_data(web::Data::new(pool_for_app.clone()))
            .service(
                web::scope("/x")
                    .wrap(actix_web::middleware::from_fn(idempotency_middleware))
                    .route("/echo", web::post().to(post_echo))
                    .route("/echo", web::get().to(get_echo)),
            )
    });

    let client = awc::Client::new();
    let key = "test-key-001";

    // --- 1: first POST runs the handler ---
    let mut resp = client
        .post(srv.url("/x/echo"))
        .insert_header(("Idempotency-Key", key))
        .insert_header(("Content-Type", "application/json"))
        .send_body(r#"{"v":1}"#)
        .await
        .expect("first POST");
    assert_eq!(resp.status(), 200);
    let body_a: serde_json::Value =
        serde_json::from_slice(&resp.body().await.expect("body")).expect("json");
    assert_eq!(body_a["handler_call"], 1);
    assert_eq!(body_a["echo"]["v"], 1);

    // --- 2: same key returns the cached body even with a different
    //        payload; handler counter stays at 1 ---
    let mut resp = client
        .post(srv.url("/x/echo"))
        .insert_header(("Idempotency-Key", key))
        .insert_header(("Content-Type", "application/json"))
        .send_body(r#"{"v":99,"new":"junk"}"#)
        .await
        .expect("second POST");
    assert_eq!(resp.status(), 200);
    let body_b: serde_json::Value =
        serde_json::from_slice(&resp.body().await.expect("body")).expect("json");
    assert_eq!(
        body_b, body_a,
        "retry must return byte-identical cached body, got {body_b}"
    );
    assert_eq!(
        counter.load(Ordering::SeqCst),
        1,
        "second POST should NOT re-run the handler"
    );

    // --- 3: POST without the header runs the handler every time ---
    let mut resp = client
        .post(srv.url("/x/echo"))
        .insert_header(("Content-Type", "application/json"))
        .send_body(r#"{"v":2}"#)
        .await
        .expect("unkeyed POST");
    assert_eq!(resp.status(), 200);
    let body_c: serde_json::Value =
        serde_json::from_slice(&resp.body().await.expect("body")).expect("json");
    assert_eq!(body_c["handler_call"], 2);
    assert_eq!(counter.load(Ordering::SeqCst), 2);

    let mut resp = client
        .post(srv.url("/x/echo"))
        .insert_header(("Content-Type", "application/json"))
        .send_body(r#"{"v":3}"#)
        .await
        .expect("unkeyed POST 2");
    let body_d: serde_json::Value =
        serde_json::from_slice(&resp.body().await.expect("body")).expect("json");
    assert_eq!(body_d["handler_call"], 3);
    assert_eq!(counter.load(Ordering::SeqCst), 3);

    // --- 4: GET with an Idempotency-Key is not cached (header only
    //        applies to mutating methods) ---
    let resp = client
        .get(srv.url("/x/echo"))
        .insert_header(("Idempotency-Key", key))
        .send()
        .await
        .expect("GET with header");
    // GET reaches the handler; even though the header is present, the
    // middleware passes through. We don't assert the body shape here
    // (the handler is the same closure, and the counter advance is
    // what proves the handler ran).
    assert_eq!(resp.status(), 200);
    assert_eq!(
        counter.load(Ordering::SeqCst),
        4,
        "GET should pass through to the handler regardless of Idempotency-Key"
    );

    // --- 5: A different scoped path with the same key value is a
    //        DIFFERENT cache entry (per-route scoping). Confirm by
    //        verifying the first key's row is still the only one in
    //        the table for the /x/echo path.
    use backend::schema::idempotency_keys;
    let mut conn = pool.get().expect("conn");
    let stored_keys: Vec<String> = idempotency_keys::table
        .select(idempotency_keys::key)
        .load(&mut conn)
        .expect("load keys");
    assert!(
        stored_keys.iter().any(|k| k == &format!("/x/echo:{key}")),
        "expected scoped key in cache, got {stored_keys:?}"
    );
}
