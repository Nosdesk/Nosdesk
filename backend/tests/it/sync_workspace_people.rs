//! A workspace's sync feed and people lists carry only its own people.
//!
//! `users` and `user_emails` have no row security (an account can belong to
//! several workspaces), so every read of them that serves one workspace has to
//! say who that workspace's people are. These drive the bootstrap stream, the
//! read-side filter the delta and live stream share, and the people lists the
//! pickers use, with two workspaces side by side.

use std::collections::HashSet;

use actix_web::dev::Service;
use actix_web::http::StatusCode;
use actix_web::test as http_test;
use actix_web::{web, App, HttpMessage};
use diesel::prelude::*;
use uuid::Uuid;

use backend::extractors::WorkspaceContext;
use backend::middleware::RequestContext;
use backend::models::{
    Claims, NewComment, NewTicket, NewUserEmail, SyncAggregate, SyncOp, UserUpdate,
};
use backend::repository::workspaces::{add_membership, SeatWriteAuthority};
use backend::sync::actor::ActorContext;
use backend::sync::session::{run_in_workspace, with_actor_context};
use backend::sync::visibility::{filter_actions, filter_actions_pinned, ActionView, SyncViewer};

use crate::common::{self, TestPool, TwoWorkspaces, WorkspaceSeed};

const REF: &str = "test:sync_workspace_people";

struct Fixture {
    pool: TestPool,
    ws: TwoWorkspaces,
    /// Belongs to B only, and is the requester of a ticket in A.
    requester_from_b: Uuid,
    /// Belongs to B only, and replied on A's ticket.
    commenter_from_b: Uuid,
}

fn email_of(uuid: Uuid) -> String {
    format!("{}@people.test", uuid.simple())
}

fn add_primary_email(conn: &mut backend::db::DbConnection, uuid: Uuid) {
    diesel::insert_into(backend::schema::user_emails::table)
        .values(&NewUserEmail {
            user_uuid: uuid,
            email: email_of(uuid),
            email_type: "personal".to_string(),
            is_primary: true,
            is_verified: true,
            source: None,
        })
        .execute(conn)
        .expect("insert primary email");
}

fn seed(db: &common::TestDb) -> Fixture {
    common::ensure_test_keyring();
    let pool = db.pool_with_size(4);
    let mut conn = pool.get().expect("conn");
    let ws = common::seed_two_workspaces(&mut conn);
    let (a, b) = (ws.a.workspace_id, ws.b.workspace_id);

    let requester_from_b = common::insert_plain_user(&mut conn, "Requester From B");
    let commenter_from_b = common::insert_plain_user(&mut conn, "Commenter From B");
    with_actor_context::<_, diesel::result::Error>(
        &mut conn,
        &ActorContext::user(ws.b.admin_uuid, None).with_workspace(b),
        |c| {
            for uuid in [requester_from_b, commenter_from_b] {
                add_membership(c, b, uuid, "member", SeatWriteAuthority::ControlPlane)?;
            }
            Ok(())
        },
    )
    .expect("join B");

    for uuid in [
        ws.a.admin_uuid,
        ws.a.member_uuid,
        ws.b.admin_uuid,
        ws.b.member_uuid,
        requester_from_b,
        commenter_from_b,
    ] {
        add_primary_email(&mut conn, uuid);
    }

    // A ticket in A requested by someone from B, with a reply from another.
    run_in_workspace(&pool, REF, a, |c| {
        let state = backend::repository::workflow_states::default_state(c)?;
        let ticket: backend::models::Ticket = diesel::insert_into(backend::schema::tickets::table)
            .values(&NewTicket {
                title: "Shared printer".to_string(),
                workflow_state_id: state.id,
                requester_uuid: Some(requester_from_b),
                ..Default::default()
            })
            .get_result(c)?;
        diesel::insert_into(backend::schema::comments::table)
            .values(&NewComment {
                content: "Same here".to_string(),
                ticket_id: ticket.id,
                user_uuid: commenter_from_b,
                is_internal: false,
                ..Default::default()
            })
            .execute(c)?;
        Ok(())
    })
    .expect("seed A's ticket");

    Fixture {
        pool,
        ws,
        requester_from_b,
        commenter_from_b,
    }
}

/// GET `uri` as `viewer`, with `seed`'s workspace resolved for the request.
async fn call_as(
    pool: &TestPool,
    seed: &WorkspaceSeed,
    viewer: Uuid,
    uri: &str,
) -> (StatusCode, String) {
    let claims = Claims {
        sub: viewer.to_string(),
        name: "Viewer".to_string(),
        email: email_of(viewer),
        platform_role: "user".to_string(),
        scope: "full".to_string(),
        sid: None,
        workspace_uuid: None,
        exp: (chrono::Utc::now().timestamp() + 3600) as usize,
        iat: chrono::Utc::now().timestamp() as usize,
    };
    let workspace = WorkspaceContext {
        workspace_id: seed.workspace_id,
        workspace_uuid: seed.workspace_uuid,
        slug: seed.slug.clone(),
        name: seed.slug.clone(),
        organisation_id: None,
        custom_domain: None,
    };
    let corr = Uuid::now_v7();
    let actor = ActorContext::user(viewer, Some(corr)).with_workspace(seed.workspace_id);
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
            .service(
                web::scope("/api")
                    .configure(backend::handlers::sync::config)
                    .route("/users", web::get().to(backend::handlers::get_users))
                    .route(
                        "/users/paginated",
                        web::get().to(backend::handlers::get_paginated_users),
                    )
                    .route(
                        "/users/{uuid}/profile",
                        web::get().to(backend::handlers::users::get_user_profile_bundle),
                    ),
            ),
    )
    .await;
    let resp =
        http_test::call_service(&app, http_test::TestRequest::get().uri(uri).to_request()).await;
    let status = resp.status();
    let body = String::from_utf8(http_test::read_body(resp).await.to_vec()).expect("utf8 body");
    (status, body)
}

/// [`call_as`], expecting 200.
async fn get_as(pool: &TestPool, seed: &WorkspaceSeed, viewer: Uuid, uri: &str) -> String {
    let (status, body) = call_as(pool, seed, viewer, uri).await;
    assert_eq!(status, StatusCode::OK, "GET {uri}: {body}");
    body
}

fn uuids_in(values: &[serde_json::Value]) -> HashSet<Uuid> {
    values
        .iter()
        .filter_map(|u| u.get("uuid").and_then(|v| v.as_str()))
        .map(|s| Uuid::parse_str(s).expect("uuid"))
        .collect()
}

/// A's own people are sent, including the people from elsewhere that A's
/// tickets name; B's people are not, and neither are their addresses.
fn assert_only_a_people(f: &Fixture, sent: &HashSet<Uuid>, body: &str, what: &str) {
    for (uuid, who) in [
        (f.ws.a.admin_uuid, "A's admin"),
        (f.ws.a.member_uuid, "A's member"),
        (f.requester_from_b, "the requester of A's ticket"),
    ] {
        assert!(sent.contains(&uuid), "{what}: {who} is one of A's people");
    }
    for (uuid, who) in [
        (f.ws.b.admin_uuid, "B's admin"),
        (f.ws.b.member_uuid, "B's member"),
    ] {
        assert!(!sent.contains(&uuid), "{what}: {who} belongs only to B");
        assert!(
            !body.contains(&email_of(uuid)),
            "{what}: {who}'s address must not be sent to A"
        );
    }
}

#[actix_web::test]
async fn bootstrap_sends_only_the_workspaces_people() {
    let db = common::TestDb::new();
    let f = seed(&db);
    let a = &f.ws.a;

    let body = get_as(
        &f.pool,
        a,
        a.member_uuid,
        &format!(
            "/api/sync/bootstrap?groups=workspace:{}&schema=test",
            a.workspace_id
        ),
    )
    .await;
    let lines: Vec<serde_json::Value> = body
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).expect("NDJSON line"))
        .collect();
    assert!(
        lines.iter().any(|l| l.get("__end__").is_some()),
        "the stream completes: {body}"
    );
    let users: Vec<serde_json::Value> = lines
        .into_iter()
        .filter(|l| l.get("__model__").and_then(|m| m.as_str()) == Some("user"))
        .collect();
    let sent = uuids_in(&users);

    assert_only_a_people(&f, &sent, &body, "bootstrap");
    assert!(
        sent.contains(&f.commenter_from_b),
        "bootstrap: the author of a reply on A's ticket renders by name"
    );
}

#[actix_web::test]
async fn people_lists_hold_only_the_workspaces_people() {
    let db = common::TestDb::new();
    let f = seed(&db);
    let a = &f.ws.a;

    let body = get_as(
        &f.pool,
        a,
        a.admin_uuid,
        "/api/users/paginated?pageSize=100",
    )
    .await;
    let page: serde_json::Value = serde_json::from_str(&body).expect("json");
    let rows = page["data"].as_array().expect("data").clone();
    assert_only_a_people(&f, &uuids_in(&rows), &body, "/users/paginated");
    assert_eq!(
        page["total"].as_i64(),
        Some(rows.len() as i64),
        "the total counts the same people the page lists"
    );

    let body = get_as(&f.pool, a, a.admin_uuid, "/api/users").await;
    let rows: Vec<serde_json::Value> = serde_json::from_str(&body).expect("json");
    assert_only_a_people(&f, &uuids_in(&rows), &body, "/users");

    // The profile page an admin opens on someone: one of A's people, or not
    // found.
    let profile = |who: Uuid| format!("/api/users/{who}/profile?include=");
    let (status, body) = call_as(&f.pool, a, a.admin_uuid, &profile(a.member_uuid)).await;
    assert_eq!(status, StatusCode::OK, "a colleague's profile: {body}");
    let (status, body) = call_as(&f.pool, a, a.admin_uuid, &profile(f.ws.b.member_uuid)).await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "the profile of someone only in B: {body}"
    );
    assert!(!body.contains(&email_of(f.ws.b.member_uuid)));
}

type UserRow = (SyncAggregate, SyncOp, String, serde_json::Value);

/// The `user` rows A's feed holds, oldest first.
fn a_user_rows(f: &Fixture) -> Vec<UserRow> {
    let a = f.ws.a.workspace_id;
    run_in_workspace(&f.pool, REF, a, |c| {
        use backend::schema::sync_actions::dsl as s;
        s::sync_actions
            .filter(s::workspace_id.eq(a))
            .filter(s::aggregate.eq(SyncAggregate::User))
            .order(s::sync_id.asc())
            .select((s::aggregate, s::op, s::aggregate_id, s::data))
            .load(c)
    })
    .expect("A's user rows")
}

/// The rows `viewer`'s sessions in A receive, through the read-side filter
/// the delta runs on its request connection and the live stream runs on a
/// pinned pool connection. The two must agree.
fn delivered_in_a(f: &Fixture, viewer: Uuid, rows: &[UserRow]) -> Vec<UserRow> {
    let a = f.ws.a.workspace_id;
    let views: Vec<ActionView> = rows
        .iter()
        .map(|(agg, op, id, data)| ActionView::from_row(*agg, *op, id, data))
        .collect();
    let mut conn = f.pool.get().expect("conn");
    let (viewer, delta) = with_actor_context::<_, diesel::result::Error>(
        &mut conn,
        &ActorContext::user(viewer, None).with_workspace(a),
        |c| {
            let me = backend::repository::users::get_user_by_uuid(&viewer, c)?;
            let viewer = SyncViewer::resolve(c, &me);
            let keep = filter_actions(c, &viewer, &views, |v: &ActionView| v.clone());
            Ok((viewer, keep))
        },
    )
    .expect("delta filter");
    let live = filter_actions_pinned(&f.pool, a, &viewer, &views, |v: &ActionView| v.clone());
    assert_eq!(delta, live, "the delta and the live stream agree");
    rows.iter()
        .zip(delta)
        .filter(|(_, keep)| *keep)
        .map(|(row, _)| row.clone())
        .collect()
}

fn about(rows: &[UserRow], who: Uuid) -> bool {
    rows.iter().any(|r| r.2 == who.to_string())
}

/// An edit made while pinned to A (an operator working from A, say) records
/// its `user.updated` in A's feed whoever it is about. The read-side filter
/// the delta and the live stream share hands A's sessions only the changes to
/// A's own people: a colleague, and someone from B whom A's ticket names.
#[test]
fn user_changes_reach_only_the_workspaces_sessions() {
    let db = common::TestDb::new();
    let f = seed(&db);
    let a = f.ws.a.workspace_id;
    let (a_member, b_member) = (f.ws.a.member_uuid, f.ws.b.member_uuid);
    let mut conn = f.pool.get().expect("conn");

    let rename = |name: &str| UserUpdate {
        name: Some(name.to_string()),
        pronouns: None,
        avatar_url: None,
        banner_url: None,
        avatar_thumb: None,
        microsoft_uuid: None,
        updated_at: None,
    };
    with_actor_context::<_, diesel::result::Error>(
        &mut conn,
        &ActorContext::user(f.ws.a.admin_uuid, None).with_workspace(a),
        |c| {
            for (who, name) in [
                (b_member, "B renamed"),
                (a_member, "A renamed"),
                (f.requester_from_b, "Requester renamed"),
            ] {
                backend::repository::users::update_user(&who, rename(name), c, None)?;
            }
            Ok(())
        },
    )
    .expect("edit all three from A");

    let rows = a_user_rows(&f);
    assert!(
        about(&rows, b_member),
        "the edit to B's member is recorded in A's feed (the case under test)"
    );

    // A's admin: none of the rows is about them, so each goes through the
    // workspace's people.
    let delivered = delivered_in_a(&f, f.ws.a.admin_uuid, &rows);
    assert!(
        about(&delivered, a_member),
        "a change to a colleague reaches A's sessions"
    );
    assert!(
        about(&delivered, f.requester_from_b),
        "a change to the requester of A's ticket reaches A's sessions"
    );
    assert!(
        !about(&delivered, b_member),
        "a change to someone only in B must not reach A's sessions"
    );
}

/// A purge removes the person from every record that made them one of the
/// workspace's people before it records `user.deleted`. The delete still has
/// to reach every session that may hold the row, or the purged name and
/// address stay in pools and caches; so it names only the row.
#[test]
fn a_purge_reaches_colleagues_sessions_as_a_bare_prune() {
    let db = common::TestDb::new();
    let f = seed(&db);
    let (a, gone) = (f.ws.a.workspace_id, f.ws.a.member_uuid);

    let mut conn = f.pool.get().expect("conn");
    with_actor_context::<_, diesel::result::Error>(
        &mut conn,
        &ActorContext::user(f.ws.a.admin_uuid, None).with_workspace(a),
        |c| backend::repository::users::purge_user(&gone, c, None).map(|_| ()),
    )
    .expect("purge A's member");

    let deletes: Vec<UserRow> = a_user_rows(&f)
        .into_iter()
        .filter(|r| r.1 == SyncOp::Delete && r.2 == gone.to_string())
        .collect();
    assert_eq!(deletes.len(), 1, "the purge records one delete");

    let delivered = delivered_in_a(&f, f.ws.a.admin_uuid, &deletes);
    assert_eq!(
        delivered.len(),
        1,
        "the delete reaches a colleague's sessions"
    );
    assert_eq!(
        delivered[0].3,
        serde_json::json!({ "uuid": gone }),
        "the delete names only the row"
    );
}
