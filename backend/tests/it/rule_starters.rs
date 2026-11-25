//! The starter rules catalogue names statuses by category and tags by name,
//! and the server resolves them in the caller's workspace: the browse list
//! shows what each step will do there, and adding a starter saves a draft
//! whose steps carry that workspace's ids, creating any tag it names that
//! the workspace doesn't have. The auto-close starters are time rules: added,
//! they wait on nothing until an admin turns them on, then run on the
//! requester's silence. Driven through the runtime pool, where a pooled
//! connection starts with no workspace.

use actix_web::dev::{Service, ServiceResponse};
use actix_web::http::StatusCode;
use actix_web::test as http_test;
use actix_web::{web, App, HttpMessage};
use chrono::{DateTime, Utc};
use diesel::prelude::*;
use serde_json::{json, Value};
use uuid::Uuid;

use backend::extractors::WorkspaceContext;
use backend::middleware::RequestContext;
use backend::models::{
    Claims, NewChannel, NewChannelMessage, NewTicket, OutboundEmail, RuleApplicationStatus,
    WorkflowStateCategory,
};
use backend::repository::ticket_merge::{execute_merge, MergeInput};
use backend::schema::{
    comments, outbound_emails, rules, tags, ticket_elapsed_queue, workflow_states,
    workspace_members,
};
use backend::services::rules::elapsed;
use backend::sync::actor::ActorContext;

use crate::common::{self, TestPool, WorkspaceSeed};
use crate::rules_event_triggers::{admin, Harness};

/// Call the rules routes as `user` in `ws`, in `locale`.
async fn call(
    pool: &TestPool,
    ws: &WorkspaceSeed,
    user: Uuid,
    locale: &str,
    req: http_test::TestRequest,
) -> ServiceResponse {
    let now = chrono::Utc::now().timestamp();
    let claims = Claims {
        sub: user.to_string(),
        name: "Someone".to_string(),
        email: "someone@example.com".to_string(),
        platform_role: "user".to_string(),
        scope: "full".to_string(),
        sid: None,
        workspace_uuid: None,
        exp: (now + 3600) as usize,
        iat: now as usize,
        file_path: None,
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
    let actor = ActorContext::user(user, Some(corr)).with_workspace(ws.workspace_id);
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
    let req = req.insert_header(("Accept-Language", locale.to_string()));
    http_test::call_service(&app, req.to_request()).await
}

async fn catalogue(pool: &TestPool, ws: &WorkspaceSeed, locale: &str) -> Vec<Value> {
    let resp = call(
        pool,
        ws,
        ws.admin_uuid,
        locale,
        http_test::TestRequest::get().uri("/api/admin/rule-starters"),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = http_test::read_body_json(resp).await;
    body.as_array().expect("starters").clone()
}

async fn add(pool: &TestPool, ws: &WorkspaceSeed, locale: &str, id: &str) -> ServiceResponse {
    call(
        pool,
        ws,
        ws.admin_uuid,
        locale,
        http_test::TestRequest::post().uri(&format!("/api/admin/rule-starters/{id}")),
    )
    .await
}

async fn added(pool: &TestPool, ws: &WorkspaceSeed, locale: &str, id: &str) -> Value {
    let resp = add(pool, ws, locale, id).await;
    assert_eq!(resp.status(), StatusCode::CREATED, "add {id}");
    http_test::read_body_json(resp).await
}

fn starter<'a>(list: &'a [Value], id: &str) -> &'a Value {
    list.iter()
        .find(|s| s["id"] == id)
        .unwrap_or_else(|| panic!("starter {id} listed"))
}

/// The config of the first step of `kind`.
fn step<'a>(actions: &'a Value, kind: &str) -> &'a Value {
    &actions
        .as_array()
        .expect("actions")
        .iter()
        .find(|a| a["kind"] == kind)
        .unwrap_or_else(|| panic!("a {kind} step in {actions}"))["config"]
}

/// Run `f` on a privileged connection pinned to `ws`.
fn in_workspace<T>(
    pool: &TestPool,
    ws: &WorkspaceSeed,
    f: impl FnOnce(&mut backend::db::DbConnection) -> QueryResult<T>,
) -> T {
    let mut conn = pool.get().expect("conn");
    let actor = ActorContext::user(ws.admin_uuid, None).with_workspace(ws.workspace_id);
    backend::sync::session::with_actor_context(&mut conn, &actor, f).expect("seed")
}

fn create_tag(pool: &TestPool, ws: &WorkspaceSeed, name: &str) -> i32 {
    in_workspace(pool, ws, |c| {
        diesel::insert_into(tags::table)
            .values(tags::name.eq(name))
            .returning(tags::id)
            .get_result(c)
    })
}

/// Tags in `ws` whose name is `name` in any case.
fn tags_named(pool: &TestPool, ws: &WorkspaceSeed, name: &str) -> Vec<(i32, String)> {
    let mut conn = pool.get().expect("conn");
    tags::table
        .filter(tags::workspace_id.eq(ws.workspace_id))
        .select((tags::id, tags::name))
        .load::<(i32, String)>(&mut conn)
        .expect("tags")
        .into_iter()
        .filter(|(_, n)| n.to_lowercase() == name.to_lowercase())
        .collect()
}

fn first_state(pool: &TestPool, ws: &WorkspaceSeed, category: WorkflowStateCategory) -> i32 {
    let mut conn = pool.get().expect("conn");
    workflow_states::table
        .filter(workflow_states::workspace_id.eq(ws.workspace_id))
        .filter(workflow_states::category.eq(category))
        .filter(workflow_states::archived_at.is_null())
        .order(workflow_states::position)
        .select(workflow_states::id)
        .first(&mut conn)
        .expect("a state in the category")
}

/// Puts a new active state ahead of the seeded one, so "first by position"
/// is a real choice. Returns its id.
fn add_leading_active_state(pool: &TestPool, ws: &WorkspaceSeed) -> i32 {
    in_workspace(pool, ws, |c| {
        diesel::update(
            workflow_states::table
                .filter(workflow_states::workspace_id.eq(ws.workspace_id))
                .filter(workflow_states::category.eq(WorkflowStateCategory::Active)),
        )
        .set(workflow_states::position.eq(workflow_states::position + 5))
        .execute(c)?;
        diesel::insert_into(workflow_states::table)
            .values((
                workflow_states::name.eq("Picked up"),
                workflow_states::category.eq(WorkflowStateCategory::Active),
                workflow_states::color.eq("blue"),
                workflow_states::position.eq(1),
                workflow_states::pauses_sla.eq(false),
            ))
            .returning(workflow_states::id)
            .get_result(c)
    })
}

#[actix_web::test]
async fn the_catalogue_resolves_statuses_and_tags_in_the_workspace() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let seeded = common::seed_two_workspaces(&mut db.pool_with_size(2).get().expect("conn"));
    let setup = db.pool_with_size(1);
    let pool = db.runtime_pool(4);
    let a = &seeded.a;

    let picked_up = add_leading_active_state(&setup, a);
    let duplicate = create_tag(&setup, a, "Duplicate");
    let done = first_state(&setup, a, WorkflowStateCategory::Done);

    let list = catalogue(&pool, a, "en-US").await;
    assert_eq!(list.len(), 8);

    let ack = starter(&list, "ack-and-acknowledge");
    assert_eq!(ack["name"], "Acknowledge and triage");
    assert_eq!(
        step(&ack["actions"], "set_status"),
        &json!({ "workflow_state_id": picked_up }),
        "the first active state by position"
    );
    assert_eq!(
        step(
            &starter(&list, "mark-resolved-with-summary")["actions"],
            "set_status"
        ),
        &json!({ "workflow_state_id": done })
    );
    // A tag the workspace doesn't have: no id, but named.
    assert_eq!(
        step(
            &starter(&list, "escalate-to-on-call")["actions"],
            "add_tags"
        ),
        &json!({ "tag_ids": [], "tag_names": ["escalated"] })
    );
    // One it has, whatever its case.
    assert_eq!(
        step(&starter(&list, "tag-and-note")["actions"], "add_tags"),
        &json!({ "tag_ids": [duplicate], "tag_names": ["duplicate"] })
    );

    // In French the names, and so the tags, are French.
    let fr = catalogue(&pool, a, "fr-FR").await;
    let dup_fr = starter(&fr, "tag-and-note");
    assert_eq!(dup_fr["name"], "Étiqueter comme doublon");
    assert_eq!(
        step(&dup_fr["actions"], "add_tags"),
        &json!({ "tag_ids": [], "tag_names": ["doublon"] })
    );
}

#[actix_web::test]
async fn adding_a_starter_saves_a_draft_with_its_steps_resolved() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let seeded = common::seed_two_workspaces(&mut db.pool_with_size(2).get().expect("conn"));
    let setup = db.pool_with_size(1);
    let pool = db.runtime_pool(4);
    let (a, b) = (&seeded.a, &seeded.b);

    let picked_up = add_leading_active_state(&setup, a);

    // Every starter adds cleanly to a workspace with the default states.
    let mut escalate_tag = None;
    for id in [
        "ack-and-acknowledge",
        "request-more-info",
        "mark-resolved-with-summary",
        "escalate-to-on-call",
        "set-high-priority",
        "tag-and-note",
    ] {
        let rule = added(&pool, a, "en-US", id).await;
        assert_eq!(rule["state"], "draft", "{id}: {rule}");
        assert_eq!(rule["trigger_kind"], "manual", "{id}: {rule}");
        assert_eq!(rule["workspace_id"], a.workspace_id, "{id}: {rule}");
        match id {
            "ack-and-acknowledge" => {
                assert_eq!(rule["name"], "Acknowledge and triage");
                assert_eq!(
                    rule["description"],
                    "For a new ticket, so the requester knows someone has it."
                );
                let kinds: Vec<&str> = rule["actions"]
                    .as_array()
                    .expect("actions")
                    .iter()
                    .filter_map(|a| a["kind"].as_str())
                    .collect();
                assert_eq!(kinds, ["reply", "set_status"]);
                assert_eq!(
                    step(&rule["actions"], "set_status"),
                    &json!({ "workflow_state_id": picked_up })
                );
            }
            "escalate-to-on-call" => {
                let created = tags_named(&setup, a, "escalated");
                assert_eq!(created.len(), 1, "the missing tag is created: {created:?}");
                assert_eq!(created[0].1, "escalated");
                assert_eq!(
                    step(&rule["actions"], "add_tags"),
                    &json!({ "tag_ids": [created[0].0] })
                );
                escalate_tag = Some(created[0].0);
            }
            _ => {}
        }
    }

    // Adding it again reuses the tag it created.
    let again = added(&pool, a, "en-US", "escalate-to-on-call").await;
    assert_eq!(
        step(&again["actions"], "add_tags"),
        &json!({ "tag_ids": [escalate_tag.expect("tag")] })
    );
    assert_eq!(tags_named(&setup, a, "escalated").len(), 1);

    // Another workspace with the tag already, in another case: reused, not
    // duplicated, and the first workspace's tag isn't touched.
    let existing = create_tag(&setup, b, "Escalated");
    let in_b = added(&pool, b, "en-US", "escalate-to-on-call").await;
    assert_eq!(
        step(&in_b["actions"], "add_tags"),
        &json!({ "tag_ids": [existing] })
    );
    assert_eq!(
        tags_named(&setup, b, "escalated"),
        vec![(existing, "Escalated".to_string())]
    );

    // In French: French name and description, and the French tag.
    let fr = added(&pool, b, "fr-FR", "tag-and-note").await;
    assert_eq!(fr["name"], "Étiqueter comme doublon");
    assert_eq!(
        fr["description"],
        "Quand un ticket répète un ticket déjà ouvert."
    );
    let doublon = tags_named(&setup, b, "doublon");
    assert_eq!(doublon.len(), 1, "{doublon:?}");
    assert_eq!(
        step(&fr["actions"], "add_tags"),
        &json!({ "tag_ids": [doublon[0].0] })
    );
    assert!(tags_named(&setup, b, "duplicate").is_empty());
}

fn tag_count(pool: &TestPool, ws: &WorkspaceSeed) -> i64 {
    let mut conn = pool.get().expect("conn");
    tags::table
        .filter(tags::workspace_id.eq(ws.workspace_id))
        .count()
        .get_result(&mut conn)
        .expect("tags")
}

/// In a workspace with no state in a starter's status category, the browse
/// list leaves the step without a status, and adding the starter is
/// refused with a reason that names the category, in the caller's
/// language. Nothing is saved.
#[actix_web::test]
async fn a_status_category_with_no_state_refuses_the_starter() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let seeded = common::seed_two_workspaces(&mut db.pool_with_size(2).get().expect("conn"));
    let setup = db.pool_with_size(1);
    let pool = db.runtime_pool(4);
    let a = &seeded.a;

    in_workspace(&setup, a, |c| {
        diesel::update(
            workflow_states::table
                .filter(workflow_states::workspace_id.eq(a.workspace_id))
                .filter(workflow_states::category.eq(WorkflowStateCategory::Done)),
        )
        .set(workflow_states::archived_at.eq(Some(chrono::Utc::now())))
        .execute(c)
    });

    let list = catalogue(&pool, a, "en-US").await;
    assert_eq!(
        step(
            &starter(&list, "mark-resolved-with-summary")["actions"],
            "set_status"
        ),
        &json!({})
    );

    let tags_before = tag_count(&setup, a);
    let resp = add(&pool, a, "en-US", "mark-resolved-with-summary").await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body: Value = http_test::read_body_json(resp).await;
    assert_eq!(body["code"], "STARTER_STATUS_MISSING", "{body}");
    assert_eq!(
        body["error"],
        "This starter moves tickets to a status in the Done category, and your workspace \
         doesn't have one. Add one in Workflow, then try again.",
        "{body}"
    );

    let resp = add(&pool, a, "fr-FR", "mark-resolved-with-summary").await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body: Value = http_test::read_body_json(resp).await;
    assert_eq!(body["code"], "STARTER_STATUS_MISSING", "{body}");
    let message = body["error"].as_str().expect("message");
    assert!(message.contains("Terminé"), "{body}");

    assert_eq!(tag_count(&setup, a), tags_before, "no tag created");
    let mut conn = setup.get().expect("conn");
    let saved: i64 = rules::table
        .filter(rules::workspace_id.eq(a.workspace_id))
        .count()
        .get_result(&mut conn)
        .expect("rules");
    assert_eq!(saved, 0);
}

/// A starter that names a tag before a status the workspace doesn't have
/// is refused before the tag is created, whatever order its steps are in.
#[actix_web::test]
async fn a_missing_status_is_refused_before_any_tag_is_created() {
    use backend::services::starter_catalog::{Purpose, ResolveError, StarterRule, WorkspaceRefs};

    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let seeded = common::seed_two_workspaces(&mut db.pool_with_size(2).get().expect("conn"));
    let setup = db.pool_with_size(1);
    let pool = db.runtime_pool(2);
    let a = &seeded.a;

    in_workspace(&setup, a, |c| {
        diesel::update(
            workflow_states::table
                .filter(workflow_states::workspace_id.eq(a.workspace_id))
                .filter(workflow_states::category.eq(WorkflowStateCategory::Active)),
        )
        .set(workflow_states::archived_at.eq(Some(chrono::Utc::now())))
        .execute(c)
    });

    let rule = StarterRule {
        id: "tag-then-status".into(),
        name_en: "Tag then status".into(),
        name_fr: "Tag then status".into(),
        name_nl: "Tag then status".into(),
        description_en: String::new(),
        description_fr: String::new(),
        description_nl: String::new(),
        trigger_kind: backend::models::RuleTriggerKind::Manual,
        trigger_config: json!({}),
        conditions: json!([]),
        actions: json!([
            { "kind": "add_tags", "config": { "tag_names": { "en": ["picked-up"] } } },
            { "kind": "set_status", "config": { "category": "active" } },
        ]),
    };
    let tags_before = tag_count(&setup, a);
    let result = in_workspace(&pool, a, |c| {
        Ok(WorkspaceRefs::default().resolve_actions(c, &rule, "en-US", Purpose::Add))
    });
    assert!(
        matches!(
            result,
            Err(ResolveError::NoStatusIn(WorkflowStateCategory::Active))
        ),
        "{result:?}"
    );
    assert_eq!(tag_count(&setup, a), tags_before, "no tag created");
    assert!(tags_named(&setup, a, "picked-up").is_empty());
}

#[actix_web::test]
async fn only_admins_add_starters_and_only_ones_that_exist() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let seeded = common::seed_two_workspaces(&mut db.pool_with_size(2).get().expect("conn"));
    let setup = db.pool_with_size(1);
    let pool = db.runtime_pool(4);
    let a = &seeded.a;

    let missing = add(&pool, a, "en-US", "no-such-starter").await;
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);

    // An agent can apply rules but not browse or add starters.
    in_workspace(&setup, a, |c| {
        diesel::update(
            workspace_members::table
                .filter(workspace_members::workspace_id.eq(a.workspace_id))
                .filter(workspace_members::user_uuid.eq(a.member_uuid)),
        )
        .set(workspace_members::role.eq("agent"))
        .execute(c)
    });
    let listed = call(
        &pool,
        a,
        a.member_uuid,
        "en-US",
        http_test::TestRequest::get().uri("/api/admin/rule-starters"),
    )
    .await;
    assert_eq!(listed.status(), StatusCode::FORBIDDEN);
    let posted = call(
        &pool,
        a,
        a.member_uuid,
        "en-US",
        http_test::TestRequest::post().uri("/api/admin/rule-starters/set-high-priority"),
    )
    .await;
    assert_eq!(posted.status(), StatusCode::FORBIDDEN);

    let mut conn = setup.get().expect("conn");
    let saved: i64 = rules::table
        .filter(rules::workspace_id.eq(a.workspace_id))
        .count()
        .get_result(&mut conn)
        .expect("rules");
    assert_eq!(saved, 0);
}

// =====================================================================
// Auto-close: time rules from the catalogue.
// =====================================================================

const CLOSE: &str = "auto-close-awaiting-requester";
const WARN: &str = "auto-close-warning";

fn awaiting_requester(minutes: i64) -> Value {
    json!({
        "clock_name": "awaiting_requester",
        "minutes": minutes,
        "repeat_every_minutes": null,
    })
}

/// What rules `ids` wait on: ticket, rule, clock started, due.
type Wait = (i32, i32, DateTime<Utc>, DateTime<Utc>);

fn waits(h: &Harness, ws: &WorkspaceSeed, ids: &[i32]) -> Vec<Wait> {
    h.as_admin(ws, |c| {
        ticket_elapsed_queue::table
            .filter(ticket_elapsed_queue::rule_id.eq_any(ids))
            .select((
                ticket_elapsed_queue::ticket_id,
                ticket_elapsed_queue::rule_id,
                ticket_elapsed_queue::clock_started_at,
                ticket_elapsed_queue::due_at,
            ))
            .order((
                ticket_elapsed_queue::ticket_id,
                ticket_elapsed_queue::rule_id,
            ))
            .load(c)
    })
}

fn wait_on(h: &Harness, ws: &WorkspaceSeed, ticket: i32, rule: i32) -> Option<Wait> {
    waits(h, ws, &[rule]).into_iter().find(|w| w.0 == ticket)
}

/// Give `user` a primary email address, so a notify step has somewhere to
/// send.
fn email(h: &Harness, user: Uuid, address: &str) {
    use backend::schema::user_emails;
    diesel::insert_into(user_emails::table)
        .values((
            user_emails::user_uuid.eq(user),
            user_emails::email.eq(address),
            user_emails::email_type.eq("personal"),
            user_emails::is_primary.eq(true),
            user_emails::is_verified.eq(true),
        ))
        .execute(&mut h.db.conn())
        .expect("insert email");
}

/// A public reply on `ticket` by `author`, written `days` ago.
fn reply_days_ago(h: &Harness, ws: &WorkspaceSeed, ticket: i32, author: Uuid, days: i64) {
    let id = h.reply(ws, ticket, author);
    h.bypass(|c| {
        diesel::update(comments::table.find(id))
            .set(comments::created_at.eq(Utc::now() - chrono::Duration::days(days)))
            .execute(c)
    });
}

/// Every email queued about `ticket`, oldest first.
fn mail(h: &Harness, ws: &WorkspaceSeed, ticket: i32) -> Vec<OutboundEmail> {
    h.as_admin(ws, |c| {
        outbound_emails::table
            .filter(outbound_emails::ticket_id.eq(ticket))
            .order(outbound_emails::id)
            .load(c)
    })
}

fn state_of(h: &Harness, ws: &WorkspaceSeed, ticket: i32) -> i32 {
    h.as_admin(ws, |c| {
        backend::repository::tickets::get_ticket_by_id(c, ticket)
    })
    .workflow_state_id
}

fn first_in(h: &Harness, ws: &WorkspaceSeed, category: WorkflowStateCategory) -> i32 {
    h.as_admin(ws, |c| {
        backend::repository::workflow_states::first_in_category(c, category)
    })
    .id
}

fn tag_id(h: &Harness, ws: &WorkspaceSeed, name: &str) -> i32 {
    h.as_admin(ws, |c| {
        tags::table
            .filter(tags::name.eq(name))
            .select(tags::id)
            .first(c)
    })
}

/// Add both auto-close starters to `ws` and turn them on, as an admin does
/// from the rules page, the close first: its clocks are queued before the
/// warning's. Before they are on they wait on nothing; once on, every ticket
/// in `waiting` waits on both. Returns (close, warn).
async fn turn_on_auto_close(h: &Harness, ws: &WorkspaceSeed, waiting: &[i32]) -> (i32, i32) {
    let mut ids = Vec::new();
    for starter in [CLOSE, WARN] {
        let rule = added(&h.pool, ws, "en-US", starter).await;
        assert_eq!(rule["state"], "draft", "{rule}");
        ids.push(rule["id"].as_i64().expect("id") as i32);
    }
    for id in &ids {
        elapsed::requeue_rule(&h.pool, ws.workspace_id, *id).expect("requeue");
    }
    assert!(waits(h, ws, &ids).is_empty(), "a draft waits on nothing");

    for id in &ids {
        let resp = call(
            &h.pool,
            ws,
            ws.admin_uuid,
            "en-US",
            http_test::TestRequest::patch()
                .uri(&format!("/api/rules/{id}/state"))
                .set_json(json!({ "state": "live" })),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK, "rule {id} goes live");
        // Going live queues the clocks off the request.
        queued_on(h, ws, *id, waiting).await;
    }
    (ids[0], ids[1])
}

/// Wait until every ticket in `waiting` waits on `rule`.
async fn queued_on(h: &Harness, ws: &WorkspaceSeed, rule: i32, waiting: &[i32]) {
    for _ in 0..50 {
        let queued = waits(h, ws, &[rule]);
        if waiting.iter().all(|t| queued.iter().any(|w| w.0 == *t)) {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    panic!(
        "rule {rule}'s clocks never queued: {:?}",
        waits(h, ws, &[rule])
    );
}

/// Each auto-close starter adds as a draft time rule on the requester's
/// silence, with no conditions and its steps resolved in the workspace:
/// the close moves to the first done state, the warning emails the
/// requester and tags the ticket with a tag the workspace gets on add.
#[actix_web::test]
async fn the_auto_close_starters_add_as_draft_time_rules() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let seeded = common::seed_two_workspaces(&mut db.pool_with_size(2).get().expect("conn"));
    let setup = db.pool_with_size(1);
    let pool = db.runtime_pool(4);
    let (a, b) = (&seeded.a, &seeded.b);
    let done = first_state(&setup, a, WorkflowStateCategory::Done);

    let list = catalogue(&pool, a, "en-US").await;
    for (id, minutes) in [(CLOSE, 43200), (WARN, 38880)] {
        let listed = starter(&list, id);
        assert_eq!(listed["trigger_kind"], "time_elapsed", "{listed}");
        assert_eq!(listed["trigger_config"], awaiting_requester(minutes));
        assert_eq!(listed["conditions"], json!([]));
    }
    assert_eq!(
        step(&starter(&list, CLOSE)["actions"], "set_status"),
        &json!({ "workflow_state_id": done })
    );

    let close = added(&pool, a, "en-US", CLOSE).await;
    assert_eq!(
        close["name"],
        "Close after 30 days waiting on the requester"
    );
    assert_eq!(close["state"], "draft");
    assert_eq!(close["trigger_kind"], "time_elapsed");
    assert_eq!(close["trigger_config"], awaiting_requester(43200));
    assert_eq!(close["conditions"], json!([]));
    assert_eq!(
        close["actions"],
        json!([{ "kind": "set_status", "config": { "workflow_state_id": done } }])
    );

    let warn = added(&pool, a, "en-US", WARN).await;
    assert_eq!(warn["name"], "Warn 3 days before auto-close");
    assert_eq!(warn["state"], "draft");
    assert_eq!(warn["trigger_kind"], "time_elapsed");
    assert_eq!(warn["trigger_config"], awaiting_requester(38880));
    assert_eq!(warn["conditions"], json!([]));
    let warned = tags_named(&setup, a, "auto-close-warned");
    assert_eq!(warned.len(), 1, "the tag is created: {warned:?}");
    let notify = step(&warn["actions"], "notify");
    assert_eq!(notify["recipient"], "requester");
    assert!(notify["subject"]
        .as_str()
        .is_some_and(|s| s.contains("3 days")));
    assert_eq!(
        step(&warn["actions"], "add_tags"),
        &json!({ "tag_ids": [warned[0].0] })
    );

    // In French, the French name, email and tag.
    let fr = added(&pool, b, "fr-FR", WARN).await;
    assert_eq!(
        fr["name"],
        "Prévenir 3 jours avant la fermeture automatique"
    );
    let notify_fr = step(&fr["actions"], "notify");
    assert_eq!(
        notify_fr["subject"],
        "Nous fermerons votre demande dans 3 jours"
    );
    assert!(
        notify_fr["body"]
            .as_str()
            .is_some_and(|b| b.starts_with("Bonjour {{customer_first_name}},")
                && b.contains("répondez simplement à cet e-mail")),
        "{notify_fr}"
    );
    assert!(notify_fr.get("texts").is_none(), "{notify_fr}");
    let annoncee = tags_named(&setup, b, "fermeture-auto-annoncée");
    assert_eq!(annoncee.len(), 1, "{annoncee:?}");
    assert_eq!(
        step(&fr["actions"], "add_tags"),
        &json!({ "tag_ids": [annoncee[0].0] })
    );
}

/// Turned on, the copies run on the elapsed clock: a ticket the team
/// answered 31 days ago, with no word from the requester since, is warned
/// and tagged, and fires nothing again until its close. One warned on
/// time, at 27 days, closes at 30 days, from the clock as before.
#[actix_web::test]
async fn turned_on_auto_close_warns_then_closes() {
    common::ensure_test_keyring();
    let h = Harness::new();
    let a = &h.seeded.a;
    email(&h, a.member_uuid, "requester@example.com");
    let ticket = h.ticket(a, "Scanner offline");
    reply_days_ago(&h, a, ticket, a.admin_uuid, 31);

    let (close, warn) = turn_on_auto_close(&h, a, &[ticket]).await;

    // The close came due before the warning was on; it waits on it.
    assert_eq!(elapsed::fire_due(&h.pool, None).expect("fire"), 2);
    let done = first_in(&h, a, WorkflowStateCategory::Done);
    let warned_tag = tag_id(&h, a, "auto-close-warned");
    assert_ne!(state_of(&h, a, ticket), done, "not yet");
    assert!(h.tags(a, ticket).contains(&warned_tag), "tagged");
    let sent = mail(&h, a, ticket);
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert_eq!(sent[0].recipient, "requester@example.com");
    assert!(
        sent[0]
            .subject
            .contains("We'll close your request in 3 days"),
        "{sent:?}"
    );
    // No channel in the workspace: plain mail from the default sender.
    assert_eq!(sent[0].channel_id, None);
    assert_eq!(sent[0].sender_identity, "workspace");
    assert!(sent[0].headers_json.get("Reply-To").is_none());
    let warned = h.applications(a, warn);
    assert_eq!(warned.len(), 1);
    assert_eq!(warned[0].status, RuleApplicationStatus::Succeeded);
    assert!(h.applications(a, close).is_empty());

    // Its writes reach the consumer and nothing fires again: the ticket
    // waits only on the close.
    h.drain().await;
    assert!(wait_on(&h, a, ticket, warn).is_none(), "warned once");
    assert!(wait_on(&h, a, ticket, close).is_some());
    assert_eq!(elapsed::fire_due(&h.pool, None).expect("fire"), 0);
    assert_eq!(mail(&h, a, ticket).len(), 1);

    // Had the warning gone out at 27 days, the close is due at 30.
    let started = wait_on(&h, a, ticket, close).expect("waiting").2;
    backdate_warning(&h, a, warn, ticket, started + chrono::Duration::days(27));
    elapsed::requeue_rule(&h.pool, a.workspace_id, close).expect("requeue");
    let due = wait_on(&h, a, ticket, close).expect("waiting");
    assert_eq!(due.3, started + chrono::Duration::days(30));
    assert!(due.3 <= Utc::now());
    assert_eq!(elapsed::fire_due(&h.pool, None).expect("fire"), 1);
    assert_eq!(state_of(&h, a, ticket), done, "closed");
    assert_eq!(
        h.applications(a, close)
            .into_iter()
            .map(|app| app.status)
            .collect::<Vec<_>>(),
        vec![RuleApplicationStatus::Succeeded]
    );
}

/// Move the warning's run on `ticket`, and the tags it put on, back to
/// `at`: as if it ran then.
fn backdate_warning(h: &Harness, ws: &WorkspaceSeed, warn: i32, ticket: i32, at: DateTime<Utc>) {
    use backend::schema::{rule_applications, ticket_tags};
    let ran: DateTime<Utc> = h.as_admin(ws, |c| {
        rule_applications::table
            .filter(rule_applications::rule_id.eq(warn))
            .filter(rule_applications::ticket_id.eq(ticket))
            .select(rule_applications::applied_at)
            .first(c)
    });
    h.bypass(|c| {
        diesel::update(
            rule_applications::table
                .filter(rule_applications::rule_id.eq(warn))
                .filter(rule_applications::ticket_id.eq(ticket)),
        )
        .set(rule_applications::applied_at.eq(at))
        .execute(c)?;
        diesel::update(
            ticket_tags::table
                .filter(ticket_tags::ticket_id.eq(ticket))
                .filter(ticket_tags::created_at.eq(ran)),
        )
        .set(ticket_tags::created_at.eq(at))
        .execute(c)
    });
}

/// A requester's reply before the threshold resets the wait: the due
/// clocks leave the queue and nothing fires. When the team answers again,
/// the 30 days count from that reply.
#[actix_web::test]
async fn a_requester_reply_resets_auto_close() {
    common::ensure_test_keyring();
    let h = Harness::new();
    let a = &h.seeded.a;
    email(&h, a.member_uuid, "requester@example.com");
    let ticket = h.ticket(a, "Laptop won't charge");
    reply_days_ago(&h, a, ticket, a.admin_uuid, 31);
    let (close, warn) = turn_on_auto_close(&h, a, &[ticket]).await;
    let due = wait_on(&h, a, ticket, close).expect("waiting");
    assert!(due.3 <= Utc::now(), "due");

    h.reply(a, ticket, a.member_uuid);
    h.drain().await;
    assert!(waits(&h, a, &[close, warn]).is_empty(), "reset");
    assert_eq!(elapsed::fire_due(&h.pool, None).expect("fire"), 0);
    assert!(h.applications(a, close).is_empty());
    assert!(h.applications(a, warn).is_empty());
    assert!(mail(&h, a, ticket).is_empty());
    assert_ne!(
        state_of(&h, a, ticket),
        first_in(&h, a, WorkflowStateCategory::Done)
    );

    let answered = h.reply(a, ticket, a.admin_uuid);
    let answered_at: DateTime<Utc> = h.as_admin(a, |c| {
        comments::table
            .find(answered)
            .select(comments::created_at)
            .first(c)
    });
    h.drain().await;
    let again = wait_on(&h, a, ticket, close).expect("waiting again");
    assert_eq!(again.2, answered_at, "counts from the new reply");
    assert_eq!(again.3, again.2 + chrono::Duration::days(30));
    let warning = wait_on(&h, a, ticket, warn).expect("warning again");
    assert_eq!(warning.3, again.2 + chrono::Duration::days(27));
    assert_eq!(elapsed::fire_due(&h.pool, None).expect("fire"), 0);
}

/// A closed, cancelled or merged ticket is left alone, however long the
/// requester has been silent: the starters carry no conditions, and the
/// clock doesn't run on such a ticket.
#[actix_web::test]
async fn auto_close_skips_closed_and_merged_tickets() {
    common::ensure_test_keyring();
    let h = Harness::new();
    let a = &h.seeded.a;
    email(&h, a.member_uuid, "requester@example.com");
    let silent = |title: &str| {
        let ticket = h.ticket(a, title);
        reply_days_ago(&h, a, ticket, a.admin_uuid, 31);
        ticket
    };
    let open = silent("Monitor flickers");
    let closed = silent("Mouse lag");
    let cancelled = silent("Old request");
    let merged = silent("Mouse lag again");
    let kept = h.ticket(a, "Mouse lag, the one kept");
    let done = first_in(&h, a, WorkflowStateCategory::Done);
    let cancel = first_in(&h, a, WorkflowStateCategory::Cancelled);
    for (ticket, state) in [(closed, done), (cancelled, cancel)] {
        h.update(
            a,
            ticket,
            backend::models::TicketUpdate {
                workflow_state_id: Some(state),
                ..Default::default()
            },
        );
    }
    execute_merge(
        &mut h.pool.get().expect("conn"),
        MergeInput {
            destination_ticket_id: kept,
            source_ticket_ids: vec![merged],
            reason: None,
            notify_customer: false,
            expected_state: Vec::new(),
            marker_body: None,
        },
        &admin(a),
    )
    .expect("merge");
    let merged_state = state_of(&h, a, merged);

    // Once the open tickets wait, the requeue has passed the others too.
    let (close, warn) = turn_on_auto_close(&h, a, &[open, kept]).await;
    let queued: Vec<i32> = waits(&h, a, &[close, warn]).iter().map(|w| w.0).collect();
    for ticket in [closed, cancelled, merged] {
        assert!(!queued.contains(&ticket), "{ticket} waits: {queued:?}");
    }

    // The merge carries the merged ticket's conversation to the one kept,
    // which is open, so it is as overdue as `open`: both are warned, and
    // their close waits 3 days on the warning.
    assert_eq!(elapsed::fire_due(&h.pool, None).expect("fire"), 4);
    for ticket in [open, kept] {
        assert_eq!(mail(&h, a, ticket).len(), 1, "{ticket} warned");
        assert_ne!(state_of(&h, a, ticket), done, "{ticket} not yet closed");
    }
    assert_eq!(state_of(&h, a, cancelled), cancel, "still cancelled");
    assert_eq!(state_of(&h, a, merged), merged_state, "still merged");
    for ticket in [closed, cancelled, merged] {
        assert!(mail(&h, a, ticket).is_empty(), "{ticket}");
        assert!(h.tags(a, ticket).is_empty(), "{ticket}");
    }
    assert!(h
        .applications(a, close)
        .iter()
        .chain(h.applications(a, warn).iter())
        .all(|app| app.ticket_id == open || app.ticket_id == kept));
}

/// A ticket already silent past both thresholds when the pair is turned on
/// is warned first, though the close was turned on first and its clock
/// queued first: due rows fire in the order they fall due, and the close
/// waits 3 days from the warning. It closes once they pass, read from the
/// warning's run, so a queue rebuilt in between holds it too.
#[actix_web::test]
async fn a_backlog_ticket_is_warned_before_it_closes() {
    use backend::repository::rule_elapsed_queue as queue;
    common::ensure_test_keyring();
    let h = Harness::new();
    let a = &h.seeded.a;
    email(&h, a.member_uuid, "requester@example.com");
    let ticket = h.ticket(a, "Old request");
    reply_days_ago(&h, a, ticket, a.admin_uuid, 40);
    let (close, warn) = turn_on_auto_close(&h, a, &[ticket]).await;

    let claimed = h.bypass(|c| queue::claim_due(c, 10, 10, 300.0));
    let order: Vec<i32> = claimed.iter().map(|r| r.rule_id).collect();
    assert_eq!(order, vec![warn, close]);
    let fired: Vec<elapsed::Fired> = claimed
        .iter()
        .map(|row| elapsed::fire_claimed(&h.pool, None, row))
        .collect();
    assert_eq!(fired, vec![elapsed::Fired::Ran, elapsed::Fired::NotDue]);
    let done = first_in(&h, a, WorkflowStateCategory::Done);
    let warned_tag = tag_id(&h, a, "auto-close-warned");
    assert_ne!(state_of(&h, a, ticket), done, "warned, not closed");
    assert!(h.tags(a, ticket).contains(&warned_tag));
    assert_eq!(mail(&h, a, ticket).len(), 1, "warned");
    assert!(h.applications(a, close).is_empty());
    let warned_at = h.applications(a, warn)[0].applied_at;
    let held = wait_on(&h, a, ticket, close).expect("the close waits");
    assert_eq!(held.3, warned_at + chrono::Duration::days(3));

    // The warning's writes reach the consumer; the close still waits.
    h.drain().await;
    assert_eq!(
        wait_on(&h, a, ticket, close).expect("waiting").3,
        warned_at + chrono::Duration::days(3)
    );
    assert_eq!(elapsed::fire_due(&h.pool, None).expect("fire"), 0);

    // Three days on, a rebuilt queue finds the close due from the warning's
    // run, and it closes.
    backdate_warning(&h, a, warn, ticket, warned_at - chrono::Duration::days(3));
    h.bypass(|c| {
        diesel::delete(
            ticket_elapsed_queue::table.filter(ticket_elapsed_queue::ticket_id.eq(ticket)),
        )
        .execute(c)
    });
    elapsed::requeue_rule(&h.pool, a.workspace_id, close).expect("requeue");
    assert!(wait_on(&h, a, ticket, close).expect("due").3 <= Utc::now());
    assert_eq!(elapsed::fire_due(&h.pool, None).expect("fire"), 1);
    assert_eq!(state_of(&h, a, ticket), done, "closed");
    assert_eq!(mail(&h, a, ticket).len(), 1, "warned once");
    assert!(
        h.tags(a, ticket).contains(&warned_tag),
        "a closed ticket keeps the tag"
    );
}

/// A requester's reply after the backlog warning resets it: the close no
/// longer waits, the warned tag comes off, and nothing closes when the 3
/// days would have passed.
#[actix_web::test]
async fn a_reply_after_the_warning_resets_it_and_takes_the_tag_off() {
    common::ensure_test_keyring();
    let h = Harness::new();
    let a = &h.seeded.a;
    email(&h, a.member_uuid, "requester@example.com");
    let ticket = h.ticket(a, "Old request");
    reply_days_ago(&h, a, ticket, a.admin_uuid, 40);
    let (close, warn) = turn_on_auto_close(&h, a, &[ticket]).await;
    // Someone else's tag stays where it is.
    let other = h.tag(a, "printer");
    let warned_tag = tag_id(&h, a, "auto-close-warned");
    h.as_admin(a, |c| {
        backend::repository::tags::set_tags_for_ticket(c, ticket, &[other], Some(a.admin_uuid))
    });

    elapsed::fire_due(&h.pool, None).expect("fire");
    h.drain().await;
    assert!(h.tags(a, ticket).contains(&warned_tag));
    let warned_at = h.applications(a, warn)[0].applied_at;

    h.reply(a, ticket, a.member_uuid);
    h.drain().await;
    assert!(waits(&h, a, &[close, warn]).is_empty(), "reset");
    assert_eq!(h.tags(a, ticket), vec![other], "the warned tag came off");

    // Three days after the warning, nothing closes, and a rebuild changes
    // nothing.
    backdate_warning(&h, a, warn, ticket, warned_at - chrono::Duration::days(3));
    elapsed::reconcile(&h.pool).expect("reconcile");
    assert!(waits(&h, a, &[close, warn]).is_empty());
    assert_eq!(elapsed::fire_due(&h.pool, None).expect("fire"), 0);
    assert!(h.applications(a, close).is_empty());
    assert_ne!(
        state_of(&h, a, ticket),
        first_in(&h, a, WorkflowStateCategory::Done)
    );

    // Put back on by hand after the reset, the tag stays.
    h.as_admin(a, |c| {
        backend::repository::tags::set_tags_for_ticket(
            c,
            ticket,
            &[other, warned_tag],
            Some(a.admin_uuid),
        )
    });
    h.drain().await;
    elapsed::reconcile(&h.pool).expect("reconcile");
    let mut tags = h.tags(a, ticket);
    tags.sort();
    let mut want = vec![other, warned_tag];
    want.sort();
    assert_eq!(tags, want);
}

/// The warned tag goes on only when the warning is sent: a requester with
/// no address, or one on the suppression list, isn't warned, so their
/// ticket isn't tagged. The run says why.
#[actix_web::test]
async fn an_unsent_warning_leaves_no_tag() {
    common::ensure_test_keyring();
    let h = Harness::new();
    let a = &h.seeded.a;
    // The member has no email address yet.
    let no_address = h.ticket(a, "Monitor flickers");
    reply_days_ago(&h, a, no_address, a.admin_uuid, 28);
    let (_, warn) = turn_on_auto_close(&h, a, &[no_address]).await;
    elapsed::fire_due(&h.pool, None).expect("fire");
    assert!(h.tags(a, no_address).is_empty(), "not tagged");
    assert!(mail(&h, a, no_address).is_empty());
    let run = &h.applications(a, warn)[0];
    assert_eq!(run.status, RuleApplicationStatus::Succeeded);
    let taken = run.actions_taken.as_ref().expect("taken");
    assert_eq!(taken[0]["skipped"], "no_email", "{taken}");
    assert_eq!(
        run.actions_skipped,
        Some(json!([{ "index": 2, "reason": "warning_not_sent" }]))
    );

    // On the suppression list: the email is recorded as suppressed, and the
    // ticket isn't tagged either.
    email(&h, a.member_uuid, "bounced@example.com");
    h.as_admin(a, |c| {
        backend::repository::email_suppressions::upsert(
            c,
            backend::models::NewEmailSuppression {
                email: "bounced@example.com".into(),
                reason: "manual".into(),
                bounce_diagnostic: None,
                workspace_id: a.workspace_id,
            },
        )
    });
    let suppressed = h.ticket(a, "Mouse lag");
    reply_days_ago(&h, a, suppressed, a.admin_uuid, 28);
    elapsed::requeue_rule(&h.pool, a.workspace_id, warn).expect("requeue");
    elapsed::fire_due(&h.pool, None).expect("fire");
    assert!(h.tags(a, suppressed).is_empty(), "not tagged");
    let sent = mail(&h, a, suppressed);
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert_eq!(sent[0].status, "suppressed");
    let run = h
        .applications(a, warn)
        .into_iter()
        .find(|app| app.ticket_id == suppressed)
        .expect("ran");
    assert_eq!(
        run.actions_taken.as_ref().expect("taken")[0]["skipped"],
        "suppressed"
    );
    assert_eq!(
        run.actions_skipped,
        Some(json!([{ "index": 2, "reason": "warning_not_sent" }]))
    );
}

/// The warning goes to the requester the way an agent's reply would: through
/// the mailbox a ticket came in by, with its Reply-To and following the mail
/// the ticket received, so their answer threads back. A ticket that didn't
/// come in by email goes through the workspace's mailbox too.
#[actix_web::test]
async fn the_warning_goes_through_the_tickets_mailbox() {
    common::ensure_test_keyring();
    let h = Harness::new();
    let a = &h.seeded.a;
    email(&h, a.member_uuid, "requester@example.com");
    let channel = h.as_admin(a, |c| {
        backend::repository::channels::create(
            c,
            NewChannel {
                provider: "email_imap".into(),
                name: "Support".into(),
                enabled: true,
                config: json!({
                    "host": "imap.example.com",
                    "username": "support@example.com",
                    "reply_domain": "example.com",
                }),
            },
        )
    });
    let by_email = h
        .as_admin(a, |c| {
            let open = backend::repository::workflow_states::default_state(c)?;
            backend::repository::tickets::create_ticket(
                c,
                NewTicket {
                    title: "Printer offline".to_string(),
                    workflow_state_id: open.id,
                    requester_uuid: Some(a.member_uuid),
                    origin_channel_id: Some(channel.id),
                    ..Default::default()
                },
            )
        })
        .id;
    h.as_admin(a, |c| {
        backend::repository::channels::record_message(
            c,
            NewChannelMessage {
                channel_id: channel.id,
                external_id: "<first@customer.example>".into(),
                direction: "inbound".into(),
                ticket_id: Some(by_email),
                comment_id: None,
                in_reply_to: None,
                from_address: Some("requester@example.com".into()),
                author_user_uuid: Some(a.member_uuid),
                raw_metadata: None,
            },
        )
    });
    reply_days_ago(&h, a, by_email, a.admin_uuid, 28);
    let from_portal = h.ticket(a, "Badge expired");
    reply_days_ago(&h, a, from_portal, a.admin_uuid, 28);

    turn_on_auto_close(&h, a, &[by_email, from_portal]).await;
    assert_eq!(elapsed::fire_due(&h.pool, None).expect("fire"), 2);

    let sent = mail(&h, a, by_email);
    assert_eq!(sent.len(), 1, "{sent:?}");
    let warning = &sent[0];
    assert_eq!(warning.recipient, "requester@example.com");
    assert_eq!(warning.channel_id, Some(channel.id));
    assert_eq!(warning.sender_identity, "workspace");
    assert_eq!(warning.headers_json["Reply-To"], "support@example.com");
    assert_eq!(warning.headers_json["Auto-Submitted"], "auto-generated");
    assert!(
        warning.message_id.ends_with("@example.com"),
        "{}",
        warning.message_id
    );
    assert_eq!(
        warning.in_reply_to.as_deref(),
        Some("<first@customer.example>")
    );

    let sent = mail(&h, a, from_portal);
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert_eq!(sent[0].channel_id, Some(channel.id));
    assert_eq!(sent[0].headers_json["Reply-To"], "support@example.com");
}

/// A warning the ticket doesn't meet the conditions of holds nothing: it
/// won't run there, so the close fires on time rather than waiting on it.
#[actix_web::test]
async fn a_warning_that_wont_run_holds_no_close() {
    use crate::rules_event_triggers::{leaf, Spec};
    use backend::models::RuleTriggerKind as Trigger;
    common::ensure_test_keyring();
    let h = Harness::new();
    let a = &h.seeded.a;
    email(&h, a.member_uuid, "requester@example.com");
    let ticket = h.ticket(a, "Old request");
    reply_days_ago(&h, a, ticket, a.admin_uuid, 31);
    let done = first_in(&h, a, WorkflowStateCategory::Done);
    let close = h.rule(
        a,
        Spec {
            trigger_config: awaiting_requester(43200),
            ..Spec::live(
                Trigger::TimeElapsed,
                json!([{ "kind": "set_status", "config": { "workflow_state_id": done } }]),
            )
        },
    );
    let urgent_only = Spec {
        trigger_config: awaiting_requester(38880),
        conditions: leaf("ticket.priority", "eq", json!("urgent")),
        ..Spec::live(
            Trigger::TimeElapsed,
            json!([{ "kind": "notify", "config": {
                "recipient": "requester", "subject": "Closing soon", "body": "Reply to keep it open."
            } }]),
        )
    };
    let warn = h.rule(a, urgent_only);
    for rule in [close, warn] {
        elapsed::requeue_rule(&h.pool, a.workspace_id, rule).expect("requeue");
    }

    assert!(
        wait_on(&h, a, ticket, warn).is_none(),
        "the warning won't run"
    );
    assert!(wait_on(&h, a, ticket, close).expect("due").3 <= Utc::now());
    assert_eq!(elapsed::fire_due(&h.pool, None).expect("fire"), 1);
    assert_eq!(state_of(&h, a, ticket), done, "closed on time");
    assert!(mail(&h, a, ticket).is_empty());
    assert!(h.applications(a, warn).is_empty());
}

/// A warning holds only a later rule that closes the ticket: an escalation
/// on the same clock that doesn't close keeps its own time, though the
/// warning before it fires on the same run.
#[actix_web::test]
async fn a_warning_holds_no_rule_that_doesnt_close() {
    use crate::rules_event_triggers::Spec;
    use backend::models::{RuleTriggerKind as Trigger, TicketPriority};
    common::ensure_test_keyring();
    let h = Harness::new();
    let a = &h.seeded.a;
    email(&h, a.member_uuid, "requester@example.com");
    let ticket = h.ticket(a, "Old request");
    reply_days_ago(&h, a, ticket, a.admin_uuid, 40);
    let warn = h.rule(
        a,
        Spec {
            trigger_config: awaiting_requester(38880),
            ..Spec::live(
                Trigger::TimeElapsed,
                json!([{ "kind": "notify", "config": {
                    "recipient": "requester", "subject": "Still there?", "body": "Reply to keep it open."
                } }]),
            )
        },
    );
    let escalate = h.rule(
        a,
        Spec {
            trigger_config: awaiting_requester(43200),
            ..Spec::live(
                Trigger::TimeElapsed,
                json!([{ "kind": "set_priority", "config": { "priority": "high" } }]),
            )
        },
    );
    for rule in [warn, escalate] {
        elapsed::requeue_rule(&h.pool, a.workspace_id, rule).expect("requeue");
    }

    let started = wait_on(&h, a, ticket, escalate).expect("waiting").2;
    assert_eq!(
        wait_on(&h, a, ticket, escalate).expect("waiting").3,
        started + chrono::Duration::days(30),
        "its own time"
    );
    assert_eq!(elapsed::fire_due(&h.pool, None).expect("fire"), 2);
    assert_eq!(mail(&h, a, ticket).len(), 1, "warned");
    assert_eq!(h.priority(a, ticket), TicketPriority::High, "escalated");
    assert_eq!(
        h.statuses(a, escalate),
        vec![RuleApplicationStatus::Succeeded]
    );
}

/// A live time rule in `ws` on `clock` at `minutes`, its clocks queued.
fn time_rule(h: &Harness, ws: &WorkspaceSeed, clock: &str, minutes: i64, actions: Value) -> i32 {
    use crate::rules_event_triggers::Spec;
    use backend::models::RuleTriggerKind as Trigger;
    let id = h.rule(
        ws,
        Spec {
            trigger_config: json!({
                "clock_name": clock,
                "minutes": minutes,
                "repeat_every_minutes": null,
            }),
            ..Spec::live(Trigger::TimeElapsed, actions)
        },
    );
    elapsed::requeue_rule(&h.pool, ws.workspace_id, id).expect("requeue");
    id
}

fn notify_requester() -> Value {
    json!({ "kind": "notify", "config": {
        "recipient": "requester", "subject": "About your request", "body": "Reply to keep it open."
    } })
}

fn add_tag(tag: i32) -> Value {
    json!({ "kind": "add_tags", "config": { "tag_ids": [tag] } })
}

/// Only a warning before a close marks the ticket as warned. A rule that
/// emails the requester with no close after it tags the ticket whether or
/// not the email went out, and the tag stays when the requester writes. A
/// warning's tags before its email stay too; those after it come off.
#[actix_web::test]
async fn only_a_warning_before_a_close_marks_the_ticket_warned() {
    common::ensure_test_keyring();
    let h = Harness::new();
    let (a, b) = (&h.seeded.a, &h.seeded.b);

    // No close on the clock, and the requester has no address.
    let missed = h.tag(a, "sla-missed");
    time_rule(
        &h,
        a,
        "awaiting_requester",
        24 * 60,
        json!([notify_requester(), add_tag(missed)]),
    );
    let ticket = h.ticket(a, "Still on it");
    reply_days_ago(&h, a, ticket, a.admin_uuid, 2);
    h.drain().await;
    assert_eq!(elapsed::fire_due(&h.pool, None).expect("fire"), 1);
    assert!(mail(&h, a, ticket).is_empty(), "no address");
    assert_eq!(h.tags(a, ticket), vec![missed], "tagged all the same");
    h.reply(a, ticket, a.member_uuid);
    h.drain().await;
    assert_eq!(h.tags(a, ticket), vec![missed], "kept on the reply");

    // A warning before a close: the tag before its email stays.
    email(&h, b.member_uuid, "requester@example.com");
    let seen = h.tag(b, "seen");
    let warned = h.tag(b, "warned");
    let done = first_in(&h, b, WorkflowStateCategory::Done);
    time_rule(
        &h,
        b,
        "awaiting_requester",
        43200,
        json!([{ "kind": "set_status", "config": { "workflow_state_id": done } }]),
    );
    time_rule(
        &h,
        b,
        "awaiting_requester",
        38880,
        json!([add_tag(seen), notify_requester(), add_tag(warned)]),
    );
    let ticket = h.ticket(b, "Printer jammed");
    reply_days_ago(&h, b, ticket, b.admin_uuid, 28);
    h.drain().await;
    elapsed::fire_due(&h.pool, None).expect("fire");
    assert_eq!(mail(&h, b, ticket).len(), 1);
    let mut both = vec![seen, warned];
    both.sort();
    assert_eq!(h.tags(b, ticket), both);
    h.reply(b, ticket, b.member_uuid);
    h.drain().await;
    assert_eq!(
        h.tags(b, ticket),
        vec![seen],
        "only the warned tag came off"
    );
}

/// The close waits on its nearest warning only: a one-day nudge on the same
/// clock, fired late on a backlog ticket, doesn't hold it for 29 days.
#[actix_web::test]
async fn the_close_waits_on_its_nearest_warning() {
    common::ensure_test_keyring();
    let h = Harness::new();
    let a = &h.seeded.a;
    email(&h, a.member_uuid, "requester@example.com");
    let ticket = h.ticket(a, "Old request");
    reply_days_ago(&h, a, ticket, a.admin_uuid, 40);
    let done = first_in(&h, a, WorkflowStateCategory::Done);
    let close = time_rule(
        &h,
        a,
        "awaiting_requester",
        43200,
        json!([{ "kind": "set_status", "config": { "workflow_state_id": done } }]),
    );
    let warn = time_rule(
        &h,
        a,
        "awaiting_requester",
        38880,
        json!([notify_requester()]),
    );
    let nudge = time_rule(
        &h,
        a,
        "awaiting_requester",
        24 * 60,
        json!([notify_requester()]),
    );

    elapsed::fire_due(&h.pool, None).expect("fire");
    assert_eq!(h.applications(a, nudge).len(), 1, "nudged");
    let warned_at = h.applications(a, warn)[0].applied_at;
    assert_eq!(mail(&h, a, ticket).len(), 2);
    assert_eq!(
        wait_on(&h, a, ticket, close).expect("waiting").3,
        warned_at + chrono::Duration::days(3)
    );
}

/// A warning paused after it warned still holds the close from its run, and
/// its tag still comes off when the requester answers.
#[actix_web::test]
async fn a_warning_paused_after_it_warned_still_holds_the_close() {
    common::ensure_test_keyring();
    let h = Harness::new();
    let a = &h.seeded.a;
    email(&h, a.member_uuid, "requester@example.com");
    let ticket = h.ticket(a, "Old request");
    reply_days_ago(&h, a, ticket, a.admin_uuid, 40);
    let (close, warn) = turn_on_auto_close(&h, a, &[ticket]).await;
    elapsed::fire_due(&h.pool, None).expect("fire");
    let warned_at = h.applications(a, warn)[0].applied_at;
    let warned_tag = tag_id(&h, a, "auto-close-warned");
    assert!(h.tags(a, ticket).contains(&warned_tag));

    let resp = call(
        &h.pool,
        a,
        a.admin_uuid,
        "en-US",
        http_test::TestRequest::patch()
            .uri(&format!("/api/rules/{warn}/state"))
            .set_json(json!({ "state": "paused" })),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK, "paused");
    h.drain().await;
    elapsed::requeue_rule(&h.pool, a.workspace_id, close).expect("requeue");
    assert_eq!(
        wait_on(&h, a, ticket, close).expect("waiting").3,
        warned_at + chrono::Duration::days(3),
        "still held"
    );
    assert_eq!(elapsed::fire_due(&h.pool, None).expect("fire"), 0);

    h.reply(a, ticket, a.member_uuid);
    h.drain().await;
    assert!(!h.tags(a, ticket).contains(&warned_tag), "came off");
}

/// A ticket three hours in its state, with a close at two hours on
/// `since_status_change` and a warning at one hour that emails the
/// requester, moves the ticket to an active state and tags it. Returns
/// (ticket, warning, its tag, the ticket's first state, the active state).
fn self_restarting_warning(h: &Harness, ws: &WorkspaceSeed) -> (i32, i32, i32, i32, i32) {
    use backend::schema::tickets;
    email(h, ws.member_uuid, "requester@example.com");
    let ticket = h.ticket(ws, "Waiting on a part");
    diesel::update(tickets::table.find(ticket))
        .set(tickets::created_at.eq(Utc::now() - chrono::Duration::hours(3)))
        .execute(&mut h.db.conn())
        .expect("backdate the ticket");
    let first = state_of(h, ws, ticket);
    let active = first_in(h, ws, WorkflowStateCategory::Active);
    assert_ne!(first, active);
    let done = first_in(h, ws, WorkflowStateCategory::Done);
    let warned = h.tag(ws, "warned");
    time_rule(
        h,
        ws,
        "since_status_change",
        120,
        json!([{ "kind": "set_status", "config": { "workflow_state_id": done } }]),
    );
    let warn = time_rule(
        h,
        ws,
        "since_status_change",
        60,
        json!([
            notify_requester(),
            { "kind": "set_status", "config": { "workflow_state_id": active } },
            add_tag(warned),
        ]),
    );
    (ticket, warn, warned, first, active)
}

/// A warning whose own step starts its clock again (it moves the ticket to
/// another open state) keeps its tag: only a later restart takes it off.
#[actix_web::test]
async fn a_warning_that_restarts_its_own_clock_keeps_its_tag() {
    common::ensure_test_keyring();
    let h = Harness::new();
    let a = &h.seeded.a;
    let (ticket, warn, warned, first, active) = self_restarting_warning(&h, a);

    elapsed::fire_due(&h.pool, None).expect("fire");
    assert_eq!(h.applications(a, warn).len(), 1, "warned");
    assert_eq!(state_of(&h, a, ticket), active);
    h.drain().await;
    assert_eq!(
        h.tags(a, ticket),
        vec![warned],
        "kept through its own restart"
    );

    // Someone else moves it on: the clock starts again, and the tag comes off.
    h.update(
        a,
        ticket,
        backend::models::TicketUpdate {
            workflow_state_id: Some(first),
            ..Default::default()
        },
    );
    h.drain().await;
    assert!(h.tags(a, ticket).is_empty(), "came off");
}

/// An agent's restart counts though a rule the warning's change set off
/// writes after it, in the warning's chain of events: the clock started
/// from the agent's change, not the run's.
#[actix_web::test]
async fn a_restart_counts_though_the_warnings_chain_writes_later() {
    use crate::rules_event_triggers::Spec;
    use backend::models::RuleTriggerKind as Trigger;
    common::ensure_test_keyring();
    let h = Harness::new();
    let a = &h.seeded.a;
    let (ticket, warn, warned, first, _) = self_restarting_warning(&h, a);
    // An event rule that tags any changed ticket: it runs on the warning's
    // change in the warning's chain, once the consumer gets to it.
    let chained = h.tag(a, "chained");
    h.rule(
        a,
        Spec::live(Trigger::TicketUpdated, json!([add_tag(chained)])),
    );

    elapsed::fire_due(&h.pool, None).expect("fire");
    assert_eq!(h.applications(a, warn).len(), 1, "warned");
    // The agent moves it on before the consumer runs the chained rule.
    h.update(
        a,
        ticket,
        backend::models::TicketUpdate {
            workflow_state_id: Some(first),
            ..Default::default()
        },
    );
    h.drain().await;
    let tags = h.tags(a, ticket);
    assert!(tags.contains(&chained), "the chained rule wrote: {tags:?}");
    assert!(!tags.contains(&warned), "came off: {tags:?}");
}

/// A warning deleted after it warned still holds the close from its run,
/// and its tag still comes off when the requester answers.
#[actix_web::test]
async fn a_warning_deleted_after_it_warned_still_holds_the_close() {
    common::ensure_test_keyring();
    let h = Harness::new();
    let a = &h.seeded.a;
    email(&h, a.member_uuid, "requester@example.com");
    let ticket = h.ticket(a, "Old request");
    reply_days_ago(&h, a, ticket, a.admin_uuid, 40);
    let (close, warn) = turn_on_auto_close(&h, a, &[ticket]).await;
    elapsed::fire_due(&h.pool, None).expect("fire");
    let warned_at = h.applications(a, warn)[0].applied_at;
    let warned_tag = tag_id(&h, a, "auto-close-warned");
    assert!(h.tags(a, ticket).contains(&warned_tag));

    let resp = call(
        &h.pool,
        a,
        a.admin_uuid,
        "en-US",
        http_test::TestRequest::delete().uri(&format!("/api/rules/{warn}")),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK, "deleted");
    assert!(h.rule_row(a, warn).archived_at.is_some());
    h.drain().await;
    elapsed::requeue_rule(&h.pool, a.workspace_id, close).expect("requeue");
    assert_eq!(
        wait_on(&h, a, ticket, close).expect("waiting").3,
        warned_at + chrono::Duration::days(3),
        "still held"
    );
    assert_eq!(elapsed::fire_due(&h.pool, None).expect("fire"), 0);

    h.reply(a, ticket, a.member_uuid);
    h.drain().await;
    assert!(!h.tags(a, ticket).contains(&warned_tag), "came off");
}

/// Only the close's nearest warning is a warning: a reminder at 3 days
/// alongside the auto-close pair tags the ticket whether or not its email
/// went out, and keeps the tag when the requester answers.
#[actix_web::test]
async fn a_reminder_before_the_auto_close_warning_keeps_its_tag() {
    common::ensure_test_keyring();
    let h = Harness::new();
    let a = &h.seeded.a;
    let reminded = h.tag(a, "reminded");
    // The member has no address yet.
    let unsent = h.ticket(a, "Mouse lag");
    reply_days_ago(&h, a, unsent, a.admin_uuid, 4);
    turn_on_auto_close(&h, a, &[]).await;
    let reminder = time_rule(
        &h,
        a,
        "awaiting_requester",
        3 * 24 * 60,
        json!([notify_requester(), add_tag(reminded)]),
    );
    elapsed::fire_due(&h.pool, None).expect("fire");
    assert!(mail(&h, a, unsent).is_empty());
    assert_eq!(h.tags(a, unsent), vec![reminded], "tagged though unsent");
    let run = &h.applications(a, reminder)[0];
    assert_eq!(
        run.condition_evaluation.as_ref().expect("evaluation")["warning"],
        false
    );

    email(&h, a.member_uuid, "requester@example.com");
    let sent = h.ticket(a, "Keyboard sticks");
    reply_days_ago(&h, a, sent, a.admin_uuid, 4);
    elapsed::requeue_rule(&h.pool, a.workspace_id, reminder).expect("requeue");
    elapsed::fire_due(&h.pool, None).expect("fire");
    assert_eq!(mail(&h, a, sent).len(), 1);
    assert_eq!(h.tags(a, sent), vec![reminded]);

    for ticket in [unsent, sent] {
        h.reply(a, ticket, a.member_uuid);
    }
    h.drain().await;
    for ticket in [unsent, sent] {
        assert_eq!(h.tags(a, ticket), vec![reminded], "{ticket} kept its tag");
    }
}

/// The warning's tag comes off on the requester's answer as it went on,
/// though the close was paused after the warning tagged the ticket.
#[actix_web::test]
async fn a_warning_tag_comes_off_though_the_close_was_paused() {
    common::ensure_test_keyring();
    let h = Harness::new();
    let a = &h.seeded.a;
    email(&h, a.member_uuid, "requester@example.com");
    let ticket = h.ticket(a, "Scanner offline");
    reply_days_ago(&h, a, ticket, a.admin_uuid, 28);
    let (close, warn) = turn_on_auto_close(&h, a, &[ticket]).await;
    elapsed::fire_due(&h.pool, None).expect("fire");
    let warned_tag = tag_id(&h, a, "auto-close-warned");
    assert!(h.tags(a, ticket).contains(&warned_tag));
    assert_eq!(
        h.applications(a, warn)[0]
            .condition_evaluation
            .as_ref()
            .expect("evaluation")["warning"],
        true
    );

    let resp = call(
        &h.pool,
        a,
        a.admin_uuid,
        "en-US",
        http_test::TestRequest::patch()
            .uri(&format!("/api/rules/{close}/state"))
            .set_json(json!({ "state": "paused" })),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK, "paused");
    h.reply(a, ticket, a.member_uuid);
    h.drain().await;
    assert!(!h.tags(a, ticket).contains(&warned_tag), "came off");
}

/// An IMAP mailbox in `ws` polling `address`, turned on or off.
fn mailbox(h: &Harness, ws: &WorkspaceSeed, address: &str, enabled: bool) -> i32 {
    h.as_admin(ws, |c| {
        backend::repository::channels::create(
            c,
            NewChannel {
                provider: "email_imap".into(),
                name: address.into(),
                enabled,
                config: json!({
                    "host": "imap.example.com",
                    "username": address,
                    "reply_domain": address.split('@').nth(1).expect("domain"),
                }),
            },
        )
    })
    .id
}

/// A ticket from `ws`'s member that came in through `channel`.
fn ticket_from(h: &Harness, ws: &WorkspaceSeed, title: &str, channel: i32) -> i32 {
    h.as_admin(ws, |c| {
        let open = backend::repository::workflow_states::default_state(c)?;
        backend::repository::tickets::create_ticket(
            c,
            NewTicket {
                title: title.to_string(),
                workflow_state_id: open.id,
                requester_uuid: Some(ws.member_uuid),
                origin_channel_id: Some(channel),
                ..Default::default()
            },
        )
    })
    .id
}

/// A ticket whose mailbox has been turned off is warned through the
/// workspace's mailbox that is on, with its Reply-To, so the answer still
/// threads back. With no mailbox that can take the answer, the warning
/// isn't sent and the ticket isn't tagged.
#[actix_web::test]
async fn a_turned_off_mailbox_warns_through_the_workspaces() {
    common::ensure_test_keyring();
    let h = Harness::new();
    let a = &h.seeded.a;
    email(&h, a.member_uuid, "requester@example.com");
    let old = mailbox(&h, a, "old@retired.example", false);
    let stranded = ticket_from(&h, a, "Printer offline", old);
    reply_days_ago(&h, a, stranded, a.admin_uuid, 28);

    let (_, warn) = turn_on_auto_close(&h, a, &[stranded]).await;
    elapsed::fire_due(&h.pool, None).expect("fire");
    assert!(mail(&h, a, stranded).is_empty(), "no mailbox to answer to");
    assert!(h.tags(a, stranded).is_empty(), "not tagged");
    let run = &h.applications(a, warn)[0];
    assert_eq!(
        run.actions_taken.as_ref().expect("taken")[0]["skipped"],
        "no_channel"
    );
    assert_eq!(
        run.actions_skipped,
        Some(json!([{ "index": 2, "reason": "warning_not_sent" }]))
    );

    let support = mailbox(&h, a, "support@example.com", true);
    let rerouted = ticket_from(&h, a, "Scanner offline", old);
    reply_days_ago(&h, a, rerouted, a.admin_uuid, 28);
    elapsed::requeue_rule(&h.pool, a.workspace_id, warn).expect("requeue");
    elapsed::fire_due(&h.pool, None).expect("fire");
    let sent = mail(&h, a, rerouted);
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert_eq!(sent[0].channel_id, Some(support));
    assert_eq!(sent[0].headers_json["Reply-To"], "support@example.com");
    assert!(
        sent[0].message_id.ends_with("@example.com"),
        "{}",
        sent[0].message_id
    );
    assert!(h
        .tags(a, rerouted)
        .contains(&tag_id(&h, a, "auto-close-warned")));
}

/// A staff member's own request waits on the team like anyone's: their own
/// public messages are the requester's, so `awaiting_requester` never starts
/// and nothing fires however long ago they wrote.
#[actix_web::test]
async fn a_staff_members_own_request_is_not_auto_closed() {
    common::ensure_test_keyring();
    let h = Harness::new();
    let a = &h.seeded.a;
    let own = h
        .as_admin(a, |c| {
            let open = backend::repository::workflow_states::default_state(c)?;
            backend::repository::tickets::create_ticket(
                c,
                NewTicket {
                    title: "My laptop".to_string(),
                    workflow_state_id: open.id,
                    requester_uuid: Some(a.admin_uuid),
                    ..Default::default()
                },
            )
        })
        .id;
    reply_days_ago(&h, a, own, a.admin_uuid, 40);
    reply_days_ago(&h, a, own, a.admin_uuid, 31);
    let control = h.ticket(a, "Their laptop");
    reply_days_ago(&h, a, control, a.admin_uuid, 31);

    let (close, warn) = turn_on_auto_close(&h, a, &[control]).await;
    let queued: Vec<i32> = waits(&h, a, &[close, warn]).iter().map(|w| w.0).collect();
    assert!(!queued.contains(&own), "{queued:?}");
    assert_eq!(elapsed::fire_due(&h.pool, None).expect("fire"), 2);
    assert_ne!(
        state_of(&h, a, own),
        first_in(&h, a, WorkflowStateCategory::Done)
    );
    assert!(h
        .applications(a, close)
        .iter()
        .chain(h.applications(a, warn).iter())
        .all(|app| app.ticket_id == control));
}
