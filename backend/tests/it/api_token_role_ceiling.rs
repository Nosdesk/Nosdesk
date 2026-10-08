//! An API token made for someone else acts at no more than the role it was
//! made for. When its holder's role rises above that, or its maker is no
//! longer an admin of the workspace, the token stops authenticating, so no
//! route, sync stream or collab connection sees the higher role.

use actix_web::dev::Service;
use actix_web::http::StatusCode;
use actix_web::test as http_test;
use actix_web::{web, App, HttpMessage};
use diesel::prelude::*;
use serde_json::json;
use uuid::Uuid;

use backend::extractors::WorkspaceContext;
use backend::middleware::RequestContext;
use backend::models::Claims;
use backend::repository::workspaces::{self, SeatWriteAuthority};
use backend::sync::actor::ActorContext;
use backend::sync::session::run_in_workspace;

use crate::common;

const REF: &str = "test:api_token_role_ceiling";

fn workspace_context(seed: &common::WorkspaceSeed) -> WorkspaceContext {
    WorkspaceContext {
        workspace_id: seed.workspace_id,
        workspace_uuid: seed.workspace_uuid,
        slug: seed.slug.clone(),
        name: "A".to_string(),
        organisation_id: None,
        custom_domain: None,
    }
}

/// Mint a token for `target` as `minter`, through the admin route.
async fn mint(
    pool: &common::TestPool,
    seed: &common::WorkspaceSeed,
    minter: Uuid,
    target: Uuid,
) -> String {
    let workspace = workspace_context(seed);
    let now = chrono::Utc::now().timestamp();
    let claims = Claims {
        sub: minter.to_string(),
        name: "Admin".to_string(),
        email: "admin@example.com".to_string(),
        platform_role: "user".to_string(),
        scope: "full".to_string(),
        sid: None,
        workspace_uuid: None,
        exp: (now + 3600) as usize,
        iat: now as usize,
    };
    let corr = Uuid::now_v7();
    let actor = ActorContext::user(minter, Some(corr)).with_workspace(seed.workspace_id);
    let app = http_test::init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .wrap_fn(move |req, srv| {
                req.extensions_mut().insert(workspace.clone());
                req.extensions_mut().insert(claims.clone());
                req.extensions_mut()
                    .insert(RequestContext::new(corr, actor.clone()));
                srv.call(req)
            })
            .service(web::scope("/api").configure(backend::handlers::api_tokens::config)),
    )
    .await;
    let resp = http_test::call_service(
        &app,
        http_test::TestRequest::post()
            .uri("/api/admin/api-tokens")
            .set_json(json!({ "name": "ci", "user_uuid": target }))
            .to_request(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    let body: serde_json::Value = http_test::read_body_json(resp).await;
    body["token"].as_str().expect("token").to_string()
}

/// The response status, or the status of the error the auth middleware
/// answered with.
fn status_of<B>(
    result: Result<actix_web::dev::ServiceResponse<B>, actix_web::Error>,
) -> StatusCode {
    match result {
        Ok(resp) => resp.status(),
        Err(e) => e.as_response_error().status_code(),
    }
}

/// Statuses for an admin route, a staff sync stream and a collab token mint,
/// presented with `token` through the real auth middleware.
async fn statuses(
    pool: &common::TestPool,
    seed: &common::WorkspaceSeed,
    token: &str,
) -> [StatusCode; 3] {
    let workspace = workspace_context(seed);
    let app = http_test::init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .wrap(actix_web::middleware::from_fn(
                backend::middleware::dual_auth_middleware,
            ))
            .wrap_fn(move |req, srv| {
                req.extensions_mut().insert(workspace.clone());
                srv.call(req)
            })
            .route(
                "/api/collaboration/token",
                web::post().to(backend::handlers::collaboration::get_collab_token),
            )
            .service(
                web::scope("/api")
                    .configure(backend::handlers::api_tokens::config)
                    .configure(backend::handlers::sync::config),
            ),
    )
    .await;
    let bearer = ("Authorization", format!("Bearer {token}"));
    let admin = status_of(
        http_test::try_call_service(
            &app,
            http_test::TestRequest::get()
                .uri("/api/admin/api-tokens")
                .insert_header(bearer.clone())
                .to_request(),
        )
        .await,
    );
    let sync = status_of(
        http_test::try_call_service(
            &app,
            http_test::TestRequest::get()
                .uri("/api/sync/bootstrap?groups=workspace")
                .insert_header(bearer.clone())
                .to_request(),
        )
        .await,
    );
    let collab = status_of(
        http_test::try_call_service(
            &app,
            http_test::TestRequest::post()
                .uri("/api/collaboration/token")
                .insert_header(bearer)
                .to_request(),
        )
        .await,
    );
    [admin, sync, collab]
}

fn set_role(pool: &common::TestPool, seed: &common::WorkspaceSeed, user: Uuid, role: &str) {
    run_in_workspace(pool, REF, seed.workspace_id, |c| {
        workspaces::update_membership_role(
            c,
            seed.workspace_id,
            user,
            role,
            SeatWriteAuthority::Product,
        )
    })
    .expect("role change");
}

#[actix_web::test]
async fn a_token_made_for_a_member_stops_when_they_are_promoted() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let seeded = common::seed_two_workspaces(&mut db.pool_with_size(2).get().expect("conn"));
    let pool = db.runtime_pool(4);
    let a = seeded.a.clone();
    {
        let mut c = db.conn();
        diesel::sql_query("UPDATE workspaces SET seat_limit = NULL WHERE id = $1")
            .bind::<diesel::sql_types::Integer, _>(a.workspace_id)
            .execute(&mut c)
            .expect("unlimited seats");
    }
    let token = mint(&pool, &a, a.admin_uuid, a.member_uuid).await;

    // As a member: no admin route, but the token authenticates.
    let [admin, sync, collab] = statuses(&pool, &a, &token).await;
    assert_eq!(admin, StatusCode::FORBIDDEN);
    assert_ne!(sync, StatusCode::UNAUTHORIZED);
    assert_eq!(collab, StatusCode::OK);

    // Promoted to admin: the token was made for a member, so it stops.
    set_role(&pool, &a, a.member_uuid, "admin");
    assert_eq!(
        statuses(&pool, &a, &token).await,
        [StatusCode::UNAUTHORIZED; 3],
        "admin route, staff sync, collab mint"
    );
}

#[actix_web::test]
async fn a_token_made_for_someone_else_stops_with_its_maker_or_a_platform_role() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let seeded = common::seed_two_workspaces(&mut db.pool_with_size(2).get().expect("conn"));
    let pool = db.runtime_pool(4);
    let a = seeded.a.clone();

    // The maker leaves the workspace.
    let token = mint(&pool, &a, a.admin_uuid, a.member_uuid).await;
    assert_ne!(
        statuses(&pool, &a, &token).await[2],
        StatusCode::UNAUTHORIZED
    );
    run_in_workspace(&pool, REF, a.workspace_id, |c| {
        workspaces::remove_membership(c, a.workspace_id, a.admin_uuid, SeatWriteAuthority::Product)
    })
    .expect("remove the maker");
    assert_eq!(
        statuses(&pool, &a, &token).await,
        [StatusCode::UNAUTHORIZED; 3]
    );

    // A platform role the token wasn't made with (workspace B's pair).
    let b = seeded.b.clone();
    let token = mint(&pool, &b, b.admin_uuid, b.member_uuid).await;
    diesel::sql_query("UPDATE users SET platform_role = 'platform_admin' WHERE uuid = $1")
        .bind::<diesel::sql_types::Uuid, _>(b.member_uuid)
        .execute(&mut db.conn())
        .expect("platform role");
    assert_eq!(
        statuses(&pool, &b, &token).await,
        [StatusCode::UNAUTHORIZED; 3]
    );
}

#[actix_web::test]
async fn a_token_made_for_oneself_follows_ones_role() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let seeded = common::seed_two_workspaces(&mut db.pool_with_size(2).get().expect("conn"));
    let pool = db.runtime_pool(4);
    let a = seeded.a.clone();
    let token = mint(&pool, &a, a.admin_uuid, a.admin_uuid).await;
    assert_eq!(statuses(&pool, &a, &token).await[0], StatusCode::OK);

    // Demoted: still authenticates, at the lower role.
    set_role(&pool, &a, a.admin_uuid, "member");
    assert_eq!(statuses(&pool, &a, &token).await[0], StatusCode::FORBIDDEN);
}

/// API tokens can't manage API tokens: otherwise a token made for an admin
/// could mint itself a successor with no ceiling that outlives its maker.
#[actix_web::test]
async fn a_token_cant_mint_or_revoke_tokens() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let seeded = common::seed_two_workspaces(&mut db.pool_with_size(2).get().expect("conn"));
    let pool = db.runtime_pool(4);
    let a = seeded.a.clone();
    {
        let mut c = db.conn();
        diesel::sql_query("UPDATE workspaces SET seat_limit = NULL WHERE id = $1")
            .bind::<diesel::sql_types::Integer, _>(a.workspace_id)
            .execute(&mut c)
            .expect("unlimited seats");
    }
    // The maker makes a token for another admin.
    set_role(&pool, &a, a.member_uuid, "admin");
    let token = mint(&pool, &a, a.admin_uuid, a.member_uuid).await;
    assert_eq!(statuses(&pool, &a, &token).await[0], StatusCode::OK);

    let workspace = workspace_context(&a);
    let app = http_test::init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .wrap(actix_web::middleware::from_fn(
                backend::middleware::dual_auth_middleware,
            ))
            .wrap_fn(move |req, srv| {
                req.extensions_mut().insert(workspace.clone());
                srv.call(req)
            })
            .service(web::scope("/api").configure(backend::handlers::api_tokens::config)),
    )
    .await;
    let bearer = ("Authorization", format!("Bearer {token}"));
    let mint_with_token = status_of(
        http_test::try_call_service(
            &app,
            http_test::TestRequest::post()
                .uri("/api/admin/api-tokens")
                .insert_header(bearer.clone())
                .set_json(json!({ "name": "successor", "user_uuid": a.member_uuid }))
                .to_request(),
        )
        .await,
    );
    assert_eq!(mint_with_token, StatusCode::FORBIDDEN);
    let revoke_with_token = status_of(
        http_test::try_call_service(
            &app,
            http_test::TestRequest::delete()
                .uri(&format!("/api/admin/api-tokens/{}", Uuid::new_v4()))
                .insert_header(bearer)
                .to_request(),
        )
        .await,
    );
    assert_eq!(revoke_with_token, StatusCode::FORBIDDEN);
}
