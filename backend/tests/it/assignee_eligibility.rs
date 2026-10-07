//! Only people who can work tickets are assigned them: a platform admin, or
//! someone whose role in the ticket's workspace is Agent or above. The ticket
//! write refuses anyone else, and everything that picks an assignee (the
//! assignment rules, bulk assign, a new occurrence of a recurring ticket, an
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
    AssignmentMethod, Claims, NewAssignmentRule, NewTicket, Ticket, TicketUpdate,
    WorkflowStateCategory,
};
use backend::repository::{tickets as ticket_repo, workflow_states};
use backend::schema::{assignment_rules, groups, tickets, user_groups};
use backend::services::search::SearchService;
use backend::services::ticket_updates::{after_update, assign_new_ticket, ActorConn};
use backend::sync::actor::ActorContext;
use backend::sync::session::with_actor_context;
use backend::utils::storage::{create_storage, Storage, StorageConfig};

use crate::common::{self, TestPool, WorkspaceSeed};

struct Fixture {
    _db: common::TestDb,
    _dir: tempfile::TempDir,
    _search_dir: tempfile::TempDir,
    pool: TestPool,
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
        Self {
            _db: db,
            _dir: dir,
            _search_dir: search_dir,
            pool,
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

    /// The only active assignment rule.
    fn rule(&self, method: AssignmentMethod, user: Option<Uuid>, group: Option<i32>) {
        self.run(|c| {
            diesel::update(assignment_rules::table)
                .set(assignment_rules::is_active.eq(false))
                .execute(c)?;
            diesel::insert_into(assignment_rules::table)
                .values(&NewAssignmentRule {
                    name: format!("{method:?}"),
                    description: None,
                    priority: 1,
                    is_active: true,
                    method,
                    target_user_uuid: user,
                    target_group_id: group,
                    trigger_on_create: true,
                    trigger_on_category_change: false,
                    category_id: None,
                    conditions: None,
                    created_by: Some(self.ws.admin_uuid),
                })
                .execute(c)
        });
    }

    /// A new ticket, through the assignment rules as every create path runs
    /// them.
    fn routed(&self, title: &str) -> Option<Uuid> {
        let ticket = self.ticket(title, None);
        let mut conn = self.pool.get().expect("conn");
        let system = ActorContext::system("assignment_rules").with_workspace(self.ws.workspace_id);
        assign_new_ticket(
            &mut ActorConn {
                conn: &mut conn,
                actor: &system,
            },
            None,
            ticket,
        )
        .assignee_uuid
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

#[test]
fn the_assignment_rules_pick_only_people_who_can_work_tickets() {
    let fx = Fixture::new();
    let team = fx.team();

    fx.rule(AssignmentMethod::GroupRoundRobin, None, Some(team));
    for title in ["VPN drops", "Switch port dead", "Laptop won't boot"] {
        assert_eq!(
            fx.routed(title),
            Some(fx.ws.admin_uuid),
            "round robin: {title}"
        );
    }

    fx.rule(AssignmentMethod::GroupRandom, None, Some(team));
    for title in [
        "Monitor flickers",
        "Keyboard sticks",
        "Mouse lost",
        "Dock dead",
    ] {
        assert_eq!(fx.routed(title), Some(fx.ws.admin_uuid), "random: {title}");
    }

    fx.rule(AssignmentMethod::DirectUser, Some(fx.ws.member_uuid), None);
    assert_eq!(
        fx.routed("Badge reader"),
        None,
        "a direct rule to a requester"
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
    let closed = fx.run(|c| {
        diesel::update(tickets::table.find(ticket.id))
            .set((
                tickets::recurrence_rule.eq("FREQ=WEEKLY"),
                tickets::due_date.eq(ticket.created_at + chrono::Duration::days(1)),
                tickets::workflow_state_id.eq(done),
            ))
            .get_result::<Ticket>(c)
    });

    let mut conn = fx.pool.get().expect("conn");
    let admin = fx.admin();
    after_update(
        &mut ActorConn {
            conn: &mut conn,
            actor: &admin,
        },
        None,
        &closed,
        false,
    );
    drop(conn);

    let next: Vec<Option<Uuid>> = fx.run(|c| {
        tickets::table
            .filter(tickets::recurrence_template_id.eq(ticket.id))
            .select(tickets::assignee_uuid)
            .load(c)
    });
    assert_eq!(next, [None], "the next occurrence starts unassigned");
}

/// An approval that times out still approves when the assignment rules would
/// hand the request to someone who can't work it; the request just isn't
/// assigned to them.
#[actix_web::test]
async fn an_approval_that_times_out_approves_without_an_ineligible_assignee() {
    use backend::schema::ticket_approvals;

    let fx = Fixture::new();
    fx.rule(AssignmentMethod::DirectUser, Some(fx.ws.member_uuid), None);
    let ticket = fx.ticket("New laptop", None);
    fx.run(|c| {
        backend::repository::site_settings::get_site_settings(c)?;
        diesel::sql_query(
            "UPDATE site_settings SET approval_auto_approve_days = 1, \
             approval_waiting_display = 'held'",
        )
        .execute(c)?;
        diesel::update(tickets::table.find(ticket.id))
            .set(tickets::approval_state.eq("pending"))
            .execute(c)?;
        diesel::insert_into(ticket_approvals::table)
            .values((
                ticket_approvals::ticket_id.eq(ticket.id),
                ticket_approvals::approver_uuid.eq(fx.ws.admin_uuid),
                ticket_approvals::round.eq(1),
                ticket_approvals::created_at.eq(chrono::Utc::now() - chrono::Duration::days(2)),
            ))
            .execute(c)
    });

    backend::services::scheduled_jobs::approval_timeouts(fx.pool.clone())
        .await
        .expect("run the job");

    let (state, assignee): (Option<String>, Option<Uuid>) = fx.run(|c| {
        tickets::table
            .find(ticket.id)
            .select((tickets::approval_state, tickets::assignee_uuid))
            .first(c)
    });
    assert_eq!(state.as_deref(), Some("approved"));
    assert_eq!(assignee, None);
}
