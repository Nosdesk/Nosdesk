//! A guest ticket awaiting email confirmation is held out of the workspace.
//!
//! While `verification_state = 'pending'` its events carry only the ticket's
//! own group (no workspace audience, so no SSE, delta, webhook, notification
//! or activity entry), and no staff viewer can see or open it. Confirmation
//! releases it with one `ticket.created` to the full audience, carrying the
//! submission's `created_via`.

#![allow(clippy::expect_used)]

use diesel::prelude::*;

use backend::db::DbConnection;
use backend::models::{NewTicket, PlatformRole, WorkspaceRole};
use backend::repository::ticket_visibility::{
    can_view_ticket, visible_ticket_ids, VisibilityContext,
};
use backend::repository::tickets::{self, TicketCreationAnnotation};
use backend::sync::groups::has_workspace_audience;

const WS: i32 = 1;

fn default_state(conn: &mut DbConnection) -> i32 {
    use backend::schema::workflow_states;
    workflow_states::table
        .filter(workflow_states::workspace_id.eq(WS))
        .filter(workflow_states::is_default.eq(true))
        .select(workflow_states::id)
        .first(conn)
        .expect("default state")
}

fn guest_ticket(
    conn: &mut DbConnection,
    requester: uuid::Uuid,
    pending: bool,
) -> backend::models::Ticket {
    let state = default_state(conn);
    tickets::create_ticket_with_annotation(
        conn,
        NewTicket {
            title: "Printer on fire".into(),
            workflow_state_id: state,
            requester_uuid: Some(requester),
            verification_state: pending.then(|| "pending".to_string()),
            ..Default::default()
        },
        TicketCreationAnnotation {
            source: Some("guest_portal".into()),
            from_email: Some("guest@example.com".into()),
            from_name: Some("Guest".into()),
            subject: Some("Printer on fire".into()),
        },
        None,
    )
    .expect("create ticket")
}

/// Every `ticket.created` for this ticket: (groups, created_via source).
fn created_events(
    conn: &mut DbConnection,
    ticket_id: i32,
) -> Vec<(Vec<Option<String>>, serde_json::Value)> {
    use backend::schema::sync_actions;
    sync_actions::table
        .filter(sync_actions::aggregate_id.eq(ticket_id.to_string()))
        .filter(sync_actions::event_type.eq("ticket.created"))
        .order(sync_actions::sync_id.asc())
        .select((sync_actions::groups, sync_actions::data))
        .load::<(Vec<Option<String>>, serde_json::Value)>(conn)
        .expect("events")
        .into_iter()
        .map(|(g, d)| (g, d["created_via"]["source"].clone()))
        .collect()
}

fn staff(user: uuid::Uuid) -> VisibilityContext {
    VisibilityContext::new(user, PlatformRole::User, Some(WorkspaceRole::Admin))
}

#[test]
fn a_pending_guest_ticket_is_held_until_its_submitter_confirms() {
    let db = crate::common::TestDb::new();
    let mut conn = db.conn();
    let guest = crate::common::insert_user(&mut conn, "Guest");
    let agent = crate::common::insert_user(&mut conn, "Agent");
    let ticket = guest_ticket(&mut conn, guest.uuid, true);

    // Held: the event reaches only the ticket's own group, and staff can't see it.
    let events = created_events(&mut conn, ticket.id);
    assert_eq!(events.len(), 1);
    assert!(
        !has_workspace_audience(&events[0].0),
        "held: {:?}",
        events[0].0
    );
    let ctx = staff(agent.uuid);
    assert!(!can_view_ticket(&mut conn, &ctx, ticket.id).expect("can view"));
    assert!(visible_ticket_ids(&mut conn, &ctx, &[ticket.id])
        .expect("visible")
        .is_empty());

    // Confirmed: one ticket.created to the workspace, as submitted.
    let released = tickets::verify_pending_tickets_for_user(&mut conn, guest.uuid).expect("verify");
    assert_eq!(released.len(), 1);
    let events = created_events(&mut conn, ticket.id);
    assert_eq!(events.len(), 2);
    assert!(
        has_workspace_audience(&events[1].0),
        "released: {:?}",
        events[1].0
    );
    assert_eq!(events[1].1, "guest_portal");
    assert!(can_view_ticket(&mut conn, &ctx, ticket.id).expect("can view"));
}

#[test]
fn a_guest_ticket_without_confirmation_reaches_the_workspace_at_once() {
    let db = crate::common::TestDb::new();
    let mut conn = db.conn();
    let guest = crate::common::insert_user(&mut conn, "Guest");
    let agent = crate::common::insert_user(&mut conn, "Agent");
    let ticket = guest_ticket(&mut conn, guest.uuid, false);

    let events = created_events(&mut conn, ticket.id);
    assert_eq!(events.len(), 1);
    assert!(has_workspace_audience(&events[0].0));
    assert!(can_view_ticket(&mut conn, &staff(agent.uuid), ticket.id).expect("can view"));
}
