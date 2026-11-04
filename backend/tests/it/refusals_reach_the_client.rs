//! A request a middleware refuses reaches the client with the refusal's own
//! status and body. The request log renders the error before actix writes it
//! out; a response that could be rendered only once went out as an empty 500,
//! so clients never saw the 401 that tells them to refresh.

#![allow(clippy::expect_used)]

use actix_web::dev::Service as _;
use actix_web::middleware::from_fn;
use actix_web::{test, web, App, HttpMessage as _, HttpResponse};

/// What the client receives: the handler's response, or the refusal rendered
/// as actix renders it (after the log has rendered it once already).
macro_rules! received {
    ($app:expr, $req:expr) => {
        match test::try_call_service($app, $req).await {
            Ok(resp) => (resp.status().as_u16(), test::read_body(resp).await.to_vec()),
            Err(e) => {
                let resp = e.error_response();
                let status = resp.status().as_u16();
                let body = actix_web::body::to_bytes(resp.into_body())
                    .await
                    .expect("body")
                    .to_vec();
                (status, body)
            }
        }
    };
}

fn code(body: &[u8]) -> String {
    serde_json::from_slice::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v["code"].as_str().map(str::to_owned))
        .unwrap_or_default()
}

#[actix_web::test]
async fn a_bad_token_is_a_401_on_the_wire() {
    crate::common::ensure_test_keyring();
    let db = crate::common::TestDb::new();
    let pool = db.pool_with_size(2);
    let mut conn = pool.get().expect("conn");
    let ws = crate::common::mint_workspace(&mut conn, "refusalws", "Refusal WS");
    let found = backend::repository::workspaces::find_by_id(&mut conn, ws)
        .expect("lookup")
        .expect("workspace");
    let ctx = backend::extractors::WorkspaceContext {
        workspace_id: found.id,
        workspace_uuid: found.uuid,
        slug: found.slug,
        name: found.name,
        custom_domain: found.custom_domain,
        organisation_id: found.organisation_id,
    };
    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .wrap(from_fn(
                backend::middleware::request_context::log_rejections,
            ))
            .wrap_fn(move |req, srv| {
                req.extensions_mut().insert(ctx.clone());
                srv.call(req)
            })
            .service(
                web::scope("/api/portal")
                    .wrap(from_fn(backend::handlers::portal::portal_auth_middleware))
                    .route("/me", web::get().to(HttpResponse::Ok)),
            )
            .service(
                web::scope("/api")
                    .wrap(from_fn(backend::middleware::dual_auth_middleware))
                    .route("/tickets", web::get().to(HttpResponse::Ok)),
            ),
    )
    .await;
    let forged = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJ4In0.bad";

    let (status, body) = received!(
        &app,
        test::TestRequest::get()
            .uri("/api/portal/me")
            .insert_header(("Cookie", format!("portal_access={forged}")))
            .to_request()
    );
    assert_eq!(status, 401, "the portal");
    assert_eq!(code(&body), "INVALID_TOKEN");

    let (status, body) = received!(
        &app,
        test::TestRequest::get()
            .uri("/api/tickets")
            .insert_header(("Authorization", format!("Bearer {forged}")))
            .to_request()
    );
    assert_eq!(status, 401, "the agent API");
    assert_eq!(code(&body), "INVALID_TOKEN");
}
