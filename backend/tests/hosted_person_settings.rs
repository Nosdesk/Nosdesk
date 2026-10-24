//! On hosted, a workspace admin manages a requester's account (name, email
//! addresses) but not a staff member's, whose Nosdesk account owns it, and
//! not anyone's sign-in, which stays the person's own there.
//!
//! A standalone binary rather than part of `tests/it`: deployment mode is
//! process-wide environment.

#![allow(clippy::expect_used)]

mod common;

use std::sync::Arc;

use actix_web::dev::Service;
use actix_web::{web, App, HttpMessage};
use diesel::prelude::*;
use serde_json::json;
use uuid::Uuid;

use backend::extractors::WorkspaceContext;
use backend::middleware::RequestContext;
use backend::models::{Claims, NewUser, User};
use backend::services::search::SearchService;
use backend::sync::actor::ActorContext;

fn agent(conn: &mut PgConnection, workspace_id: i32) -> User {
    use backend::schema::{users, workspace_members};
    let user: User = diesel::insert_into(users::table)
        .values(&NewUser {
            uuid: Uuid::new_v4(),
            name: "Agent".to_string(),
            pronouns: None,
            avatar_url: None,
            banner_url: None,
            avatar_thumb: None,
            microsoft_uuid: None,
            mfa_secret: None,
            mfa_secret_kek_id: None,
            mfa_enabled: false,
            platform_role: None,
        })
        .get_result(conn)
        .expect("insert user");
    diesel::insert_into(workspace_members::table)
        .values((
            workspace_members::workspace_id.eq(workspace_id),
            workspace_members::user_uuid.eq(user.uuid),
            workspace_members::role.eq("agent"),
        ))
        .execute(conn)
        .expect("insert member");
    user
}

#[actix_web::test]
async fn a_workspace_admin_manages_requesters_but_not_staff_on_hosted() {
    std::env::set_var("NOSDESK_DEPLOYMENT_MODE", "hosted");
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let pool = db.pool_with_size(4);
    let (ws, staff) = {
        let mut conn = pool.get().expect("conn");
        let seeded = common::seed_two_workspaces(&mut conn);
        let staff = agent(&mut conn, seeded.a.workspace_id);
        (seeded.a, staff)
    };

    let claims = Claims {
        sub: ws.admin_uuid.to_string(),
        name: "Admin".to_string(),
        email: String::new(),
        platform_role: "user".to_string(),
        scope: "full".to_string(),
        sid: None,
        workspace_uuid: None,
        exp: (chrono::Utc::now().timestamp() + 3600) as usize,
        iat: chrono::Utc::now().timestamp() as usize,
    };
    let (ws_id, ws_uuid, slug, admin) = (
        ws.workspace_id,
        ws.workspace_uuid,
        ws.slug.clone(),
        ws.admin_uuid,
    );
    let search_dir = Arc::new(tempfile::tempdir().expect("search dir"));
    let search = Arc::new(SearchService::new(search_dir.path(), &pool).expect("init search"));
    let app_pool = pool.clone();
    let srv = actix_test::start(move || {
        let _keep = &search_dir;
        let claims = claims.clone();
        let corr = Uuid::now_v7();
        let actor = ActorContext::user(admin, Some(corr)).with_workspace(ws_id);
        let ctx = WorkspaceContext {
            workspace_id: ws_id,
            workspace_uuid: ws_uuid,
            slug: slug.clone(),
            name: "A".to_string(),
            organisation_id: None,
            custom_domain: None,
        };
        App::new()
            .app_data(web::Data::new(app_pool.clone()))
            .app_data(web::Data::new(search.clone()))
            .wrap_fn(move |req, srv| {
                req.extensions_mut().insert(ctx.clone());
                req.extensions_mut().insert(claims.clone());
                req.extensions_mut()
                    .insert(RequestContext::new(corr, actor.clone()));
                srv.call(req)
            })
            .route(
                "/users/{uuid}/emails",
                web::get().to(backend::handlers::users::get_user_emails),
            )
            .route(
                "/users/{uuid}/security-info",
                web::get().to(backend::handlers::users::get_user_security_info),
            )
            .route(
                "/users/{uuid}/profile",
                web::get().to(backend::handlers::users::get_user_profile_bundle),
            )
            .route(
                "/users/{uuid}",
                web::put().to(backend::handlers::users::update_user_by_uuid),
            )
    });
    let client = awc::Client::new();

    // A requester: their account is the workspace's to manage.
    let resp = client
        .get(srv.url(&format!("/users/{}/emails", ws.member_uuid)))
        .send()
        .await
        .expect("send");
    assert_eq!(resp.status(), 200, "a requester's addresses");
    let resp = client
        .put(srv.url(&format!("/users/{}", ws.member_uuid)))
        .send_json(&json!({ "name": "Renamed requester" }))
        .await
        .expect("send");
    assert_eq!(resp.status(), 200, "a requester renamed");
    let resp = client
        .get(srv.url(&format!("/users/{}/security-info", ws.member_uuid)))
        .send()
        .await
        .expect("send");
    assert_eq!(
        resp.status(),
        403,
        "sign-in stays the person's own on hosted"
    );

    // Staff: their Nosdesk account owns it.
    let mut resp = client
        .put(srv.url(&format!("/users/{}", staff.uuid)))
        .send_json(&json!({ "name": "Renamed agent" }))
        .await
        .expect("send");
    assert_eq!(resp.status(), 409);
    let body: serde_json::Value = resp.json().await.expect("json");
    assert_eq!(body["code"], "externally_managed");
    let resp = client
        .get(srv.url(&format!("/users/{}/emails", staff.uuid)))
        .send()
        .await
        .expect("send");
    assert_eq!(resp.status(), 409, "a staff member's addresses");

    let mut resp = client
        .get(srv.url(&format!("/users/{}/profile", staff.uuid)))
        .send()
        .await
        .expect("send");
    assert_eq!(resp.status(), 200);
    let bundle: serde_json::Value = resp.json().await.expect("json");
    assert!(bundle.get("emails").is_none());
    assert_eq!(bundle["editable"]["name"], false);
    assert_eq!(bundle["editable"]["credentials"], false);
    assert_eq!(bundle["editable"]["account_locked"], "managed");
}
