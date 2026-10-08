//! A merged ticket refuses changes through the agent REST routes: its fields,
//! tags, links, watching and replies (409 `ticket_merged`). Unwatching stays
//! allowed, so someone can stop following a merged ticket. A write racing the
//! merge is refused too, once the merge commits.

use std::sync::Arc;

use actix_web::dev::Service;
use actix_web::test as http_test;
use actix_web::{web, App, HttpMessage};
use diesel::prelude::*;
use serde_json::json;
use uuid::Uuid;

use backend::extractors::WorkspaceContext;
use backend::middleware::RequestContext;
use backend::models::{Claims, NewTicket, Ticket, TicketUpdate, WorkflowStateCategory};
use backend::repository::ticket_merge::{execute_merge, MergeInput};
use backend::repository::tickets::TicketWriteError;
use backend::services::outbound_email::OutboundEmailResolver;
use backend::services::search::SearchService;
use backend::sync::actor::ActorContext;
use backend::sync::session::{run_in_workspace, set_actor, with_actor_context};
use backend::utils::storage::{create_storage, Storage, StorageConfig};

use crate::common;

const REF: &str = "test:merged_ticket_writes";

#[actix_web::test]
async fn a_merged_ticket_refuses_rest_writes_but_can_be_unwatched() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let pool = db.pool_with_size(4);
    let seeded = common::seed_two_workspaces(&mut pool.get().expect("conn")).a;
    let ws = seeded.workspace_id;
    let admin = seeded.admin_uuid;
    let ticket = |title: &str| -> Ticket {
        run_in_workspace(&pool, REF, ws, |c| {
            let state = backend::repository::workflow_states::default_state(c)?;
            diesel::insert_into(backend::schema::tickets::table)
                .values(&NewTicket {
                    title: title.to_string(),
                    workflow_state_id: state.id,
                    ..Default::default()
                })
                .get_result(c)
        })
        .expect("insert ticket")
    };
    let destination = ticket("VPN drops");
    let source = ticket("VPN keeps dropping");
    let other = ticket("Wi-Fi slow");
    // Watching the source before the merge, to unwatch it after.
    run_in_workspace(&pool, REF, ws, |c| {
        backend::repository::ticket_watchers::add_watcher(c, source.id, admin, false)
    })
    .expect("watch");
    execute_merge(
        &mut pool.get().expect("conn"),
        MergeInput {
            destination_ticket_id: destination.id,
            source_ticket_ids: vec![source.id],
            reason: None,
            notify_customer: false,
            expected_state: Vec::new(),
            marker_body: None,
        },
        &ActorContext::user(admin, None).with_workspace(ws),
    )
    .expect("merge");

    let dir = tempfile::tempdir().expect("storage dir");
    let storage: Arc<dyn Storage> = create_storage(StorageConfig::Local {
        base_path: dir.path().to_string_lossy().into_owned(),
    });
    let search_dir = tempfile::tempdir().expect("search dir");
    let search = Arc::new(SearchService::new(search_dir.path(), &pool).expect("init search"));
    let workspace = WorkspaceContext {
        workspace_id: ws,
        workspace_uuid: seeded.workspace_uuid,
        slug: seeded.slug.clone(),
        name: "A".to_string(),
        organisation_id: None,
        custom_domain: None,
    };
    let claims = Claims {
        sub: admin.to_string(),
        name: "Admin".to_string(),
        email: "admin@example.com".to_string(),
        platform_role: "user".to_string(),
        scope: "full".to_string(),
        sid: None,
        workspace_uuid: None,
        exp: (chrono::Utc::now().timestamp() + 3600) as usize,
        iat: chrono::Utc::now().timestamp() as usize,
    };
    let corr = Uuid::now_v7();
    let actor = ActorContext::user(admin, Some(corr)).with_workspace(ws);
    let app = http_test::init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .app_data(web::Data::new(storage))
            .app_data(web::Data::new(search))
            .app_data(web::Data::new(Arc::new(OutboundEmailResolver::new(
                pool.clone(),
                None,
            ))))
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

    let s = source.id;
    let refused = [
        http_test::TestRequest::patch()
            .uri(&format!("/api/tickets/{s}"))
            .set_json(json!({ "title": "Renamed after the merge" })),
        http_test::TestRequest::put()
            .uri(&format!("/api/tickets/{s}/tags"))
            .set_json(json!({ "tag_ids": [] })),
        http_test::TestRequest::post().uri(&format!("/api/tickets/{s}/watch")),
        http_test::TestRequest::patch()
            .uri(&format!("/api/tickets/{s}/watch/preferences"))
            .set_json(json!({ "notify_on_internal_notes": false })),
        http_test::TestRequest::post().uri(&format!("/api/tickets/{s}/link/{}", other.id)),
        http_test::TestRequest::post().uri(&format!("/api/tickets/{}/link/{s}", other.id)),
        http_test::TestRequest::delete().uri(&format!("/api/tickets/{s}/unlink/{}", other.id)),
        http_test::TestRequest::post()
            .uri(&format!("/api/tickets/{s}/comments"))
            .set_json(json!({ "content": "Any update?", "attachments": [] })),
    ];
    for request in refused {
        let request = request.to_request();
        let route = format!("{} {}", request.method(), request.path());
        let resp = http_test::call_service(&app, request).await;
        assert_eq!(resp.status().as_u16(), 409, "{route} on a merged ticket");
        let body: serde_json::Value = http_test::read_body_json(resp).await;
        assert_eq!(body["code"], "ticket_merged", "{route}: {body}");
    }

    let unwatch = http_test::call_service(
        &app,
        http_test::TestRequest::delete()
            .uri(&format!("/api/tickets/{s}/watch"))
            .to_request(),
    )
    .await;
    assert!(
        unwatch.status().is_success(),
        "unwatch: {}",
        unwatch.status()
    );

    let after: Ticket = run_in_workspace(&pool, REF, ws, |c| {
        backend::schema::tickets::table.find(s).first(c)
    })
    .expect("reload");
    assert_eq!(after.title, "VPN keeps dropping");
}

/// A write that reaches a ticket while it's being merged waits for the merge,
/// then is refused, instead of landing on the ticket after the merge commits.
#[test]
fn a_write_racing_a_merge_is_refused_once_the_merge_commits() {
    let db = common::TestDb::new();
    let pool = db.pool_with_size(3);
    let seeded = common::seed_two_workspaces(&mut pool.get().expect("conn")).a;
    let ws = seeded.workspace_id;
    let admin = ActorContext::user(seeded.admin_uuid, None).with_workspace(ws);
    let ticket = |title: &str| -> Ticket {
        run_in_workspace(&pool, REF, ws, |c| {
            let state = backend::repository::workflow_states::default_state(c)?;
            diesel::insert_into(backend::schema::tickets::table)
                .values(&NewTicket {
                    title: title.to_string(),
                    workflow_state_id: state.id,
                    ..Default::default()
                })
                .get_result(c)
        })
        .expect("insert ticket")
    };
    let destination = ticket("Disk full");
    let source = ticket("Disk full again");

    // A merge part way through: the source is moved to merged and the merge
    // recorded, not yet committed.
    let mut merging = pool.get().expect("conn");
    diesel::sql_query("BEGIN")
        .execute(&mut merging)
        .expect("begin");
    set_actor(&mut merging, &admin).expect("actor");
    let merged_state = backend::repository::workflow_states::first_in_category(
        &mut merging,
        WorkflowStateCategory::Merged,
    )
    .expect("merged state");
    diesel::update(backend::schema::tickets::table.find(source.id))
        .set(backend::schema::tickets::workflow_state_id.eq(merged_state.id))
        .execute(&mut merging)
        .expect("move to merged");
    {
        use backend::schema::ticket_merges;
        diesel::insert_into(ticket_merges::table)
            .values((
                ticket_merges::ticket_id.eq(source.id),
                ticket_merges::merged_into_ticket_id.eq(destination.id),
                ticket_merges::merged_at.eq(chrono::Utc::now()),
                ticket_merges::merged_by_user_uuid.eq(seeded.admin_uuid),
            ))
            .execute(&mut merging)
            .expect("record the merge");
    }

    let writer = {
        let pool = pool.clone();
        let admin = admin.clone();
        let id = source.id;
        std::thread::spawn(move || {
            let mut conn = pool.get().expect("conn");
            diesel::sql_query("SET lock_timeout = '10s'")
                .execute(&mut conn)
                .expect("lock timeout");
            with_actor_context(&mut conn, &admin, |c| {
                backend::repository::tickets::update_ticket_partial(
                    c,
                    id,
                    TicketUpdate {
                        title: Some("Renamed mid-merge".to_string()),
                        ..Default::default()
                    },
                    None,
                )
            })
        })
    };
    // Commit the merge once the write is waiting on it.
    #[derive(QueryableByName)]
    struct Waiting {
        #[diesel(sql_type = diesel::sql_types::BigInt)]
        n: i64,
    }
    let started = std::time::Instant::now();
    loop {
        let waiting: Waiting = diesel::sql_query(
            "SELECT count(*) AS n FROM pg_stat_activity \
             WHERE datname = current_database() AND wait_event_type = 'Lock'",
        )
        .get_result(&mut pool.get().expect("conn"))
        .expect("waiting");
        if waiting.n > 0 {
            break;
        }
        assert!(
            started.elapsed() < std::time::Duration::from_secs(10),
            "the write never waited on the merge"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    diesel::sql_query("COMMIT")
        .execute(&mut merging)
        .expect("commit the merge");

    let written = writer.join().expect("writer");
    assert!(
        matches!(written, Err(TicketWriteError::Merged)),
        "the write after the merge: {written:?}"
    );
    let after: Ticket = run_in_workspace(&pool, REF, ws, |c| {
        backend::schema::tickets::table.find(source.id).first(c)
    })
    .expect("reload");
    assert_eq!(after.title, "Disk full again");
}
