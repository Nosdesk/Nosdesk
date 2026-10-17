//! The starter rules catalogue names statuses by category and tags by name,
//! and the server resolves them in the caller's workspace: the browse list
//! shows what each step will do there, and adding a starter saves a draft
//! whose steps carry that workspace's ids, creating any tag it names that
//! the workspace doesn't have. Driven through the runtime pool, where a
//! pooled connection starts with no workspace.

use actix_web::dev::{Service, ServiceResponse};
use actix_web::http::StatusCode;
use actix_web::test as http_test;
use actix_web::{web, App, HttpMessage};
use diesel::prelude::*;
use serde_json::{json, Value};
use uuid::Uuid;

use backend::extractors::WorkspaceContext;
use backend::middleware::RequestContext;
use backend::models::{Claims, WorkflowStateCategory};
use backend::schema::{rules, tags, workflow_states, workspace_members};
use backend::sync::actor::ActorContext;

use crate::common::{self, TestPool, WorkspaceSeed};

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
    assert_eq!(list.len(), 6);

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

/// The editor refuses a status step without a status, and a starter is held
/// to the same check: in a workspace with no state in the category, adding
/// it is refused and nothing is saved.
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

    let resp = add(&pool, a, "en-US", "mark-resolved-with-summary").await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body: Value = http_test::read_body_json(resp).await;
    assert_eq!(body["code"], "RULE_VALIDATION", "{body}");

    let mut conn = setup.get().expect("conn");
    let saved: i64 = rules::table
        .filter(rules::workspace_id.eq(a.workspace_id))
        .count()
        .get_result(&mut conn)
        .expect("rules");
    assert_eq!(saved, 0);
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
