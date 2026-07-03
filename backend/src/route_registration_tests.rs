//! Route-registration guard for the `main.rs` route decomposition.
//!
//! As domain routes move from the `main.rs` `App` builder into per-domain
//! `handlers::<domain>::config` functions, these probes assert every route each
//! `config` registers still resolves. A registered route returns 401/4xx/5xx
//! from its auth/DB extractors (or an app-data error), never 404 — so a 404
//! means a route was dropped or renamed during extraction. `{param}` segments
//! accept any value, so the probes use placeholders.
//!
//! These live centrally (rather than one per handler module) so the whole route
//! surface is verifiable in one place; they use the crate-internal
//! `test_helpers`, so they must be unit tests, not integration tests.

use crate::test_helpers::assert_config_registers;

#[actix_web::test]
async fn sse_config_routes_registered() {
    assert_config_registers(crate::handlers::sse::config, &[("POST", "/events/token")]).await;
}

#[actix_web::test]
async fn search_config_routes_registered() {
    assert_config_registers(
        crate::handlers::search::config,
        &[
            ("GET", "/search"),
            ("POST", "/search/rebuild"),
            ("GET", "/search/stats"),
        ],
    )
    .await;
}

#[actix_web::test]
async fn notifications_config_routes_registered() {
    assert_config_registers(
        crate::handlers::notifications::config,
        &[
            ("GET", "/notifications"),
            ("GET", "/notifications/count"),
            ("POST", "/notifications/read"),
            ("POST", "/notifications/read-all"),
            ("GET", "/notifications/preferences"),
            ("PUT", "/notifications/preferences"),
            ("POST", "/notifications/delete"),
        ],
    )
    .await;
}

#[actix_web::test]
async fn bug_reports_config_routes_registered() {
    assert_config_registers(
        crate::handlers::bug_reports::config,
        &[("POST", "/bug-reports")],
    )
    .await;
}
