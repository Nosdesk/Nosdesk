//! The app boots on a 1.0.12 database holding realistic data and serves it.
//!
//! `build_server` runs the pending migrations itself, the way an upgraded
//! install does on its first boot (advisory lock, drift check, schema-hash
//! stamp, partition provisioning, seeds, background workers), then a ticket
//! is read over HTTP with an API token created under 1.0.12. Its own binary
//! because `build_server` reads the process env and marks the database
//! initialised process-wide.

#![allow(clippy::expect_used)]

mod common;

use std::net::TcpListener;
use std::path::PathBuf;

use backend::config::Config;
use backend::startup::build_server;

use common::upgrade_1_0_12::{self as fixture, UpgradeDb};

/// The app's upload and search directories, removed however the test ends.
struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[actix_web::test]
async fn the_app_boots_on_an_upgraded_1_0_12_database() {
    let db = UpgradeDb::at_1_0_12();
    let seeded = fixture::seed(&mut db.conn());
    let redis = std::env::var("TEST_REDIS_URL").expect(
        "TEST_REDIS_URL not set. Bring up the dev stack (redis on 127.0.0.1:63799) \
         or set TEST_REDIS_URL; CI provides it.",
    );

    // The only test in this binary, so these can't race another test.
    std::env::set_var("DATABASE_URL", &db.url);
    std::env::set_var("MIGRATION_DATABASE_URL", &db.url);
    std::env::set_var("REDIS_URL", &redis);
    std::env::set_var(
        "MFA_KEK_V1",
        "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
    );
    let scratch =
        Scratch(std::env::temp_dir().join(format!("nosdesk-upgrade-{}", std::process::id())));
    std::env::set_var("NOSDESK_UPLOAD_DIR", scratch.0.join("uploads"));
    std::env::set_var("SEARCH_INDEX_PATH", scratch.0.join("search"));
    // No registry sync: it would fetch https://nosdesk.com/registry.
    std::env::set_var("NOSDESK_REGISTRY_URL", "");

    let config = Config::from_source(&|k| match k {
        "ENVIRONMENT" => Some("development".to_string()),
        "JWT_SECRET" => Some("0123456789abcdef0123456789abcdef01".to_string()),
        "REDIS_URL" => Some(redis.clone()),
        _ => None,
    })
    .expect("test config builds");

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
    let addr = listener.local_addr().expect("local_addr");
    let built = build_server(config, listener)
        .await
        .expect("the app boots on the upgraded database");
    let handle = built.server.handle();
    let scheduler = built.scheduler_shutdown.clone();
    let server_task = actix_web::rt::spawn(built.server);

    let base = format!("http://{addr}");
    let client = reqwest::Client::new();
    let ready = client
        .get(format!("{base}/readiness"))
        .send()
        .await
        .expect("GET /readiness");
    assert_eq!(ready.status().as_u16(), 200, "GET /readiness");

    let ticket = client
        .get(format!("{base}/api/tickets/{}", seeded.tickets.open))
        .bearer_auth(&seeded.api_token)
        .send()
        .await
        .expect("GET the upgraded ticket");
    assert_eq!(
        ticket.status().as_u16(),
        200,
        "a 1.0.12 API token reads a ticket"
    );
    let body: serde_json::Value = ticket.json().await.expect("ticket JSON");
    assert_eq!(body["title"], "Printer on level 3 is jammed", "{body}");

    scheduler.cancel();
    handle.stop(false).await;
    let _ = server_task.await;
}
