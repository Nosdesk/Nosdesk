//! A plugin's emitted event reaches clients as an event, never as a row. The
//! server sets what decides that (the aggregate id, the op, the payload's
//! shape), so a caller can't make clients write, replace or delete a pooled
//! `plugin` row, its own or another plugin's.

use actix_web::dev::Service;
use actix_web::http::StatusCode;
use actix_web::test as http_test;
use actix_web::{web, App, HttpMessage};
use diesel::prelude::*;
use serde_json::json;
use uuid::Uuid;

use backend::extractors::WorkspaceContext;
use backend::middleware::RequestContext;
use backend::models::{Claims, SyncOp};
use backend::sync::actor::ActorContext;

use crate::common;

#[actix_web::test]
async fn a_plugin_event_is_recorded_as_an_event_of_the_emitting_plugin() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let seeded = common::seed_two_workspaces(&mut db.pool_with_size(2).get().expect("conn"));
    let pool = db.runtime_pool(2);
    let a = seeded.a.clone();
    let member = a.member_uuid;
    let now = chrono::Utc::now().timestamp();
    let claims = Claims {
        sub: member.to_string(),
        name: "Member".to_string(),
        email: "member@example.com".to_string(),
        platform_role: "user".to_string(),
        scope: "full".to_string(),
        sid: None,
        workspace_uuid: None,
        exp: (now + 3600) as usize,
        iat: now as usize,
    };
    let workspace = WorkspaceContext {
        workspace_id: a.workspace_id,
        workspace_uuid: a.workspace_uuid,
        slug: a.slug.clone(),
        name: "A".to_string(),
        organisation_id: None,
        custom_domain: None,
    };
    let corr = Uuid::now_v7();
    let actor = ActorContext::user(member, Some(corr)).with_workspace(a.workspace_id);
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
            .route(
                "/api/plugins/{uuid}/events",
                web::post().to(backend::handlers::plugin_events::emit_plugin_event),
            ),
    )
    .await;

    // Aimed at another plugin's row, as a delete, with a row-shaped payload.
    let other = seeded.b.plugin_uuid;
    let resp = http_test::call_service(
        &app,
        http_test::TestRequest::post()
            .uri(&format!("/api/plugins/{}/events", a.plugin_uuid))
            .set_json(json!({
                "aggregate": "plugin",
                "aggregate_id": other.to_string(),
                "op": "D",
                "event_type": common::FIXTURE_PLUGIN_EVENT,
                "data": { "uuid": other, "id": 7, "trust": "verified" },
            }))
            .to_request(),
    )
    .await;
    let status = resp.status();
    let body: serde_json::Value =
        serde_json::from_slice(&http_test::read_body(resp).await).unwrap_or_default();
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let sync_id = body["sync_id"].as_i64().expect("sync_id");

    let (aggregate_id, op, data): (String, SyncOp, serde_json::Value) = {
        use backend::schema::sync_actions;
        sync_actions::table
            .filter(sync_actions::sync_id.eq(sync_id))
            .select((
                sync_actions::aggregate_id,
                sync_actions::op,
                sync_actions::data,
            ))
            .first(&mut db.conn())
            .expect("recorded event")
    };
    assert_eq!(aggregate_id, a.plugin_uuid.to_string());
    assert_eq!(op, SyncOp::Update);
    // No row key at the top level: clients take it as an event.
    assert!(
        data.get("uuid").is_none() && data.get("id").is_none(),
        "{data}"
    );
    assert_eq!(data["event"]["trust"], "verified");

    // Only a known plugin event; a made-up name is refused.
    let resp = http_test::call_service(
        &app,
        http_test::TestRequest::post()
            .uri(&format!("/api/plugins/{}/events", a.plugin_uuid))
            .set_json(json!({
                "aggregate": "plugin",
                "event_type": "report:ready",
                "data": {},
            }))
            .to_request(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}
