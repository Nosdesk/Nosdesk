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

mod common;

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
        is_doc_admin: false,
    }
}

fn staff_viewer(uuid: Uuid) -> SyncViewer {
    SyncViewer {
        ctx: VisibilityContext::new(
            uuid,
            PlatformRole::from_db("user"),
            Some(WorkspaceRole::Agent),
        ),
        is_doc_admin: false,
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
    common::ensure_test_keyring();
    let db = common::TestDb::new();
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
    common::ensure_test_keyring();
    let db = common::TestDb::new();
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
    common::ensure_test_keyring();
    let db = common::TestDb::new();
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
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let mut conn = db.conn();

    let alice = user(&mut conn, "Alice");
    let viewer = member_viewer(alice);
    let items = vec![
        av(SyncAggregate::User),
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
