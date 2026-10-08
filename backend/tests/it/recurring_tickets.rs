//! A recurring ticket's next occurrence comes from the write that closes it,
//! whichever way that write arrives, and only once.

use diesel::prelude::*;

use backend::models::{NewTicket, TicketUpdate, WorkflowStateCategory};
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

    let requester = ActorContext::user(ws.member_uuid, None).with_workspace(ws.workspace_id);
    let resolved = with_actor_context(&mut conn, &requester, |c| {
        ticket_ratings::resolve_as_requester(c, ticket, ws.member_uuid, None)
    })
    .expect("resolve");
    assert!(resolved);
    assert_eq!(successors(&mut conn, ws.workspace_id, ticket), 1);
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
