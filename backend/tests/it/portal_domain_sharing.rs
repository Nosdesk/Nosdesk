//! Sharing requests within an organisation. With `portal_share_by_domain` on,
//! a requester's portal also lists requests from colleagues at their verified
//! domain. Requests that staff raised under their own name are shared only
//! when `portal_share_staff_requests` is also on. Whether a request was raised
//! by staff is fixed when it is raised, so a later change of role moves
//! nothing in or out of the shared view.

use actix_web::dev::Service;
use actix_web::http::StatusCode;
use actix_web::test as http_test;
use actix_web::{web, App, HttpMessage};
use diesel::prelude::*;
use serde_json::json;
use uuid::Uuid;

use backend::extractors::WorkspaceContext;
use backend::handlers::portal::PortalContext;
use backend::middleware::RequestContext;
use backend::models::{Claims, NewTicket, Ticket, UpdateSiteSettings};
use backend::repository::site_settings::update_site_settings;
use backend::repository::workspaces::{
    add_membership, update_membership_role, SeatWriteAuthority, UpdateMembershipRoleResult,
};
use backend::sync::actor::ActorContext;
use backend::sync::session::run_in_workspace;

use crate::common::{self, TestPool, WorkspaceSeed};

const REF: &str = "test:portal_domain_sharing";

struct Fixture {
    _db: common::TestDb,
    pool: TestPool,
    ws: WorkspaceSeed,
    /// An ordinary requester at the viewer's domain.
    colleague: Uuid,
    /// An agent at the viewer's domain.
    agent: Uuid,
    /// Requests the domain's ordinary colleague raised.
    colleagues: Ticket,
    /// Requests an agent at the same domain raised under their own name.
    agents: Ticket,
    /// Requests the workspace admin raised under their own name.
    admins: Ticket,
}

impl Fixture {
    fn new() -> Self {
        common::ensure_test_keyring();
        let db = common::TestDb::new();
        let seed = db.pool_with_size(2);
        let ws = common::seed_two_workspaces(&mut seed.get().expect("conn")).a;
        let a = ws.workspace_id;
        let colleague = common::insert_plain_user(&mut seed.get().expect("conn"), "Colleague");
        let agent = common::insert_plain_user(&mut seed.get().expect("conn"), "Agent");
        run_in_workspace(&seed, REF, a, |c| {
            add_membership(c, a, colleague, "member", SeatWriteAuthority::ControlPlane)?;
            add_membership(c, a, agent, "agent", SeatWriteAuthority::ControlPlane)
        })
        .expect("memberships");
        for (user, email) in [
            (ws.member_uuid, "viewer@acme.test"),
            (colleague, "colleague@acme.test"),
            (agent, "agent@acme.test"),
            (ws.admin_uuid, "admin@acme.test"),
        ] {
            use backend::schema::user_emails;
            diesel::insert_into(user_emails::table)
                .values((
                    user_emails::user_uuid.eq(user),
                    user_emails::email.eq(email),
                    user_emails::email_type.eq("personal"),
                    user_emails::is_primary.eq(true),
                    user_emails::is_verified.eq(true),
                ))
                .execute(&mut seed.get().expect("conn"))
                .expect("verified address");
        }
        let ticket = |title: &str, requester: Uuid| -> Ticket {
            run_in_workspace(&seed, REF, a, |c| {
                let state = backend::repository::workflow_states::default_state(c)?;
                diesel::insert_into(backend::schema::tickets::table)
                    .values(&NewTicket {
                        title: title.to_string(),
                        workflow_state_id: state.id,
                        requester_uuid: Some(requester),
                        ..Default::default()
                    })
                    .get_result(c)
            })
            .expect("insert ticket")
        };
        let colleagues = ticket("Printer on level 2 jammed", colleague);
        let agents = ticket("Offboard a departing employee", agent);
        let admins = ticket("Follow-up from Tuesday's outage", ws.admin_uuid);
        let pool = db.runtime_pool(4);
        Self {
            _db: db,
            pool,
            ws,
            colleague,
            agent,
            colleagues,
            agents,
            admins,
        }
    }

    fn settings(&self, update: UpdateSiteSettings) {
        let actor =
            ActorContext::user(self.ws.admin_uuid, None).with_workspace(self.ws.workspace_id);
        backend::sync::session::with_actor_context::<_, diesel::result::Error>(
            &mut self.pool.get().expect("conn"),
            &actor,
            |c| update_site_settings(c, update).map(|_| ()),
        )
        .expect("update settings");
    }

    /// Give `user` the workspace role `role`, as an admin or the control plane
    /// would.
    fn set_role(&self, user: Uuid, role: &str) {
        let actor =
            ActorContext::user(self.ws.admin_uuid, None).with_workspace(self.ws.workspace_id);
        let result = backend::sync::session::with_actor_context::<_, diesel::result::Error>(
            &mut self.pool.get().expect("conn"),
            &actor,
            |c| {
                update_membership_role(
                    c,
                    self.ws.workspace_id,
                    user,
                    role,
                    SeatWriteAuthority::ControlPlane,
                )
            },
        )
        .expect("change role");
        assert!(
            matches!(result, UpdateMembershipRoleResult::Updated(_)),
            "{result:?}"
        );
    }

    fn share(&self) {
        self.settings(UpdateSiteSettings {
            portal_share_by_domain: Some(true),
            ..Default::default()
        });
    }

    fn workspace(&self) -> WorkspaceContext {
        WorkspaceContext {
            workspace_id: self.ws.workspace_id,
            workspace_uuid: self.ws.workspace_uuid,
            slug: self.ws.slug.clone(),
            name: "A".to_string(),
            organisation_id: None,
            custom_domain: None,
        }
    }
}

/// The portal routes the requester `user` uses, as its middlewares would set
/// them up.
macro_rules! portal_as {
    ($fx:expr, $user:expr) => {{
        let user: Uuid = $user;
        let a = $fx.ws.workspace_id;
        let workspace = $fx.workspace();
        let portal = PortalContext {
            user_uuid: user,
            workspace_id: a,
            workspace_uuid: $fx.ws.workspace_uuid,
        };
        let corr = Uuid::now_v7();
        let actor = ActorContext::user(user, Some(corr)).with_workspace(a);
        http_test::init_service(
            App::new()
                .app_data(web::Data::new($fx.pool.clone()))
                .wrap_fn(move |req, srv| {
                    req.extensions_mut().insert(workspace.clone());
                    req.extensions_mut().insert(portal.clone());
                    req.extensions_mut()
                        .insert(RequestContext::new(corr, actor.clone()));
                    srv.call(req)
                })
                .route(
                    "/api/portal/tickets",
                    web::get().to(backend::handlers::portal::list_my_tickets),
                )
                .route(
                    "/api/portal/tickets/{id}",
                    web::get().to(backend::handlers::portal::get_my_ticket),
                ),
        )
        .await
    }};
}

/// The ticket ids the portal lists, and whether each listed fixture ticket's
/// detail page opens.
macro_rules! seen {
    ($app:expr, $fx:expr) => {{
        let resp = http_test::call_service(
            $app,
            http_test::TestRequest::get()
                .uri("/api/portal/tickets")
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK, "list");
        let body: Vec<serde_json::Value> = http_test::read_body_json(resp).await;
        let listed: Vec<i64> = body.iter().filter_map(|t| t["id"].as_i64()).collect();
        let mut opens = Vec::new();
        for t in [&$fx.colleagues, &$fx.agents, &$fx.admins] {
            let resp = http_test::call_service(
                $app,
                http_test::TestRequest::get()
                    .uri(&format!("/api/portal/tickets/{}", t.id))
                    .to_request(),
            )
            .await;
            opens.push((t.id, resp.status() == StatusCode::OK));
        }
        (listed, opens)
    }};
}

/// The guest-access admin endpoint, as a workspace admin of `fx`.
macro_rules! settings_as_admin {
    ($fx:expr) => {{
        let now = chrono::Utc::now().timestamp();
        let claims = Claims {
            sub: $fx.ws.admin_uuid.to_string(),
            name: "Admin".to_string(),
            email: "admin@acme.test".to_string(),
            platform_role: "user".to_string(),
            scope: "full".to_string(),
            sid: None,
            workspace_uuid: None,
            exp: (now + 3600) as usize,
            iat: now as usize,
        };
        let workspace = $fx.workspace();
        let corr = Uuid::now_v7();
        let actor =
            ActorContext::user($fx.ws.admin_uuid, Some(corr)).with_workspace($fx.ws.workspace_id);
        http_test::init_service(
            App::new()
                .app_data(web::Data::new($fx.pool.clone()))
                .wrap_fn(move |req, srv| {
                    req.extensions_mut().insert(workspace.clone());
                    req.extensions_mut().insert(claims.clone());
                    req.extensions_mut()
                        .insert(RequestContext::new(corr, actor.clone()));
                    srv.call(req)
                })
                .service(web::scope("/api").configure(backend::handlers::guest_settings::config)),
        )
        .await
    }};
}

/// Sharing on, staff requests left out: a requester sees the ordinary
/// colleague's request, and neither the agent's nor the admin's, in the list
/// or by opening it.
#[actix_web::test]
async fn sharing_leaves_out_requests_staff_raised() {
    let fx = Fixture::new();
    fx.share();
    let app = portal_as!(fx, fx.ws.member_uuid);
    let (listed, opens) = seen!(&app, fx);

    assert!(
        listed.contains(&i64::from(fx.colleagues.id)),
        "a colleague's request is shared: {listed:?}"
    );
    assert!(
        !listed.contains(&i64::from(fx.agents.id)),
        "an agent's own request is not shared: {listed:?}"
    );
    assert!(
        !listed.contains(&i64::from(fx.admins.id)),
        "an admin's own request is not shared: {listed:?}"
    );
    assert_eq!(
        opens,
        vec![
            (fx.colleagues.id, true),
            (fx.agents.id, false),
            (fx.admins.id, false)
        ],
        "detail follows the list"
    );
}

/// With staff requests included, the requester sees all three.
#[actix_web::test]
async fn sharing_includes_staff_requests_when_turned_on() {
    let fx = Fixture::new();
    fx.share();
    // The new flag goes through the admin endpoint, so this compiles (and
    // runs) against a build that doesn't know it yet.
    let admin_app = settings_as_admin!(fx);
    let resp = http_test::call_service(
        &admin_app,
        http_test::TestRequest::patch()
            .uri("/api/admin/guest-settings")
            .set_json(json!({ "portal_share_staff_requests": true }))
            .to_request(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);

    let app = portal_as!(fx, fx.ws.member_uuid);
    let (listed, opens) = seen!(&app, fx);
    for t in [&fx.colleagues, &fx.agents, &fx.admins] {
        assert!(listed.contains(&i64::from(t.id)), "{}: {listed:?}", t.title);
    }
    assert!(opens.iter().all(|(_, ok)| *ok), "{opens:?}");
}

/// The admin endpoint reads and writes the staff flag, which starts off.
#[actix_web::test]
async fn the_staff_flag_round_trips_through_guest_settings() {
    let fx = Fixture::new();
    let app = settings_as_admin!(fx);
    let get = || {
        http_test::TestRequest::get()
            .uri("/api/admin/guest-settings")
            .to_request()
    };

    let before: serde_json::Value = http_test::call_and_read_body_json(&app, get()).await;
    assert_eq!(
        before["portal_share_staff_requests"],
        json!(false),
        "off by default: {before}"
    );

    let resp = http_test::call_service(
        &app,
        http_test::TestRequest::patch()
            .uri("/api/admin/guest-settings")
            .set_json(json!({ "portal_share_staff_requests": true }))
            .to_request(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let saved: serde_json::Value = http_test::read_body_json(resp).await;
    assert_eq!(saved["portal_share_staff_requests"], json!(true), "{saved}");

    let after: serde_json::Value = http_test::call_and_read_body_json(&app, get()).await;
    assert_eq!(after["portal_share_staff_requests"], json!(true), "{after}");
    assert_eq!(
        after["portal_share_by_domain"],
        json!(false),
        "the other flag is untouched: {after}"
    );
}

/// An agent's own request stays out of the shared view after they are
/// demoted to requester: it was raised by staff. This is how a hosted seat is
/// taken away (the control plane leaves the person a `member`).
#[actix_web::test]
async fn a_request_raised_as_staff_stays_unshared_after_a_demotion() {
    let fx = Fixture::new();
    fx.share();
    fx.set_role(fx.agent, "member");
    let app = portal_as!(fx, fx.ws.member_uuid);
    let (listed, opens) = seen!(&app, fx);

    assert!(
        !listed.contains(&i64::from(fx.agents.id)),
        "the demoted agent's request stays unshared: {listed:?}"
    );
    assert!(opens.contains(&(fx.agents.id, false)), "{opens:?}");
}

/// A requester's request stays shared after they are promoted to agent: it was
/// raised by a requester.
#[actix_web::test]
async fn a_request_raised_as_a_requester_stays_shared_after_a_promotion() {
    let fx = Fixture::new();
    fx.share();
    fx.set_role(fx.colleague, "agent");
    let app = portal_as!(fx, fx.ws.member_uuid);
    let (listed, opens) = seen!(&app, fx);

    assert!(
        listed.contains(&i64::from(fx.colleagues.id)),
        "the promoted colleague's earlier request stays shared: {listed:?}"
    );
    assert!(opens.contains(&(fx.colleagues.id, true)), "{opens:?}");
}

/// A request an agent files for themselves through the API is raised by
/// staff, and stays out of the shared view after they are demoted.
#[actix_web::test]
async fn a_request_an_agent_files_through_the_api_is_raised_by_staff() {
    let fx = Fixture::new();
    fx.share();
    let now = chrono::Utc::now().timestamp();
    let claims = Claims {
        sub: fx.agent.to_string(),
        name: "Agent".to_string(),
        email: "agent@acme.test".to_string(),
        platform_role: "user".to_string(),
        scope: "full".to_string(),
        sid: None,
        workspace_uuid: None,
        exp: (now + 3600) as usize,
        iat: now as usize,
    };
    let state = run_in_workspace(&fx.pool, REF, fx.ws.workspace_id, |c| {
        backend::repository::workflow_states::default_state(c)
    })
    .expect("default state")
    .id;
    let workspace = fx.workspace();
    let corr = Uuid::now_v7();
    let actor = ActorContext::user(fx.agent, Some(corr)).with_workspace(fx.ws.workspace_id);
    let search_dir = tempfile::tempdir().expect("search dir");
    let search = std::sync::Arc::new(
        backend::services::search::SearchService::new(search_dir.path(), &fx.pool)
            .expect("init search"),
    );
    let api = http_test::init_service(
        App::new()
            .app_data(web::Data::new(fx.pool.clone()))
            .app_data(web::Data::new(search))
            .wrap_fn(move |req, srv| {
                req.extensions_mut().insert(workspace.clone());
                req.extensions_mut().insert(claims.clone());
                req.extensions_mut()
                    .insert(RequestContext::new(corr, actor.clone()));
                srv.call(req)
            })
            .route(
                "/api/tickets",
                web::post().to(backend::handlers::create_ticket),
            ),
    )
    .await;
    let resp = http_test::call_service(
        &api,
        http_test::TestRequest::post()
            .uri("/api/tickets")
            .set_json(json!({
                "title": "Laptop for a new starter",
                "workflow_state_id": state,
                "priority": "medium",
                "requester_uuid": fx.agent,
            }))
            .to_request(),
    )
    .await;
    assert!(resp.status().is_success(), "create: {}", resp.status());
    let created: serde_json::Value = http_test::read_body_json(resp).await;
    let filed = created["id"].as_i64().expect("ticket id");

    fx.set_role(fx.agent, "member");
    let app = portal_as!(fx, fx.ws.member_uuid);
    let (listed, _) = seen!(&app, fx);
    assert!(
        !listed.contains(&filed),
        "the agent's API request stays unshared: {listed:?}"
    );
    assert!(
        listed.contains(&i64::from(fx.colleagues.id)),
        "sharing is on: {listed:?}"
    );
}
