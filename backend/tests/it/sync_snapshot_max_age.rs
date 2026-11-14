//! The bootstrap header and every delta say how old a snapshot may grow
//! before the client takes a fresh one (`SYNC_SNAPSHOT_MAX_AGE_SECS`), so the
//! server owns the number whichever way the client caught up.

use actix_web::dev::Service;
use actix_web::http::StatusCode;
use actix_web::test as http_test;
use actix_web::{web, App, HttpMessage};
use uuid::Uuid;

use backend::extractors::WorkspaceContext;
use backend::middleware::RequestContext;
use backend::models::Claims;
use backend::sync::actor::ActorContext;

use crate::common::{self, TestPool, WorkspaceSeed};

/// GET `uri` as `viewer`, with `seed`'s workspace resolved for the request.
async fn get_as(pool: &TestPool, seed: &WorkspaceSeed, viewer: Uuid, uri: &str) -> String {
    let claims = Claims {
        sub: viewer.to_string(),
        name: "Viewer".to_string(),
        email: format!("{}@max-age.test", viewer.simple()),
        platform_role: "user".to_string(),
        scope: "full".to_string(),
        sid: None,
        workspace_uuid: None,
        exp: (chrono::Utc::now().timestamp() + 3600) as usize,
        iat: chrono::Utc::now().timestamp() as usize,
    };
    let workspace = WorkspaceContext {
        workspace_id: seed.workspace_id,
        workspace_uuid: seed.workspace_uuid,
        slug: seed.slug.clone(),
        name: seed.slug.clone(),
        organisation_id: None,
        custom_domain: None,
    };
    let corr = Uuid::now_v7();
    let actor = ActorContext::user(viewer, Some(corr)).with_workspace(seed.workspace_id);
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
            .service(web::scope("/api").configure(backend::handlers::sync::config)),
    )
    .await;
    let resp =
        http_test::call_service(&app, http_test::TestRequest::get().uri(uri).to_request()).await;
    let status = resp.status();
    let body = String::from_utf8(http_test::read_body(resp).await.to_vec()).expect("utf8 body");
    assert_eq!(status, StatusCode::OK, "GET {uri}: {body}");
    body
}

#[actix_web::test]
async fn the_bootstrap_and_every_delta_carry_the_snapshot_max_age() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = common::seed_two_workspaces(&mut pool.get().expect("conn"));
    let a = &ws.a;
    let group = format!("workspace:{}", a.workspace_id);

    let expected = backend::handlers::sync::snapshot_max_age_secs();
    if std::env::var("SYNC_SNAPSHOT_MAX_AGE_SECS").is_err() {
        assert_eq!(expected, 86_400, "a day when unset");
    }

    let body = get_as(
        &pool,
        a,
        a.admin_uuid,
        &format!("/api/sync/bootstrap?groups={group}&schema=test"),
    )
    .await;
    let meta = body
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .find_map(|l| l.get("__meta__").cloned())
        .expect("a __meta__ header");
    assert_eq!(meta["snapshot_max_age_secs"], expected, "bootstrap: {meta}");

    // A delta for a granted group, and one for a group the viewer doesn't
    // hold, which returns early with no actions.
    for groups in [group.clone(), "project:2147483647".to_string()] {
        let body = get_as(
            &pool,
            a,
            a.admin_uuid,
            &format!("/api/sync/delta?from=0&groups={groups}"),
        )
        .await;
        let delta: serde_json::Value = serde_json::from_str(&body).expect("delta json");
        assert_eq!(
            delta["snapshot_max_age_secs"], expected,
            "delta for {groups}: {body}"
        );
    }
}
