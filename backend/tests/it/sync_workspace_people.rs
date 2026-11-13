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
        file_path: None,
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
        .map(|(agg, op, id, data)| ActionView::from_row(*agg, *op, id, data, &[]))
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

/// A feed row after a baseline: event type, aggregate id, data, groups.
type FeedRow = (String, String, serde_json::Value, Vec<Option<String>>);

/// The rows `workspace`'s feed holds after `after`, oldest first.
fn feed_after(f: &Fixture, workspace: i32, after: i64) -> Vec<FeedRow> {
    run_in_workspace(&f.pool, REF, workspace, |c| {
        use backend::schema::sync_actions::dsl as s;
        s::sync_actions
            .filter(s::workspace_id.eq(workspace))
            .filter(s::sync_id.gt(after))
            .order(s::sync_id.asc())
            .select((s::event_type, s::aggregate_id, s::data, s::groups))
            .load(c)
    })
    .expect("feed rows")
}

fn of_type<'a>(rows: &'a [FeedRow], event_type: &str, id: &str) -> Vec<&'a FeedRow> {
    rows.iter()
        .filter(|r| r.0 == event_type && r.1 == id)
        .collect()
}

/// A purge reaches every workspace that holds the person, each with the
/// changes to its own records: the person is B's member, A's requester, and
/// in B an assignee, a reply's author and an asset's user and manager. Run
/// as the retention job runs it, elevated and pinned to the home workspace.
/// Each workspace gets `user.deleted`, `comment.deleted` for the replies it
/// held and a `ticket.people_cleared` carrying only what was cleared on each
/// ticket; nothing about one workspace lands in the other's feed.
#[test]
fn a_purge_reaches_each_workspace_with_its_own_changes() {
    let db = common::TestDb::new();
    let f = seed(&db);
    let (a, b) = (f.ws.a.workspace_id, f.ws.b.workspace_id);
    let gone = f.requester_from_b;
    let mut conn = f.pool.get().expect("conn");
    // The pages the person wrote pass to A's admin, A having no owner.
    let successor = f.ws.a.admin_uuid;

    let a_ticket: i32 = run_in_workspace(&f.pool, REF, a, |c| {
        use backend::schema::tickets::dsl as t;
        t::tickets
            .filter(t::requester_uuid.eq(gone))
            .select(t::id)
            .first(c)
    })
    .expect("A's ticket");
    let a_page = page_in(&f.pool, a, "Printer setup", gone);
    let (b_ticket, b_comment, b_file, b_asset) = run_in_workspace(&f.pool, REF, b, |c| {
        let state = backend::repository::workflow_states::default_state(c)?;
        let ticket: backend::models::Ticket = diesel::insert_into(backend::schema::tickets::table)
            .values(&NewTicket {
                title: "Scanner jam".to_string(),
                workflow_state_id: state.id,
                requester_uuid: Some(f.ws.b.member_uuid),
                assignee_uuid: Some(gone),
                ..Default::default()
            })
            .get_result(c)?;
        let comment: i32 = diesel::insert_into(backend::schema::comments::table)
            .values(&NewComment {
                content: "On it".to_string(),
                ticket_id: ticket.id,
                user_uuid: gone,
                is_internal: false,
                ..Default::default()
            })
            .returning(backend::schema::comments::id)
            .get_result(c)?;
        let file: i32 = diesel::insert_into(backend::schema::attachments::table)
            .values(&backend::models::NewAttachment {
                url: "/uploads/tickets/scan.png".to_string(),
                name: "scan.png".to_string(),
                file_size: Some(10),
                mime_type: Some("image/png".to_string()),
                checksum: None,
                comment_id: Some(comment),
                uploaded_by: Some(gone),
                transcription: None,
            })
            .returning(backend::schema::attachments::id)
            .get_result(c)?;
        let asset: i32 = diesel::insert_into(backend::schema::assets::table)
            .values(&backend::models::NewAsset {
                name: "Scanner".to_string(),
                serial_number: None,
                manufacturer: None,
                model: None,
                location: None,
                notes: None,
                primary_user_uuid: Some(gone),
                purchase_date: None,
                asset_tag: None,
                kind: "device".to_string(),
                attributes: serde_json::json!({}),
                quantity: None,
                unit: None,
                external_sync_source: None,
                low_stock_threshold: None,
                created_by: None,
            })
            .returning(backend::schema::assets::id)
            .get_result(c)?;
        diesel::update(backend::schema::assets::table.find(asset))
            .set(backend::schema::assets::managed_by_user_uuid.eq(gone))
            .execute(c)?;
        Ok((ticket.id, comment, file, asset))
    })
    .expect("seed B's records");

    let baseline: i64 = [a, b]
        .into_iter()
        .map(|ws| {
            run_in_workspace(&f.pool, REF, ws, |c| {
                use backend::schema::sync_actions::dsl as s;
                s::sync_actions
                    .filter(s::workspace_id.eq(ws))
                    .select(diesel::dsl::max(s::sync_id))
                    .first::<Option<i64>>(c)
            })
            .expect("baseline")
            .unwrap_or(0)
        })
        .max()
        .unwrap_or(0);

    let pin_after = backend::sync::session::with_actor_bypass_context::<_, diesel::result::Error>(
        &mut conn,
        &ActorContext::system(REF),
        |c| {
            backend::sync::session::pin_workspace(c, b)?;
            backend::repository::users::purge_user(&gone, c, None)?;
            backend::sync::session::current_workspace_id(c)
        },
    )
    .expect("purge");
    assert_eq!(pin_after, Some(b), "the purge puts the caller's pin back");

    let in_a = feed_after(&f, a, baseline);
    let in_b = feed_after(&f, b, baseline);
    for (rows, own, other) in [(&in_a, a, b), (&in_b, b, a)] {
        for row in rows {
            let groups: Vec<&str> = row.3.iter().flatten().map(String::as_str).collect();
            assert!(
                !groups.contains(&format!("workspace:{other}").as_str()),
                "a row in {own}'s feed names {other}: {row:?}"
            );
        }
    }

    let gone_id = gone.to_string();
    let (a_ticket_id, b_ticket_id) = (a_ticket.to_string(), b_ticket.to_string());
    for (name, rows) in [("A", &in_a), ("B", &in_b)] {
        let deletes = of_type(rows, "user.deleted", &gone_id);
        assert_eq!(deletes.len(), 1, "{name} hears the delete once");
        assert_eq!(deletes[0].2, serde_json::json!({ "uuid": gone }));
    }

    let cleared_a = of_type(&in_a, "ticket.people_cleared", &a_ticket_id);
    assert_eq!(cleared_a.len(), 1, "A's ticket loses its requester");
    assert_eq!(
        cleared_a[0].2,
        serde_json::json!({ "id": a_ticket, "requester_uuid": null }),
        "only the requester, which is all that changed on it"
    );
    let cleared_b = of_type(&in_b, "ticket.people_cleared", &b_ticket_id);
    assert_eq!(cleared_b.len(), 1, "B's ticket loses its assignee");
    assert_eq!(
        cleared_b[0].2,
        serde_json::json!({ "id": b_ticket, "assignee_uuid": null })
    );

    let replies = of_type(&in_b, "comment.deleted", &b_comment.to_string());
    assert_eq!(replies.len(), 1, "B hears the reply go");
    assert_eq!(
        replies[0].2,
        serde_json::json!({ "id": b_comment, "ticket_id": b_ticket })
    );
    assert_eq!(
        of_type(&in_b, "attachment.deleted", &b_file.to_string()).len(),
        1,
        "and the reply's file with it"
    );

    let asset = of_type(&in_b, "asset.updated", &b_asset.to_string());
    assert_eq!(asset.len(), 1, "B's asset loses its user and manager");
    assert!(asset[0].2["primary_user_uuid"].is_null());
    assert!(asset[0].2["managed_by_user_uuid"].is_null());

    let page = of_type(
        &in_a,
        "documentation_page.metadata_changed",
        &a_page.to_string(),
    );
    assert_eq!(page.len(), 1, "A's page passes to the successor");
    assert_eq!(page[0].2["created_by"], successor.to_string());
    assert_eq!(page[0].2["last_edited_by"], successor.to_string());

    // Each workspace's feed holds nothing about the other's records.
    let about_record = |rows: &[FeedRow], prefix: &str, id: i32| {
        rows.iter()
            .any(|r| r.0.starts_with(prefix) && r.1 == id.to_string())
    };
    assert!(!about_record(&in_a, "ticket.", b_ticket), "B's ticket in A");
    assert!(
        !about_record(&in_a, "comment.", b_comment),
        "B's reply in A"
    );
    assert!(!about_record(&in_a, "attachment.", b_file), "B's file in A");
    assert!(!about_record(&in_a, "asset.", b_asset), "B's asset in A");
    assert!(!about_record(&in_b, "ticket.", a_ticket), "A's ticket in B");
    assert!(
        !about_record(&in_b, "documentation_page.", a_page),
        "A's page in B"
    );
}

/// A published page in `workspace` written and last edited by `author`.
fn page_in(pool: &TestPool, workspace: i32, title: &str, author: Uuid) -> i32 {
    run_in_workspace(pool, REF, workspace, |c| {
        diesel::insert_into(backend::schema::documentation_pages::table)
            .values(&backend::models::NewDocumentationPage {
                uuid: Uuid::new_v4(),
                title: title.to_string(),
                slug: format!("page-{}", Uuid::new_v4().simple()),
                icon: None,
                cover_image: None,
                status: backend::models::DocumentationStatus::Published,
                created_by: author,
                last_edited_by: author,
                parent_id: None,
                display_order: None,
                is_public: false,
                is_template: false,
                yjs_state_vector: None,
                yjs_document: None,
                yjs_client_id: None,
                has_unsaved_changes: false,
            })
            .returning(backend::schema::documentation_pages::id)
            .get_result(c)
    })
    .expect("insert page")
}

/// No platform admin anywhere, as on a hosted deployment.
fn without_platform_admins(pool: &TestPool) {
    let mut conn = pool.get().expect("conn");
    diesel::update(
        backend::schema::users::table
            .filter(backend::schema::users::platform_role.eq("platform_admin")),
    )
    .set(backend::schema::users::platform_role.eq("user"))
    .execute(&mut conn)
    .expect("drop platform admins");
}

/// Purge `who` as the retention job does: elevated, pinned to `home`.
fn purge_from(pool: &TestPool, home: i32, who: Uuid) -> Result<usize, diesel::result::Error> {
    let mut conn = pool.get().expect("conn");
    backend::sync::session::with_actor_bypass_context(&mut conn, &ActorContext::system(REF), |c| {
        backend::sync::session::pin_workspace(c, home)?;
        backend::repository::users::purge_user(&who, c, None)
    })
}

/// Who a page names as its author and last editor now.
fn page_people(pool: &TestPool, workspace: i32, page: i32) -> (Uuid, Uuid) {
    run_in_workspace(pool, REF, workspace, |c| {
        use backend::schema::documentation_pages::dsl as d;
        d::documentation_pages
            .find(page)
            .select((d::created_by, d::last_edited_by))
            .first(c)
    })
    .expect("page people")
}

/// With no platform admin anywhere, a person's pages in each workspace pass to
/// that workspace's own owner, ahead of its admins, and each workspace hears
/// of its own page.
#[test]
fn a_purge_hands_each_workspaces_pages_to_its_own_owner() {
    let db = common::TestDb::new();
    let f = seed(&db);
    let (a, b) = (f.ws.a.workspace_id, f.ws.b.workspace_id);
    let gone = f.requester_from_b;
    without_platform_admins(&f.pool);

    let mut conn = f.pool.get().expect("conn");
    let owner_a = common::insert_plain_user(&mut conn, "Owner Of A");
    let owner_b = common::insert_plain_user(&mut conn, "Owner Of B");
    drop(conn);
    for (ws, owner, admin) in [
        (a, owner_a, f.ws.a.admin_uuid),
        (b, owner_b, f.ws.b.admin_uuid),
    ] {
        let mut conn = f.pool.get().expect("conn");
        with_actor_context::<_, diesel::result::Error>(
            &mut conn,
            &ActorContext::user(admin, None).with_workspace(ws),
            |c| add_membership(c, ws, owner, "owner", SeatWriteAuthority::ControlPlane).map(|_| ()),
        )
        .expect("add the owner");
    }
    let a_page = page_in(&f.pool, a, "Printer setup", gone);
    let b_page = page_in(&f.pool, b, "Scanner setup", gone);
    let baseline: i64 = [a, b]
        .into_iter()
        .map(|ws| {
            run_in_workspace(&f.pool, REF, ws, |c| {
                use backend::schema::sync_actions::dsl as s;
                s::sync_actions
                    .filter(s::workspace_id.eq(ws))
                    .select(diesel::dsl::max(s::sync_id))
                    .first::<Option<i64>>(c)
            })
            .expect("baseline")
            .unwrap_or(0)
        })
        .max()
        .unwrap_or(0);

    purge_from(&f.pool, b, gone).expect("the purge succeeds with no platform admin");

    assert_eq!(
        page_people(&f.pool, a, a_page),
        (owner_a, owner_a),
        "A's page goes to A's owner"
    );
    assert_eq!(
        page_people(&f.pool, b, b_page),
        (owner_b, owner_b),
        "B's page goes to B's owner"
    );
    for (name, ws, own, other, owner) in [
        ("A", a, a_page, b_page, owner_a),
        ("B", b, b_page, a_page, owner_b),
    ] {
        let rows = feed_after(&f, ws, baseline);
        let own_rows = of_type(
            &rows,
            "documentation_page.metadata_changed",
            &own.to_string(),
        );
        assert_eq!(own_rows.len(), 1, "{name} hears of its page once");
        assert_eq!(own_rows[0].2["created_by"], owner.to_string());
        assert_eq!(own_rows[0].2["last_edited_by"], owner.to_string());
        assert!(
            of_type(
                &rows,
                "documentation_page.metadata_changed",
                &other.to_string()
            )
            .is_empty(),
            "{name} hears nothing of the other workspace's page"
        );
    }
}

/// A workspace with no one to take over the person's pages (no other owner or
/// admin, and no platform admin) refuses the purge with a named error, and
/// nothing is changed.
#[test]
fn a_purge_with_no_one_to_take_the_pages_is_refused_by_name() {
    let db = common::TestDb::new();
    let f = seed(&db);
    let a = f.ws.a.workspace_id;
    // A's only admin wrote the page; A has no owner and no platform admin exists.
    let gone = f.ws.a.admin_uuid;
    without_platform_admins(&f.pool);
    let page = page_in(&f.pool, a, "Runbook", gone);

    let err = purge_from(&f.pool, a, gone).expect_err("the purge is refused");
    assert_eq!(
        backend::repository::users::no_page_successor(&err),
        Some(backend::repository::users::NoPageSuccessor { workspace_id: a }),
        "the error names the workspace: {err}"
    );
    assert_eq!(
        page_people(&f.pool, a, page),
        (gone, gone),
        "the page is untouched"
    );
    let mut conn = f.pool.get().expect("conn");
    assert!(
        backend::repository::users::get_user_by_uuid(&gone, &mut conn).is_ok(),
        "the account is still there"
    );
}

/// The `user.updated` rows about `who` that `workspace`'s feed holds.
fn updates_about_in(f: &Fixture, workspace: i32, who: Uuid) -> Vec<UserRow> {
    run_in_workspace(&f.pool, REF, workspace, |c| {
        use backend::schema::sync_actions::dsl as s;
        s::sync_actions
            .filter(s::workspace_id.eq(workspace))
            .filter(s::event_type.eq("user.updated"))
            .filter(s::aggregate_id.eq(who.to_string()))
            .order(s::sync_id.asc())
            .select((s::aggregate, s::op, s::aggregate_id, s::data))
            .load(c)
    })
    .expect("user.updated rows")
}

/// A change to the user row itself (an avatar) holds in every workspace, so it
/// reaches each workspace whose pool carries the person, whichever one the
/// writer is pinned to: B, where they are a member, and A, whose ticket names
/// them though they hold no membership there. Each event carries the person's
/// role in its own workspace, and the writer's pin is put back.
#[test]
fn an_avatar_change_reaches_every_workspace_that_holds_the_person() {
    let db = common::TestDb::new();
    let f = seed(&db);
    let (a, b) = (f.ws.a.workspace_id, f.ws.b.workspace_id);
    let who = f.requester_from_b;
    let thumb = "/uploads/users/thumbs/requester_thumb.webp";
    // Joining B in the seed already sent B one.
    let (seen_a, seen_b) = (
        updates_about_in(&f, a, who).len(),
        updates_about_in(&f, b, who).len(),
    );

    let mut conn = f.pool.get().expect("conn");
    let pin_after = with_actor_context::<_, diesel::result::Error>(
        &mut conn,
        &ActorContext::system(REF).with_workspace(b),
        |c| {
            backend::repository::users::set_avatar_thumb(c, &who, Some(thumb))?;
            backend::sync::session::current_workspace_id(c)
        },
    )
    .expect("set the thumb from B");
    assert_eq!(pin_after, Some(b), "the writer puts the caller's pin back");

    let in_b = updates_about_in(&f, b, who).split_off(seen_b);
    assert_eq!(in_b.len(), 1, "B, where they are a member, hears it once");
    assert_eq!(in_b[0].3["avatar_thumb"], thumb);
    assert_eq!(
        in_b[0].3["workspace_role"], "member",
        "B's event has B's role"
    );

    let in_a = updates_about_in(&f, a, who).split_off(seen_a);
    assert_eq!(
        in_a.len(),
        1,
        "A, whose ticket names them, hears it once (the case under test)"
    );
    assert_eq!(in_a[0].3["avatar_thumb"], thumb);
    assert!(
        in_a[0].3["workspace_role"].is_null(),
        "they hold no role in A"
    );
    let delivered = delivered_in_a(&f, f.ws.a.admin_uuid, &in_a);
    assert_eq!(delivered.len(), 1, "A's sessions receive it");

    // Someone only in B is not one of A's people: A's feed never hears of them.
    let seen_b_member = updates_about_in(&f, b, f.ws.b.member_uuid).len();
    with_actor_context::<_, diesel::result::Error>(
        &mut conn,
        &ActorContext::system(REF).with_workspace(b),
        |c| backend::repository::users::set_avatar_url(c, &f.ws.b.member_uuid, Some("/b.png")),
    )
    .expect("set B's member's avatar from B");
    assert_eq!(
        updates_about_in(&f, b, f.ws.b.member_uuid).len(),
        seen_b_member + 1
    );
    assert!(updates_about_in(&f, a, f.ws.b.member_uuid).is_empty());
}

/// Runs `write` as B's admin, pinned to B, and returns the `user.updated` rows
/// about `who` that it added to A's and B's feeds.
fn new_updates_from_b(
    f: &Fixture,
    who: Uuid,
    write: impl FnOnce(&mut backend::db::DbConnection) -> diesel::QueryResult<()>,
) -> (Vec<UserRow>, Vec<UserRow>) {
    let (a, b) = (f.ws.a.workspace_id, f.ws.b.workspace_id);
    let (seen_a, seen_b) = (
        updates_about_in(f, a, who).len(),
        updates_about_in(f, b, who).len(),
    );
    let mut conn = f.pool.get().expect("conn");
    let pin_after = with_actor_context::<_, diesel::result::Error>(
        &mut conn,
        &ActorContext::user(f.ws.b.admin_uuid, None).with_workspace(b),
        |c| {
            write(c)?;
            backend::sync::session::current_workspace_id(c)
        },
    )
    .expect("write from B");
    assert_eq!(pin_after, Some(b), "the writer puts the caller's pin back");
    (
        updates_about_in(f, a, who).split_off(seen_a),
        updates_about_in(f, b, who).split_off(seen_b),
    )
}

/// A workspace role holds in one workspace, so a change to it reaches that
/// workspace's pool with the new role and no other.
#[test]
fn a_workspace_role_change_reaches_that_workspace_with_the_new_role() {
    let db = common::TestDb::new();
    let f = seed(&db);
    let b = f.ws.b.workspace_id;
    let who = f.requester_from_b;

    let (in_a, in_b) = new_updates_from_b(&f, who, |c| {
        backend::repository::workspaces::update_membership_role(
            c,
            b,
            who,
            "agent",
            SeatWriteAuthority::Product,
        )
        .map(|_| ())
    });
    assert_eq!(in_b.len(), 1, "B hears the role change once");
    assert_eq!(in_b[0].3["workspace_role"], "agent");
    assert!(in_a.is_empty(), "A, where they hold no role, hears nothing");

    // The admin edit path rewrites both roles; only the workspace role moves.
    let (in_a, in_b) = new_updates_from_b(&f, who, |c| {
        backend::repository::users::set_user_roles(
            c,
            b,
            who,
            "user",
            "admin",
            SeatWriteAuthority::Product,
        )
        .map(|_| ())
    });
    assert_eq!(in_b.len(), 1, "B hears the edit once");
    assert_eq!(in_b[0].3["workspace_role"], "admin");
    assert!(in_a.is_empty(), "a workspace role stays in its workspace");

    // Setting the role it already has sends nothing.
    let (_, in_b) = new_updates_from_b(&f, who, |c| {
        backend::repository::users::set_user_roles(
            c,
            b,
            who,
            "user",
            "admin",
            SeatWriteAuthority::Product,
        )
        .map(|_| ())
    });
    assert!(in_b.is_empty(), "a no-op edit sends nothing");
}

/// The bulk role action runs `set_user_roles` once per person in the request's
/// workspace: each person's pooled row there gets the new role.
#[test]
fn a_bulk_role_change_reaches_the_workspace_for_each_person() {
    let db = common::TestDb::new();
    let f = seed(&db);
    let b = f.ws.b.workspace_id;
    let people = [f.requester_from_b, f.commenter_from_b];
    let seen: Vec<usize> = people
        .iter()
        .map(|who| updates_about_in(&f, b, *who).len())
        .collect();

    let mut conn = f.pool.get().expect("conn");
    let actor = ActorContext::user(f.ws.b.admin_uuid, None).with_workspace(b);
    for who in people {
        with_actor_context::<_, diesel::result::Error>(&mut conn, &actor, |c| {
            backend::repository::users::set_user_roles(
                c,
                b,
                who,
                "user",
                "agent",
                SeatWriteAuthority::Product,
            )
        })
        .expect("bulk set-role");
    }

    for (who, seen) in people.iter().zip(seen) {
        let in_b = updates_about_in(&f, b, *who).split_off(seen);
        assert_eq!(in_b.len(), 1, "B hears each person's change once");
        assert_eq!(in_b[0].3["workspace_role"], "agent");
    }
}

/// A platform role holds in every workspace, so a change to it reaches each
/// workspace holding the person, each with the person's own role there.
#[test]
fn a_platform_role_change_reaches_every_workspace_that_holds_the_person() {
    let db = common::TestDb::new();
    let f = seed(&db);
    let b = f.ws.b.workspace_id;
    let who = f.requester_from_b;

    let (in_a, in_b) = new_updates_from_b(&f, who, |c| {
        backend::repository::users::set_user_roles(
            c,
            b,
            who,
            "audit_reviewer",
            "member",
            SeatWriteAuthority::Product,
        )
        .map(|_| ())
    });
    assert_eq!(in_b.len(), 1, "B hears it once");
    assert_eq!(in_b[0].3["platform_role"], "audit_reviewer");
    assert_eq!(in_b[0].3["workspace_role"], "member");
    assert_eq!(in_a.len(), 1, "A, whose ticket names them, hears it once");
    assert_eq!(in_a[0].3["platform_role"], "audit_reviewer");
    assert!(in_a[0].3["workspace_role"].is_null(), "no role in A");
}

/// An existing account joining a workspace has no `user.created` to carry it:
/// the join sends the row, with the new role, to the workspace it joined, and
/// the person is now one of its people, so its sessions receive it.
#[test]
fn a_join_reaches_the_workspace_joined() {
    let db = common::TestDb::new();
    let f = seed(&db);
    let a = f.ws.a.workspace_id;
    let who = f.ws.b.member_uuid;
    assert!(updates_about_in(&f, a, who).is_empty());

    let mut conn = f.pool.get().expect("conn");
    with_actor_context::<_, diesel::result::Error>(
        &mut conn,
        &ActorContext::user(f.ws.a.admin_uuid, None).with_workspace(a),
        |c| add_membership(c, a, who, "agent", SeatWriteAuthority::Product).map(|_| ()),
    )
    .expect("join A");

    let in_a = updates_about_in(&f, a, who);
    assert_eq!(in_a.len(), 1, "A hears the join once");
    assert_eq!(in_a[0].3["workspace_role"], "agent");
    assert_eq!(in_a[0].3["email"], email_of(who));
    let delivered = delivered_in_a(&f, f.ws.a.admin_uuid, &in_a);
    assert_eq!(delivered.len(), 1, "A's sessions receive the new member");

    // Granting what they already hold sends nothing.
    with_actor_context::<_, diesel::result::Error>(
        &mut conn,
        &ActorContext::user(f.ws.a.admin_uuid, None).with_workspace(a),
        |c| {
            add_membership(c, a, who, "agent", SeatWriteAuthority::Product)?;
            backend::repository::workspaces::ensure_membership(c, a, who, "member").map(|_| ())
        },
    )
    .expect("grant again");
    assert_eq!(
        updates_about_in(&f, a, who).len(),
        1,
        "no event for a no-op"
    );
}

/// A removal is an update carrying no role, not a delete: it takes the person
/// out of the member lists while their row stays in the pool, so the byline
/// on the reply they left still renders.
#[actix_web::test]
async fn a_removal_drops_the_role_and_keeps_the_byline() {
    let db = common::TestDb::new();
    let f = seed(&db);
    let a = f.ws.a.workspace_id;
    let who = f.commenter_from_b;

    let mut conn = f.pool.get().expect("conn");
    let actor = ActorContext::user(f.ws.a.admin_uuid, None).with_workspace(a);
    with_actor_context::<_, diesel::result::Error>(&mut conn, &actor, |c| {
        add_membership(c, a, who, "agent", SeatWriteAuthority::Product).map(|_| ())
    })
    .expect("join A as an agent");
    let assignable = "/api/users/paginated?pageSize=100&assignable=true";
    let body = get_as(&f.pool, &f.ws.a, f.ws.a.admin_uuid, assignable).await;
    let page: serde_json::Value = serde_json::from_str(&body).expect("json");
    assert!(uuids_in(page["data"].as_array().expect("data")).contains(&who));

    let seen = updates_about_in(&f, a, who).len();
    with_actor_context::<_, diesel::result::Error>(&mut conn, &actor, |c| {
        backend::repository::workspaces::remove_membership(c, a, who, SeatWriteAuthority::Product)
            .map(|_| ())
    })
    .expect("remove from A");

    let in_a = updates_about_in(&f, a, who).split_off(seen);
    assert_eq!(in_a.len(), 1, "A hears the removal once");
    assert_eq!(in_a[0].1, SyncOp::Update, "an update, not a delete");
    assert!(in_a[0].3["workspace_role"].is_null(), "the role is gone");
    assert_eq!(in_a[0].3["name"], "Commenter From B", "the row stays whole");
    let delivered = delivered_in_a(&f, f.ws.a.admin_uuid, &in_a);
    assert_eq!(delivered.len(), 1, "A's sessions receive the removal");

    let body = get_as(&f.pool, &f.ws.a, f.ws.a.admin_uuid, assignable).await;
    let page: serde_json::Value = serde_json::from_str(&body).expect("json");
    assert!(
        !uuids_in(page["data"].as_array().expect("data")).contains(&who),
        "a removed member is no longer offered"
    );

    // A fresh session still gets their row, for the reply's byline.
    let body = get_as(
        &f.pool,
        &f.ws.a,
        f.ws.a.member_uuid,
        &format!("/api/sync/bootstrap?groups=workspace:{a}&schema=test"),
    )
    .await;
    let row = body
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str::<serde_json::Value>(l).expect("NDJSON line"))
        .find(|l| {
            l.get("__model__").and_then(|m| m.as_str()) == Some("user")
                && l.get("uuid").and_then(|u| u.as_str()) == Some(who.to_string().as_str())
        })
        .expect("the bootstrap still carries the removed member's row");
    assert!(row["workspace_role"].is_null());
}

/// A profile edit changes the one user row every workspace's pool holds, so it
/// reaches each of them, whichever workspace the editor is pinned to.
#[test]
fn a_profile_edit_reaches_every_workspace_that_holds_the_person() {
    let db = common::TestDb::new();
    let f = seed(&db);
    let who = f.requester_from_b;

    let (in_a, in_b) = new_updates_from_b(&f, who, |c| {
        backend::repository::users::update_user(
            &who,
            UserUpdate {
                name: Some("Requester Renamed".to_string()),
                pronouns: None,
                avatar_url: None,
                banner_url: None,
                avatar_thumb: None,
                microsoft_uuid: None,
                updated_at: None,
            },
            c,
            None,
        )
        .map(|_| ())
    });
    assert_eq!(in_b.len(), 1, "B hears the edit once");
    assert_eq!(in_b[0].3["name"], "Requester Renamed");
    assert_eq!(in_a.len(), 1, "A, whose ticket names them, hears it once");
    assert_eq!(in_a[0].3["name"], "Requester Renamed");
    let delivered = delivered_in_a(&f, f.ws.a.admin_uuid, &in_a);
    assert_eq!(delivered.len(), 1, "A's sessions receive it");
}

/// The primary address is part of the pooled row, so a swap reaches every
/// workspace holding the person.
#[test]
fn a_primary_email_swap_reaches_every_workspace_that_holds_the_person() {
    let db = common::TestDb::new();
    let f = seed(&db);
    let who = f.requester_from_b;
    let next = format!("next-{}@people.test", who.simple());

    let mut conn = f.pool.get().expect("conn");
    let added: backend::models::UserEmail =
        diesel::insert_into(backend::schema::user_emails::table)
            .values(&NewUserEmail {
                user_uuid: who,
                email: next.clone(),
                email_type: "personal".to_string(),
                is_primary: false,
                is_verified: true,
                source: None,
            })
            .get_result(&mut conn)
            .expect("insert a second address");
    drop(conn);

    let (in_a, in_b) = new_updates_from_b(&f, who, |c| {
        backend::repository::user_emails::set_primary(
            c,
            &who,
            added.id,
            &backend::models::UserEmailUpdate {
                is_primary: Some(true),
                updated_at: None,
            },
        )
        .map(|_| ())
    });
    assert_eq!(in_b.len(), 1, "B hears the swap once");
    assert_eq!(in_b[0].3["email"], next);
    assert_eq!(in_a.len(), 1, "A hears the swap once");
    assert_eq!(in_a[0].3["email"], next);
}
