//! The public setup status says whether the first admin will be asked for a
//! second factor, so the setup page only promises that step when it follows.

use actix_web::test as http_test;
use actix_web::{web, App};

use crate::common;

#[actix_web::test]
async fn setup_status_says_whether_admins_need_a_second_factor() {
    let db = common::TestDb::new();
    let app = http_test::init_service(App::new().app_data(web::Data::new(db.pool())).route(
        "/api/auth/setup/status",
        web::get().to(backend::handlers::check_setup_status),
    ))
    .await;

    let req = http_test::TestRequest::get()
        .uri("/api/auth/setup/status")
        .to_request();
    let body: serde_json::Value = http_test::call_and_read_body_json(&app, req).await;

    assert_eq!(
        body["admin_mfa_required"].as_bool(),
        Some(backend::utils::mfa::admin_mfa_required()),
        "the status carries the REQUIRE_ADMIN_MFA posture: {body}"
    );
}
