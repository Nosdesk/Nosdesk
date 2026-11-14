//! A ticket update's sync action names what the update set in
//! `data.changed_fields` (`ticket.<column>`, sorted; a state change also names
//! `ticket.workflow_state.category`; a tag change names `ticket.tag_ids`), so an
//! event rule can filter on it. One update is still one event, an update that
//! sets nothing records none, and the consumers that read the rest of `data`
//! (the notification outbox trigger and deriver) work as before.

use std::sync::Arc;

use actix_web::dev::Service;
use actix_web::http::{Method, StatusCode};
use actix_web::test as http_test;
use actix_web::{web, App, HttpMessage};
use chrono::{DateTime, Utc};
use diesel::prelude::*;
use serde_json::{json, Value};
use uuid::Uuid;

use backend::extractors::WorkspaceContext;
use backend::middleware::RequestContext;
use backend::models::{Claims, NewTag, NewTicket, SyncAggregate, WorkflowStateCategory};
use backend::repository::workspaces::{add_membership, SeatWriteAuthority};
use backend::services::notifications::deriver::{derive, Intent, SyncActionRow};
use backend::services::outbound_email::OutboundEmailResolver;
use backend::services::search::SearchService;
use backend::sync::actor::ActorContext;
use backend::sync::session::run_in_workspace;
use backend::utils::storage::{create_storage, Storage, StorageConfig};

use crate::common::{self, TestPool, WorkspaceSeed};

const REF: &str = "test:ticket_update_changed_fields";

struct Fixture {
    pool: TestPool,
    ws: WorkspaceSeed,
    admin: Uuid,
    /// A second agent, so an assignment is someone else's and notifies them.
    agent: Uuid,
    ticket: i32,
    /// A state in another category than the ticket's default one.
    active_state: i32,
    tags: [i32; 2],
    search: Arc<SearchService>,
    storage: Arc<dyn Storage>,
    _dirs: [tempfile::TempDir; 2],
}

fn seed(db: &common::TestDb) -> Fixture {
    common::ensure_test_keyring();
    let pool = db.pool_with_size(6);
    let ws = common::seed_two_workspaces(&mut pool.get().expect("conn")).a;
    let admin = ws.admin_uuid;
    let agent = common::insert_plain_user(&mut pool.get().expect("conn"), "Agent");
    let (ticket, active_state, tags) = run_in_workspace(&pool, REF, ws.workspace_id, |c| {
        add_membership(
            c,
            ws.workspace_id,
            agent,
            "agent",
            SeatWriteAuthority::ControlPlane,
        )?;
        let default = backend::repository::workflow_states::default_state(c)?;
        assert_ne!(default.category, WorkflowStateCategory::Active);
        let active = backend::repository::workflow_states::first_in_category(
            c,
            WorkflowStateCategory::Active,
        )?;
        let ticket: i32 = diesel::insert_into(backend::schema::tickets::table)
            .values(&NewTicket {
                title: "Printer offline".to_string(),
                workflow_state_id: default.id,
                ..Default::default()
            })
            .returning(backend::schema::tickets::id)
            .get_result(c)?;
        let tag = |c: &mut backend::db::DbConnection, name: &str| {
            backend::repository::tags::create_tag(
                c,
                NewTag {
                    name: name.to_string(),
                    color: None,
                    description: None,
                },
            )
            .map(|t| t.id)
        };
        let tags = [tag(c, "hardware")?, tag(c, "network")?];
        Ok::<_, diesel::result::Error>((ticket, active.id, tags))
    })
    .expect("seed ticket");
    let storage_dir = tempfile::tempdir().expect("storage dir");
    let storage = create_storage(StorageConfig::Local {
        base_path: storage_dir.path().to_string_lossy().into_owned(),
    });
    let search_dir = tempfile::tempdir().expect("search dir");
    let search = Arc::new(SearchService::new(search_dir.path(), &pool).expect("init search"));
    Fixture {
        pool,
        ws,
        admin,
        agent,
        ticket,
        active_state,
        tags,
        search,
        storage,
        _dirs: [storage_dir, search_dir],
    }
}

/// Call `uri` as the workspace admin, through the ticket and sync routes.
async fn call(f: &Fixture, method: Method, uri: &str, body: Value) -> Value {
    let claims = Claims {
        sub: f.admin.to_string(),
        name: "Admin".to_string(),
        email: "admin@example.com".to_string(),
        platform_role: "user".to_string(),
        scope: "full".to_string(),
        sid: None,
        workspace_uuid: None,
        exp: (chrono::Utc::now().timestamp() + 3600) as usize,
        iat: chrono::Utc::now().timestamp() as usize,
    };
    let workspace = WorkspaceContext {
        workspace_id: f.ws.workspace_id,
        workspace_uuid: f.ws.workspace_uuid,
        slug: f.ws.slug.clone(),
        name: f.ws.slug.clone(),
        organisation_id: None,
        custom_domain: None,
    };
    let corr = Uuid::now_v7();
    let actor = ActorContext::user(f.admin, Some(corr)).with_workspace(f.ws.workspace_id);
    let app = http_test::init_service(
        App::new()
            .app_data(web::Data::new(f.pool.clone()))
            .app_data(web::Data::new(f.storage.clone()))
            .app_data(web::Data::new(f.search.clone()))
            .app_data(web::Data::new(Arc::new(OutboundEmailResolver::new(
                f.pool.clone(),
                None,
            ))))
            .wrap_fn(move |req, srv| {
                req.extensions_mut().insert(workspace.clone());
                req.extensions_mut().insert(claims.clone());
                req.extensions_mut()
                    .insert(RequestContext::new(corr, actor.clone()));
                srv.call(req)
            })
            .service(
                web::scope("/api")
                    .configure(backend::handlers::sync::config)
                    .configure(backend::handlers::tickets::config),
            ),
    )
    .await;
    let req = http_test::TestRequest::default()
        .method(method.clone())
        .uri(uri)
        .set_json(body)
        .to_request();
    let resp = http_test::call_service(&app, req).await;
    let status = resp.status();
    let body = String::from_utf8(http_test::read_body(resp).await.to_vec()).expect("utf8 body");
    assert_eq!(status, StatusCode::OK, "{method} {uri}: {body}");
    serde_json::from_str(&body).unwrap_or(Value::Null)
}

async fn patch(f: &Fixture, body: Value) {
    call(
        f,
        Method::PATCH,
        &format!("/api/tickets/{}", f.ticket),
        body,
    )
    .await;
}

/// Push one ticket update through the sync push route; it must apply.
async fn push(f: &Fixture, patch: Value) {
    let tx_id = Uuid::now_v7().to_string();
    let body = call(
        f,
        Method::POST,
        "/api/sync/push",
        json!([{
            "tx_id": tx_id,
            "aggregate": "ticket",
            "model_id": f.ticket.to_string(),
            "op": "U",
            "patch": patch,
        }]),
    )
    .await;
    assert_eq!(body["applied"], json!([tx_id]), "{body}");
}

/// The `sync_actions` columns the notification deriver reads.
type DeriverColumns = (i64, i32, String, Value, Option<Uuid>, String, DateTime<Utc>);

/// The ticket's sync actions after `after`, oldest first.
fn events_after(f: &Fixture, after: i64) -> Vec<SyncActionRow> {
    use backend::schema::sync_actions;
    let rows: Vec<DeriverColumns> = run_in_workspace(&f.pool, REF, f.ws.workspace_id, |c| {
        sync_actions::table
            .filter(sync_actions::aggregate.eq(SyncAggregate::Ticket))
            .filter(sync_actions::aggregate_id.eq(f.ticket.to_string()))
            .filter(sync_actions::sync_id.gt(after))
            .order(sync_actions::sync_id.asc())
            .select((
                sync_actions::sync_id,
                sync_actions::workspace_id,
                sync_actions::event_type,
                sync_actions::data,
                sync_actions::actor_uuid,
                sync_actions::actor_kind,
                sync_actions::occurred_at,
            ))
            .load(c)
    })
    .expect("load sync actions");
    rows.into_iter()
        .map(
            |(sync_id, workspace_id, event_type, data, actor_uuid, actor_kind, occurred_at)| {
                SyncActionRow {
                    sync_id,
                    workspace_id,
                    event_type,
                    data,
                    actor_uuid,
                    actor_kind,
                    occurred_at,
                }
            },
        )
        .collect()
}

fn newest(f: &Fixture) -> i64 {
    events_after(f, 0).last().map_or(0, |row| row.sync_id)
}

fn changed_fields(row: &SyncActionRow) -> Vec<&str> {
    row.data["changed_fields"]
        .as_array()
        .unwrap_or_else(|| panic!("{} has no changed_fields: {}", row.event_type, row.data))
        .iter()
        .map(|v| v.as_str().expect("a field name"))
        .collect()
}

#[actix_web::test]
async fn a_rest_update_lists_the_one_field_it_set() {
    let db = common::TestDb::new();
    let f = seed(&db);
    let before = newest(&f);

    patch(&f, json!({ "priority": "high" })).await;

    let events = events_after(&f, before);
    assert_eq!(events.len(), 1, "one update, one event");
    assert_eq!(events[0].event_type, "ticket.priority_changed");
    assert_eq!(changed_fields(&events[0]), ["ticket.priority"]);
    // The rest of the row is there as before.
    assert_eq!(events[0].data["priority"], "high");
    assert_eq!(events[0].data["id"], f.ticket);
}

#[actix_web::test]
async fn a_rest_update_of_several_fields_lists_them_all_in_one_event() {
    let db = common::TestDb::new();
    let f = seed(&db);
    let before = newest(&f);

    patch(
        &f,
        json!({
            "priority": "low",
            "assignee": f.agent.to_string(),
            "workflow_state_id": f.active_state,
            "title": "Printer offline on floor 2",
        }),
    )
    .await;

    let events = events_after(&f, before);
    assert_eq!(events.len(), 1, "one update, one event: {:?}", events);
    let row = &events[0];
    assert_eq!(row.event_type, "ticket.workflow_state_changed");
    assert_eq!(
        changed_fields(row),
        [
            "ticket.assignee_uuid",
            "ticket.priority",
            "ticket.title",
            "ticket.workflow_state.category",
            "ticket.workflow_state_id",
        ]
    );

    // Consumers that ignore the new key read the row as before: the outbox
    // trigger enqueued it and the deriver finds the assignment.
    let enqueued: i64 = run_in_workspace(&f.pool, REF, f.ws.workspace_id, |c| {
        use backend::schema::notification_outbox;
        notification_outbox::table
            .filter(notification_outbox::sync_id.eq(row.sync_id))
            .count()
            .get_result(c)
    })
    .expect("count outbox");
    assert_eq!(
        enqueued, 1,
        "the assignment reaches the notification outbox"
    );
    assert!(
        derive(row).contains(&Intent::Assigned {
            ticket_id: f.ticket,
            ticket_title: "Printer offline on floor 2".to_string(),
            assignee: f.agent,
        }),
        "{:?}",
        derive(row)
    );
    assert_eq!(row.data["previous_assignee_uuid"], Value::Null);
}

#[actix_web::test]
async fn an_update_that_sets_nothing_records_no_event() {
    let db = common::TestDb::new();
    let f = seed(&db);
    let before = newest(&f);

    // The stored title sent back, over REST and over sync push.
    patch(&f, json!({ "title": "Printer offline" })).await;
    push(&f, json!({ "title": "Printer offline" })).await;
    // Tags set to what they are (none).
    call(
        &f,
        Method::PUT,
        &format!("/api/tickets/{}/tags", f.ticket),
        json!({ "tag_ids": [] }),
    )
    .await;

    let events = events_after(&f, before);
    assert!(events.is_empty(), "no-op updates recorded {events:?}");
}

#[actix_web::test]
async fn a_tag_change_lists_tag_ids() {
    let db = common::TestDb::new();
    let f = seed(&db);
    let before = newest(&f);

    call(
        &f,
        Method::PUT,
        &format!("/api/tickets/{}/tags", f.ticket),
        json!({ "tag_ids": [f.tags[0]] }),
    )
    .await;

    let events = events_after(&f, before);
    assert_eq!(events.len(), 1, "{events:?}");
    assert_eq!(events[0].event_type, "ticket.tags_changed");
    assert_eq!(changed_fields(&events[0]), ["ticket.tag_ids"]);
    assert_eq!(events[0].data["tag_ids"], json!([f.tags[0]]));
    assert_eq!(events[0].data["added"], json!([f.tags[0]]));
}

#[actix_web::test]
async fn a_sync_push_update_lists_what_it_set() {
    let db = common::TestDb::new();
    let f = seed(&db);

    let before = newest(&f);
    push(&f, json!({ "priority": "urgent" })).await;
    let events = events_after(&f, before);
    assert_eq!(events.len(), 1, "{events:?}");
    assert_eq!(events[0].event_type, "ticket.priority_changed");
    assert_eq!(changed_fields(&events[0]), ["ticket.priority"]);

    // Scalar fields and tags in one push are two writes, as before: the
    // update's event and the tag event, each naming its own fields.
    let before = newest(&f);
    push(
        &f,
        json!({
            "workflow_state_id": f.active_state,
            "assignee_uuid": f.agent,
            "tag_ids": f.tags,
        }),
    )
    .await;
    let events = events_after(&f, before);
    let kinds: Vec<&str> = events.iter().map(|e| e.event_type.as_str()).collect();
    assert_eq!(
        kinds,
        ["ticket.workflow_state_changed", "ticket.tags_changed"],
        "{events:?}"
    );
    assert_eq!(
        changed_fields(&events[0]),
        [
            "ticket.assignee_uuid",
            "ticket.workflow_state.category",
            "ticket.workflow_state_id",
        ]
    );
    assert_eq!(changed_fields(&events[1]), ["ticket.tag_ids"]);
}
