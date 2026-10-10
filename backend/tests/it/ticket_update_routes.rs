//! `PUT` and `PATCH /api/tickets/{id}` save through one path. The columns the
//! server owns stay as they are whatever a PUT body says about them, someone
//! who doesn't handle tickets may change only the title, a PUT records the
//! event PATCH's does and still answers with the ticket row, "not spam" clears
//! the spam flag, every change raises the `ticket.updated` webhook, and a
//! ticket read with GET can be sent straight back as a PUT.

use std::sync::Arc;

use actix_web::dev::Service;
use actix_web::http::{Method, StatusCode};
use actix_web::test as http_test;
use actix_web::{web, App, HttpMessage};
use diesel::prelude::*;
use diesel::sql_types::Integer;
use serde_json::{json, Value};
use uuid::Uuid;

use backend::extractors::WorkspaceContext;
use backend::middleware::RequestContext;
use backend::models::{Claims, NewTicket, Ticket, TicketPriority, WorkflowStateCategory};
use backend::repository::workflow_states;
use backend::services::search::SearchService;
use backend::sync::actor::ActorContext;
use backend::sync::session::run_in_workspace;

use crate::common::{self, TestPool, WorkspaceSeed};

const REF: &str = "test:ticket_update_routes";

struct Fixture {
    _db: common::TestDb,
    _search_dir: tempfile::TempDir,
    pool: TestPool,
    ws: WorkspaceSeed,
    search: Arc<SearchService>,
}

impl Fixture {
    fn new() -> Self {
        let db = common::TestDb::new();
        let pool = db.pool_with_size(4);
        let ws = common::seed_two_workspaces(&mut pool.get().expect("conn")).a;
        let search_dir = tempfile::tempdir().expect("search dir");
        let search = Arc::new(SearchService::new(search_dir.path(), &pool).expect("init search"));
        Self {
            _db: db,
            _search_dir: search_dir,
            pool,
            ws,
            search,
        }
    }

    fn state(&self, category: WorkflowStateCategory) -> i32 {
        run_in_workspace(&self.pool, REF, self.ws.workspace_id, |c| {
            workflow_states::first_in_category(c, category)
        })
        .expect("state")
        .id
    }

    fn open_state(&self) -> i32 {
        run_in_workspace(
            &self.pool,
            REF,
            self.ws.workspace_id,
            workflow_states::default_state,
        )
        .expect("default state")
        .id
    }

    fn insert(&self, ticket: NewTicket) -> Ticket {
        run_in_workspace(&self.pool, REF, self.ws.workspace_id, |c| {
            diesel::insert_into(backend::schema::tickets::table)
                .values(&ticket)
                .get_result(c)
        })
        .expect("insert ticket")
    }

    fn ticket(&self, id: i32) -> Ticket {
        run_in_workspace(&self.pool, REF, self.ws.workspace_id, |c| {
            backend::repository::get_ticket_by_id(c, id)
        })
        .expect("read ticket")
    }

    /// The sync actions recorded for the ticket, oldest first.
    fn events(&self, id: i32) -> Vec<(String, Value)> {
        run_in_workspace(&self.pool, REF, self.ws.workspace_id, |c| {
            use backend::schema::sync_actions;
            sync_actions::table
                .filter(sync_actions::aggregate_id.eq(id.to_string()))
                .filter(sync_actions::event_type.like("ticket.%"))
                .order(sync_actions::sync_id.asc())
                .select((sync_actions::event_type, sync_actions::data))
                .load(c)
        })
        .expect("read sync actions")
    }

    /// PUT `body` to the ticket as `user`, a member of the workspace.
    async fn put(&self, user: Uuid, ticket_id: i32, body: Value) -> (StatusCode, Value) {
        self.send(Method::PUT, user, ticket_id, body).await
    }

    /// GET the ticket as `user`, a member of the workspace.
    async fn get(&self, user: Uuid, ticket_id: i32) -> (StatusCode, Value) {
        self.send(Method::GET, user, ticket_id, Value::Null).await
    }

    /// PATCH `body` to the ticket as `user`, a member of the workspace.
    async fn patch(&self, user: Uuid, ticket_id: i32, body: Value) -> (StatusCode, Value) {
        self.send(Method::PATCH, user, ticket_id, body).await
    }

    async fn send(
        &self,
        method: Method,
        user: Uuid,
        ticket_id: i32,
        body: Value,
    ) -> (StatusCode, Value) {
        let row = backend::repository::users::get_user_by_uuid(
            &user,
            &mut self.pool.get().expect("conn"),
        )
        .expect("load user");
        let claims = Claims {
            sub: row.uuid.to_string(),
            name: row.name.clone(),
            email: "someone@example.com".to_string(),
            platform_role: "user".to_string(),
            scope: "full".to_string(),
            sid: None,
            workspace_uuid: None,
            exp: (chrono::Utc::now().timestamp() + 3600) as usize,
            iat: chrono::Utc::now().timestamp() as usize,
        };
        let workspace = WorkspaceContext {
            workspace_id: self.ws.workspace_id,
            workspace_uuid: self.ws.workspace_uuid,
            slug: self.ws.slug.clone(),
            name: "A".to_string(),
            organisation_id: None,
            custom_domain: None,
        };
        let corr = Uuid::now_v7();
        let actor = ActorContext::user(user, Some(corr)).with_workspace(self.ws.workspace_id);
        let app = http_test::init_service(
            App::new()
                .app_data(web::Data::new(self.pool.clone()))
                .app_data(web::Data::new(self.search.clone()))
                .wrap_fn(move |req, srv| {
                    req.extensions_mut().insert(workspace.clone());
                    req.extensions_mut().insert(claims.clone());
                    req.extensions_mut()
                        .insert(RequestContext::new(corr, actor.clone()));
                    srv.call(req)
                })
                .service(
                    web::scope("/api")
                        .route(
                            "/tickets/{id}",
                            web::get().to(backend::handlers::get_ticket),
                        )
                        .route(
                            "/tickets/{id}",
                            web::put().to(backend::handlers::update_ticket),
                        )
                        .route(
                            "/tickets/{id}",
                            web::patch().to(backend::handlers::update_ticket_partial),
                        ),
                ),
        )
        .await;
        let mut req = http_test::TestRequest::default()
            .method(method)
            .uri(&format!("/api/tickets/{ticket_id}"));
        if !body.is_null() {
            req = req.set_json(body);
        }
        let req = req.to_request();
        let resp = http_test::call_service(&app, req).await;
        let status = resp.status();
        let bytes = http_test::read_body(resp).await;
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    /// A ticket that came in by email: every column the server owns is set.
    fn email_ticket(&self) -> Ticket {
        #[derive(QueryableByName)]
        struct Id {
            #[diesel(sql_type = Integer)]
            id: i32,
        }
        let channel = run_in_workspace(&self.pool, REF, self.ws.workspace_id, |c| {
            diesel::sql_query(
                "INSERT INTO channels (provider, name, config) \
                 VALUES ('email', 'Support', '{}') RETURNING id",
            )
            .get_result::<Id>(c)
        })
        .expect("seed channel")
        .id;
        let open = self.open_state();
        let template = self.insert(NewTicket {
            title: "Weekly backup check".to_string(),
            workflow_state_id: open,
            ..Default::default()
        });
        self.insert(NewTicket {
            title: "Printer jammed".to_string(),
            workflow_state_id: open,
            priority: TicketPriority::Low,
            submitted_via: Some("email".to_string()),
            guest_lookup_token: Some(Uuid::new_v4()),
            verification_state: Some("verified".to_string()),
            origin_channel_id: Some(channel),
            triage_state: Some("untriaged".to_string()),
            recurrence_template_id: Some(template.id),
            spam_suspected: true,
            ..Default::default()
        })
    }
}

/// The columns the server owns, as a tuple to compare before and after.
#[allow(clippy::type_complexity)]
fn server_columns(
    t: &Ticket,
) -> (
    Option<String>,
    Option<Uuid>,
    Option<String>,
    Option<i32>,
    Option<String>,
    Option<i32>,
    bool,
) {
    (
        t.submitted_via.clone(),
        t.guest_lookup_token,
        t.verification_state.clone(),
        t.origin_channel_id,
        t.triage_state.clone(),
        t.recurrence_template_id,
        t.spam_suspected,
    )
}

#[actix_web::test]
async fn a_put_carrying_server_columns_leaves_them_as_they_are() {
    let fx = Fixture::new();
    let before = fx.email_ticket();

    let (status, _) = fx
        .put(
            fx.ws.admin_uuid,
            before.id,
            json!({
                "title": "Printer still jammed",
                "workflow_state_id": before.workflow_state_id,
                "priority": "low",
                "submitted_via": "web",
                "guest_lookup_token": Uuid::new_v4(),
                "verification_state": "unverified",
                "triage_state": "triaged",
                "spam_suspected": false,
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK);

    let after = fx.ticket(before.id);
    assert_eq!(after.triage_state, before.triage_state, "triage_state");
    assert_eq!(
        after.verification_state, before.verification_state,
        "verification_state"
    );
    assert!(after.spam_suspected, "a flagged ticket stays flagged");
    assert_eq!(server_columns(&after), server_columns(&before));
    assert_eq!(after.title, "Printer still jammed", "the title is saved");
}

#[actix_web::test]
async fn a_put_leaving_out_server_columns_leaves_them_as_they_are() {
    let fx = Fixture::new();
    let before = fx.email_ticket();

    let (status, body) = fx
        .put(
            fx.ws.admin_uuid,
            before.id,
            json!({
                "title": "Printer jammed again",
                "workflow_state_id": before.workflow_state_id,
                "priority": "high",
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let after = fx.ticket(before.id);
    assert_eq!(after.origin_channel_id, before.origin_channel_id);
    assert_eq!(server_columns(&after), server_columns(&before));
    assert_eq!(after.title, "Printer jammed again");
    assert_eq!(after.priority, TicketPriority::High);
}

#[actix_web::test]
async fn a_requester_put_may_change_only_the_title() {
    let fx = Fixture::new();
    let requester = fx.ws.member_uuid;
    let open = fx.open_state();
    let before = fx.insert(NewTicket {
        title: "Printer jammed".to_string(),
        workflow_state_id: open,
        priority: TicketPriority::Low,
        requester_uuid: Some(requester),
        ..Default::default()
    });
    let body = |title: &str, priority: &str| {
        json!({
            "title": title,
            "workflow_state_id": open,
            "priority": priority,
            "requester_uuid": requester,
            "spam_suspected": false,
        })
    };

    let (status, _) = fx
        .put(requester, before.id, body("Printer jammed", "urgent"))
        .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "a requester can't re-prioritise"
    );
    let (status, _) = fx
        .put(requester, before.id, body("Printer on fire", "urgent"))
        .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "nor with a new title alongside"
    );
    assert_eq!(fx.ticket(before.id).priority, TicketPriority::Low);
    assert_eq!(fx.ticket(before.id).title, "Printer jammed");

    let (status, _) = fx
        .put(requester, before.id, body("My printer is jammed", "low"))
        .await;
    assert_eq!(status, StatusCode::OK, "a retitle that echoes the rest");
    assert_eq!(fx.ticket(before.id).title, "My printer is jammed");
}

#[actix_web::test]
async fn a_put_that_moves_the_state_records_the_previous_state() {
    let fx = Fixture::new();
    let open = fx.open_state();
    let done = fx.state(WorkflowStateCategory::Done);
    let before = fx.insert(NewTicket {
        title: "Printer jammed".to_string(),
        workflow_state_id: open,
        ..Default::default()
    });
    let body = |state: i32| {
        json!({
            "title": "Printer jammed",
            "workflow_state_id": state,
            "priority": "medium",
            "spam_suspected": false,
        })
    };

    let (status, row) = fx.put(fx.ws.admin_uuid, before.id, body(done)).await;
    assert_eq!(status, StatusCode::OK);
    let events = fx.events(before.id);
    let (event_type, data) = events.last().expect("an event");
    assert_eq!(event_type, "ticket.workflow_state_changed");
    assert_eq!(data["previous_workflow_state_id"], json!(open), "{data}");
    assert_eq!(data["workflow_state_id"], json!(done));

    // The response is still the ticket row.
    assert_eq!(row["id"], json!(before.id));
    assert_eq!(row["workflow_state_id"], json!(done));
    assert!(row["closed_at"].is_string(), "{row}");
    assert!(
        row.get("comments").is_none(),
        "the row, not the complete ticket"
    );

    let (status, row) = fx.put(fx.ws.admin_uuid, before.id, body(done)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(row["id"], json!(before.id));
    assert_eq!(
        fx.events(before.id).len(),
        events.len(),
        "a PUT that changes nothing records nothing"
    );
}

#[actix_web::test]
async fn not_spam_clears_the_spam_flag() {
    let fx = Fixture::new();
    let flagged = fx.email_ticket();
    assert!(flagged.spam_suspected);

    let (status, body) = fx
        .patch(
            fx.ws.admin_uuid,
            flagged.id,
            json!({ "spam_suspected": false }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(!fx.ticket(flagged.id).spam_suspected, "the flag is cleared");
    assert_eq!(
        fx.events(flagged.id)
            .last()
            .map(|(t, d)| (t.as_str(), d["spam_suspected"].clone())),
        Some(("ticket.updated", json!(false))),
        "clients hear of it"
    );
}

/// Every change a PUT or PATCH makes raises the `ticket.updated` webhook, the
/// resolution notes included.
#[actix_web::test]
async fn a_resolution_notes_change_raises_the_ticket_updated_webhook() {
    use backend::schema::webhook_outbox;
    use backend::services::webhooks::WebhookService;
    use backend::sync::session::with_actor_bypass_context;

    let fx = Fixture::new();
    let webhook = run_in_workspace(&fx.pool, REF, fx.ws.workspace_id, |c| {
        backend::repository::webhooks::create_webhook(
            c,
            "updates".into(),
            "https://sink.invalid/updates".into(),
            "secret".into(),
            vec!["ticket.updated".into()],
            None,
            Some(fx.ws.admin_uuid),
        )
    })
    .expect("create webhook");
    let open = fx.open_state();
    let ticket = fx.insert(NewTicket {
        title: "Printer jammed".to_string(),
        workflow_state_id: open,
        ..Default::default()
    });

    let (status, body) = fx
        .put(
            fx.ws.admin_uuid,
            ticket.id,
            json!({
                "title": "Printer jammed",
                "workflow_state_id": open,
                "priority": "medium",
                "resolution_notes": "Cleared the paper path",
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    // Drain the whole outbox, as the background worker does.
    let mut conn = fx.pool.get().expect("conn");
    let mut tasks = Vec::new();
    loop {
        let (batch, _) = with_actor_bypass_context(
            &mut conn,
            &ActorContext::system(REF),
            WebhookService::drain_batch_txn,
        )
        .expect("drain outbox");
        let left: i64 = with_actor_bypass_context(&mut conn, &ActorContext::system(REF), |c| {
            webhook_outbox::table.count().get_result(c)
        })
        .expect("count outbox");
        tasks.extend(batch);
        if left == 0 {
            break;
        }
    }
    let raised: Vec<_> = tasks
        .iter()
        .filter(|t| t.webhook_id == webhook.id && t.payload.data["id"] == json!(ticket.id))
        .map(|t| t.payload.event_type.clone())
        .collect();
    assert_eq!(
        raised,
        ["ticket.updated"],
        "events recorded: {:?}",
        fx.events(ticket.id)
            .into_iter()
            .map(|(t, _)| t)
            .collect::<Vec<_>>()
    );
}

#[actix_web::test]
async fn a_ticket_read_with_get_can_be_put_back() {
    let fx = Fixture::new();
    let before = fx.insert(NewTicket {
        title: "Printer jammed".to_string(),
        workflow_state_id: fx.open_state(),
        ..Default::default()
    });
    assert_eq!(before.guest_lookup_token, None);

    let (status, mut body) = fx.get(fx.ws.admin_uuid, before.id).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body["title"] = json!("Printer still jammed");

    let (status, saved) = fx.put(fx.ws.admin_uuid, before.id, body).await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(saved["title"], "Printer still jammed");
    assert_eq!(fx.ticket(before.id).title, "Printer still jammed");
}

#[actix_web::test]
async fn a_ticket_read_with_get_leaves_out_the_guest_lookup_token() {
    let fx = Fixture::new();
    let ticket = fx.email_ticket();
    assert!(ticket.guest_lookup_token.is_some());

    let (status, body) = fx.get(fx.ws.admin_uuid, ticket.id).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.get("guest_lookup_token").is_none(), "{body}");
}
