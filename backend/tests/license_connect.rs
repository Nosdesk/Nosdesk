//! Connecting to Nosdesk Cloud from the instance side, against a scripted
//! control plane: the code is shown, the poll follows the cloud's answer
//! (denied, expired), and an unreachable cloud is reported, not fatal.
//!
//! Installing the licence a real approval delivers needs a Nosdesk-signed
//! token, which cannot be minted here; that path is covered by the licence
//! unit tests and the staging walk. Standalone binary: it sets
//! `NOSDESK_RELAY_URL` and drives process-wide link state.

#![allow(clippy::expect_used)]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use actix_web::{web, App, HttpResponse};

use backend::services::license_cloud::{self, CloudError};

mod common;

/// A control plane that answers `start` and then `poll` with `answer`.
fn scripted_cloud(answer: &'static str, polls: Arc<AtomicUsize>) -> actix_test::TestServer {
    actix_test::start(move || {
        let polls = polls.clone();
        App::new()
            .route(
                "/api/relay/v1/link/start",
                web::post().to(|| async {
                    HttpResponse::Ok().json(serde_json::json!({
                        "device_code": "d".repeat(43),
                        "user_code": "BCDF-GHJK",
                        "verification_uri": "https://manage.example/account/license/link",
                        "verification_uri_complete": "https://manage.example/account/license/link?code=BCDF-GHJK",
                        "expires_in": 900,
                        "interval": 1,
                    }))
                }),
            )
            .route(
                "/api/relay/v1/link/poll",
                web::post().to(move || {
                    let polls = polls.clone();
                    async move {
                        let n = polls.fetch_add(1, Ordering::SeqCst);
                        // One pending answer first, as a real approval takes a moment.
                        let error = if n == 0 { "authorization_pending" } else { answer };
                        HttpResponse::BadRequest().json(serde_json::json!({ "error": error }))
                    }
                }),
            )
    })
}

async fn wait_for_status(want: &str) -> Option<String> {
    // Up to 15s: the scripted poll interval is 1s, but a loaded machine lags.
    for _ in 0..100 {
        let status = license_cloud::current_link().map(|l| l.status.to_string());
        if status.as_deref() == Some(want) {
            return status;
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
    license_cloud::current_link().map(|l| l.status.to_string())
}

#[actix_web::test]
async fn the_poll_follows_the_clouds_answer() {
    common::ensure_test_keyring();
    let test_db = common::TestDb::new();
    let pool = test_db.pool_with_size(2);

    // Denied by the admin.
    let polls = Arc::new(AtomicUsize::new(0));
    let srv = scripted_cloud("access_denied", polls.clone());
    std::env::set_var("NOSDESK_RELAY_URL", srv.url(""));
    let view = license_cloud::start_link(
        pool.clone(),
        None,
        "inst-1".into(),
        Some("helpdesk.local".into()),
        None,
    )
    .await
    .expect("start");
    assert_eq!(view.user_code, "BCDF-GHJK");
    assert_eq!(view.status, "pending");
    assert!(view.verification_uri_complete.ends_with("?code=BCDF-GHJK"));
    assert_eq!(wait_for_status("denied").await.as_deref(), Some("denied"));
    assert!(
        polls.load(Ordering::SeqCst) >= 2,
        "kept polling through authorization_pending"
    );

    // Expired before anyone approved it.
    let polls = Arc::new(AtomicUsize::new(0));
    let srv = scripted_cloud("expired_token", polls);
    std::env::set_var("NOSDESK_RELAY_URL", srv.url(""));
    license_cloud::start_link(pool.clone(), None, "inst-1".into(), None, None)
        .await
        .expect("start");
    assert_eq!(wait_for_status("expired").await.as_deref(), Some("expired"));

    // Cancelling forgets it.
    license_cloud::cancel_link();
    assert!(license_cloud::current_link().is_none());

    // Nothing listening: reported as unreachable, nothing left running.
    std::env::set_var("NOSDESK_RELAY_URL", "http://127.0.0.1:9");
    let err = license_cloud::start_link(pool.clone(), None, "inst-1".into(), None, None)
        .await
        .expect_err("no cloud");
    assert_eq!(err, CloudError::Unreachable);
    assert!(license_cloud::current_link().is_none());

    // With no stored licence there is nothing to renew, and nothing is asked.
    assert_eq!(
        license_cloud::refresh(pool.clone(), None).await,
        license_cloud::RefreshOutcome::NotApplicable
    );
}
