//! `/api/workspace/portal` tells the agent app where the workspace's public
//! portal is and which public surfaces are on, so links to the request form
//! and public docs point at the portal rather than the agent app's origin.

#![allow(clippy::expect_used)]

use actix_web::dev::Service;
use actix_web::{web, App, HttpMessage};
use diesel::prelude::*;
use serde_json::Value;
use uuid::Uuid;

use backend::extractors::WorkspaceContext;
use backend::middleware::RequestContext;
use backend::models::{Claims, NewUser};
use backend::sync::actor::ActorContext;

const WS: i32 = 1;

#[actix_web::test]
async fn portal_info_reports_the_portal_and_its_public_surfaces() {
    let db = crate::common::TestDb::new();
    let pool = db.pool_with_size(3);
    let user_uuid = {
        use backend::schema::{site_settings, users, workspace_members};
        let mut conn = pool.get().expect("conn");
        let user: backend::models::User = diesel::insert_into(users::table)
            .values(&NewUser {
                uuid: Uuid::new_v4(),
                name: "Member".to_string(),
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
            .get_result(&mut conn)
            .expect("user");
        diesel::insert_into(workspace_members::table)
            .values((
                workspace_members::workspace_id.eq(WS),
                workspace_members::user_uuid.eq(user.uuid),
                workspace_members::role.eq("member"),
            ))
            .execute(&mut conn)
            .expect("member");
        diesel::update(site_settings::table)
            .set((
                site_settings::guest_tickets_enabled.eq(true),
                site_settings::guest_public_docs_enabled.eq(false),
            ))
            .execute(&mut conn)
            .expect("settings");
        user.uuid
    };

    let srv = actix_test::start(move || {
        let claims = Claims {
            sub: user_uuid.to_string(),
            name: "Member".to_string(),
            email: "member@example.com".to_string(),
            platform_role: "user".to_string(),
            scope: "full".to_string(),
            sid: None,
            workspace_uuid: None,
            exp: (chrono::Utc::now().timestamp() + 3600) as usize,
            iat: chrono::Utc::now().timestamp() as usize,
        };
        let corr = Uuid::now_v7();
        let actor = ActorContext::user(user_uuid, Some(corr)).with_workspace(WS);
        let ws = WorkspaceContext {
            workspace_id: WS,
            workspace_uuid: Uuid::nil(),
            slug: "acme".to_string(),
            name: "Acme".to_string(),
            organisation_id: None,
            custom_domain: Some("help.acme.example".to_string()),
        };
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .wrap_fn(move |req, srv| {
                req.extensions_mut().insert(ws.clone());
                req.extensions_mut().insert(claims.clone());
                req.extensions_mut()
                    .insert(RequestContext::new(corr, actor.clone()));
                srv.call(req)
            })
            .service(web::scope("/api").configure(backend::handlers::guest_settings::config))
    });

    let mut resp = awc::Client::new()
        .get(srv.url("/api/workspace/portal"))
        .send()
        .await
        .expect("send");
    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.expect("json");
    // A custom domain is the portal, whatever origin the agent app is on.
    assert_eq!(body["portal_url"], "https://help.acme.example");
    assert_eq!(body["request_form_enabled"], true);
    assert_eq!(body["public_docs_enabled"], false);
}
