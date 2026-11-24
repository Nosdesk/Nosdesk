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
use backend::models::{Claims, RuleApplicationStatus, WorkflowStateCategory};
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

/// Who the mail queued about `ticket` goes to, and its subject.
fn mail(h: &Harness, ws: &WorkspaceSeed, ticket: i32) -> Vec<(String, String)> {
    h.as_admin(ws, |c| {
        outbound_emails::table
            .filter(outbound_emails::ticket_id.eq(ticket))
            .order(outbound_emails::id)
            .select((outbound_emails::recipient, outbound_emails::subject))
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
/// from the rules page. Before they are on they wait on nothing; once on,
/// every ticket in `waiting` waits on both. Returns (close, warn).
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
    }
    // Going live queues the clocks off the request.
    for _ in 0..50 {
        let queued = waits(h, ws, &ids);
        if waiting.iter().all(|t| {
            ids.iter()
                .all(|r| queued.iter().any(|w| w.0 == *t && w.1 == *r))
        }) {
            return (ids[0], ids[1]);
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    panic!(
        "the auto-close clocks never queued: {:?}",
        waits(h, ws, &ids)
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

    // In French, the French name and tag.
    let fr = added(&pool, b, "fr-FR", WARN).await;
    assert_eq!(
        fr["name"],
        "Prévenir 3 jours avant la fermeture automatique"
    );
    let annoncee = tags_named(&setup, b, "fermeture-auto-annoncée");
    assert_eq!(annoncee.len(), 1, "{annoncee:?}");
    assert_eq!(
        step(&fr["actions"], "add_tags"),
        &json!({ "tag_ids": [annoncee[0].0] })
    );
}

/// Turned on, the copies run on the elapsed clock: a ticket the team
/// answered 31 days ago, with no word from the requester since, is warned,
/// tagged and closed; one answered 28 days ago is only warned. Each fires
/// once.
#[actix_web::test]
async fn turned_on_auto_close_warns_then_closes() {
    common::ensure_test_keyring();
    let h = Harness::new();
    let a = &h.seeded.a;
    email(&h, a.member_uuid, "requester@example.com");
    let overdue = h.ticket(a, "Printer jammed");
    reply_days_ago(&h, a, overdue, a.member_uuid, 40);
    reply_days_ago(&h, a, overdue, a.admin_uuid, 31);
    let warned_only = h.ticket(a, "Scanner offline");
    reply_days_ago(&h, a, warned_only, a.admin_uuid, 28);

    let (close, warn) = turn_on_auto_close(&h, a, &[overdue, warned_only]).await;

    assert_eq!(elapsed::fire_due(&h.pool, None).expect("fire"), 3);
    let done = first_in(&h, a, WorkflowStateCategory::Done);
    let warned_tag = tag_id(&h, a, "auto-close-warned");
    assert_eq!(state_of(&h, a, overdue), done, "closed");
    assert_ne!(state_of(&h, a, warned_only), done, "not yet");
    for ticket in [overdue, warned_only] {
        assert!(h.tags(a, ticket).contains(&warned_tag), "{ticket} tagged");
        let sent = mail(&h, a, ticket);
        assert_eq!(sent.len(), 1, "{ticket}: {sent:?}");
        assert_eq!(sent[0].0, "requester@example.com");
        assert!(
            sent[0].1.contains("We'll close your request in 3 days"),
            "{sent:?}"
        );
    }
    let statuses = |rule| {
        h.applications(a, rule)
            .into_iter()
            .map(|app| (app.ticket_id, app.status))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        statuses(close),
        vec![(overdue, RuleApplicationStatus::Succeeded)]
    );
    let mut warned = statuses(warn);
    warned.sort_by_key(|w| w.0);
    assert_eq!(
        warned,
        vec![
            (overdue, RuleApplicationStatus::Succeeded),
            (warned_only, RuleApplicationStatus::Succeeded)
        ]
    );

    // Their writes reach the consumer and nothing fires again: the closed
    // ticket waits on nothing, the warned one only on the close.
    h.drain().await;
    assert!(wait_on(&h, a, overdue, close).is_none());
    assert!(wait_on(&h, a, warned_only, warn).is_none(), "warned once");
    assert!(wait_on(&h, a, warned_only, close).is_some());
    assert_eq!(elapsed::fire_due(&h.pool, None).expect("fire"), 0);
    assert_eq!(mail(&h, a, warned_only).len(), 1);
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
    // which is open, so it is as overdue as `open`.
    assert_eq!(elapsed::fire_due(&h.pool, None).expect("fire"), 4);
    assert_eq!(state_of(&h, a, open), done);
    assert_eq!(state_of(&h, a, kept), done);
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
