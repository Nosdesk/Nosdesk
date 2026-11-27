//! A sign-in email's link changes nothing when it is opened: mail scanners
//! open every link on delivery. It lands on the portal's sign-in page, and
//! only that page's POST spends the link, signs in and marks the address
//! proven.

use actix_web::dev::Service as _;
use actix_web::{test as http_test, web, App, HttpMessage as _};
use diesel::prelude::*;

use backend::schema::{reset_tokens, user_emails};
use backend::utils::reset_tokens::{ResetTokenUtils, TokenType};

use crate::common::{self, TestDb};
use crate::portal_session::context_of;

#[actix_web::test]
async fn only_the_sign_in_pages_post_spends_the_link_and_proves_the_address() {
    common::ensure_test_keyring();
    let db = TestDb::new();
    let seeded = common::seed_two_workspaces(&mut db.pool_with_size(2).get().expect("conn"));
    let ws = seeded.a;
    let person = ws.member_uuid;
    let mut c = db.conn();
    diesel::insert_into(user_emails::table)
        .values((
            user_emails::user_uuid.eq(person),
            user_emails::email.eq("member@example.com"),
            user_emails::email_type.eq("personal"),
            user_emails::is_primary.eq(true),
            user_emails::is_verified.eq(false),
        ))
        .execute(&mut c)
        .expect("address");
    let issued = ResetTokenUtils::create_reset_token(person, TokenType::PortalMagicLink);
    backend::repository::reset_tokens::create_reset_token(
        &mut c,
        &issued.token_hash,
        person,
        TokenType::PortalMagicLink.as_str(),
        None,
        None,
        issued.expires_at,
        None,
    )
    .expect("mint token");
    let pool = db.runtime_pool(4);
    let ctx = context_of(&mut pool.get().expect("conn"), ws.workspace_id);
    let app = http_test::init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .wrap_fn(move |req, srv| {
                req.extensions_mut().insert(ctx.clone());
                srv.call(req)
            })
            .service(
                web::scope("/api/portal/auth").configure(backend::handlers::portal::auth_config),
            ),
    )
    .await;
    let used = |c: &mut backend::db::DbConnection| -> bool {
        reset_tokens::table
            .find(&issued.token_hash)
            .select(reset_tokens::is_used)
            .first(c)
            .expect("token")
    };
    let proven = |c: &mut backend::db::DbConnection| -> Option<chrono::DateTime<chrono::Utc>> {
        user_emails::table
            .filter(user_emails::user_uuid.eq(person))
            .select(user_emails::proven_at)
            .first(c)
            .expect("address")
    };

    // A scanner (or anyone) opening the link, twice.
    for _ in 0..2 {
        let resp = http_test::call_service(
            &app,
            http_test::TestRequest::get()
                .uri(&format!(
                    "/api/portal/auth/callback?token={}",
                    issued.raw_token
                ))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), 302);
        let location = resp
            .headers()
            .get("location")
            .expect("location")
            .to_str()
            .expect("ascii");
        assert!(location.contains("/sign-in?token="), "{location}");
        assert!(
            resp.headers().get("set-cookie").is_none(),
            "no session from a GET"
        );
    }
    assert!(!used(&mut c), "opening the link spends nothing");
    assert_eq!(proven(&mut c), None, "and proves nothing");

    // The page's Continue.
    let continue_with = |token: &str| {
        http_test::TestRequest::post()
            .uri("/api/portal/auth/callback")
            .set_json(serde_json::json!({ "token": token }))
            .to_request()
    };
    let resp = http_test::call_service(&app, continue_with(&issued.raw_token)).await;
    assert_eq!(resp.status(), 200);
    assert!(
        resp.headers().get_all("set-cookie").count() > 0,
        "a session"
    );
    assert!(used(&mut c));
    assert!(proven(&mut c).is_some(), "the mailbox answered");

    let again = http_test::call_service(&app, continue_with(&issued.raw_token)).await;
    assert_eq!(again.status(), 400, "the link works once");
}
