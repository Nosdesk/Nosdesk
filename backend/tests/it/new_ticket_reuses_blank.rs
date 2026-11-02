//! "New ticket" (`POST /api/tickets/empty`) gives the caller their blank
//! ticket: the one an earlier press made, while nothing has touched it, and a
//! new one otherwise. Presses that arrive together get the same ticket.

#![allow(clippy::expect_used)]

use std::sync::{Arc, Barrier};

use actix_web::dev::Service as _;
use actix_web::http::StatusCode;
use actix_web::test as http_test;
use actix_web::{web, App, HttpMessage as _};
use diesel::prelude::*;
use serde_json::Value;
use uuid::Uuid;

use backend::extractors::WorkspaceContext;
use backend::middleware::RequestContext;
use backend::models::{Claims, NewComment, TicketUpdate};
use backend::schema::tickets;
use backend::services::search::SearchService;
use backend::services::ticket_updates::{blank_ticket, ActorConn, BlankTicket};
use backend::sync::actor::ActorContext;
use backend::sync::session::with_actor_context;

use crate::common;

fn claims_for(user: Uuid) -> Claims {
    let now = chrono::Utc::now().timestamp();
    Claims {
        sub: user.to_string(),
        name: "Someone".to_string(),
        email: "someone@example.com".to_string(),
        platform_role: "user".to_string(),
        scope: "full".to_string(),
        sid: None,
        workspace_uuid: None,
        exp: (now + 3600) as usize,
        iat: now as usize,
    }
}

/// Presses "New ticket" as `user`: the status and the ticket's id.
macro_rules! press_new_ticket {
    ($pool:expr, $search:expr, $ws:expr, $user:expr) => {{
        let ws: &common::WorkspaceSeed = $ws;
        let workspace = WorkspaceContext {
            workspace_id: ws.workspace_id,
            workspace_uuid: ws.workspace_uuid,
            slug: ws.slug.clone(),
            name: "A".to_string(),
            organisation_id: None,
            custom_domain: None,
        };
        let claims = claims_for($user);
        let corr = Uuid::now_v7();
        let actor = ActorContext::user($user, Some(corr)).with_workspace(ws.workspace_id);
        let app = http_test::init_service(
            App::new()
                .app_data(web::Data::new($pool.clone()))
                .app_data(web::Data::new($search.clone()))
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
        let resp = http_test::call_service(
            &app,
            http_test::TestRequest::post()
                .uri("/api/tickets/empty")
                .to_request(),
        )
        .await;
        let status = resp.status();
        let body: Value = http_test::read_body_json(resp).await;
        (status, body["id"].as_i64().expect("ticket id") as i32)
    }};
}

fn as_user<T>(
    pool: &common::TestPool,
    ws: &common::WorkspaceSeed,
    user: Uuid,
    f: impl FnOnce(&mut backend::db::DbConnection) -> QueryResult<T>,
) -> T {
    let actor = ActorContext::user(user, None).with_workspace(ws.workspace_id);
    with_actor_context(&mut pool.get().expect("conn"), &actor, f).expect("as user")
}

fn tickets_of(pool: &common::TestPool, ws: &common::WorkspaceSeed, user: Uuid) -> i64 {
    as_user(pool, ws, user, |c| {
        tickets::table
            .filter(tickets::requester_uuid.eq(user))
            .count()
            .get_result(c)
    })
}

#[actix_web::test]
async fn new_ticket_opens_the_blank_ticket_until_it_is_touched() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let pool = db.pool_with_size(4);
    let seeded = common::seed_two_workspaces(&mut pool.get().expect("conn"));
    let a = &seeded.a;
    let search_dir = tempfile::tempdir().expect("search dir");
    let search = Arc::new(SearchService::new(search_dir.path(), &pool).expect("init search"));

    let (status, first) = press_new_ticket!(pool, search, a, a.admin_uuid);
    assert_eq!(status, StatusCode::CREATED);

    // Pressed again, it opens the same ticket and makes none.
    let (status, again) = press_new_ticket!(pool, search, a, a.admin_uuid);
    assert_eq!(status, StatusCode::OK);
    assert_eq!(again, first, "the blank ticket comes back");
    assert_eq!(tickets_of(&pool, a, a.admin_uuid), 1);

    // Someone else gets their own.
    let (status, theirs) = press_new_ticket!(pool, search, a, a.member_uuid);
    assert_eq!(status, StatusCode::CREATED);
    assert_ne!(theirs, first);

    // A title makes it a ticket: the next press makes a new one.
    as_user(&pool, a, a.admin_uuid, |c| {
        backend::repository::tickets::update_ticket_partial(
            c,
            first,
            TicketUpdate {
                title: Some("Printer jam".into()),
                ..Default::default()
            },
            None,
        )
        .map_err(|e| diesel::result::Error::QueryBuilderError(format!("{e:?}").into()))
    });
    let (status, second) = press_new_ticket!(pool, search, a, a.admin_uuid);
    assert_eq!(status, StatusCode::CREATED);
    assert_ne!(second, first);

    // So does a comment, though it leaves the title alone.
    as_user(&pool, a, a.admin_uuid, |c| {
        backend::repository::comments::create_comment(
            c,
            NewComment {
                content: "<p>It's jammed again</p>".into(),
                ticket_id: second,
                user_uuid: a.admin_uuid,
                ..Default::default()
            },
            None,
        )
    });
    let (status, third) = press_new_ticket!(pool, search, a, a.admin_uuid);
    assert_eq!(status, StatusCode::CREATED);
    assert_ne!(third, second);
    assert_eq!(tickets_of(&pool, a, a.admin_uuid), 3);
}

/// Two presses together, as from an app without the single-flight Create
/// button: both get the one ticket.
#[test]
fn presses_that_arrive_together_get_one_ticket() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let pool = db.pool_with_size(4);
    let seeded = common::seed_two_workspaces(&mut pool.get().expect("conn"));
    let a = seeded.a.clone();

    let together = Arc::new(Barrier::new(2));
    let presses: Vec<_> = (0..2)
        .map(|_| {
            let pool = pool.clone();
            let together = together.clone();
            let actor = ActorContext::user(a.admin_uuid, None).with_workspace(a.workspace_id);
            let requester = a.admin_uuid;
            std::thread::spawn(move || {
                let mut conn = pool.get().expect("conn");
                together.wait();
                with_actor_context(&mut conn, &actor, |c| {
                    match blank_ticket(
                        &mut ActorConn {
                            conn: c,
                            actor: &actor,
                        },
                        requester,
                        None,
                    )? {
                        BlankTicket::Reused(t) | BlankTicket::Made(t, _) => {
                            Ok::<_, diesel::result::Error>(t.id)
                        }
                    }
                })
                .expect("press")
            })
        })
        .collect();
    let ids: Vec<i32> = presses
        .into_iter()
        .map(|p| p.join().expect("thread"))
        .collect();
    assert_eq!(ids[0], ids[1], "both presses opened one ticket");
    assert_eq!(tickets_of(&pool, &a, a.admin_uuid), 1);
}

/// The assignment rules assigning the new ticket are part of making it: it is
/// still blank, and the next press opens it.
#[actix_web::test]
async fn a_blank_ticket_the_rules_assigned_is_still_blank() {
    use backend::models::{AssignmentMethod, NewAssignmentRule};
    use backend::schema::assignment_rules;

    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let pool = db.pool_with_size(4);
    let seeded = common::seed_two_workspaces(&mut pool.get().expect("conn"));
    let a = &seeded.a;
    let search_dir = tempfile::tempdir().expect("search dir");
    let search = Arc::new(SearchService::new(search_dir.path(), &pool).expect("init search"));
    as_user(&pool, a, a.admin_uuid, |c| {
        diesel::insert_into(assignment_rules::table)
            .values(&NewAssignmentRule {
                name: "Everything to the admin".into(),
                description: None,
                priority: 1,
                is_active: true,
                method: AssignmentMethod::DirectUser,
                target_user_uuid: Some(a.admin_uuid),
                target_group_id: None,
                trigger_on_create: true,
                trigger_on_category_change: false,
                category_id: None,
                conditions: None,
                created_by: Some(a.admin_uuid),
            })
            .execute(c)
    });

    let (status, first) = press_new_ticket!(pool, search, a, a.member_uuid);
    assert_eq!(status, StatusCode::CREATED);
    let assignee: Option<Uuid> = as_user(&pool, a, a.admin_uuid, |c| {
        tickets::table
            .find(first)
            .select(tickets::assignee_uuid)
            .first(c)
    });
    assert_eq!(assignee, Some(a.admin_uuid), "the rule assigned it");

    let (status, again) = press_new_ticket!(pool, search, a, a.member_uuid);
    assert_eq!(status, StatusCode::OK);
    assert_eq!(again, first);
}
