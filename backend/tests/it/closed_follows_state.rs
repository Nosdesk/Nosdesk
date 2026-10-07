//! A ticket's closed_at and closed_by follow its workflow state in the
//! database (`ticket_closed_follows_state`), whichever path writes the state:
//! created closed, reopened, closed, moved between closing states, merged, or
//! written with its own closing time.

use diesel::prelude::*;

use backend::models::{NewTicket, Ticket, TicketUpdate, WorkflowStateCategory as Cat};
use backend::repository::{tickets, workflow_states};
use backend::sync::actor::ActorContext;
use backend::sync::session::with_actor_context;

use crate::common;

#[test]
fn closed_at_and_closed_by_follow_the_state() {
    let db = common::TestDb::new();
    let pool = db.pool_with_size(2);
    let seeded = common::seed_two_workspaces(&mut pool.get().expect("conn"));
    let ws = seeded.a.workspace_id;
    let agent = seeded.a.admin_uuid;
    let mut conn = pool.get().expect("conn");

    with_actor_context(
        &mut conn,
        &ActorContext::user(agent, None).with_workspace(ws),
        |c| {
            let open = workflow_states::default_state(c)?;
            let done = workflow_states::first_in_category(c, Cat::Done)?;
            let cancelled = workflow_states::first_in_category(c, Cat::Cancelled)?;
            let merged = workflow_states::first_in_category(c, Cat::Merged)?;
            let move_to = |c: &mut _, id: i32, state: i32| {
                tickets::update_ticket_partial(
                    c,
                    id,
                    TicketUpdate {
                        workflow_state_id: Some(state),
                        ..Default::default()
                    },
                    None,
                )
            };

            // Created already closed, as quick-add into Done does: stamped, by
            // the acting agent.
            let t: Ticket = diesel::insert_into(backend::schema::tickets::table)
                .values(&NewTicket {
                    title: "Closed on arrival".to_string(),
                    workflow_state_id: done.id,
                    ..Default::default()
                })
                .get_result(c)?;
            assert!(t.closed_at.is_some());
            assert_eq!(t.closed_by, Some(agent));

            // Reopened: neither.
            let t = move_to(c, t.id, open.id)?;
            assert_eq!((t.closed_at, t.closed_by), (None, None));

            // Closed: stamped again, and the state-change event clients and
            // webhooks receive carries the stamp (GitHub #590 saw it null).
            let t = move_to(c, t.id, done.id)?;
            assert!(t.closed_at.is_some());
            assert_eq!(t.closed_by, Some(agent));
            let event: serde_json::Value = {
                use backend::schema::sync_actions;
                sync_actions::table
                    .filter(sync_actions::aggregate_id.eq(t.id.to_string()))
                    .filter(sync_actions::event_type.eq("ticket.workflow_state_changed"))
                    .order(sync_actions::sync_id.desc())
                    .select(sync_actions::data)
                    .first(c)?
            };
            assert!(!event["closed_at"].is_null(), "{event}");
            assert_eq!(event["closed_by"], serde_json::json!(agent));

            // A statement that sets its own closing time keeps it (one that
            // differs from the trigger's own stamp, and not before creation).
            let t = move_to(c, t.id, open.id)?;
            let given = t.created_at + chrono::Duration::hours(1);
            let t = tickets::update_ticket_partial(
                c,
                t.id,
                TicketUpdate {
                    workflow_state_id: Some(done.id),
                    closed_at: Some(Some(given)),
                    ..Default::default()
                },
                None,
            )?;
            assert_eq!(t.closed_at, Some(given));

            // Between closing states, and into Merged, the first stamp stays.
            let t = move_to(c, t.id, cancelled.id)?;
            assert_eq!(t.closed_at, Some(given));
            let t = move_to(c, t.id, merged.id)?;
            assert_eq!(t.closed_at, Some(given));
            Ok::<_, diesel::result::Error>(())
        },
    )
    .expect("state moves");
}
