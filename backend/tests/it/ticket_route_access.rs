//! Ticket-scoped routes check that the caller may see the ticket, and the
//! staff-only ticket actions check the caller's role.
//!
//! A requester-role member gets a 404 for a ticket they can't see, the same
//! as for one that doesn't exist, and the normal response for their own.
#![allow(clippy::expect_used)]

use actix_web::dev::Service;
use actix_web::{web, App, HttpMessage};
use diesel::prelude::*;
use serde_json::json;
use uuid::Uuid;

use backend::extractors::WorkspaceContext;
use backend::handlers::portal::PortalContext;
use backend::middleware::RequestContext;
use backend::models::{Claims, NewTicket, NewUser, Ticket, User};
use backend::sync::actor::ActorContext;

const WS: i32 = 1;
const MISSING: i32 = 999_999;

fn member(conn: &mut PgConnection, name: &str, role: &str) -> User {
    member_of(conn, WS, name, role)
}

fn member_of(conn: &mut PgConnection, workspace: i32, name: &str, role: &str) -> User {
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
            workspace_members::workspace_id.eq(workspace),
            workspace_members::user_uuid.eq(user.uuid),
            workspace_members::role.eq(role),
        ))
        .execute(conn)
        .expect("insert workspace member");
    user
}

fn ticket_for(conn: &mut PgConnection, state: i32, requester: Uuid, title: &str) -> Ticket {
    diesel::insert_into(backend::schema::tickets::table)
        .values(&NewTicket {
            title: title.to_string(),
            workflow_state_id: state,
            requester_uuid: Some(requester),
            ..Default::default()
        })
        .get_result(conn)
        .expect("insert ticket")
}

struct Fixture {
    _db: crate::common::TestDb,
    pool: crate::common::TestPool,
    workspace_uuid: Uuid,
    requester: User,
    agent: User,
    /// The requester's own ticket.
    mine: Ticket,
    /// Someone else's ticket the requester can't see.
    theirs: Ticket,
    asset_id: i32,
}

impl Fixture {
    fn new() -> Self {
        crate::common::ensure_test_keyring();
        let db = crate::common::TestDb::new();
        let pool = db.pool_with_size(4);
        let mut conn = pool.get().expect("conn");
        let requester = member(&mut conn, "Requester", "member");
        let other = member(&mut conn, "Other", "member");
        let agent = member(&mut conn, "Agent", "agent");
        let state = backend::repository::workflow_states::default_state(&mut conn)
            .expect("default state")
            .id;
        let mine = ticket_for(&mut conn, state, requester.uuid, "Mine");
        let theirs = ticket_for(&mut conn, state, other.uuid, "Theirs");
        let asset_id = crate::common::insert_stock_asset(&mut conn, "Cable");
        let workspace_uuid = backend::schema::workspaces::table
            .find(WS)
            .select(backend::schema::workspaces::uuid)
            .first(&mut conn)
            .expect("workspace uuid");
        drop(conn);
        Self {
            _db: db,
            pool,
            workspace_uuid,
            requester,
            agent,
            mine,
            theirs,
            asset_id,
        }
    }

    fn workspace(&self) -> WorkspaceContext {
        WorkspaceContext {
            workspace_id: WS,
            workspace_uuid: self.workspace_uuid,
            slug: "default".to_string(),
            name: "Default".to_string(),
            organisation_id: None,
            custom_domain: None,
        }
    }

    /// The agent API as `user`, with its middlewares' extensions in place.
    fn app_as(&self, user: &User) -> actix_test::TestServer {
        let pool = self.pool.clone();
        let ws = self.workspace();
        let claims = Claims {
            sub: user.uuid.to_string(),
            name: user.name.clone(),
            email: "someone@example.com".to_string(),
            platform_role: "user".to_string(),
            scope: "full".to_string(),
            sid: None,
            workspace_uuid: None,
            exp: (chrono::Utc::now().timestamp() + 3600) as usize,
            iat: chrono::Utc::now().timestamp() as usize,
        };
        let user_uuid = user.uuid;
        actix_test::start(move || {
            let corr = Uuid::now_v7();
            let actor = ActorContext::user(user_uuid, Some(corr)).with_workspace(WS);
            let ws = ws.clone();
            let claims = claims.clone();
            App::new()
                .app_data(web::Data::new(pool.clone()))
                .app_data(web::Data::new(backend::handlers::sse::SseState::new()))
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
                            "/tickets/{id}/asset-usage",
                            web::get().to(backend::handlers::asset_usage::list_for_ticket),
                        )
                        .route(
                            "/tickets/{id}/sla/explain",
                            web::get().to(backend::handlers::sla::explain_for_ticket),
                        )
                        .route(
                            "/tickets/{ticket_id}/documentation",
                            web::get().to(backend::handlers::get_documentation_pages_by_ticket_id),
                        )
                        .route(
                            "/tickets/{ticket_id}/documentation-pages",
                            web::get().to(backend::handlers::list_ticket_doc_links),
                        )
                        .route(
                            "/tickets/{id}/tags",
                            web::put().to(backend::handlers::tags::set_ticket_tags),
                        )
                        .route(
                            "/tickets/{id}/field-preview",
                            web::post().to(backend::handlers::preview_ticket_field),
                        )
                        .route(
                            "/assets/{id}/usage",
                            web::get().to(backend::handlers::asset_usage::list_for_asset),
                        ),
                )
        })
    }

    /// The portal as `user`.
    fn portal_as(&self, user: Uuid) -> actix_test::TestServer {
        let pool = self.pool.clone();
        let ws = self.workspace();
        let portal = PortalContext {
            user_uuid: user,
            workspace_id: WS,
            workspace_uuid: self.workspace_uuid,
        };
        actix_test::start(move || {
            let corr = Uuid::now_v7();
            let actor = ActorContext::user(user, Some(corr)).with_workspace(WS);
            let ws = ws.clone();
            let portal = portal.clone();
            App::new()
                .app_data(web::Data::new(pool.clone()))
                .wrap_fn(move |req, srv| {
                    req.extensions_mut().insert(ws.clone());
                    req.extensions_mut().insert(portal.clone());
                    req.extensions_mut()
                        .insert(RequestContext::new(corr, actor.clone()));
                    srv.call(req)
                })
                .service(
                    web::scope("/api/portal")
                        .route(
                            "/tickets/{id}/participants",
                            web::post().to(backend::handlers::portal::add_participant),
                        )
                        .route(
                            "/tickets/{id}/participants/{user_uuid}",
                            web::delete().to(backend::handlers::portal::remove_participant),
                        ),
                )
        })
    }
}

async fn status_of(req: awc::ClientRequest) -> u16 {
    req.send().await.expect("send").status().as_u16()
}

async fn get_status(srv: &actix_test::TestServer, path: &str) -> u16 {
    status_of(awc::Client::new().get(srv.url(path))).await
}

#[actix_web::test]
async fn ticket_reads_are_gated_on_ticket_access() {
    let fx = Fixture::new();
    let srv = fx.app_as(&fx.requester);
    for route in [
        "asset-usage",
        "sla/explain",
        "documentation",
        "documentation-pages",
    ] {
        assert_eq!(
            get_status(&srv, &format!("/api/tickets/{}/{route}", fx.mine.id)).await,
            200,
            "{route}: own ticket"
        );
        assert_eq!(
            get_status(&srv, &format!("/api/tickets/{}/{route}", fx.theirs.id)).await,
            404,
            "{route}: someone else's ticket"
        );
        assert_eq!(
            get_status(&srv, &format!("/api/tickets/{MISSING}/{route}")).await,
            404,
            "{route}: missing ticket"
        );
    }

    let staff = fx.app_as(&fx.agent);
    assert_eq!(
        get_status(
            &staff,
            &format!("/api/tickets/{}/sla/explain", fx.theirs.id)
        )
        .await,
        200,
        "staff see every ticket"
    );
}

#[actix_web::test]
async fn tagging_and_staff_field_previews_need_staff() {
    let fx = Fixture::new();
    let requester = fx.app_as(&fx.requester);
    let agent = fx.app_as(&fx.agent);
    let tags = |srv: &actix_test::TestServer| {
        awc::Client::new()
            .put(srv.url(&format!("/api/tickets/{}/tags", fx.mine.id)))
            .send_json(&json!({ "tag_ids": [] }))
    };
    assert_eq!(tags(&requester).await.expect("send").status(), 403);
    assert_eq!(tags(&agent).await.expect("send").status(), 200);

    // A requester may rename their ticket, so may preview the title, and
    // nothing else.
    let preview = |srv: &actix_test::TestServer, field: &'static str| {
        awc::Client::new()
            .post(srv.url(&format!("/api/tickets/{}/field-preview", fx.mine.id)))
            .send_json(&json!({ "field": field, "value": "Draft" }))
    };
    let sent = |srv, field| async move { preview(srv, field).await.expect("send").status() };
    assert_eq!(sent(&requester, "resolution_notes").await, 403);
    assert_eq!(sent(&requester, "title").await, 202);
    assert_eq!(sent(&agent, "resolution_notes").await, 202);
}

#[actix_web::test]
async fn asset_usage_history_hides_ticket_details_from_non_staff() {
    let fx = Fixture::new();
    {
        use backend::schema::asset_usage_log as log;
        let mut conn = fx.pool.get().expect("conn");
        diesel::insert_into(log::table)
            .values((
                log::asset_id.eq(fx.asset_id),
                log::ticket_id.eq(Some(fx.theirs.id)),
                log::quantity_used.eq(bigdecimal::BigDecimal::from(2)),
                log::unit.eq("m"),
                log::recorded_by.eq(Some(fx.agent.uuid)),
                log::notes.eq(Some("for the boardroom")),
                log::event_kind.eq("usage"),
            ))
            .execute(&mut conn)
            .expect("insert usage row");
    }
    let rows = |srv: actix_test::TestServer, id: i32| async move {
        let mut resp = awc::Client::new()
            .get(srv.url(&format!("/api/assets/{id}/usage")))
            .send()
            .await
            .expect("send");
        assert_eq!(resp.status(), 200);
        let body: serde_json::Value = resp.json().await.expect("json");
        body.as_array().expect("rows").clone()
    };

    let staff = rows(fx.app_as(&fx.agent), fx.asset_id).await;
    assert_eq!(staff.len(), 1);
    assert_eq!(staff[0]["ticket_id"], json!(fx.theirs.id));
    assert_eq!(staff[0]["notes"], json!("for the boardroom"));

    let member = rows(fx.app_as(&fx.requester), fx.asset_id).await;
    assert_eq!(member.len(), 1);
    assert_eq!(member[0]["ticket_id"], serde_json::Value::Null);
    assert_eq!(member[0]["notes"], serde_json::Value::Null);
    assert_eq!(member[0]["recorded_by"], serde_json::Value::Null);
    assert_eq!(member[0]["quantity_used"], staff[0]["quantity_used"]);
}

#[actix_web::test]
async fn portal_participants_need_a_visible_ticket() {
    let fx = Fixture::new();
    let srv = fx.portal_as(fx.requester.uuid);
    let add = |id: i32| {
        awc::Client::new()
            .post(srv.url(&format!("/api/portal/tickets/{id}/participants")))
            .send_json(&json!({ "email": "colleague@example.com" }))
    };
    let remove = |id: i32, who: Uuid| {
        status_of(
            awc::Client::new()
                .delete(srv.url(&format!("/api/portal/tickets/{id}/participants/{who}"))),
        )
    };

    // Someone else's ticket and a missing one look the same.
    assert_eq!(add(fx.theirs.id).await.expect("send").status(), 404);
    assert_eq!(add(MISSING).await.expect("send").status(), 404);
    assert_eq!(remove(fx.theirs.id, fx.requester.uuid).await, 404);
    assert_eq!(remove(MISSING, fx.requester.uuid).await, 404);

    // Their own ticket works as before.
    let mut resp = add(fx.mine.id).await.expect("send");
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.expect("json");
    let added = body["participants"]
        .as_array()
        .expect("participants")
        .iter()
        .find(|p| p["is_requester"] == json!(false))
        .and_then(|p| p["uuid"].as_str())
        .and_then(|u| Uuid::parse_str(u).ok())
        .expect("added participant");
    assert_eq!(remove(fx.mine.id, added).await, 200);
}

fn verified_email(conn: &mut PgConnection, user: Uuid, email: &str) {
    use backend::schema::user_emails;
    diesel::insert_into(user_emails::table)
        .values((
            user_emails::user_uuid.eq(user),
            user_emails::email.eq(email),
            user_emails::email_type.eq("personal"),
            user_emails::is_primary.eq(true),
            user_emails::is_verified.eq(true),
        ))
        .execute(conn)
        .expect("insert email");
}

#[actix_web::test]
async fn a_participant_from_another_workspace_joins_under_the_address_name() {
    let fx = Fixture::new();
    {
        let mut conn = fx.pool.get().expect("conn");
        let elsewhere = crate::common::mint_workspace(&mut conn, "elsewhere", "Elsewhere");
        let outsider = member_of(&mut conn, elsewhere, "Real Name", "member");
        verified_email(&mut conn, outsider.uuid, "ceo@elsewhere.example");
        let colleague = member(&mut conn, "Colleague Name", "member");
        verified_email(&mut conn, colleague.uuid, "colleague@here.example");
    }
    let srv = fx.portal_as(fx.requester.uuid);
    let add = |email: &'static str| {
        awc::Client::new()
            .post(srv.url(&format!("/api/portal/tickets/{}/participants", fx.mine.id)))
            .send_json(&json!({ "email": email }))
    };
    let names = |body: serde_json::Value| -> Vec<String> {
        body["participants"]
            .as_array()
            .expect("participants")
            .iter()
            .filter_map(|p| p["name"].as_str().map(str::to_string))
            .collect()
    };

    let mut resp = add("ceo@elsewhere.example").await.expect("send");
    assert_eq!(resp.status(), 200);
    let shown = names(resp.json().await.expect("json"));
    assert!(shown.contains(&"ceo".to_string()), "{shown:?}");
    assert!(!shown.contains(&"Real Name".to_string()), "{shown:?}");

    // Someone already in this workspace keeps the name it knows them by.
    let mut resp = add("colleague@here.example").await.expect("send");
    assert_eq!(resp.status(), 200);
    let shown = names(resp.json().await.expect("json"));
    assert!(shown.contains(&"Colleague Name".to_string()), "{shown:?}");
    assert!(!shown.contains(&"Real Name".to_string()), "{shown:?}");
}
