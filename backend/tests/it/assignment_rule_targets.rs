//! An assignment rule names one of its own workspace's agents or admins.
//! Accounts are shared across workspaces, so a uuid alone could name anyone
//! in the deployment: create and update refuse anyone else with one answer
//! (a stranger reads as a uuid no account has), a rule stored before the
//! check never assigns outside the workspace, and the rule list doesn't show
//! a stranger's name.

use actix_web::dev::Service;
use actix_web::http::StatusCode;
use actix_web::test as http_test;
use actix_web::{web, App, HttpMessage};
use diesel::prelude::*;
use serde_json::{json, Value};
use uuid::Uuid;

use backend::extractors::WorkspaceContext;
use backend::middleware::RequestContext;
use backend::models::{AssignmentMethod, Claims, NewAssignmentRule, NewTicket, Ticket};
use backend::repository::workflow_states;
use backend::schema::{assignment_rules, groups, tickets, users};
use backend::services::ticket_updates::{assign_new_ticket, ActorConn};
use backend::sync::actor::ActorContext;
use backend::sync::session::with_actor_context;

use crate::common::{self, TestPool, TwoWorkspaces};

struct Fixture {
    _db: common::TestDb,
    pool: TestPool,
    ws: TwoWorkspaces,
    /// A platform admin with no seat anywhere.
    operator: Uuid,
}

impl Fixture {
    fn new() -> Self {
        common::ensure_test_keyring();
        let db = common::TestDb::new();
        let pool = db.pool_with_size(4);
        let mut conn = pool.get().expect("conn");
        let ws = common::seed_two_workspaces(&mut conn);
        let operator = common::insert_user(&mut conn, "Seatless Operator").uuid;
        drop(conn);
        Self {
            _db: db,
            pool,
            ws,
            operator,
        }
    }

    fn admin(&self) -> ActorContext {
        ActorContext::user(self.ws.a.admin_uuid, None).with_workspace(self.ws.a.workspace_id)
    }

    /// Run `f` in workspace A as its admin.
    fn run<T>(
        &self,
        f: impl FnOnce(&mut backend::db::DbConnection) -> diesel::QueryResult<T>,
    ) -> T {
        let mut conn = self.pool.get().expect("conn");
        with_actor_context(&mut conn, &self.admin(), f).expect("run in workspace")
    }

    /// Accounts that aren't A's staff: B's admin, a seatless platform admin,
    /// A's requester, and a uuid no account has.
    fn strangers(&self) -> [(&'static str, Uuid); 4] {
        [
            ("another workspace's admin", self.ws.b.admin_uuid),
            ("a seatless platform admin", self.operator),
            ("a requester here", self.ws.a.member_uuid),
            ("no account", Uuid::new_v4()),
        ]
    }

    fn name_of(&self, user: Uuid) -> String {
        self.run(|c| users::table.find(user).select(users::name).first(c))
    }

    /// A rule written straight to the table, as one stored before targets
    /// were checked.
    fn stored_rule(&self, name: &str, method: AssignmentMethod, user: Uuid) -> i32 {
        let group = (method != AssignmentMethod::DirectUser).then(|| self.group());
        self.run(|c| {
            diesel::update(assignment_rules::table)
                .set(assignment_rules::is_active.eq(false))
                .execute(c)?;
            diesel::insert_into(assignment_rules::table)
                .values(&NewAssignmentRule {
                    name: name.to_string(),
                    description: None,
                    priority: 1,
                    is_active: true,
                    method,
                    target_user_uuid: Some(user),
                    target_group_id: group,
                    trigger_on_create: true,
                    trigger_on_category_change: false,
                    category_id: None,
                    conditions: None,
                    created_by: Some(self.ws.a.admin_uuid),
                })
                .returning(assignment_rules::id)
                .get_result(c)
        })
    }

    fn group(&self) -> i32 {
        self.run(|c| {
            diesel::insert_into(groups::table)
                .values(groups::name.eq(format!("Team {}", Uuid::new_v4().simple())))
                .returning(groups::id)
                .get_result(c)
        })
    }

    fn target_of(&self, rule: i32) -> Option<Uuid> {
        self.run(|c| {
            assignment_rules::table
                .find(rule)
                .select(assignment_rules::target_user_uuid)
                .first(c)
        })
    }

    /// A new ticket in A, through the assignment rules as every create path
    /// runs them.
    fn routed(&self, title: &str) -> Option<Uuid> {
        let ticket: Ticket = self.run(|c| {
            let open = workflow_states::default_state(c)?.id;
            diesel::insert_into(tickets::table)
                .values(&NewTicket {
                    title: title.to_string(),
                    workflow_state_id: open,
                    requester_uuid: Some(self.ws.a.member_uuid),
                    ..Default::default()
                })
                .get_result(c)
        });
        let mut conn = self.pool.get().expect("conn");
        let system =
            ActorContext::system("assignment_rules").with_workspace(self.ws.a.workspace_id);
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

    /// Call the assignment rule routes as A's admin.
    async fn call(&self, req: http_test::TestRequest) -> (StatusCode, Value) {
        let now = chrono::Utc::now().timestamp();
        let admin = self.ws.a.admin_uuid;
        let claims = Claims {
            sub: admin.to_string(),
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
            workspace_id: self.ws.a.workspace_id,
            workspace_uuid: self.ws.a.workspace_uuid,
            slug: self.ws.a.slug.clone(),
            name: "A".to_string(),
            organisation_id: None,
            custom_domain: None,
        };
        let corr = Uuid::now_v7();
        let actor = ActorContext::user(admin, Some(corr)).with_workspace(self.ws.a.workspace_id);
        let app = http_test::init_service(
            App::new()
                .app_data(web::Data::new(self.pool.clone()))
                .wrap_fn(move |req, srv| {
                    req.extensions_mut().insert(workspace.clone());
                    req.extensions_mut().insert(claims.clone());
                    req.extensions_mut()
                        .insert(RequestContext::new(corr, actor.clone()));
                    srv.call(req)
                })
                .service(web::scope("/api").configure(backend::handlers::assignment_rules::config)),
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

    async fn create(&self, body: Value) -> (StatusCode, Value) {
        self.call(
            http_test::TestRequest::post()
                .uri("/api/admin/assignment-rules")
                .set_json(body),
        )
        .await
    }

    async fn update(&self, rule: i32, body: Value) -> (StatusCode, Value) {
        self.call(
            http_test::TestRequest::patch()
                .uri(&format!("/api/admin/assignment-rules/{rule}"))
                .set_json(body),
        )
        .await
    }
}

#[actix_web::test]
async fn creating_a_rule_takes_only_this_workspaces_staff() {
    let fx = Fixture::new();
    let group = fx.group();

    let mut refusals = Vec::new();
    for (i, (who, user)) in fx.strangers().into_iter().enumerate() {
        let (status, body) = fx
            .create(json!({
                "name": format!("Direct {i}"),
                "method": "direct_user",
                "target_user_uuid": user,
            }))
            .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{who}: {body}");
        refusals.push(body);

        // A group rule carrying a user is refused too: the list would show it.
        let (status, _) = fx
            .create(json!({
                "name": format!("Group {i}"),
                "method": "group_queue",
                "target_group_id": group,
                "target_user_uuid": user,
            }))
            .await;
        assert_eq!(
            status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{who} on a group rule"
        );
    }
    assert!(
        refusals.windows(2).all(|w| w[0] == w[1]),
        "one answer for every stranger: {refusals:?}"
    );
    let stored: i64 = fx.run(|c| {
        assignment_rules::table
            .filter(
                assignment_rules::name
                    .like("Direct %")
                    .or(assignment_rules::name.like("Group %")),
            )
            .count()
            .get_result(c)
    });
    assert_eq!(stored, 0, "nothing stored");

    let (status, body) = fx
        .create(json!({
            "name": "To the admin",
            "method": "direct_user",
            "target_user_uuid": fx.ws.a.admin_uuid,
        }))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["target_user"]["uuid"], json!(fx.ws.a.admin_uuid));
}

#[actix_web::test]
async fn updating_a_rule_takes_only_this_workspaces_staff() {
    let fx = Fixture::new();
    let (status, body) = fx
        .create(json!({
            "name": "To the admin",
            "method": "direct_user",
            "target_user_uuid": fx.ws.a.admin_uuid,
        }))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let rule = body["id"].as_i64().expect("id") as i32;

    for (who, user) in fx.strangers() {
        let (status, body) = fx.update(rule, json!({ "target_user_uuid": user })).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{who}: {body}");
        assert_eq!(fx.target_of(rule), Some(fx.ws.a.admin_uuid), "{who}");
    }

    // A rule stored before the check can still be renamed, turned off and
    // pointed at someone who counts.
    let stored = fx.stored_rule("Old rule", AssignmentMethod::DirectUser, fx.ws.b.admin_uuid);
    for change in [
        json!({ "name": "Old rule, renamed" }),
        json!({ "is_active": false }),
    ] {
        let (status, body) = fx.update(stored, change.clone()).await;
        assert_eq!(status, StatusCode::OK, "{change}: {body}");
    }
    let (status, body) = fx
        .update(stored, json!({ "target_user_uuid": fx.ws.a.admin_uuid }))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(fx.target_of(stored), Some(fx.ws.a.admin_uuid));

    // A group rule whose stored user is a stranger doesn't become a direct
    // rule to them.
    let grouped = fx.stored_rule("Old group rule", AssignmentMethod::GroupQueue, fx.operator);
    let (status, body) = fx.update(grouped, json!({ "method": "direct_user" })).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
}

#[actix_web::test]
async fn the_rule_list_shows_no_strangers_name() {
    let fx = Fixture::new();
    let foreign = fx.stored_rule(
        "To B's admin",
        AssignmentMethod::DirectUser,
        fx.ws.b.admin_uuid,
    );
    let seatless = fx.stored_rule("To the operator", AssignmentMethod::DirectUser, fx.operator);
    let own = fx.stored_rule(
        "To A's admin",
        AssignmentMethod::DirectUser,
        fx.ws.a.admin_uuid,
    );
    // Rules that ran before the check made B's admin and the operator
    // assignees here.
    for assignee in [fx.ws.b.admin_uuid, fx.operator] {
        fx.run(|c| {
            let open = workflow_states::default_state(c)?.id;
            diesel::insert_into(tickets::table)
                .values(&NewTicket {
                    title: "Assigned before the check".to_string(),
                    workflow_state_id: open,
                    requester_uuid: Some(fx.ws.a.member_uuid),
                    assignee_uuid: Some(assignee),
                    ..Default::default()
                })
                .execute(c)
        });
    }

    let (status, body) = fx
        .call(http_test::TestRequest::get().uri("/api/admin/assignment-rules"))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let target = |id: i32| {
        body.as_array()
            .expect("rules")
            .iter()
            .find(|r| r["id"] == json!(id))
            .expect("listed")["target_user"]
            .clone()
    };
    assert_eq!(target(foreign), Value::Null);
    assert_eq!(target(seatless), Value::Null);
    assert_eq!(target(own)["uuid"], json!(fx.ws.a.admin_uuid));
    let text = body.to_string();
    for stranger in [fx.ws.b.admin_uuid, fx.operator] {
        let name = fx.name_of(stranger);
        assert!(!text.contains(&name), "{name} isn't listed");
    }

    let (status, body) = fx
        .call(http_test::TestRequest::get().uri(&format!("/api/admin/assignment-rules/{foreign}")))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["target_user"], Value::Null);
}

#[test]
fn a_rule_stored_before_the_check_assigns_no_stranger() {
    let fx = Fixture::new();
    // A uuid no account has can't be stored: the column is a foreign key.
    for (who, user) in fx.strangers().into_iter().take(3) {
        fx.stored_rule(who, AssignmentMethod::DirectUser, user);
        assert_eq!(fx.routed(&format!("Routed past {who}")), None, "{who}");
    }

    // A's staff still get the ticket, the seatless operator once seated as
    // an agent too.
    fx.stored_rule(
        "To A's admin",
        AssignmentMethod::DirectUser,
        fx.ws.a.admin_uuid,
    );
    assert_eq!(fx.routed("Printer jammed"), Some(fx.ws.a.admin_uuid));
    common::seat(
        &mut fx.pool.get().expect("conn"),
        fx.ws.a.workspace_id,
        fx.operator,
        "agent",
    );
    fx.stored_rule("To the operator", AssignmentMethod::DirectUser, fx.operator);
    assert_eq!(fx.routed("VPN drops"), Some(fx.operator));
}
