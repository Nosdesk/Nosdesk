//! The rules admin works where the app runs as `nosdesk_app`. Each handler
//! runs its queries in the request's workspace, so row security shows and
//! accepts that workspace's rules and nothing else. Driven through a pool
//! shaped like production's, where a pooled connection starts with no
//! workspace.

use actix_web::dev::{Service, ServiceResponse};
use actix_web::http::StatusCode;
use actix_web::test as http_test;
use actix_web::{web, App, HttpMessage};
use serde_json::{json, Value};
use uuid::Uuid;

use backend::extractors::WorkspaceContext;
use backend::middleware::RequestContext;
use backend::models::Claims;
use backend::sync::actor::ActorContext;

use crate::common::{self, TestPool, WorkspaceSeed};

/// Call the rules routes as `ws`'s admin.
async fn as_admin(
    pool: &TestPool,
    ws: &WorkspaceSeed,
    req: http_test::TestRequest,
) -> ServiceResponse {
    let now = chrono::Utc::now().timestamp();
    let claims = Claims {
        sub: ws.admin_uuid.to_string(),
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
        workspace_id: ws.workspace_id,
        workspace_uuid: ws.workspace_uuid,
        slug: ws.slug.clone(),
        name: ws.slug.clone(),
        organisation_id: None,
        custom_domain: None,
    };
    let corr = Uuid::now_v7();
    let actor = ActorContext::user(ws.admin_uuid, Some(corr)).with_workspace(ws.workspace_id);
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
            .service(web::scope("/api").configure(backend::handlers::rules::config)),
    )
    .await;
    http_test::call_service(&app, req.to_request()).await
}

#[actix_web::test]
async fn an_admin_saves_and_manages_rules_in_their_workspace() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let seeded = common::seed_two_workspaces(&mut db.pool_with_size(2).get().expect("conn"));
    let pool = db.runtime_pool(4);
    let (a, b) = (&seeded.a, &seeded.b);

    let created = as_admin(
        &pool,
        a,
        http_test::TestRequest::post()
            .uri("/api/rules")
            .set_json(json!({
                "name": "Escalate printer outages",
                "trigger_kind": "manual",
                "actions": [{ "kind": "set_priority", "config": { "priority": "high" } }],
            })),
    )
    .await;
    assert_eq!(created.status(), StatusCode::CREATED);
    let rule: Value = http_test::read_body_json(created).await;
    let id = rule["id"].as_i64().expect("rule id");

    let listed: Value = http_test::read_body_json(
        as_admin(&pool, a, http_test::TestRequest::get().uri("/api/rules")).await,
    )
    .await;
    let ids: Vec<i64> = listed
        .as_array()
        .expect("rule list")
        .iter()
        .filter_map(|r| r["id"].as_i64())
        .collect();
    assert_eq!(ids, vec![id]);

    let renamed = as_admin(
        &pool,
        a,
        http_test::TestRequest::put()
            .uri(&format!("/api/rules/{id}"))
            .set_json(json!({ "name": "Escalate outages" })),
    )
    .await;
    assert_eq!(renamed.status(), StatusCode::OK);

    let versions: Value = http_test::read_body_json(
        as_admin(
            &pool,
            a,
            http_test::TestRequest::get().uri(&format!("/api/rules/{id}/versions")),
        )
        .await,
    )
    .await;
    assert_eq!(
        versions.as_array().map(Vec::len),
        Some(2),
        "created, then renamed"
    );

    // Another workspace's admin doesn't see it.
    let elsewhere = as_admin(
        &pool,
        b,
        http_test::TestRequest::get().uri(&format!("/api/rules/{id}")),
    )
    .await;
    assert_eq!(elsewhere.status(), StatusCode::NOT_FOUND);

    let archived = as_admin(
        &pool,
        a,
        http_test::TestRequest::delete().uri(&format!("/api/rules/{id}")),
    )
    .await;
    assert_eq!(archived.status(), StatusCode::OK);
}

/// A team of `members` (the admin and the member, a requester who can't work
/// tickets, in the first test), where the admin already has an open ticket,
/// plus an unassigned ticket to apply a rule to.
fn seed_team(
    conn: &mut backend::db::DbConnection,
    ws: &WorkspaceSeed,
    members: &[Uuid],
) -> (i32, i32) {
    use backend::models::NewTicket;
    use backend::schema::{groups, tickets, user_groups, workflow_states};
    use diesel::prelude::*;

    let actor = ActorContext::user(ws.admin_uuid, None).with_workspace(ws.workspace_id);
    backend::sync::session::with_actor_context(conn, &actor, |c| {
        let open_state: i32 = workflow_states::table
            .filter(workflow_states::workspace_id.eq(ws.workspace_id))
            .filter(workflow_states::is_default.eq(true))
            .select(workflow_states::id)
            .first(c)?;
        let team: i32 = diesel::insert_into(groups::table)
            .values(groups::name.eq("Network"))
            .returning(groups::id)
            .get_result(c)?;
        for &user in members {
            diesel::insert_into(user_groups::table)
                .values((
                    user_groups::group_id.eq(team),
                    user_groups::user_uuid.eq(user),
                ))
                .execute(c)?;
        }
        let mut ticket = |title: &str, assignee: Option<Uuid>| {
            diesel::insert_into(tickets::table)
                .values(&NewTicket {
                    title: title.to_string(),
                    workflow_state_id: open_state,
                    assignee_uuid: assignee,
                    ..Default::default()
                })
                .returning(tickets::id)
                .get_result::<i32>(c)
        };
        ticket("VPN drops hourly", Some(ws.admin_uuid))?;
        let target = ticket("Switch port dead", None)?;
        Ok::<_, diesel::result::Error>((team, target))
    })
    .expect("seed team")
}

#[actix_web::test]
async fn a_team_step_assigns_whoever_can_work_tickets_and_has_the_fewest_open() {
    use backend::schema::tickets;
    use diesel::prelude::*;

    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let seeded = common::seed_two_workspaces(&mut db.pool_with_size(2).get().expect("conn"));
    let a = &seeded.a;
    let (team, target) = seed_team(
        &mut db.pool_with_size(1).get().expect("conn"),
        a,
        &[a.admin_uuid, a.member_uuid],
    );
    let pool = db.runtime_pool(4);

    // A step without its team is refused when the rule is saved.
    let incomplete = as_admin(
        &pool,
        a,
        http_test::TestRequest::post()
            .uri("/api/rules")
            .set_json(json!({
                "name": "Hand to network",
                "trigger_kind": "manual",
                "actions": [{ "kind": "assign", "config": { "method": "group" } }],
            })),
    )
    .await;
    assert_eq!(incomplete.status(), StatusCode::BAD_REQUEST);

    let created = as_admin(
        &pool,
        a,
        http_test::TestRequest::post()
            .uri("/api/rules")
            .set_json(json!({
                "name": "Hand to network",
                "trigger_kind": "manual",
                "actions": [
                    { "kind": "set_priority", "config": { "priority": "high" } },
                    { "kind": "assign", "config": { "method": "group", "group_id": team } },
                ],
            })),
    )
    .await;
    assert_eq!(created.status(), StatusCode::CREATED);
    let id = http_test::read_body_json::<Value, _>(created).await["id"]
        .as_i64()
        .expect("rule id");

    let live = as_admin(
        &pool,
        a,
        http_test::TestRequest::patch()
            .uri(&format!("/api/rules/{id}/state"))
            .set_json(json!({ "state": "live" })),
    )
    .await;
    assert_eq!(live.status(), StatusCode::OK);

    let applied = as_admin(
        &pool,
        a,
        http_test::TestRequest::post()
            .uri(&format!("/api/rules/{id}/apply"))
            .set_json(json!({ "ticket_id": target })),
    )
    .await;
    assert_eq!(applied.status(), StatusCode::OK);

    // The member has no open tickets but can't work them, so the admin gets
    // this one too.
    let mut conn = db.pool_with_size(1).get().expect("conn");
    let assignee: Option<Uuid> = tickets::table
        .find(target)
        .select(tickets::assignee_uuid)
        .first(&mut conn)
        .expect("ticket");
    assert_eq!(assignee, Some(a.admin_uuid));

    // Each change reached the sync stream the way a manual edit does, so
    // clients update and the new assignee can be notified.
    use backend::schema::sync_actions;
    let events: Vec<(String, Value)> = sync_actions::table
        .filter(sync_actions::aggregate_id.eq(target.to_string()))
        .select((sync_actions::event_type, sync_actions::data))
        .load(&mut conn)
        .expect("sync actions");
    assert!(
        events
            .iter()
            .any(|(kind, data)| kind == "ticket.priority_changed" && data["priority"] == "high"),
        "priority change emitted: {events:?}"
    );
    assert!(
        events
            .iter()
            .any(|(kind, data)| kind == "ticket.assignee_changed"
                && data["assignee_uuid"] == json!(a.admin_uuid)
                && data.get("previous_assignee_uuid").is_some()),
        "assignment emitted with the previous assignee: {events:?}"
    );
}

/// A team step on a team with no one who can work tickets is the rule's
/// mistake, not a server fault: 400, and the ticket stays unassigned.
#[actix_web::test]
async fn a_team_step_with_no_one_who_can_work_tickets_is_refused() {
    use backend::schema::tickets;
    use diesel::prelude::*;

    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let seeded = common::seed_two_workspaces(&mut db.pool_with_size(2).get().expect("conn"));
    let a = &seeded.a;
    let (team, target) = seed_team(
        &mut db.pool_with_size(1).get().expect("conn"),
        a,
        &[a.member_uuid],
    );
    let pool = db.runtime_pool(4);

    let created = as_admin(
        &pool,
        a,
        http_test::TestRequest::post()
            .uri("/api/rules")
            .set_json(json!({
                "name": "Hand to the front desk",
                "trigger_kind": "manual",
                "actions": [
                    { "kind": "assign", "config": { "method": "group", "group_id": team } },
                ],
            })),
    )
    .await;
    assert_eq!(created.status(), StatusCode::CREATED);
    let id = http_test::read_body_json::<Value, _>(created).await["id"]
        .as_i64()
        .expect("rule id");
    let live = as_admin(
        &pool,
        a,
        http_test::TestRequest::patch()
            .uri(&format!("/api/rules/{id}/state"))
            .set_json(json!({ "state": "live" })),
    )
    .await;
    assert_eq!(live.status(), StatusCode::OK);

    let applied = as_admin(
        &pool,
        a,
        http_test::TestRequest::post()
            .uri(&format!("/api/rules/{id}/apply"))
            .set_json(json!({ "ticket_id": target })),
    )
    .await;
    assert_eq!(applied.status(), StatusCode::BAD_REQUEST);
    let body = http_test::read_body_json::<Value, _>(applied).await;
    assert_eq!(body["code"], "INVALID_ASSIGNEE", "{body}");

    let mut conn = db.pool_with_size(1).get().expect("conn");
    let assignee: Option<Uuid> = tickets::table
        .find(target)
        .select(tickets::assignee_uuid)
        .first(&mut conn)
        .expect("ticket");
    assert_eq!(assignee, None);
}

/// An archived rule stays archived: it can't go live, be paused or be
/// edited, whether it was archived from the list (which stamps only
/// `archived_at`) or by moving its state to archived.
#[actix_web::test]
async fn an_archived_rule_cannot_go_live() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let seeded = common::seed_two_workspaces(&mut db.pool_with_size(2).get().expect("conn"));
    let pool = db.runtime_pool(4);
    let a = &seeded.a;

    let create = |name: &'static str| {
        let pool = pool.clone();
        async move {
            let created = as_admin(
                &pool,
                a,
                http_test::TestRequest::post()
                    .uri("/api/rules")
                    .set_json(json!({
                        "name": name,
                        "trigger_kind": "manual",
                        "actions": [{ "kind": "set_priority", "config": { "priority": "high" } }],
                    })),
            )
            .await;
            assert_eq!(created.status(), StatusCode::CREATED);
            http_test::read_body_json::<Value, _>(created).await["id"]
                .as_i64()
                .expect("rule id")
        }
    };

    // Archived from the list: DELETE without `hard`.
    let deleted = create("Archived from the list").await;
    let archived = as_admin(
        &pool,
        a,
        http_test::TestRequest::delete().uri(&format!("/api/rules/{deleted}")),
    )
    .await;
    assert_eq!(archived.status(), StatusCode::OK);
    let first: Value = http_test::read_body_json(archived).await;

    // Deleting it again from the list changes nothing: same archived_at, no
    // new version.
    let versions = |id: i64| {
        let pool = pool.clone();
        async move {
            let listed: Value = http_test::read_body_json(
                as_admin(
                    &pool,
                    a,
                    http_test::TestRequest::get().uri(&format!("/api/rules/{id}/versions")),
                )
                .await,
            )
            .await;
            listed.as_array().map(Vec::len).expect("versions")
        }
    };
    let before = versions(deleted).await;
    let again = as_admin(
        &pool,
        a,
        http_test::TestRequest::delete().uri(&format!("/api/rules/{deleted}")),
    )
    .await;
    assert_eq!(again.status(), StatusCode::OK);
    let second: Value = http_test::read_body_json(again).await;
    assert_eq!(second["archived_at"], first["archived_at"], "{second}");
    assert_eq!(versions(deleted).await, before, "no new version");

    // Archived by its state.
    let moved = create("Archived by state").await;
    let archived = as_admin(
        &pool,
        a,
        http_test::TestRequest::patch()
            .uri(&format!("/api/rules/{moved}/state"))
            .set_json(json!({ "state": "archived" })),
    )
    .await;
    assert_eq!(archived.status(), StatusCode::OK);

    for id in [deleted, moved] {
        for state in ["live", "dry_run", "draft"] {
            let resp = as_admin(
                &pool,
                a,
                http_test::TestRequest::patch()
                    .uri(&format!("/api/rules/{id}/state"))
                    .set_json(json!({ "state": state })),
            )
            .await;
            assert_eq!(resp.status(), StatusCode::CONFLICT, "rule {id} to {state}");
            let body: Value = http_test::read_body_json(resp).await;
            assert_eq!(
                body["code"], "RULE_ARCHIVED",
                "rule {id} to {state}: {body}"
            );
        }

        let edited = as_admin(
            &pool,
            a,
            http_test::TestRequest::put()
                .uri(&format!("/api/rules/{id}"))
                .set_json(json!({ "name": "Back from the dead" })),
        )
        .await;
        assert_eq!(edited.status(), StatusCode::CONFLICT, "rule {id} edit");
        let body: Value = http_test::read_body_json(edited).await;
        assert_eq!(body["code"], "RULE_ARCHIVED", "rule {id} edit: {body}");

        // Archiving it again changes nothing.
        let again = as_admin(
            &pool,
            a,
            http_test::TestRequest::patch()
                .uri(&format!("/api/rules/{id}/state"))
                .set_json(json!({ "state": "archived" })),
        )
        .await;
        assert_eq!(again.status(), StatusCode::OK, "rule {id} archived again");

        let rule: Value = http_test::read_body_json(
            as_admin(
                &pool,
                a,
                http_test::TestRequest::get().uri(&format!("/api/rules/{id}")),
            )
            .await,
        )
        .await;
        assert!(
            rule["archived_at"].is_string(),
            "rule {id} still archived: {rule}"
        );
        assert_ne!(rule["state"], "live", "rule {id}: {rule}");
        assert_ne!(rule["name"], "Back from the dead", "rule {id}: {rule}");
    }
}
