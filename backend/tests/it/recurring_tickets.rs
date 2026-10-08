//! A recurring ticket's next occurrence comes from the write that closes it,
//! whichever way that write arrives, and only once.

use diesel::prelude::*;

use backend::models::{NewTicket, TicketUpdate, WorkflowStateCategory};
use backend::repository::tickets::TicketUpdatedObserver;
use backend::repository::{ticket_ratings, tickets as ticket_repo, workflow_states};
use backend::schema::tickets;
use backend::sync::actor::ActorContext;
use backend::sync::session::with_actor_context;

use crate::common;

/// A weekly ticket in workspace A, requested by its member, due tomorrow.
fn weekly_ticket(conn: &mut backend::db::DbConnection, ws: &common::WorkspaceSeed) -> i32 {
    let admin = ActorContext::user(ws.admin_uuid, None).with_workspace(ws.workspace_id);
    with_actor_context(conn, &admin, |c| {
        let open = workflow_states::default_state(c)?.id;
        diesel::insert_into(tickets::table)
            .values(&NewTicket {
                title: "Check the backups".to_string(),
                workflow_state_id: open,
                requester_uuid: Some(ws.member_uuid),
                due_date: Some(chrono::Utc::now().naive_utc() + chrono::Duration::days(1)),
                recurrence_rule: Some("FREQ=WEEKLY".to_string()),
                ..Default::default()
            })
            .returning(tickets::id)
            .get_result(c)
    })
    .expect("seed recurring ticket")
}

/// What the search index was asked to update.
#[derive(Default)]
struct Indexed(std::sync::Mutex<Vec<i32>>);

impl TicketUpdatedObserver for Indexed {
    fn ticket_updated(
        &self,
        ticket: &backend::models::Ticket,
        _article: Option<&backend::models::ArticleContent>,
    ) {
        self.0.lock().expect("lock").push(ticket.id);
    }
}

fn successors(conn: &mut backend::db::DbConnection, ws: i32, ticket: i32) -> i64 {
    let system = ActorContext::system("test:recurring_tickets").with_workspace(ws);
    with_actor_context(conn, &system, |c| {
        tickets::table
            .filter(tickets::recurrence_template_id.eq(ticket))
            .count()
            .get_result(c)
    })
    .expect("count successors")
}

/// The requester saying it's fixed closes the ticket like any other close.
#[test]
fn a_recurring_ticket_closed_by_its_requester_comes_back() {
    let db = common::TestDb::new();
    let pool = db.pool_with_size(2);
    let mut conn = pool.get().expect("conn");
    let ws = common::seed_two_workspaces(&mut conn).a;
    let ticket = weekly_ticket(&mut conn, &ws);

    let seen = Indexed::default();
    let requester = ActorContext::user(ws.member_uuid, None).with_workspace(ws.workspace_id);
    let resolved = with_actor_context(&mut conn, &requester, |c| {
        ticket_ratings::resolve_as_requester(c, ticket, ws.member_uuid, None, Some(&seen))
    })
    .expect("resolve");
    assert!(resolved);
    assert_eq!(successors(&mut conn, ws.workspace_id, ticket), 1);
    let indexed = seen.0.lock().expect("lock").clone();
    assert_eq!(
        indexed.len(),
        2,
        "the closed ticket and its next occurrence: {indexed:?}"
    );
    assert_eq!(indexed[0], ticket);
}

/// Two saves that close the same ticket, or a save after it closed, make one
/// next occurrence.
#[test]
fn a_recurring_ticket_closed_twice_comes_back_once() {
    let db = common::TestDb::new();
    let pool = db.pool_with_size(2);
    let mut conn = pool.get().expect("conn");
    let ws = common::seed_two_workspaces(&mut conn).a;
    let ticket = weekly_ticket(&mut conn, &ws);
    let admin = ActorContext::user(ws.admin_uuid, None).with_workspace(ws.workspace_id);

    let save = |conn: &mut backend::db::DbConnection, update: TicketUpdate| {
        with_actor_context(conn, &admin, |c| {
            ticket_repo::update_ticket_partial(c, ticket, update, None)
        })
        .expect("save")
    };
    let state = |conn: &mut backend::db::DbConnection, category| {
        with_actor_context(conn, &admin, |c| {
            workflow_states::first_in_category(c, category)
        })
        .expect("state")
        .id
    };
    let done = state(&mut conn, WorkflowStateCategory::Done);
    let cancelled = state(&mut conn, WorkflowStateCategory::Cancelled);

    save(
        &mut conn,
        TicketUpdate {
            workflow_state_id: Some(done),
            ..Default::default()
        },
    );
    save(
        &mut conn,
        TicketUpdate {
            workflow_state_id: Some(done),
            ..Default::default()
        },
    );
    save(
        &mut conn,
        TicketUpdate {
            workflow_state_id: Some(cancelled),
            ..Default::default()
        },
    );
    save(
        &mut conn,
        TicketUpdate {
            title: Some("Check the nightly backups".into()),
            ..Default::default()
        },
    );
    assert_eq!(successors(&mut conn, ws.workspace_id, ticket), 1);
}

/// Closing it again in a later period, while the occurrence the first close
/// made is still open, makes no second one, even when the ticket has no due
/// date and so the next date moves with the close.
#[test]
fn a_reclosed_ticket_with_an_open_occurrence_makes_no_second_one() {
    let db = common::TestDb::new();
    let pool = db.pool_with_size(2);
    let mut conn = pool.get().expect("conn");
    let ws = common::seed_two_workspaces(&mut conn).a;
    let ticket = weekly_ticket(&mut conn, &ws);
    let admin = ActorContext::user(ws.admin_uuid, None).with_workspace(ws.workspace_id);
    with_actor_context(&mut conn, &admin, |c| {
        diesel::update(tickets::table.find(ticket))
            .set(tickets::due_date.eq(None::<chrono::NaiveDateTime>))
            .execute(c)
    })
    .expect("no due date");

    let state = |conn: &mut backend::db::DbConnection, category| {
        with_actor_context(conn, &admin, |c| {
            workflow_states::first_in_category(c, category)
        })
        .expect("state")
        .id
    };
    let done = state(&mut conn, WorkflowStateCategory::Done);
    let backlog = state(&mut conn, WorkflowStateCategory::Backlog);
    let save = |conn: &mut backend::db::DbConnection, update: TicketUpdate| {
        with_actor_context(conn, &admin, |c| {
            ticket_repo::update_ticket_partial(c, ticket, update, None)
        })
        .expect("save")
    };

    save(
        &mut conn,
        TicketUpdate {
            workflow_state_id: Some(done),
            ..Default::default()
        },
    );
    save(
        &mut conn,
        TicketUpdate {
            workflow_state_id: Some(backlog),
            ..Default::default()
        },
    );
    // Closed again a fortnight on: the next date is a later one.
    save(
        &mut conn,
        TicketUpdate {
            workflow_state_id: Some(done),
            closed_at: Some(Some(
                chrono::Utc::now().naive_utc() + chrono::Duration::days(14),
            )),
            ..Default::default()
        },
    );
    assert_eq!(successors(&mut conn, ws.workspace_id, ticket), 1);
}

/// Closing takes a lock on the ticket that a reply being added to it at the
/// same moment doesn't wait on (the reply's insert holds a key-share lock on
/// the ticket). A lock that conflicted with it deadlocked a requester's reply
/// that reopens a closed ticket against another reply on the same ticket.
#[test]
fn closing_doesnt_wait_on_a_reply_being_added() {
    let db = common::TestDb::new();
    let pool = db.pool_with_size(3);
    let mut setup = pool.get().expect("conn");
    let ws = common::seed_two_workspaces(&mut setup).a;
    let ticket = weekly_ticket(&mut setup, &ws);
    drop(setup);

    // Another transaction holds what a comment insert on the ticket holds.
    let mut other = pool.get().expect("conn");
    diesel::sql_query("BEGIN")
        .execute(&mut other)
        .expect("begin");
    diesel::sql_query("SELECT id FROM tickets WHERE id = $1 FOR KEY SHARE")
        .bind::<diesel::sql_types::Integer, _>(ticket)
        .execute(&mut other)
        .expect("key share");

    let mut conn = pool.get().expect("conn");
    diesel::sql_query("SET lock_timeout = '3s'")
        .execute(&mut conn)
        .expect("lock timeout");
    let admin = ActorContext::user(ws.admin_uuid, None).with_workspace(ws.workspace_id);
    let closed = with_actor_context(&mut conn, &admin, |c| {
        let done = workflow_states::first_in_category(c, WorkflowStateCategory::Done)?.id;
        ticket_repo::update_ticket_partial(
            c,
            ticket,
            TicketUpdate {
                workflow_state_id: Some(done),
                ..Default::default()
            },
            None,
        )
    });
    diesel::sql_query("ROLLBACK")
        .execute(&mut other)
        .expect("rollback");
    assert!(
        closed.is_ok(),
        "the close waited on the key-share lock: {closed:?}"
    );
}
