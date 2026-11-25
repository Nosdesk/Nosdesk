//! Hosted: neither directory sync is offered. A hosted workspace's LDAP
//! directory would link and rename accounts other customers hold, and the
//! Microsoft Graph sync and proxy run on the operator's own Entra app, so every
//! `/ldap` route refuses the workspace's own owner with `ldap.self_hosted_only`
//! and every Graph route with `msgraph.self_hosted_only`, before
//! reading or writing anything, and the nightly LDAP reconcile skips a
//! settings row left from before.
//!
//! Part of the hosted binary rather than `tests/it`: the deployment mode is
//! process-wide environment.

use crate::common;

use actix_web::dev::Service;
use actix_web::http::StatusCode;
use actix_web::test::{self as http_test, TestRequest};
use actix_web::{web, App, HttpMessage};
use diesel::prelude::*;
use serde_json::json;
use uuid::Uuid;

use backend::extractors::WorkspaceContext;
use backend::handlers::ldap_integration as ldap;
use backend::middleware::RequestContext;
use backend::models::{Claims, UpsertWorkspaceLdapSettings};
use backend::repository::workspaces::{add_membership, SeatWriteAuthority};
use backend::schema::{sync_history, workspace_ldap_settings};
use backend::sync::actor::{ActorContext, BOOTSTRAP_WORKSPACE_ID};
use backend::sync::session::{run_in_workspace, with_actor_bypass_context};

const REF: &str = "test:hosted_ldap_refused";

fn settings() -> UpsertWorkspaceLdapSettings {
    UpsertWorkspaceLdapSettings {
        enabled: true,
        host: "ldap.invalid".into(),
        port: 636,
        tls_mode: "ldaps".into(),
        verify_certs: true,
        ca_cert_pem: None,
        follow_referrals: false,
        connect_timeout_secs: 5,
        auth_mode: "simple_bind".into(),
        bind_dn: "cn=svc,dc=acme,dc=test".into(),
        user_base_dn: "ou=people,dc=acme,dc=test".into(),
        username_attribute: "uid".into(),
        user_filter: "(uid={username})".into(),
        page_size: 500,
        attribute_map: json!({}),
        group_config: json!({}),
        provisioning: json!({}),
    }
}

#[actix_web::test]
async fn hosted_owners_are_refused_every_ldap_route() {
    crate::hosted_env();
    common::ensure_test_keyring();
    let test_db = common::TestDb::new();
    let pool = test_db.pool_with_size(4);
    let (workspace_id, owner) = {
        let mut conn = pool.get().expect("conn");
        let ws = common::mint_workspace(&mut conn, "hosted-ldap", "Hosted LDAP");
        let owner = common::insert_plain_user(&mut conn, "Hosted Owner");
        (ws, owner)
    };
    run_in_workspace(&pool, REF, workspace_id, |c| {
        add_membership(
            c,
            workspace_id,
            owner,
            "owner",
            SeatWriteAuthority::ControlPlane,
        )
    })
    .expect("owner seat");

    let now = chrono::Utc::now().timestamp();
    let claims = Claims {
        sub: owner.to_string(),
        name: "Owner".to_string(),
        email: "owner@example.com".to_string(),
        platform_role: "user".to_string(),
        scope: "full".to_string(),
        sid: None,
        workspace_uuid: None,
        exp: (now + 3600) as usize,
        iat: now as usize,
    };
    let workspace = WorkspaceContext {
        workspace_id,
        workspace_uuid: Uuid::now_v7(),
        slug: "hosted-ldap".into(),
        name: "Hosted LDAP".into(),
        organisation_id: None,
        custom_domain: None,
    };
    let corr = Uuid::now_v7();
    let actor = ActorContext::user(owner, Some(corr)).with_workspace(workspace_id);
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
            .route("/ldap/settings", web::get().to(ldap::get_ldap_settings))
            .route("/ldap/settings", web::put().to(ldap::set_ldap_settings))
            .route(
                "/ldap/sync-history",
                web::get().to(ldap::get_ldap_sync_history),
            )
            .route("/ldap/presets", web::get().to(ldap::get_ldap_presets))
            .route(
                "/ldap/test-connection",
                web::post().to(ldap::test_ldap_connection),
            )
            .route(
                "/ldap/discover-groups",
                web::get().to(ldap::discover_ldap_groups),
            )
            .route("/ldap/preview", web::post().to(ldap::preview_ldap))
            .route("/ldap/sync", web::post().to(ldap::run_ldap_sync)),
    )
    .await;

    let requests = [
        TestRequest::get().uri("/ldap/settings"),
        TestRequest::put()
            .uri("/ldap/settings")
            .set_json(json!({ "settings": {
                "enabled": true, "host": "ldap.invalid", "port": 636, "tls_mode": "ldaps",
                "verify_certs": true, "ca_cert_pem": null, "follow_referrals": false,
                "connect_timeout_secs": 5, "auth_mode": "simple_bind",
                "bind_dn": "cn=svc,dc=acme,dc=test", "user_base_dn": "ou=people,dc=acme,dc=test",
                "username_attribute": "uid", "user_filter": "(uid={username})", "page_size": 500,
                "attribute_map": {}, "group_config": {}, "provisioning": {}
            } })),
        TestRequest::get().uri("/ldap/sync-history"),
        TestRequest::get().uri("/ldap/presets"),
        TestRequest::post().uri("/ldap/test-connection"),
        TestRequest::get().uri("/ldap/discover-groups"),
        TestRequest::post().uri("/ldap/preview"),
        TestRequest::post().uri("/ldap/sync"),
    ];
    for req in requests {
        let req = req.to_request();
        let path = req.path().to_string();
        let resp = http_test::call_service(&app, req).await;
        let status = resp.status();
        let body: serde_json::Value =
            serde_json::from_slice(&http_test::read_body(resp).await).unwrap_or_default();
        assert_eq!(status, StatusCode::FORBIDDEN, "{path}: {body}");
        assert_eq!(body["code"], "ldap.self_hosted_only", "{path}: {body}");
    }

    let saved: i64 = with_actor_bypass_context(
        &mut pool.get().expect("conn"),
        &ActorContext::system(REF),
        |c| {
            workspace_ldap_settings::table
                .filter(workspace_ldap_settings::workspace_id.eq(workspace_id))
                .count()
                .get_result(c)
        },
    )
    .expect("count settings");
    assert_eq!(saved, 0, "the refused PUT saved nothing");
}

#[actix_web::test]
async fn the_nightly_reconcile_skips_a_hosted_settings_row() {
    crate::hosted_env();
    let test_db = common::TestDb::new();
    let pool = test_db.pool_with_size(2);
    run_in_workspace(&pool, REF, BOOTSTRAP_WORKSPACE_ID, |c| {
        backend::repository::workspace_ldap_settings::upsert(c, settings()).map(|_| ())
    })
    .expect("a settings row from before");

    backend::services::ldap::reconcile::run_scheduled_reconcile(&pool)
        .await
        .expect("the reconcile is a no-op");

    let runs: i64 = with_actor_bypass_context(
        &mut pool.get().expect("conn"),
        &ActorContext::system(REF),
        |c| {
            sync_history::table
                .filter(sync_history::sync_type.eq("ldap_reconcile"))
                .count()
                .get_result(c)
        },
    )
    .expect("count runs");
    assert_eq!(runs, 0, "no sync ran");
}

#[actix_web::test]
async fn hosted_owners_are_refused_every_graph_route() {
    crate::hosted_env();
    common::ensure_test_keyring();
    let test_db = common::TestDb::new();
    let pool = test_db.pool_with_size(4);
    let (workspace_id, owner) = {
        let mut conn = pool.get().expect("conn");
        let ws = common::mint_workspace(&mut conn, "hosted-graph", "Hosted Graph");
        let owner = common::insert_plain_user(&mut conn, "Hosted Owner");
        (ws, owner)
    };
    run_in_workspace(&pool, REF, workspace_id, |c| {
        add_membership(
            c,
            workspace_id,
            owner,
            "owner",
            SeatWriteAuthority::ControlPlane,
        )
    })
    .expect("owner seat");

    let now = chrono::Utc::now().timestamp();
    let claims = Claims {
        sub: owner.to_string(),
        name: "Owner".to_string(),
        email: "owner@example.com".to_string(),
        platform_role: "user".to_string(),
        scope: "full".to_string(),
        sid: None,
        workspace_uuid: None,
        exp: (now + 3600) as usize,
        iat: now as usize,
    };
    let workspace = WorkspaceContext {
        workspace_id,
        workspace_uuid: Uuid::now_v7(),
        slug: "hosted-graph".into(),
        name: "Hosted Graph".into(),
        organisation_id: None,
        custom_domain: None,
    };
    let corr = Uuid::now_v7();
    let actor = ActorContext::user(owner, Some(corr)).with_workspace(workspace_id);
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
            .configure(backend::handlers::msgraph_integration::config)
            .configure(backend::handlers::microsoft_graph::config),
    )
    .await;

    let requests = [
        TestRequest::get().uri("/integrations/graph/config"),
        TestRequest::get().uri("/integrations/graph/status"),
        TestRequest::post().uri("/integrations/graph/test"),
        TestRequest::post()
            .uri("/integrations/graph/sync")
            .set_json(json!({ "entities": ["users"], "use_delta": false })),
        TestRequest::get().uri("/integrations/graph/progress/1"),
        TestRequest::get().uri("/integrations/graph/active-syncs"),
        TestRequest::get().uri("/integrations/graph/last-sync"),
        TestRequest::post().uri("/integrations/graph/cancel/1"),
        TestRequest::get().uri("/integrations/graph/entra-object-id/device-1"),
        // The Graph proxy.
        TestRequest::post()
            .uri("/auth/microsoft/graph")
            .set_json(json!({ "endpoint": "/users" })),
        TestRequest::post()
            .uri("/msgraph/request")
            .set_json(json!({ "endpoint": "/users" })),
        TestRequest::get().uri("/msgraph/users"),
        TestRequest::get().uri("/msgraph/devices"),
        TestRequest::get().uri("/msgraph/groups"),
        TestRequest::get().uri("/msgraph/directory-objects"),
    ];
    for req in requests {
        let req = req.to_request();
        let path = req.path().to_string();
        let resp = http_test::call_service(&app, req).await;
        let status = resp.status();
        let body: serde_json::Value =
            serde_json::from_slice(&http_test::read_body(resp).await).unwrap_or_default();
        assert_eq!(status, StatusCode::FORBIDDEN, "{path}: {body}");
        assert_eq!(body["code"], "msgraph.self_hosted_only", "{path}: {body}");
    }
}
