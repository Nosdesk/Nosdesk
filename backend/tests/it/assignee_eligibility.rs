//! Only people who can work tickets are assigned them: a platform admin, or
//! someone whose role in the ticket's workspace is Agent or above. The ticket
//! write refuses anyone else, and everything that picks an assignee (an
//! assignment rule, bulk assign, a new occurrence of a recurring ticket, an
//! approval that times out) offers only them.

use std::sync::Arc;

use actix_web::dev::Service;
use actix_web::http::StatusCode;
use actix_web::test as http_test;
use actix_web::{web, App, HttpMessage};
use diesel::prelude::*;
use serde_json::{json, Value};
use uuid::Uuid;

use backend::extractors::WorkspaceContext;
use backend::middleware::RequestContext;
use backend::models::{
    Claims, NewRule, NewTicket, RuleState, RuleTriggerKind, Ticket, TicketUpdate,
    WorkflowStateCategory,
};
use backend::repository::{tickets as ticket_repo, workflow_states};
use backend::schema::{groups, rules, tickets, user_groups};
use backend::services::rules::engine::Dispatcher;
use backend::services::search::SearchService;
use backend::sync::actor::ActorContext;
use backend::sync::session::with_actor_context;
use backend::utils::storage::{create_storage, Storage, StorageConfig};

use crate::common::{self, TestPool, WorkspaceSeed};

struct Fixture {
    _db: common::TestDb,
    _dir: tempfile::TempDir,
    _search_dir: tempfile::TempDir,
    pool: TestPool,
    /// The rules engine's dispatcher, on the production-shaped pool.
    engine: Dispatcher,
    ws: WorkspaceSeed,
    search: Arc<SearchService>,
    storage: Arc<dyn Storage>,
}

impl Fixture {
    fn new() -> Self {
        common::ensure_test_keyring();
        let db = common::TestDb::new();
        let pool = db.pool_with_size(4);
        let ws = common::seed_two_workspaces(&mut pool.get().expect("conn")).a;
        let dir = tempfile::tempdir().expect("temp dir");
        let search_dir = tempfile::tempdir().expect("search dir");
        let search = Arc::new(SearchService::new(search_dir.path(), &pool).expect("init search"));
        let storage = create_storage(StorageConfig::Local {
            base_path: dir.path().to_string_lossy().into_owned(),
        });
        let engine = Dispatcher::new(db.runtime_pool(2), None);
        Self {
            _db: db,
            _dir: dir,
            _search_dir: search_dir,
            pool,
            engine,
            ws,
            search,
            storage,
        }
    }

    fn admin(&self) -> ActorContext {
        ActorContext::user(self.ws.admin_uuid, None).with_workspace(self.ws.workspace_id)
    }

    /// Run `f` in the workspace as its admin.
    fn run<T>(
        &self,
        f: impl FnOnce(&mut backend::db::DbConnection) -> diesel::QueryResult<T>,
    ) -> T {
        let mut conn = self.pool.get().expect("conn");
        with_actor_context(&mut conn, &self.admin(), f).expect("run in workspace")
    }

    fn ticket(&self, title: &str, assignee: Option<Uuid>) -> Ticket {
        self.run(|c| {
            let open = workflow_states::default_state(c)?.id;
            diesel::insert_into(tickets::table)
                .values(&NewTicket {
                    title: title.to_string(),
                    workflow_state_id: open,
                    requester_uuid: Some(self.ws.member_uuid),
                    assignee_uuid: assignee,
                    ..Default::default()
                })
                .get_result(c)
        })
    }

    fn assignee_of(&self, id: i32) -> Option<Uuid> {
        self.run(|c| {
            tickets::table
                .find(id)
                .select(tickets::assignee_uuid)
                .first(c)
        })
    }

    /// A team of the member (who can't work tickets) and the admin, in that
    /// order.
    fn team(&self) -> i32 {
        self.run(|c| {
            let team: i32 = diesel::insert_into(groups::table)
                .values(groups::name.eq("Service desk"))
                .returning(groups::id)
                .get_result(c)?;
            for user in [self.ws.member_uuid, self.ws.admin_uuid] {
                diesel::insert_into(user_groups::table)
                    .values((
                        user_groups::group_id.eq(team),
                        user_groups::user_uuid.eq(user),
                    ))
                    .execute(c)?;
            }
            Ok(team)
        })
    }

    /// The only live rule: a new ticket nobody is assigned goes to `assign`.
    fn rule(&self, assign: Value) {
        self.run(|c| {
            diesel::update(rules::table)
                .set(rules::archived_at.eq(Some(chrono::Utc::now())))
                .execute(c)?;
            backend::repository::rules::create(
                c,
                NewRule {
                    workspace_id: self.ws.workspace_id,
                    name: assign.to_string(),
                    description: None,
                    trigger_kind: RuleTriggerKind::TicketCreated,
                    trigger_config: json!({}),
                    conditions: json!({ "kind": "and", "children": [
                        { "kind": "leaf", "field": "ticket.assignee_uuid", "op": "is_null" },
                    ] }),
                    actions: json!([
                        { "kind": "assign", "config": assign },
                        { "kind": "stop_processing" },
                    ]),
                    reads_set: Vec::new(),
                    writes_set: Vec::new(),
                    state: RuleState::Live,
                    priority: 1010,
                    created_by: Some(self.ws.admin_uuid),
                },
            )
        });
    }

    /// Run the rules engine until it has nothing left to do.
    async fn dispatch(&self) {
        while self.engine.dispatch_batch().await.expect("dispatch") > 0 {}
    }

    /// A new ticket, made the way the app makes one, once the rules engine
    /// has run its rules.
    async fn routed(&self, title: &str) -> Option<Uuid> {
        let ticket = self.run(|c| {
            let open = workflow_states::default_state(c)?.id;
            ticket_repo::create_ticket(
                c,
                NewTicket {
                    title: title.to_string(),
                    workflow_state_id: open,
                    requester_uuid: Some(self.ws.member_uuid),
                    ..Default::default()
                },
            )
        });
        self.dispatch().await;
        self.assignee_of(ticket.id)
    }

    /// Call the ticket routes as the admin.
    async fn call(&self, req: http_test::TestRequest) -> (StatusCode, Value) {
        let now = chrono::Utc::now().timestamp();
        let claims = Claims {
            sub: self.ws.admin_uuid.to_string(),
            name: "Admin".to_string(),
            email: "admin@example.com".to_string(),
            platform_role: "user".to_string(),
            scope: "full".to_string(),
            sid: None,
            workspace_uuid: None,
            exp: (now + 3600) as usize,
            iat: now as usize,
            file_path: None,
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
        let actor =
            ActorContext::user(self.ws.admin_uuid, Some(corr)).with_workspace(self.ws.workspace_id);
        let app = http_test::init_service(
            App::new()
                .app_data(web::Data::new(self.pool.clone()))
                .app_data(web::Data::new(self.search.clone()))
                .app_data(web::Data::new(self.storage.clone()))
                .wrap_fn(move |req, srv| {
                    req.extensions_mut().insert(workspace.clone());
                    req.extensions_mut().insert(claims.clone());
                    req.extensions_mut()
                        .insert(RequestContext::new(corr, actor.clone()));
                    srv.call(req)
                })
                .service(web::scope("/api").configure(backend::handlers::tickets::config)),
        )
        .await;
        let resp = http_test::call_service(&app, req.to_request()).await;
        let status = resp.status();
        let bytes = http_test::read_body(resp).await;
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }
}

#[test]
fn the_ticket_write_refuses_an_assignee_who_cant_work_tickets() {
    let fx = Fixture::new();
    let ticket = fx.ticket("Printer jammed", None);
    let assign = |user: Uuid| {
        let mut conn = fx.pool.get().expect("conn");
        with_actor_context::<_, diesel::result::Error>(&mut conn, &fx.admin(), |c| {
            Ok(ticket_repo::update_ticket_partial(
                c,
                ticket.id,
                TicketUpdate {
                    assignee_uuid: Some(Some(user)),
                    ..Default::default()
                },
                None,
            ))
        })
        .expect("run in workspace")
    };

    let refused = assign(fx.ws.member_uuid);
    assert!(
        matches!(
            refused,
            Err(ticket_repo::TicketWriteError::IneligibleAssignee(user)) if user == fx.ws.member_uuid
        ),
        "a requester can't be assigned: {refused:?}"
    );
    assert_eq!(fx.assignee_of(ticket.id), None);

    assert!(assign(fx.ws.admin_uuid).is_ok(), "an admin can");
    assert_eq!(fx.assignee_of(ticket.id), Some(fx.ws.admin_uuid));
}

#[actix_web::test]
async fn the_rules_assign_only_people_who_can_work_tickets() {
    let fx = Fixture::new();
    let team = fx.team();

    fx.rule(json!({ "method": "group", "group_id": team }));
    for title in ["VPN drops", "Switch port dead", "Laptop won't boot"] {
        assert_eq!(
            fx.routed(title).await,
            Some(fx.ws.admin_uuid),
            "a team: {title}"
        );
    }

    fx.rule(json!({ "method": "direct", "user_uuid": fx.ws.member_uuid }));
    assert_eq!(
        fx.routed("Badge reader").await,
        None,
        "a rule assigning a requester directly"
    );
}

#[actix_web::test]
async fn bulk_assign_to_someone_who_cant_work_tickets_is_refused() {
    let fx = Fixture::new();
    let one = fx.ticket("Printer jammed", None);
    let two = fx.ticket("Scanner offline", None);

    let (status, body) = fx
        .call(
            http_test::TestRequest::post()
                .uri("/api/tickets/bulk")
                .set_json(json!({
                    "action": "assign",
                    "ids": [one.id, two.id],
                    "value": fx.ws.member_uuid.to_string(),
                })),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["code"], "INVALID_ASSIGNEE");
    assert_eq!(fx.assignee_of(one.id), None);
    assert_eq!(fx.assignee_of(two.id), None);

    let (status, body) = fx
        .call(
            http_test::TestRequest::post()
                .uri("/api/tickets/bulk")
                .set_json(json!({
                    "action": "assign",
                    "ids": [one.id, two.id],
                    "value": fx.ws.admin_uuid.to_string(),
                })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["affected"], json!(2));
}

#[actix_web::test]
async fn the_rest_routes_refuse_an_assignee_who_cant_work_tickets() {
    let fx = Fixture::new();
    let open = fx.run(workflow_states::default_state).id;

    let (status, body) = fx
        .call(
            http_test::TestRequest::post()
                .uri("/api/tickets")
                .set_json(json!({
                    "title": "Printer jammed",
                    "workflow_state_id": open,
                    "priority": "medium",
                    "assignee_uuid": fx.ws.member_uuid,
                })),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "create: {body}");
    assert_eq!(body["code"], "INVALID_ASSIGNEE");

    let ticket = fx.ticket("Scanner offline", None);
    for (method, body) in [
        (
            http_test::TestRequest::patch(),
            json!({ "assignee": fx.ws.member_uuid.to_string() }),
        ),
        (
            http_test::TestRequest::put(),
            json!({
                "title": "Scanner offline",
                "workflow_state_id": open,
                "priority": "medium",
                "assignee_uuid": fx.ws.member_uuid,
            }),
        ),
    ] {
        let (status, resp) = fx
            .call(
                method
                    .uri(&format!("/api/tickets/{}", ticket.id))
                    .set_json(body),
            )
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "update: {resp}");
        assert_eq!(resp["code"], "INVALID_ASSIGNEE");
    }
    assert_eq!(fx.assignee_of(ticket.id), None);
}

/// Only a change of assignee is checked: a ticket whose assignee has since
/// become a requester can still be retitled and moved.
#[actix_web::test]
async fn a_ticket_whose_assignee_was_demoted_can_still_be_edited() {
    let fx = Fixture::new();
    let ticket = fx.ticket("Printer jammed", Some(fx.ws.member_uuid));
    let open = ticket.workflow_state_id;
    let done = fx
        .run(|c| workflow_states::first_in_category(c, WorkflowStateCategory::Done))
        .id;

    let (status, body) = fx
        .call(
            http_test::TestRequest::patch()
                .uri(&format!("/api/tickets/{}", ticket.id))
                .set_json(json!({ "title": "Printer still jammed", "workflow_state_id": done })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "PATCH: {body}");

    let (status, body) = fx
        .call(
            http_test::TestRequest::put()
                .uri(&format!("/api/tickets/{}", ticket.id))
                .set_json(json!({
                    "title": "Printer jammed again",
                    "workflow_state_id": open,
                    "priority": "medium",
                    "assignee_uuid": fx.ws.member_uuid,
                })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "PUT echoing the assignee: {body}");

    let after: Ticket = fx.run(|c| ticket_repo::get_ticket_by_id(c, ticket.id));
    assert_eq!(after.title, "Printer jammed again");
    assert_eq!(after.workflow_state_id, open);
    assert_eq!(after.assignee_uuid, Some(fx.ws.member_uuid));
}

/// A new occurrence is a future ticket, not history: it doesn't carry an
/// assignee who can no longer work tickets.
#[test]
fn a_recurring_ticket_whose_assignee_was_demoted_comes_back_unassigned() {
    let fx = Fixture::new();
    // Assigned while they could work tickets; they're a requester now.
    let ticket = fx.ticket("Check the backups", Some(fx.ws.member_uuid));
    let done = fx
        .run(|c| workflow_states::first_in_category(c, WorkflowStateCategory::Done))
        .id;
    fx.run(|c| {
        diesel::update(tickets::table.find(ticket.id))
            .set((
                tickets::recurrence_rule.eq("FREQ=WEEKLY"),
                tickets::due_date.eq((ticket.created_at + chrono::Duration::days(1))
                    .date()
                    .and_time(chrono::NaiveTime::MIN)),
            ))
            .execute(c)
    });
    // Closing it is what makes the next occurrence.
    let mut conn = fx.pool.get().expect("conn");
    with_actor_context(&mut conn, &fx.admin(), |c| {
        ticket_repo::update_ticket_partial(
            c,
            ticket.id,
            TicketUpdate {
                workflow_state_id: Some(done),
                ..Default::default()
            },
            None,
        )
    })
    .expect("close");
    drop(conn);

    let next: Vec<Option<Uuid>> = fx.run(|c| {
        tickets::table
            .filter(tickets::recurrence_template_id.eq(ticket.id))
            .select(tickets::assignee_uuid)
            .load(c)
    });
    assert_eq!(next, [None], "the next occurrence starts unassigned");
}

/// An approval that times out still approves when the rules would hand the
/// request to someone who can't work it; the request just isn't assigned to
/// them. One they would hand to someone who can is assigned once the rules
/// engine runs: the approval releases a held request to its rules.
#[actix_web::test]
async fn an_approval_that_times_out_approves_without_an_ineligible_assignee() {
    use backend::schema::ticket_approvals;

    let fx = Fixture::new();
    fx.run(|c| {
        backend::repository::site_settings::get_site_settings(c)?;
        diesel::sql_query(
            "UPDATE site_settings SET approval_auto_approve_days = 1, \
             approval_waiting_display = 'held'",
        )
        .execute(c)
    });
    // A request waiting two days for the admin's approval, made the way the
    // app makes one, so its rules are turned away while it waits.
    let waiting = |title: &str| {
        let ticket = fx.run(|c| {
            let open = workflow_states::default_state(c)?.id;
            let ticket = ticket_repo::create_ticket(
                c,
                NewTicket {
                    title: title.to_string(),
                    workflow_state_id: open,
                    requester_uuid: Some(fx.ws.member_uuid),
                    ..Default::default()
                },
            )?;
            diesel::update(tickets::table.find(ticket.id))
                .set((
                    tickets::approval_state.eq("pending"),
                    tickets::rules_held_at.eq(Some(chrono::Utc::now().naive_utc())),
                ))
                .execute(c)?;
            diesel::insert_into(ticket_approvals::table)
                .values((
                    ticket_approvals::ticket_id.eq(ticket.id),
                    ticket_approvals::approver_uuid.eq(fx.ws.admin_uuid),
                    ticket_approvals::round.eq(1),
                    ticket_approvals::created_at.eq(chrono::Utc::now() - chrono::Duration::days(2)),
                ))
                .execute(c)?;
            Ok(ticket)
        });
        ticket.id
    };
    let approved = |id: i32| {
        let (state, assignee): (Option<String>, Option<Uuid>) = fx.run(|c| {
            tickets::table
                .find(id)
                .select((tickets::approval_state, tickets::assignee_uuid))
                .first(c)
        });
        assert_eq!(state.as_deref(), Some("approved"));
        assignee
    };

    fx.rule(json!({ "method": "direct", "user_uuid": fx.ws.member_uuid }));
    let laptop = waiting("New laptop");
    fx.dispatch().await;
    backend::services::scheduled_jobs::approval_timeouts(fx.pool.clone())
        .await
        .expect("run the job");
    fx.dispatch().await;
    assert_eq!(approved(laptop), None);

    fx.rule(json!({ "method": "direct", "user_uuid": fx.ws.admin_uuid }));
    let monitor = waiting("New monitor");
    fx.dispatch().await;
    assert_eq!(fx.assignee_of(monitor), None, "held while it waits");
    backend::services::scheduled_jobs::approval_timeouts(fx.pool.clone())
        .await
        .expect("run the job");
    fx.dispatch().await;
    assert_eq!(approved(monitor), Some(fx.ws.admin_uuid));
}
