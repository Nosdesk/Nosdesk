//! DB-backed coverage for the shared sync visibility filter.
//!
//! `sync::visibility::filter_actions` is the single brain the bootstrap /
//! delta / SSE read paths use to decide which `sync_actions` a viewer may
//! receive. Restricted (workspace `member`) users must see only tickets
//! they requested or watch, and only non-internal comments on those; staff
//! (`sees_all`) see the whole ticket family. This exercises the real DB
//! resolution (`visible_ticket_ids` over tickets + ticket_watchers, and the
//! attachment comment->ticket resolution), complementing the pure
//! per-aggregate unit tests in the `sync::visibility` module.

#![allow(clippy::expect_used)]

use diesel::prelude::*;
use uuid::Uuid;

use backend::models::{NewComment, NewTicket, NewUser, PlatformRole, SyncAggregate, WorkspaceRole};
use backend::repository::ticket_visibility::VisibilityContext;
use backend::sync::visibility::{filter_actions, ActionView, SyncViewer};

const WS: i32 = 1;

fn default_state_id(conn: &mut PgConnection) -> i32 {
    use backend::schema::workflow_states::dsl as s;
    s::workflow_states
        .filter(s::workspace_id.eq(WS))
        .filter(s::is_default.eq(true))
        .select(s::id)
        .first(conn)
        .expect("default workflow state seeded")
}

fn user(conn: &mut PgConnection, name: &str) -> Uuid {
    use backend::schema::users;
    let u: backend::models::User = diesel::insert_into(users::table)
        .values(&NewUser {
            uuid: Uuid::new_v4(),
            name: name.to_string(),
            pronouns: None,
            avatar_url: None,
            banner_url: None,
            avatar_thumb: None,
            microsoft_uuid: None,
            mfa_secret: None,
            mfa_secret_kek_id: None,
            mfa_enabled: false,
            platform_role: None,
        })
        .get_result(conn)
        .expect("insert user");
    u.uuid
}

fn ticket(conn: &mut PgConnection, title: &str, requester: Uuid) -> i32 {
    use backend::schema::tickets;
    let state_id = default_state_id(conn);
    let t: backend::models::Ticket = diesel::insert_into(tickets::table)
        .values(&NewTicket {
            title: title.to_string(),
            workflow_state_id: state_id,
            requester_uuid: Some(requester),
            ..Default::default()
        })
        .get_result(conn)
        .expect("insert ticket");
    t.id
}

fn watch(conn: &mut PgConnection, ticket_id: i32, user_uuid: Uuid) {
    use backend::schema::ticket_watchers;
    diesel::insert_into(ticket_watchers::table)
        .values((
            ticket_watchers::ticket_id.eq(ticket_id),
            ticket_watchers::user_uuid.eq(user_uuid),
            ticket_watchers::auto_added.eq(false),
            ticket_watchers::notify_on_internal_notes.eq(false),
        ))
        .execute(conn)
        .expect("insert watcher");
}

fn comment(conn: &mut PgConnection, ticket_id: i32, author: Uuid, is_internal: bool) -> i32 {
    use backend::schema::comments;
    let c: backend::models::Comment = diesel::insert_into(comments::table)
        .values(&NewComment {
            content: "x".to_string(),
            ticket_id,
            user_uuid: author,
            is_internal,
            ..Default::default()
        })
        .get_result(conn)
        .expect("insert comment");
    c.id
}

fn member_viewer(uuid: Uuid) -> SyncViewer {
    SyncViewer {
        ctx: VisibilityContext::new(
            uuid,
            PlatformRole::from_db("user"),
            Some(WorkspaceRole::Member),
        ),
        is_admin: false,
    }
}

fn staff_viewer(uuid: Uuid) -> SyncViewer {
    SyncViewer {
        ctx: VisibilityContext::new(
            uuid,
            PlatformRole::from_db("user"),
            Some(WorkspaceRole::Agent),
        ),
        is_admin: false,
    }
}

fn av(aggregate: SyncAggregate) -> ActionView {
    ActionView {
        aggregate: Some(aggregate),
        is_delete: false,
        aggregate_id: None,
        ticket_id: None,
        is_internal: None,
        comment_id: None,
        subject_uuid: None,
        bare_id: false,
        addressees: Vec::new(),
        gap_id: None,
    }
}

fn run(
    conn: &mut backend::db::DbConnection,
    viewer: &SyncViewer,
    items: &[ActionView],
) -> Vec<bool> {
    filter_actions(conn, viewer, items, |v: &ActionView| v.clone())
}

#[test]
fn member_sees_only_own_and_watched_tickets() {
    crate::common::ensure_test_keyring();
    let db = crate::common::TestDb::new();
    let mut conn = db.conn();

    let alice = user(&mut conn, "Alice");
    let bob = user(&mut conn, "Bob");
    let alice_ticket = ticket(&mut conn, "Alice's", alice);
    let bob_ticket = ticket(&mut conn, "Bob's", bob);
    let watched = ticket(&mut conn, "Watched", bob);
    watch(&mut conn, watched, alice);

    let mut own = av(SyncAggregate::Ticket);
    own.aggregate_id = Some(alice_ticket);
    let mut others = av(SyncAggregate::Ticket);
    others.aggregate_id = Some(bob_ticket);
    let mut watched_av = av(SyncAggregate::Ticket);
    watched_av.aggregate_id = Some(watched);

    let viewer = member_viewer(alice);
    let mask = run(&mut conn, &viewer, &[own, others, watched_av]);
    assert_eq!(
        mask,
        vec![true, false, true],
        "own + watched kept, other dropped"
    );

    // Staff sees all three.
    let staff = staff_viewer(user(&mut conn, "Agent"));
    let mut t1 = av(SyncAggregate::Ticket);
    t1.aggregate_id = Some(alice_ticket);
    let mut t2 = av(SyncAggregate::Ticket);
    t2.aggregate_id = Some(bob_ticket);
    let staff_mask = run(&mut conn, &staff, &[t1, t2]);
    assert_eq!(staff_mask, vec![true, true], "staff sees all tickets");
}

#[test]
fn member_comment_and_junction_follow_ticket() {
    crate::common::ensure_test_keyring();
    let db = crate::common::TestDb::new();
    let mut conn = db.conn();

    let alice = user(&mut conn, "Alice");
    let bob = user(&mut conn, "Bob");
    let alice_ticket = ticket(&mut conn, "Alice's", alice);
    let bob_ticket = ticket(&mut conn, "Bob's", bob);

    let mk = |agg: SyncAggregate, tid: i32, internal: Option<bool>| {
        let mut v = av(agg);
        v.ticket_id = Some(tid);
        v.is_internal = internal;
        v
    };

    let items = vec![
        mk(SyncAggregate::Comment, alice_ticket, Some(false)), // 0 keep
        mk(SyncAggregate::Comment, alice_ticket, Some(true)),  // 1 internal -> drop
        mk(SyncAggregate::Comment, bob_ticket, Some(false)),   // 2 other -> drop
        mk(SyncAggregate::TicketAsset, alice_ticket, None),    // 3 keep
        mk(SyncAggregate::TicketAsset, bob_ticket, None),      // 4 drop
        mk(SyncAggregate::LinkedTicket, alice_ticket, None),   // 5 keep
        mk(SyncAggregate::ProjectTicket, bob_ticket, None),    // 6 drop
    ];
    let viewer = member_viewer(alice);
    let mask = run(&mut conn, &viewer, &items);
    assert_eq!(
        mask,
        vec![true, false, false, true, false, true, false],
        "comment+junction visibility follows ticket; internal dropped"
    );
}

#[test]
fn member_attachment_resolves_through_comment() {
    crate::common::ensure_test_keyring();
    let db = crate::common::TestDb::new();
    let mut conn = db.conn();

    let alice = user(&mut conn, "Alice");
    let bob = user(&mut conn, "Bob");
    let alice_ticket = ticket(&mut conn, "Alice's", alice);
    let bob_ticket = ticket(&mut conn, "Bob's", bob);

    let public_own = comment(&mut conn, alice_ticket, alice, false);
    let internal_own = comment(&mut conn, alice_ticket, alice, true);
    let public_other = comment(&mut conn, bob_ticket, bob, false);

    let attach = |comment_id: i32, is_delete: bool| {
        let mut v = av(SyncAggregate::Attachment);
        v.is_delete = is_delete;
        v.comment_id = if is_delete { None } else { Some(comment_id) };
        v
    };

    let items = vec![
        attach(public_own, false),   // 0 keep (public comment on own ticket)
        attach(internal_own, false), // 1 drop (internal comment)
        attach(public_other, false), // 2 drop (other's ticket)
        attach(0, true),             // 3 keep (attachment.delete: bare-id prune)
    ];
    let viewer = member_viewer(alice);
    let mask = run(&mut conn, &viewer, &items);
    assert_eq!(
        mask,
        vec![true, false, false, true],
        "attachment.created gated via comment->ticket; delete passes as bare-id prune"
    );
}

#[test]
fn reference_data_always_allowed() {
    crate::common::ensure_test_keyring();
    let db = crate::common::TestDb::new();
    let mut conn = db.conn();

    let alice = user(&mut conn, "Alice");
    let viewer = member_viewer(alice);
    // A `user` row is scoped to the workspace's people; the viewer's own
    // row always goes (see `sync_workspace_people`).
    let items = vec![
        ActionView {
            subject_uuid: Some(alice),
            ..av(SyncAggregate::User)
        },
        av(SyncAggregate::Asset),
        av(SyncAggregate::WorkflowState),
        av(SyncAggregate::Project),
        av(SyncAggregate::Cycle),
    ];
    let mask = run(&mut conn, &viewer, &items);
    assert!(
        mask.iter().all(|&k| k),
        "reference data allowed for members"
    );
}

/// The live event stream filters on a pool rather than a request connection,
/// through `filter_actions_pinned`. Pinned to the viewer's workspace, the
/// filter sees a restricted page there and hides it from a member outside its
/// group. The pool it gets has the production shape (runtime role, nothing
/// pinned), where a filter that didn't pin the workspace couldn't load the page
/// and would let it through.
#[test]
fn pinned_filter_hides_restricted_pages_in_the_viewers_workspace() {
    use backend::schema::{documentation_page_visibility, documentation_pages, groups};
    use backend::sync::session::run_in_workspace;
    use backend::sync::visibility::filter_actions_pinned;

    let db = crate::common::TestDb::new();
    let pool = db.pool_with_size(2);
    let seeded = crate::common::seed_two_workspaces(&mut pool.get().expect("conn"));
    let b = seeded.b.workspace_id;

    let (restricted, open) = run_in_workspace(&pool, "test:sync_visibility", b, |c| {
        let page = |c: &mut backend::db::DbConnection, title: &str| {
            diesel::insert_into(documentation_pages::table)
                .values((
                    documentation_pages::title.eq(title),
                    documentation_pages::slug.eq(title.to_lowercase()),
                    documentation_pages::status.eq(backend::models::DocumentationStatus::Published),
                    documentation_pages::created_by.eq(seeded.b.admin_uuid),
                    documentation_pages::last_edited_by.eq(seeded.b.admin_uuid),
                ))
                .returning(documentation_pages::id)
                .get_result::<i32>(c)
        };
        let restricted = page(c, "Salaries")?;
        let open = page(c, "Handbook")?;
        let group: i32 = diesel::insert_into(groups::table)
            .values(groups::name.eq("Payroll"))
            .returning(groups::id)
            .get_result(c)?;
        diesel::update(documentation_pages::table.find(restricted))
            .set(documentation_pages::restricted.eq(true))
            .execute(c)?;
        diesel::insert_into(documentation_page_visibility::table)
            .values((
                documentation_page_visibility::page_id.eq(restricted),
                documentation_page_visibility::group_id.eq(group),
            ))
            .execute(c)?;
        Ok((restricted, open))
    })
    .expect("seed pages");

    let rows: Vec<ActionView> = [restricted, open]
        .into_iter()
        .map(|id| ActionView {
            aggregate_id: Some(id),
            ..av(SyncAggregate::DocumentationPage)
        })
        .collect();
    // One connection, left as the production pool hands it out.
    let runtime_pool = db.pool_with_size(1);
    {
        let mut conn = runtime_pool.get().expect("conn");
        diesel::sql_query("SET ROLE nosdesk_app")
            .execute(&mut conn)
            .expect("set role");
        diesel::sql_query("SELECT set_config('app.workspace_id', '', false)")
            .execute(&mut conn)
            .expect("clear pin");
    }
    let filter = |viewer: &SyncViewer| {
        filter_actions_pinned(&runtime_pool, b, viewer, &rows, |v: &ActionView| v.clone())
    };

    assert_eq!(
        filter(&member_viewer(seeded.b.member_uuid)),
        vec![false, true],
        "a member outside the page's group sees only the open page"
    );
    let doc_admin = SyncViewer {
        is_admin: true,
        ..staff_viewer(seeded.b.admin_uuid)
    };
    assert_eq!(filter(&doc_admin), vec![true, true]);
}

/// A documentation row whose visibility lookup fails is neither sent nor
/// dropped: the delta and the live stream fail the batch instead, so the
/// client asks again rather than moving its cursor past a row that may have
/// been a withdrawal of access.
#[test]
fn a_doc_row_whose_lookup_fails_is_not_passed_over() {
    use backend::sync::visibility::{deliveries, Delivery};

    let db = crate::common::TestDb::new();
    let pool = db.pool_with_size(1);
    let mut conn = pool.get().expect("conn");
    let viewer = member_viewer(Uuid::new_v4());
    let _ = conn.transaction::<(), diesel::result::Error, _>(|c| {
        // Every query after this one fails: the transaction is aborted.
        let _ = diesel::sql_query("SELECT 1/0").execute(c);
        let rows = vec![ActionView {
            aggregate_id: Some(1),
            ..av(SyncAggregate::DocumentationPage)
        }];
        let decided = deliveries(c, &viewer, &rows, |v: &ActionView| v.clone());
        assert!(
            decided
                .iter()
                .all(|d| !matches!(d, Delivery::Send | Delivery::Drop)),
            "a doc row whose lookup failed: {decided:?}"
        );
        Err(diesel::result::Error::RollbackTransaction)
    });
}

/// GET `uri` as `viewer` through the sync handlers, with `seed`'s workspace
/// resolved for the request, as JSON.
async fn sync_get_as(
    pool: &crate::common::TestPool,
    seed: &crate::common::WorkspaceSeed,
    viewer: Uuid,
    uri: &str,
) -> String {
    use actix_web::dev::Service;
    use actix_web::{test as http_test, web, App, HttpMessage};
    let claims = backend::models::Claims {
        sub: viewer.to_string(),
        name: "Viewer".to_string(),
        email: format!("{}@visibility.test", viewer.simple()),
        platform_role: "user".to_string(),
        scope: "full".to_string(),
        sid: None,
        workspace_uuid: None,
        exp: (chrono::Utc::now().timestamp() + 3600) as usize,
        iat: chrono::Utc::now().timestamp() as usize,
    };
    let workspace = backend::extractors::WorkspaceContext {
        workspace_id: seed.workspace_id,
        workspace_uuid: seed.workspace_uuid,
        slug: seed.slug.clone(),
        name: seed.slug.clone(),
        organisation_id: None,
        custom_domain: None,
    };
    let corr = Uuid::now_v7();
    let actor = backend::sync::actor::ActorContext::user(viewer, Some(corr))
        .with_workspace(seed.workspace_id);
    let app = http_test::init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .wrap_fn(move |req, srv| {
                req.extensions_mut().insert(workspace.clone());
                req.extensions_mut().insert(claims.clone());
                req.extensions_mut()
                    .insert(backend::middleware::RequestContext::new(
                        corr,
                        actor.clone(),
                    ));
                srv.call(req)
            })
            .service(web::scope("/api").configure(backend::handlers::sync::config)),
    )
    .await;
    let resp =
        http_test::call_service(&app, http_test::TestRequest::get().uri(uri).to_request()).await;
    let status = resp.status();
    let body = String::from_utf8(http_test::read_body(resp).await.to_vec()).expect("utf8 body");
    assert_eq!(status, 200, "GET {uri}: {body}");
    body
}

/// A delta whose span holds the change in what its viewer may see asks that
/// viewer for a fresh snapshot, whichever groups it named: the marker is
/// addressed to them alone, in a group clients don't ask the delta for.
/// Anyone else reading the same span, and the viewer's next page, gets no flag.
#[actix_web::test]
async fn a_delta_over_the_viewers_access_change_asks_for_a_fresh_snapshot() {
    use backend::repository::workspaces::{add_membership, upsert_membership_role};
    use backend::schema::sync_actions;
    use backend::sync::actor::ActorContext;
    use backend::sync::session::{run_in_workspace, with_actor_context};

    crate::common::ensure_test_keyring();
    let db = crate::common::TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = crate::common::seed_two_workspaces(&mut pool.get().expect("conn"));
    let a = &ws.a;
    let workspace_group = format!("workspace:{}", a.workspace_id);
    let admin_in_a = ActorContext::user(a.admin_uuid, None).with_workspace(a.workspace_id);

    let agent = {
        let mut conn = pool.get().expect("conn");
        let agent = crate::common::insert_plain_user(&mut conn, "Demoted Agent");
        with_actor_context::<_, diesel::result::Error>(&mut conn, &admin_in_a, |c| {
            add_membership(
                c,
                a.workspace_id,
                agent,
                "agent",
                backend::repository::workspaces::SeatWriteAuthority::ControlPlane,
            )?;
            Ok(())
        })
        .expect("add the agent");
        agent
    };

    // The newest action A's workspace group holds.
    let newest_in_a = || -> i64 {
        run_in_workspace(&pool, "test:sync_visibility", a.workspace_id, |c| {
            sync_actions::table
                .filter(sync_actions::groups.overlaps_with(vec![Some(workspace_group.clone())]))
                .select(diesel::dsl::max(sync_actions::sync_id))
                .first::<Option<i64>>(c)
        })
        .expect("newest action")
        .expect("the workspace has actions")
    };
    // `viewer`'s delta from `from` once it covers `newest`.
    let delta_after = |viewer: Uuid, from: (i64, i64), legacy: bool, newest: i64| {
        let pool = pool.clone();
        let workspace_group = workspace_group.clone();
        async move {
            let mut uri = format!(
                "/api/sync/delta?from={}&limit=5000&groups={workspace_group}",
                from.0
            );
            if !legacy {
                uri.push_str(&format!("&from_xid8={}", from.1));
            }
            let wait = crate::common::FeedWait::new(newest);
            loop {
                let body: serde_json::Value =
                    serde_json::from_str(&sync_get_as(&pool, a, viewer, &uri).await)
                        .expect("delta json");
                if body["last_sync_id"].as_i64().expect("last_sync_id") >= newest {
                    break body;
                }
                wait.pause().await;
            }
        }
    };
    let cursor_of = |page: &serde_json::Value| {
        (
            page["last_sync_id"].as_i64().expect("last_sync_id"),
            page["last_xid8"].as_i64().expect("last_xid8"),
        )
    };

    // A cursor past everything so far, the joins that seeded A included
    // (each of those changed what its person may see, too).
    let caught_up = delta_after(agent, (0, 0), false, newest_in_a()).await;
    let cursor = cursor_of(&caught_up);

    with_actor_context::<_, diesel::result::Error>(
        &mut pool.get().expect("conn"),
        &admin_in_a,
        |c| upsert_membership_role(c, a.workspace_id, agent, "member").map(|_| ()),
    )
    .expect("demote the agent");
    let newest = newest_in_a();

    let page = delta_after(agent, cursor, false, newest).await;
    assert_eq!(page["resnapshot"], true, "the demoted viewer: {page}");
    let legacy = delta_after(agent, cursor, true, newest).await;
    assert_eq!(
        legacy["resnapshot"], true,
        "a cursor without an xid8: {legacy}"
    );

    let colleague = delta_after(a.admin_uuid, cursor, false, newest).await;
    assert!(
        colleague.get("resnapshot").is_none(),
        "someone else over the same span: {colleague}"
    );

    let next = cursor_of(&page);
    let after = sync_get_as(
        &pool,
        a,
        agent,
        &format!(
            "/api/sync/delta?from={}&from_xid8={}&groups={workspace_group}",
            next.0, next.1
        ),
    )
    .await;
    let after: serde_json::Value = serde_json::from_str(&after).expect("delta json");
    assert!(
        after.get("resnapshot").is_none(),
        "the page after the change: {after}"
    );

    // A group join records the membership before the marker, so the page
    // can end on the membership row with the marker after it in the same
    // transaction; the flag still rides that page.
    with_actor_context::<_, diesel::result::Error>(
        &mut pool.get().expect("conn"),
        &admin_in_a,
        |c| {
            let group: i32 = diesel::insert_into(backend::schema::groups::table)
                .values(backend::schema::groups::name.eq("Night shift"))
                .returning(backend::schema::groups::id)
                .get_result(c)?;
            backend::repository::groups::add_user_to_group(c, agent, group, None).map(|_| ())
        },
    )
    .expect("add the agent to a group");
    let joined = delta_after(agent, next, false, newest_in_a()).await;
    assert_eq!(joined["resnapshot"], true, "a group join: {joined}");
    let then = delta_after(agent, cursor_of(&joined), false, newest_in_a()).await;
    assert!(
        then.get("resnapshot").is_none(),
        "the join is reported once: {then}"
    );
}
