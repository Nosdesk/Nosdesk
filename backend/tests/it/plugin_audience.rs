//! A plugin's runtime endpoints admit only callers in its audience (the widest
//! audience among its components). A requester calling a staff-only plugin's
//! network proxy, runtime settings, bundle token or events gets the same 404 as
//! for a missing plugin, so the plugin's stored credentials are never used on
//! their behalf; staff still reach it.

use actix_web::dev::Service;
use actix_web::http::StatusCode;
use actix_web::test as http_test;
use actix_web::{web, App, HttpMessage};
use serde_json::json;
use uuid::Uuid;

use backend::extractors::WorkspaceContext;
use backend::middleware::RequestContext;
use backend::models::{Claims, NewPlugin, Plugin, PluginState, SyncAggregate, SyncOp};
use backend::sync::actor::ActorContext;
use backend::sync::session::with_actor_context;
use backend::sync::visibility::{deliveries_pinned, ActionView, Delivery, SyncViewer};

use crate::common;

const EVENT: &str = common::FIXTURE_PLUGIN_EVENT;

fn seed_staff_plugin(conn: &mut backend::db::DbConnection, ws_id: i32, admin: Uuid) -> Uuid {
    use diesel::prelude::*;
    let actor = ActorContext::user(admin, None).with_workspace(ws_id);
    with_actor_context::<_, diesel::result::Error>(conn, &actor, |c| {
        let new_plugin = NewPlugin {
            name: "staff-crm".to_string(),
            display_name: "Staff CRM".to_string(),
            version: "1.0.0".to_string(),
            description: None,
            manifest: json!({
                "manifest_version": 1,
                "name": "staff-crm",
                "displayName": "Staff CRM",
                "version": "1.0.0",
                "engines": { "nosdesk": ">=1.0.0", "plugin_api": "1" },
                "permissions": ["network:crm.example.com"],
                "events": [EVENT],
                "components": {
                    "panel": { "slot": "ticket-sidebar", "entry": "p.js", "audience": "staff" }
                },
            }),
            state: PluginState::Installed,
            trust_level: "verified".to_string(),
            installed_by: Some(admin),
            source: "provisioned".to_string(),
            signer_pubkey: None,
            signer_source: None,
            signature_metadata: None,
            icon_svg: None,
            consented_permissions: Some(json!(["network:crm.example.com"])),
            consented_at: None,
            consented_by: None,
        };
        let p: Plugin = diesel::insert_into(backend::schema::plugins::table)
            .values(&new_plugin)
            .get_result(c)?;
        Ok(p.uuid)
    })
    .expect("seed staff plugin")
}

fn claims_for(user: Uuid) -> Claims {
    let now = chrono::Utc::now().timestamp();
    Claims {
        sub: user.to_string(),
        name: "Caller".to_string(),
        email: "caller@example.com".to_string(),
        platform_role: "user".to_string(),
        scope: "full".to_string(),
        sid: None,
        workspace_uuid: None,
        exp: (now + 3600) as usize,
        iat: now as usize,
    }
}

macro_rules! app_as {
    ($pool:expr, $ws:expr, $user:expr) => {{
        let claims = claims_for($user);
        let workspace = WorkspaceContext {
            workspace_id: $ws.workspace_id,
            workspace_uuid: $ws.workspace_uuid,
            slug: $ws.slug.clone(),
            name: "A".to_string(),
            organisation_id: None,
            custom_domain: None,
        };
        let actor = ActorContext::user($user, None).with_workspace($ws.workspace_id);
        http_test::init_service(
            App::new()
                .app_data(web::Data::new($pool.clone()))
                .app_data(web::Data::new(
                    backend::services::plugins::PluginProxyService::new(),
                ))
                .wrap_fn(move |req, srv| {
                    req.extensions_mut().insert(workspace.clone());
                    req.extensions_mut().insert(claims.clone());
                    req.extensions_mut()
                        .insert(RequestContext::new(Uuid::now_v7(), actor.clone()));
                    srv.call(req)
                })
                .route(
                    "/api/plugins/{uuid}/events",
                    web::post().to(backend::handlers::plugin_events::emit_plugin_event),
                )
                .route(
                    "/api/plugins/{uuid}/proxy",
                    web::post().to(backend::handlers::plugins::proxy_plugin_request),
                )
                .route(
                    "/api/plugins/{uuid}/settings",
                    web::get().to(backend::handlers::plugins::get_plugin_runtime_settings),
                )
                .route(
                    "/api/plugins/{uuid}/bundle-token",
                    web::get().to(backend::handlers::plugin_sandbox::mint_bundle_token),
                ),
        )
        .await
    }};
}

#[actix_web::test]
async fn a_requester_cannot_use_a_staff_only_plugin() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let mut conn = db.pool_with_size(2).get().expect("conn");
    let seeded = common::seed_two_workspaces(&mut conn);
    let a = seeded.a.clone();
    let plugin = seed_staff_plugin(&mut conn, a.workspace_id, a.admin_uuid);
    let pool = db.runtime_pool(2);

    let member = app_as!(pool, a, a.member_uuid);
    let proxy = http_test::TestRequest::post()
        .uri(&format!("/api/plugins/{plugin}/proxy"))
        .set_json(json!({ "url": "https://crm.example.com/contacts", "method": "GET" }))
        .to_request();
    assert_eq!(
        http_test::call_service(&member, proxy).await.status(),
        StatusCode::NOT_FOUND
    );
    let settings = http_test::TestRequest::get()
        .uri(&format!("/api/plugins/{plugin}/settings"))
        .to_request();
    assert_eq!(
        http_test::call_service(&member, settings).await.status(),
        StatusCode::NOT_FOUND
    );
    let token = http_test::TestRequest::get()
        .uri(&format!("/api/plugins/{plugin}/bundle-token"))
        .to_request();
    assert_eq!(
        http_test::call_service(&member, token).await.status(),
        StatusCode::NOT_FOUND
    );
    let event = http_test::TestRequest::post()
        .uri(&format!("/api/plugins/{plugin}/events"))
        .set_json(json!({ "aggregate": "plugin", "event_type": EVENT, "data": {} }))
        .to_request();
    assert_eq!(
        http_test::call_service(&member, event).await.status(),
        StatusCode::NOT_FOUND
    );

    let admin = app_as!(pool, a, a.admin_uuid);
    let settings = http_test::TestRequest::get()
        .uri(&format!("/api/plugins/{plugin}/settings"))
        .to_request();
    assert_eq!(
        http_test::call_service(&admin, settings).await.status(),
        StatusCode::OK
    );
    let event = http_test::TestRequest::post()
        .uri(&format!("/api/plugins/{plugin}/events"))
        .set_json(json!({ "aggregate": "plugin", "event_type": EVENT, "data": {} }))
        .to_request();
    let resp = http_test::call_service(&admin, event).await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    let body: serde_json::Value = http_test::read_body_json(resp).await;
    let sync_id = body["sync_id"].as_i64().expect("sync_id");

    // The event the admin raised reaches staff, not the requester.
    let row: (
        SyncAggregate,
        SyncOp,
        String,
        serde_json::Value,
        Vec<Option<String>>,
    ) = {
        use backend::schema::sync_actions;
        use diesel::prelude::*;
        sync_actions::table
            .filter(sync_actions::sync_id.eq(sync_id))
            .select((
                sync_actions::aggregate,
                sync_actions::op,
                sync_actions::aggregate_id,
                sync_actions::data,
                sync_actions::groups,
            ))
            .first(&mut db.conn())
            .expect("recorded event")
    };
    let deliver_to = |user: Uuid| {
        let mut conn = pool.get().expect("conn");
        let viewer = with_actor_context::<_, diesel::result::Error>(
            &mut conn,
            &ActorContext::user_at_workspace(user, a.workspace_id),
            |c| {
                let u = backend::repository::users::find_active_by_uuid(&user, c)?;
                Ok(SyncViewer::resolve(c, &u))
            },
        )
        .expect("viewer");
        deliveries_pinned(&pool, a.workspace_id, &viewer, &[row.clone()], |r| {
            ActionView::from_row(r.0, r.1, &r.2, &r.3, &r.4)
        })[0]
    };
    assert_eq!(deliver_to(a.member_uuid), Delivery::Drop);
    assert_eq!(deliver_to(a.admin_uuid), Delivery::Send);
}
