//! Who may read the unified audit feed, and which tiers they see.
//!
//! Workspace owners and admins read their own workspace's app events
//! (tier 1, `sync_actions`) and row changes (tier 3, `audit_log`), both
//! scoped by RLS on the workspace-pinned connection. The instance-wide
//! sign-in and security events (tier 2, `security_events`) carry no
//! workspace key and stay with platform admins and audit reviewers. Every
//! reader needs the `audit:read` scope.
//!
//! Requests run on the production-shaped pool (NOBYPASSRLS role, nothing
//! pinned ambiently), so a row from the other workspace showing up here
//! would mean the tenant tiers were read outside RLS.

#![allow(clippy::expect_used)]

use actix_web::dev::Service;
use actix_web::http::StatusCode;
use actix_web::test as http_test;
use actix_web::{web, App, HttpMessage};
use diesel::prelude::*;
use serde_json::{json, Value};
use uuid::Uuid;

use backend::extractors::WorkspaceContext;
use backend::middleware::RequestContext;
use backend::models::{Claims, SyncAggregate, SyncOp};
use backend::repository::workspaces::{add_membership, SeatWriteAuthority};
use backend::sync::actor::ActorContext;
use backend::sync::session::with_actor_context;

use crate::common::{self, TestPool, WorkspaceSeed};

const INSTANCE_EVENT: &str = "auth.login.failure";

struct World {
    db: common::TestDb,
    a: WorkspaceSeed,
    b: WorkspaceSeed,
    a_owner: Uuid,
    a_agent: Uuid,
    platform_admin: Uuid,
    audit_reviewer: Uuid,
}

fn plain_user_with_platform_role(conn: &mut PgConnection, name: &str, role: &str) -> Uuid {
    use backend::schema::users;
    let uuid = common::insert_plain_user(conn, name);
    diesel::update(users::table.filter(users::uuid.eq(uuid)))
        .set(users::platform_role.eq(role))
        .execute(conn)
        .expect("set platform role");
    uuid
}

/// Two workspaces, each with a tier-1 probe event and the fixture's
/// tier-3 rows, plus one instance-wide security event.
fn seed() -> World {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let pool = db.pool_with_size(2);
    let mut conn = pool.get().expect("conn");
    let seeded = common::seed_two_workspaces(&mut conn);
    let (a, b) = (seeded.a, seeded.b);

    let a_owner = common::insert_plain_user(&mut conn, "A Owner");
    let a_agent = common::insert_plain_user(&mut conn, "A Agent");
    let platform_admin = plain_user_with_platform_role(&mut conn, "Operator", "platform_admin");
    let audit_reviewer = plain_user_with_platform_role(&mut conn, "Reviewer", "audit_reviewer");

    let actor = ActorContext::user(a.admin_uuid, None).with_workspace(a.workspace_id);
    with_actor_context::<_, diesel::result::Error>(&mut conn, &actor, |c| {
        for (user, role) in [(a_owner, "owner"), (a_agent, "agent")] {
            add_membership(
                c,
                a.workspace_id,
                user,
                role,
                SeatWriteAuthority::ControlPlane,
            )?;
        }
        Ok(())
    })
    .expect("seed A roles");

    for ws in [&a, &b] {
        let actor = ActorContext::user(ws.admin_uuid, None).with_workspace(ws.workspace_id);
        with_actor_context::<_, diesel::result::Error>(&mut conn, &actor, |c| {
            backend::sync::emit::record(
                c,
                backend::sync::emit::SyncEmit {
                    aggregate: SyncAggregate::Data,
                    aggregate_id: probe(ws),
                    op: SyncOp::Insert,
                    event_type: "data.audit.probe",
                    data: json!({}),
                    groups: backend::sync::groups::workspace(),
                    causation_id: None,
                },
            )?;
            Ok(())
        })
        .expect("emit probe");
    }

    backend::utils::security_events::record_security_event(
        &mut conn,
        backend::utils::security_events::SecurityEventInput {
            user_uuid: None,
            event_type: INSTANCE_EVENT,
            severity: "warning",
            details: Some(json!({ "reason": "bad_password" })),
            request: None,
        },
    )
    .expect("record security event");

    drop(conn);
    World {
        db,
        a,
        b,
        a_owner,
        a_agent,
        platform_admin,
        audit_reviewer,
    }
}

fn probe(ws: &WorkspaceSeed) -> String {
    format!("probe-{}", ws.slug)
}

/// GET `uri` against the audit routes as `user` in workspace `ws`.
async fn get(
    pool: &TestPool,
    ws: &WorkspaceSeed,
    user: Uuid,
    platform_role: &str,
    scope: &str,
    uri: &str,
) -> (StatusCode, Value) {
    let now = chrono::Utc::now().timestamp();
    let claims = Claims {
        sub: user.to_string(),
        name: "Reader".to_string(),
        email: "reader@example.com".to_string(),
        platform_role: platform_role.to_string(),
        scope: scope.to_string(),
        sid: None,
        workspace_uuid: None,
        exp: (now + 3600) as usize,
        iat: now as usize,
    };
    let workspace = WorkspaceContext {
        workspace_id: ws.workspace_id,
        workspace_uuid: ws.workspace_uuid,
        slug: ws.slug.clone(),
        name: ws.slug.clone(),
        organisation_id: None,
        custom_domain: None,
    };
    let corr = Uuid::now_v7();
    let actor = ActorContext::user(user, Some(corr)).with_workspace(ws.workspace_id);
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
            .service(web::scope("/api").configure(backend::handlers::audit::config)),
    )
    .await;
    let resp =
        http_test::call_service(&app, http_test::TestRequest::get().uri(uri).to_request()).await;
    let status = resp.status();
    let body = http_test::read_body(resp).await;
    let json = serde_json::from_slice(&body).unwrap_or(Value::Null);
    (status, json)
}

fn entries(body: &Value) -> &Vec<Value> {
    body["entries"].as_array().expect("entries array")
}

fn has_source(body: &Value, source: &str) -> bool {
    entries(body).iter().any(|e| e["source"] == source)
}

fn has_target(body: &Value, kind: &str, id: &str) -> bool {
    entries(body)
        .iter()
        .any(|e| e["target"]["kind"] == kind && e["target"]["id"] == id)
}

/// The workspace tiers of `own` are there; nothing of `other`'s is, and
/// nothing instance-wide.
fn assert_own_workspace_only(body: &Value, own: &WorkspaceSeed, other: &WorkspaceSeed) {
    assert!(
        has_target(body, "data", &probe(own)),
        "own tier-1 probe visible"
    );
    assert!(
        has_target(body, "webhooks", &own.webhook_id.to_string()),
        "own tier-3 webhook row visible"
    );
    assert!(
        !has_target(body, "data", &probe(other)),
        "other workspace's tier-1 event must not appear"
    );
    assert!(
        !has_target(body, "webhooks", &other.webhook_id.to_string()),
        "other workspace's tier-3 row must not appear"
    );
    assert!(!has_source(body, "tier2"), "no instance-wide events");
}

#[actix_web::test]
async fn workspace_owner_and_admin_read_their_workspace_tiers_only() {
    let w = seed();
    let pool = w.db.runtime_pool(4);

    for (who, user) in [("owner", w.a_owner), ("admin", w.a.admin_uuid)] {
        let (status, body) = get(
            &pool,
            &w.a,
            user,
            "user",
            "full",
            "/api/admin/audit?limit=200",
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{who}: {body}");
        assert_own_workspace_only(&body, &w.a, &w.b);

        // Asking for sign-in events by prefix finds none of the
        // instance-wide rows.
        let (status, body) = get(
            &pool,
            &w.a,
            user,
            "user",
            "full",
            "/api/admin/audit?event_prefix=auth.&limit=200",
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{who}");
        assert!(!has_source(&body, "tier2"), "{who}: no tier 2 by prefix");

        for uri in ["/api/admin/audit?tier=2", "/api/admin/audit/export?tier=2"] {
            let (status, _) = get(&pool, &w.a, user, "user", "full", uri).await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{who}: {uri}");
        }

        let (status, body) =
            get(&pool, &w.a, user, "user", "full", "/api/admin/audit/export").await;
        assert_eq!(status, StatusCode::OK, "{who}: export");
        assert_own_workspace_only(&body, &w.a, &w.b);
    }

    // B's admin sees B, never A.
    let (status, body) = get(
        &pool,
        &w.b,
        w.b.admin_uuid,
        "user",
        "full",
        "/api/admin/audit?limit=200",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_own_workspace_only(&body, &w.b, &w.a);
}

#[actix_web::test]
async fn agents_and_members_are_refused() {
    let w = seed();
    let pool = w.db.runtime_pool(4);

    for (who, user) in [("agent", w.a_agent), ("member", w.a.member_uuid)] {
        for uri in ["/api/admin/audit", "/api/admin/audit/export"] {
            let (status, _) = get(&pool, &w.a, user, "user", "full", uri).await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{who}: {uri}");
        }
    }

    // An admin of B holds no role in A.
    let (status, _) = get(
        &pool,
        &w.a,
        w.b.admin_uuid,
        "user",
        "full",
        "/api/admin/audit",
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "B's admin in A");
}

#[actix_web::test]
async fn platform_audit_readers_also_see_instance_events() {
    let w = seed();
    let pool = w.db.runtime_pool(4);

    for (who, user, role) in [
        ("platform admin", w.platform_admin, "platform_admin"),
        ("audit reviewer", w.audit_reviewer, "audit_reviewer"),
    ] {
        let (status, body) = get(
            &pool,
            &w.a,
            user,
            role,
            "full",
            "/api/admin/audit?limit=200",
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{who}: {body}");
        assert!(has_source(&body, "tier2"), "{who}: tier 2 visible");
        assert!(has_target(&body, "data", &probe(&w.a)), "{who}: tier 1");
        assert!(
            has_target(&body, "webhooks", &w.a.webhook_id.to_string()),
            "{who}: tier 3"
        );
        assert!(
            !has_target(&body, "data", &probe(&w.b)),
            "{who}: tenant tiers stay pinned to the request's workspace"
        );

        let (status, body) = get(&pool, &w.a, user, role, "full", "/api/admin/audit?tier=2").await;
        assert_eq!(status, StatusCode::OK, "{who}: tier=2");
        assert!(
            entries(&body)
                .iter()
                .any(|e| e["event_type"] == INSTANCE_EVENT),
            "{who}: the security event"
        );
    }

    // The SIEM shape: an audit reviewer on an audit:read-only token.
    let (status, body) = get(
        &pool,
        &w.a,
        w.audit_reviewer,
        "audit_reviewer",
        "audit:read",
        "/api/admin/audit?tier=2",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(has_source(&body, "tier2"));
}

#[actix_web::test]
async fn a_token_without_audit_read_is_refused() {
    let w = seed();
    let pool = w.db.runtime_pool(4);

    for (who, user, role) in [
        ("workspace admin", w.a.admin_uuid, "user"),
        ("owner", w.a_owner, "user"),
        ("platform admin", w.platform_admin, "platform_admin"),
        ("audit reviewer", w.audit_reviewer, "audit_reviewer"),
    ] {
        for uri in ["/api/admin/audit", "/api/admin/audit/export"] {
            let (status, _) = get(&pool, &w.a, user, role, "tickets:read", uri).await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{who}: {uri}");
        }
    }

    // With the scope, a workspace admin's token reads the workspace tiers.
    let (status, body) = get(
        &pool,
        &w.a,
        w.a.admin_uuid,
        "user",
        "audit:read",
        "/api/admin/audit?limit=200",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_own_workspace_only(&body, &w.a, &w.b);
}
