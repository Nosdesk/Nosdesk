//! Bootstrap streams the pinned workspace's tickets when the client
//! subscribes with that workspace's group.
//!
//! Regression for the hardcoded `workspace:1` sync grant: the frontend
//! subscribes `workspace:<real id>` (dcf44ff9), but the backend granted
//! and matched only the literal `workspace:1`, so on any instance whose
//! tenant workspace wasn't id 1 the bootstrap granted nothing and
//! streamed zero tickets — "All Tickets" only ever showed rows injected
//! by detail-page visits. This drives the real handler stack (hosted
//! workspace resolution → dual auth membership gate → NDJSON streamer)
//! for a workspace that is deliberately NOT id 1 and asserts both
//! directions: the tenant's ticket streams, workspace 1's does not.

#![allow(clippy::expect_used)]

use actix_web::middleware::from_fn;
use actix_web::{test, web, App};
use diesel::prelude::*;

use backend::middleware::api_token::dual_auth_middleware;
use backend::middleware::{DeploymentMode, WorkspaceContextConfig, WorkspaceContextMiddleware};
use backend::models::{NewActiveSession, NewTicket};
use backend::repository::workspaces::add_membership;
use backend::sync::actor::ActorContext;
use backend::sync::session::with_actor_context;
use backend::utils::jwt::JwtUtils;

mod common;

/// Insert a default workflow state for the pinned workspace and a
/// ticket in it. Runs under the actor context so the RLS WITH CHECK
/// and the `workspace_id` column defaults resolve from the pin.
fn seed_ticket(
    conn: &mut backend::db::DbConnection,
    actor: &ActorContext,
    title: &str,
    requester: uuid::Uuid,
) {
    with_actor_context::<_, diesel::result::Error>(conn, actor, |c| {
        use backend::schema::workflow_states::dsl as s;
        let state_id: i32 = match s::workflow_states
            .filter(s::is_default.eq(true))
            .select(s::id)
            .first(c)
            .optional()?
        {
            Some(id) => id,
            None => {
                diesel::sql_query(
                    "INSERT INTO workflow_states (name, category, color, position, is_default) \
                     VALUES ('Backlog', 'backlog', 'slate', 0, true)",
                )
                .execute(c)?;
                s::workflow_states
                    .filter(s::is_default.eq(true))
                    .select(s::id)
                    .first(c)?
            }
        };
        diesel::insert_into(backend::schema::tickets::table)
            .values(&NewTicket {
                title: title.to_string(),
                workflow_state_id: state_id,
                requester_uuid: Some(requester),
                ..Default::default()
            })
            .execute(c)?;
        Ok(())
    })
    .expect("seed workflow state + ticket");
}

#[actix_web::test]
async fn bootstrap_streams_tickets_for_a_non_default_workspace() {
    common::ensure_test_keyring();
    if std::env::var("JWT_SECRET").is_err() {
        std::env::set_var("JWT_SECRET", "test-secret-key-for-testing-only-32chars");
    }
    let db = common::TestDb::new();
    let pool = db.pool_with_size(4);

    // Tenant workspace `mercury-boot` (id != 1, like a control-plane
    // provisioned instance), an agent who is a member, one ticket in
    // the tenant and a decoy in the bootstrap workspace.
    let (ws, jwt) = {
        let mut conn = pool.get().expect("conn");
        let ws = common::mint_workspace(&mut conn, "mercury-boot", "Mercury Boot");
        assert_ne!(ws, 1, "the regression needs a non-default workspace");
        let agent = common::insert_user(&mut conn, "Boot Agent");

        let tenant_actor = ActorContext::user(agent.uuid, None).with_workspace(ws);
        with_actor_context::<_, diesel::result::Error>(&mut conn, &tenant_actor, |c| {
            add_membership(c, ws, agent.uuid, "admin")?;
            Ok(())
        })
        .expect("add membership");

        seed_ticket(
            &mut conn,
            &tenant_actor,
            "Mercury printer on fire",
            agent.uuid,
        );
        seed_ticket(
            &mut conn,
            &ActorContext::user(agent.uuid, None).with_workspace(1),
            "Default workspace decoy",
            agent.uuid,
        );

        let session = backend::repository::active_sessions::create_session(
            &mut conn,
            NewActiveSession {
                user_uuid: agent.uuid,
                device_name: Some("bootstrap-scope-test".into()),
                ip_address: None,
                user_agent: Some("nosdesk-test".into()),
                location: None,
                expires_at: (chrono::Utc::now() + chrono::Duration::hours(1)).naive_utc(),
                is_current: true,
                oidc_id_token: None,
            },
        )
        .expect("create session");
        let jwt = JwtUtils::create_token(&agent, &session.session_id).expect("mint JWT");
        (ws, jwt)
    };

    // The real stack: hosted workspace resolution from the Host
    // subdomain, then dual auth (which membership-gates and pins the
    // request actor to the resolved workspace), then the sync routes.
    let server_pool = pool.clone();
    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(server_pool))
            .service(
                web::scope("/api")
                    .wrap(from_fn(dual_auth_middleware))
                    .configure(backend::handlers::sync::config),
            )
            .wrap(WorkspaceContextMiddleware::new(WorkspaceContextConfig {
                mode: DeploymentMode::Hosted,
                bootstrap: None,
            })),
    )
    .await;

    let req = test::TestRequest::get()
        .uri(&format!(
            "/api/sync/bootstrap?groups=workspace:{ws}&schema=test"
        ))
        .insert_header(("Host", "mercury-boot.nosdesk.test"))
        .insert_header(("Authorization", format!("Bearer {jwt}")))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), 200, "bootstrap should stream");
    let body = String::from_utf8(test::read_body(resp).await.to_vec()).expect("utf8 body");

    let lines: Vec<serde_json::Value> = body
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).expect("NDJSON line parses"))
        .collect();

    // The workspace's group must be granted (the old code granted only
    // the literal workspace:1, so this set came back empty)…
    let meta = lines
        .iter()
        .find_map(|l| l.get("__meta__"))
        .expect("bootstrap emits a __meta__ header");
    let granted: Vec<String> = meta["groups_granted"]
        .as_array()
        .expect("groups_granted array")
        .iter()
        .filter_map(|g| g.as_str().map(String::from))
        .collect();
    assert!(
        granted.contains(&format!("workspace:{ws}")),
        "the tenant's workspace group must be granted, got {granted:?}"
    );

    // …and the grant must actually unlock workspace-wide ticket
    // streaming, scoped by RLS to the pinned tenant only.
    let ticket_titles: Vec<&str> = lines
        .iter()
        .filter(|l| l.get("__model__").and_then(|m| m.as_str()) == Some("ticket"))
        .filter_map(|l| l.get("title").and_then(|t| t.as_str()))
        .collect();
    assert!(
        ticket_titles.contains(&"Mercury printer on fire"),
        "the tenant workspace's ticket must stream, got {ticket_titles:?}"
    );
    assert!(
        !ticket_titles.contains(&"Default workspace decoy"),
        "another workspace's ticket must not stream, got {ticket_titles:?}"
    );
}
