//! HTTP-layer integration tests for the ticket merge endpoints.
//!
//! The repository lifecycle is covered exhaustively by the unit tests
//! in `repository::ticket_merge::tests`. These tests exercise the
//! handler wiring: routing, the workspace-role gate, the MergeError ->
//! status-code mapping, and the response JSON shape. Auth + workspace
//! middleware is replaced by a `wrap_fn` that injects the same request
//! extensions the production middleware would (Claims, WorkspaceContext,
//! RequestContext), so each test runs as a chosen user + role.

#![allow(clippy::expect_used)]

use actix_web::dev::Service;
use actix_web::{web, App, HttpMessage};
use diesel::prelude::*;
use serde_json::json;
use uuid::Uuid;

use backend::extractors::WorkspaceContext;
use backend::middleware::RequestContext;
use backend::models::{Claims, NewTicket, NewUser, Ticket, User};
use backend::sync::actor::ActorContext;

mod common;

const WS: i32 = 1;

fn default_state_id(conn: &mut PgConnection) -> i32 {
    use backend::schema::workflow_states::dsl as s;
    s::workflow_states
        .filter(s::workspace_id.eq(WS))
        .filter(s::is_default.eq(true))
        .select(s::id)
        .first(conn)
        .expect("default workflow state seeded")
}

/// Insert a user and grant them `role` in workspace 1.
fn member(conn: &mut PgConnection, name: &str, role: &str) -> User {
    use backend::schema::{users, workspace_members};
    let user: User = diesel::insert_into(users::table)
        .values(&NewUser {
            uuid: Uuid::new_v4(),
            name: name.to_string(),
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
            workspace_members::workspace_id.eq(WS),
            workspace_members::user_uuid.eq(user.uuid),
            workspace_members::role.eq(role),
        ))
        .execute(conn)
        .expect("insert workspace member");
    user
}

fn ticket(conn: &mut PgConnection, title: &str, state_id: i32, requester: Uuid) -> Ticket {
    use backend::schema::tickets;
    diesel::insert_into(tickets::table)
        .values(&NewTicket {
            title: title.to_string(),
            workflow_state_id: state_id,
            requester_uuid: Some(requester),
            ..Default::default()
        })
        .get_result(conn)
        .expect("insert ticket")
}

fn claims_for(user: &User) -> Claims {
    Claims {
        sub: user.uuid.to_string(),
        name: user.name.clone(),
        email: "agent@example.com".to_string(),
        role: "technician".to_string(),
        platform_role: None,
        scope: "full".to_string(),
        sid: None,
        exp: (chrono::Utc::now().timestamp() + 3600) as usize,
        iat: chrono::Utc::now().timestamp() as usize,
    }
}

/// Start a test server that runs every request as `user`, with the
/// workspace context pinned to workspace 1.
fn spawn(pool: &common::TestPool, user: &User) -> actix_test::TestServer {
    let pool = pool.clone();
    let claims = claims_for(user);
    let user_uuid = user.uuid;
    actix_test::start(move || {
        let pool = pool.clone();
        let claims = claims.clone();
        let corr = Uuid::now_v7();
        let actor = ActorContext::user(user_uuid, Some(corr)).with_workspace(WS);
        let ws = WorkspaceContext {
            workspace_id: WS,
            workspace_uuid: Uuid::nil(),
            slug: "default".to_string(),
            name: "Default".to_string(),
            organisation_id: None,
        };
        App::new()
            .app_data(web::Data::new(pool))
            .wrap_fn(move |req, srv| {
                req.extensions_mut().insert(ws.clone());
                req.extensions_mut().insert(claims.clone());
                req.extensions_mut()
                    .insert(RequestContext::new(corr, actor.clone()));
                srv.call(req)
            })
            .service(
                web::scope("/api")
                    .route(
                        "/tickets/merge",
                        web::post().to(backend::handlers::ticket_merge::merge_tickets),
                    )
                    .route(
                        "/tickets/{id}/merge-history",
                        web::get().to(backend::handlers::ticket_merge::get_merge_history),
                    ),
            )
    })
}

#[actix_web::test]
async fn merge_happy_path_returns_200() {
    common::ensure_test_keyring();
    let test_db = common::TestDb::new();
    let pool = test_db.pool_with_size(4);
    let mut conn = pool.get().expect("conn");

    let agent = member(&mut conn, "Agent", "agent");
    let state = default_state_id(&mut conn);
    let dest = ticket(&mut conn, "Dest", state, agent.uuid);
    let src = ticket(&mut conn, "Source", state, agent.uuid);

    let srv = spawn(&pool, &agent);
    let client = awc::Client::new();
    let mut resp = client
        .post(srv.url("/api/tickets/merge"))
        .send_json(&json!({
            "destination_ticket_id": dest.id,
            "source_ticket_ids": [src.id],
            "reason": "same outage",
            "notify_customer": false,
        }))
        .await
        .expect("send merge");

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.expect("json");
    assert_eq!(body["destination_ticket"]["id"], dest.id);
    assert_eq!(body["merged_sources"].as_array().unwrap().len(), 1);
    assert_eq!(body["merged_sources"][0]["id"], src.id);
}

#[actix_web::test]
async fn self_merge_returns_400() {
    common::ensure_test_keyring();
    let test_db = common::TestDb::new();
    let pool = test_db.pool_with_size(4);
    let mut conn = pool.get().expect("conn");

    let agent = member(&mut conn, "Agent", "agent");
    let state = default_state_id(&mut conn);
    let t = ticket(&mut conn, "T", state, agent.uuid);

    let srv = spawn(&pool, &agent);
    let client = awc::Client::new();
    let mut resp = client
        .post(srv.url("/api/tickets/merge"))
        .send_json(&json!({ "destination_ticket_id": t.id, "source_ticket_ids": [t.id] }))
        .await
        .expect("send");
    assert_eq!(resp.status(), 400);
    let body: serde_json::Value = resp.json().await.expect("json");
    assert_eq!(body["code"], "MERGE_VALIDATION");
}

#[actix_web::test]
async fn chain_merge_returns_400() {
    common::ensure_test_keyring();
    let test_db = common::TestDb::new();
    let pool = test_db.pool_with_size(4);
    let mut conn = pool.get().expect("conn");

    let agent = member(&mut conn, "Agent", "agent");
    let state = default_state_id(&mut conn);
    let dest = ticket(&mut conn, "Dest", state, agent.uuid);
    let other = ticket(&mut conn, "Other", state, agent.uuid);
    let src = ticket(&mut conn, "Source", state, agent.uuid);

    let srv = spawn(&pool, &agent);
    let client = awc::Client::new();

    let first = client
        .post(srv.url("/api/tickets/merge"))
        .send_json(&json!({ "destination_ticket_id": dest.id, "source_ticket_ids": [src.id] }))
        .await
        .expect("send first");
    assert_eq!(first.status(), 200);

    // src is now merged; merging it again must be refused.
    let mut second = client
        .post(srv.url("/api/tickets/merge"))
        .send_json(&json!({ "destination_ticket_id": other.id, "source_ticket_ids": [src.id] }))
        .await
        .expect("send second");
    assert_eq!(second.status(), 400);
    let body: serde_json::Value = second.json().await.expect("json");
    assert_eq!(body["code"], "MERGE_VALIDATION");
}

#[actix_web::test]
async fn non_agent_returns_403() {
    common::ensure_test_keyring();
    let test_db = common::TestDb::new();
    let pool = test_db.pool_with_size(4);
    let mut conn = pool.get().expect("conn");

    let regular = member(&mut conn, "Regular", "member");
    let state = default_state_id(&mut conn);
    let dest = ticket(&mut conn, "Dest", state, regular.uuid);
    let src = ticket(&mut conn, "Source", state, regular.uuid);

    let srv = spawn(&pool, &regular);
    let client = awc::Client::new();
    let resp = client
        .post(srv.url("/api/tickets/merge"))
        .send_json(&json!({ "destination_ticket_id": dest.id, "source_ticket_ids": [src.id] }))
        .await
        .expect("send");
    assert_eq!(resp.status(), 403);
}

#[actix_web::test]
async fn missing_ticket_returns_404() {
    common::ensure_test_keyring();
    let test_db = common::TestDb::new();
    let pool = test_db.pool_with_size(4);
    let mut conn = pool.get().expect("conn");

    let agent = member(&mut conn, "Agent", "agent");
    let state = default_state_id(&mut conn);
    let dest = ticket(&mut conn, "Dest", state, agent.uuid);

    let srv = spawn(&pool, &agent);
    let client = awc::Client::new();
    let resp = client
        .post(srv.url("/api/tickets/merge"))
        .send_json(&json!({ "destination_ticket_id": dest.id, "source_ticket_ids": [999_999] }))
        .await
        .expect("send");
    assert_eq!(resp.status(), 404);
}

#[actix_web::test]
async fn optimistic_conflict_returns_409() {
    common::ensure_test_keyring();
    let test_db = common::TestDb::new();
    let pool = test_db.pool_with_size(4);
    let mut conn = pool.get().expect("conn");

    let agent = member(&mut conn, "Agent", "agent");
    let state = default_state_id(&mut conn);
    let dest = ticket(&mut conn, "Dest", state, agent.uuid);
    let src = ticket(&mut conn, "Source", state, agent.uuid);

    let srv = spawn(&pool, &agent);
    let client = awc::Client::new();
    let mut resp = client
        .post(srv.url("/api/tickets/merge"))
        .send_json(&json!({
            "destination_ticket_id": dest.id,
            "source_ticket_ids": [src.id],
            "expected_state": [{ "ticket_id": dest.id, "workflow_state_id": state + 9999 }],
        }))
        .await
        .expect("send");
    assert_eq!(resp.status(), 409);
    let body: serde_json::Value = resp.json().await.expect("json");
    assert_eq!(body["code"], "MERGE_STATE_CONFLICT");
}
