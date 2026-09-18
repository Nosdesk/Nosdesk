//! Integration test for
//! `GET /api/internal/v1/workspaces/{slug}/activation`.
//!
//! The control plane reads this to decide whether a trial workspace is
//! being used. The endpoint reports facts from the workspace's own tables;
//! this test checks that the facts move for the right reasons:
//!
//!   1. Freshly created -> everything empty, no integrations.
//!   2. Owner + agent + requester projected, a ticket, an internal agent
//!      note and a requester reply -> ticket counted, no agent reply yet.
//!   3. A public agent comment -> `first_agent_reply_at` set.
//!   4. The seeded Getting Started page is not a document; a page the
//!      workspace wrote is. A webhook counts as an integration.
//!   5. Unknown slug -> 404; wrong-scope token -> 401.

#![allow(clippy::expect_used)]

use actix_web::{web, App};
use diesel::prelude::*;
use serde_json::json;
use uuid::Uuid;

use backend::handlers::internal_workspaces;
use backend::middleware::idempotency_middleware;
use backend::repository::workspaces::SeatWriteAuthority;
use backend::sync::actor::ActorContext;
use backend::sync::session::with_actor_context;

mod common;

const SLUG: &str = "act-co";

fn workspace_id_for_slug(pool: &backend::db::Pool, slug: &str) -> i32 {
    use backend::schema::workspaces;
    let mut conn = pool.get().expect("conn");
    workspaces::table
        .filter(workspaces::slug.eq(slug))
        .select(workspaces::id)
        .first(&mut conn)
        .expect("workspace id")
}

fn pinned<T>(
    pool: &backend::db::Pool,
    workspace_id: i32,
    f: impl FnOnce(&mut backend::db::DbConnection) -> diesel::QueryResult<T>,
) -> T {
    let mut conn = pool.get().expect("conn");
    let actor = ActorContext::system("test:activation").with_workspace(workspace_id);
    with_actor_context::<_, diesel::result::Error>(&mut conn, &actor, f).expect("pinned op")
}

fn add_member(pool: &backend::db::Pool, workspace_id: i32, user: Uuid, role: &str) {
    backend::repository::workspaces::add_membership(
        &mut pool.get().expect("conn"),
        workspace_id,
        user,
        role,
        SeatWriteAuthority::ControlPlane,
    )
    .expect("add membership");
}

fn comment(pool: &backend::db::Pool, workspace_id: i32, ticket_id: i32, by: Uuid, internal: bool) {
    pinned(pool, workspace_id, |c| {
        backend::repository::comments::create_comment(
            c,
            backend::models::NewComment {
                content: "hello".to_string(),
                ticket_id,
                user_uuid: by,
                is_internal: internal,
                ..Default::default()
            },
            None,
        )
        .map(|_| ())
    });
}

#[actix_web::test]
async fn workspace_activation_full_contract() {
    common::ensure_test_keyring();
    common::enable_platform_auth();
    let test_db = common::TestDb::new();
    let pool = test_db.pool_with_size(4);
    let (owner, agent, requester) = {
        let mut conn = pool.get().expect("conn");
        (
            common::insert_user(&mut conn, "ActOwner"),
            common::insert_user(&mut conn, "ActAgent"),
            common::insert_user(&mut conn, "ActRequester"),
        )
    };
    let platform_token = common::mint_platform_jwt("platform:provision", 300);
    let wrong_scope_token = common::mint_platform_jwt("platform:other", 300);

    let pool_for_app = pool.clone();
    let srv = actix_test::start(move || {
        App::new()
            .app_data(web::Data::new(pool_for_app.clone()))
            .service(
                web::scope("/api/internal/v1")
                    .wrap(actix_web::middleware::from_fn(idempotency_middleware))
                    .wrap(actix_web::middleware::from_fn(
                        backend::extractors::platform_auth_middleware,
                    ))
                    .route(
                        "/workspaces/create",
                        web::post().to(internal_workspaces::create_workspace),
                    )
                    .route(
                        "/workspaces/{slug}/activation",
                        web::get().to(internal_workspaces::workspace_activation),
                    ),
            )
    });
    let client = awc::Client::new();
    let activation_url = srv.url(&format!("/api/internal/v1/workspaces/{SLUG}/activation"));
    let read = || async {
        let mut resp = client
            .get(&activation_url)
            .insert_header(("Authorization", format!("Bearer {platform_token}")))
            .send()
            .await
            .expect("send activation");
        assert_eq!(resp.status(), 200, "activation must 200");
        let body: serde_json::Value =
            serde_json::from_slice(&resp.body().await.expect("body")).expect("json");
        body
    };

    let resp = client
        .post(srv.url("/api/internal/v1/workspaces/create"))
        .insert_header(("Authorization", format!("Bearer {platform_token}")))
        .insert_header(("Idempotency-Key", format!("provision-{}", Uuid::new_v4())))
        .insert_header(("Content-Type", "application/json"))
        .send_json(&json!({
            "slug": SLUG,
            "name": "Act Co",
            "owner_user_uuid": owner.uuid,
            "owner_email": "owner@act.example",
        }))
        .await
        .expect("send create");
    assert_eq!(resp.status(), 201, "create must 201");
    let workspace_id = workspace_id_for_slug(&pool, SLUG);

    // --- 1: fresh ---
    let fresh = read().await;
    assert_eq!(fresh["slug"], SLUG);
    assert!(fresh["workspace_uuid"].is_string());
    assert_eq!(fresh["tickets"], 0, "{fresh}");
    assert_eq!(fresh["members"], 0, "{fresh}");
    assert_eq!(fresh["documents"], 0, "{fresh}");
    assert!(fresh["first_ticket_at"].is_null(), "{fresh}");
    assert!(fresh["first_agent_reply_at"].is_null(), "{fresh}");
    assert!(fresh["first_member_joined_at"].is_null(), "{fresh}");
    assert!(fresh["last_activity_at"].is_null(), "{fresh}");
    assert_eq!(
        fresh["integrations"],
        json!({
            "inbound_channel": false,
            "outbound_email": false,
            "ldap": false,
            "msgraph": false,
            "webhooks": 0,
            "plugins": 0,
        }),
        "{fresh}"
    );

    // --- 2: people and a ticket, but no agent reply ---
    add_member(&pool, workspace_id, owner.uuid, "owner");
    let after_owner = read().await;
    assert_eq!(after_owner["members"], 1, "{after_owner}");
    assert!(
        after_owner["first_member_joined_at"].is_null(),
        "the owner joining is not a second member: {after_owner}"
    );

    add_member(&pool, workspace_id, agent.uuid, "agent");
    add_member(&pool, workspace_id, requester.uuid, "member");
    let ticket_id = pinned(&pool, workspace_id, |c| {
        use backend::schema::workflow_states::dsl as s;
        let state: i32 = s::workflow_states
            .filter(s::is_default.eq(true))
            .select(s::id)
            .first(c)?;
        backend::repository::tickets::create_ticket(
            c,
            backend::models::NewTicket {
                title: "Printer on fire".to_string(),
                workflow_state_id: state,
                requester_uuid: Some(requester.uuid),
                ..Default::default()
            },
        )
        .map(|t| t.id)
    });
    comment(&pool, workspace_id, ticket_id, agent.uuid, true);
    comment(&pool, workspace_id, ticket_id, requester.uuid, false);

    let partial = read().await;
    assert_eq!(partial["members"], 3, "{partial}");
    assert!(partial["first_member_joined_at"].is_string(), "{partial}");
    assert_eq!(partial["tickets"], 1, "{partial}");
    assert!(partial["first_ticket_at"].is_string(), "{partial}");
    assert!(partial["last_activity_at"].is_string(), "{partial}");
    assert!(
        partial["first_agent_reply_at"].is_null(),
        "an internal note and a requester reply are not an agent reply: {partial}"
    );

    // --- 3: the agent replies ---
    comment(&pool, workspace_id, ticket_id, agent.uuid, false);
    let replied = read().await;
    assert!(replied["first_agent_reply_at"].is_string(), "{replied}");

    // --- 4: seeded docs don't count, written docs and a webhook do ---
    pinned(&pool, workspace_id, |c| {
        backend::services::seed::seed_getting_started(c, owner.uuid)
    });
    let seeded = read().await;
    assert_eq!(
        seeded["documents"], 0,
        "seeded page is not a document: {seeded}"
    );

    pinned(&pool, workspace_id, |c| {
        backend::repository::documentation::create_documentation_page(
            backend::models::NewDocumentationPage {
                uuid: Uuid::new_v4(),
                title: "Runbook".to_string(),
                slug: "runbook".to_string(),
                icon: None,
                cover_image: None,
                status: backend::models::DocumentationStatus::Published,
                created_by: owner.uuid,
                last_edited_by: owner.uuid,
                parent_id: None,
                display_order: None,
                is_public: false,
                is_template: false,
                yjs_state_vector: None,
                yjs_document: None,
                yjs_client_id: None,
                has_unsaved_changes: false,
            },
            c,
        )?;
        backend::repository::webhooks::create_webhook(
            c,
            "Pager".to_string(),
            "https://hooks.example/x".to_string(),
            "s".to_string(),
            vec!["ticket.created".to_string()],
            None,
            Some(owner.uuid),
        )
        .map(|_| ())
    });
    let used = read().await;
    assert_eq!(used["documents"], 1, "{used}");
    assert_eq!(used["integrations"]["webhooks"], 1, "{used}");

    // --- 5: unknown slug -> 404, wrong scope -> 401 ---
    let resp = client
        .get(srv.url("/api/internal/v1/workspaces/does-not-exist/activation"))
        .insert_header(("Authorization", format!("Bearer {platform_token}")))
        .send()
        .await
        .expect("send unknown slug");
    assert_eq!(resp.status(), 404, "unknown slug must 404");

    let resp = client
        .get(&activation_url)
        .insert_header(("Authorization", format!("Bearer {wrong_scope_token}")))
        .send()
        .await
        .expect("send wrong-scope-token");
    assert_eq!(resp.status(), 401, "wrong-scope token must 401");
}
