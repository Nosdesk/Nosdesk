//! A new ticket goes through the assignment rules whichever way it arrives.
//! `assign_new_ticket` is what the app, the portal, the guest form and the
//! email pipeline all call, so it is tested once here.

use diesel::prelude::*;
use serde_json::{json, Value};

use backend::models::{AssignmentMethod, NewAssignmentRule, NewTicket, Ticket};
use backend::schema::{assignment_rules, sync_actions, tickets, workflow_states};
use backend::services::ticket_updates::{assign_new_ticket, ActorConn};
use backend::sync::actor::ActorContext;
use backend::sync::session::with_actor_context;

use crate::common;

#[test]
fn a_new_ticket_is_assigned_by_the_rules_and_the_assignee_is_told() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let seeded = common::seed_two_workspaces(&mut db.pool_with_size(2).get().expect("conn"));
    let a = &seeded.a;
    let mut conn = db.pool_with_size(1).get().expect("conn");

    let admin = ActorContext::user(a.admin_uuid, None).with_workspace(a.workspace_id);
    let ticket: Ticket = with_actor_context(&mut conn, &admin, |c| {
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
            .execute(c)?;
        let open_state: i32 = workflow_states::table
            .filter(workflow_states::workspace_id.eq(a.workspace_id))
            .filter(workflow_states::is_default.eq(true))
            .select(workflow_states::id)
            .first(c)?;
        diesel::insert_into(tickets::table)
            .values(&NewTicket {
                title: "Printer jam".into(),
                workflow_state_id: open_state,
                requester_uuid: Some(a.member_uuid),
                ..Default::default()
            })
            .get_result(c)
    })
    .expect("seed rule and ticket");

    // The email pipeline runs the rules as the system.
    let system = ActorContext::system("assignment_rules").with_workspace(a.workspace_id);
    let assigned = assign_new_ticket(
        &mut ActorConn {
            conn: &mut conn,
            actor: &system,
        },
        None,
        ticket,
    );
    assert_eq!(assigned.assignee_uuid, Some(a.admin_uuid));

    // The assignment went out as a change with the previous assignee, which
    // is what the notification deriver reads to tell the new assignee.
    let events: Vec<(String, Value)> = sync_actions::table
        .filter(sync_actions::aggregate_id.eq(assigned.id.to_string()))
        .select((sync_actions::event_type, sync_actions::data))
        .load(&mut conn)
        .expect("sync actions");
    assert!(
        events
            .iter()
            .any(|(kind, data)| kind == "ticket.assignee_changed"
                && data["assignee_uuid"] == json!(a.admin_uuid)
                && data.get("previous_assignee_uuid").is_some()),
        "{events:?}"
    );
}
