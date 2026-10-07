//! End-to-end protocol smoke tests.
//!
//! Exercises the bootstrap / delta / push handlers against a real
//! database via setup_test_connection. Doesn't go through actix-web
//! to keep the test surface narrow — the handlers are thin enough
//! that the value here is in the SQL + emit pipeline behaviour, not
//! the HTTP routing.

#![allow(dead_code)]

use diesel::prelude::*;
use serde_json::json;
use uuid::Uuid;

use crate::models::{NewProject, ProjectUpdate, SyncAggregate, SyncOp};
use crate::repository::projects;
use crate::schema::sync_actions;
use crate::sync::actor::ActorContext;
use crate::sync::session;
use crate::test_helpers::{setup_test_connection, TestFixtures};

/// A round-trip fixture: create a project, update it, pull a delta
/// from the resulting sync_actions and assert both events appear.
#[test]
fn delta_returns_events_for_user_visible_groups() {
    let mut conn = setup_test_connection();
    let admin = TestFixtures::create_user(&mut conn, "sync_proto_admin", "admin");
    let actor = ActorContext::user(admin.uuid, None);

    // Establish a baseline cursor before any of our writes.
    let baseline: i64 = sync_actions::table
        .select(diesel::dsl::max(sync_actions::sync_id))
        .first::<Option<i64>>(&mut conn)
        .unwrap()
        .unwrap_or(0);

    let project = conn
        .transaction::<_, diesel::result::Error, _>(|conn| {
            session::set_actor(conn, &actor)?;
            projects::create_project(
                conn,
                NewProject {
                    name: format!("p-{}", Uuid::now_v7()),
                    description: None,
                    status: crate::models::ProjectStatus::Active,
                    start_date: None,
                    end_date: None,
                },
                None,
            )
        })
        .unwrap();

    conn.transaction::<_, diesel::result::Error, _>(|conn| {
        session::set_actor(conn, &actor)?;
        projects::update_project(
            conn,
            project.id,
            ProjectUpdate {
                name: Some("renamed".into()),
                description: None,
                status: None,
                start_date: None,
                end_date: None,
                updated_at: None,
            },
            None,
        )?;
        Ok(())
    })
    .unwrap();

    // Two events: project.created + project.updated, both tagged with
    // workspace + project:<id> groups.
    let project_group = format!("project:{}", project.id);
    let group_filter: Vec<Option<String>> = vec![Some(project_group.clone())];
    let events: Vec<(SyncAggregate, SyncOp, String)> = sync_actions::table
        .filter(sync_actions::sync_id.gt(baseline))
        .filter(sync_actions::groups.overlaps_with(group_filter))
        .order(sync_actions::sync_id.asc())
        .select((
            sync_actions::aggregate,
            sync_actions::op,
            sync_actions::event_type,
        ))
        .load(&mut conn)
        .unwrap();

    assert!(
        events.len() >= 2,
        "expected at least two project events, got {events:?}"
    );
    assert_eq!(events[0].0, SyncAggregate::Project);
    assert_eq!(events[0].1, SyncOp::Insert);
    assert_eq!(events[0].2, "project.created");
    let updated_idx = events
        .iter()
        .position(|e| e.2 == "project.updated")
        .expect("should have project.updated event");
    assert_eq!(events[updated_idx].0, SyncAggregate::Project);
    assert_eq!(events[updated_idx].1, SyncOp::Update);
}

/// Push-handler unit test: the apply_transaction dispatch returns
/// the assigned sync_id on success, and rejects unsupported aggregate
/// variants without touching the DB.
#[test]
fn push_rejects_unsupported_aggregate() {
    use super::push::PushTransaction;
    let mut conn = setup_test_connection();
    let admin = TestFixtures::create_user(&mut conn, "sync_proto_pushdeny", "admin");
    let actor = ActorContext::user(admin.uuid, None);

    let tx = PushTransaction {
        tx_id: Uuid::now_v7().to_string(),
        aggregate: SyncAggregate::Comment,
        model_id: "1".into(),
        op: SyncOp::Update,
        patch: json!({}),
        base_sync_id: None,
    };

    let result = super::push::apply_transaction_for_test(&mut conn, &tx, &actor);
    assert!(result.is_err());
    let (reason, _detail) = result.unwrap_err();
    assert_eq!(reason, "unsupported_aggregate");
}

/// A ticket-update push carrying `tag_ids` applies the tag set via the
/// join-table path (B4), while `watcher_uuids` is refused (it stays on the
/// dedicated per-user watch endpoint).
#[test]
fn push_ticket_applies_tag_ids_and_rejects_watchers() {
    use super::push::PushTransaction;
    use crate::models::NewTag;
    use crate::schema::ticket_tags;

    let mut conn = setup_test_connection();
    let admin = TestFixtures::create_user(&mut conn, "sync_push_tags", "admin");
    let actor = ActorContext::user(admin.uuid, None).with_workspace(1);

    let (ticket_id, tag_id) = conn
        .transaction::<_, diesel::result::Error, _>(|conn| {
            session::set_actor(conn, &actor)?;
            let ticket = TestFixtures::create_ticket(conn, "tag me", Some(admin.uuid), None);
            let tag = crate::repository::tags::create_tag(
                conn,
                NewTag {
                    name: format!("t-{}", Uuid::now_v7()),
                    color: None,
                    description: None,
                },
            )?;
            Ok((ticket.id, tag.id))
        })
        .unwrap();

    // Push a `tag_ids` patch — the tag set is applied on the ticket.
    let tag_tx = PushTransaction {
        tx_id: Uuid::now_v7().to_string(),
        aggregate: SyncAggregate::Ticket,
        model_id: ticket_id.to_string(),
        op: SyncOp::Update,
        patch: json!({ "tag_ids": [tag_id] }),
        base_sync_id: None,
    };
    super::push::apply_transaction_for_test(&mut conn, &tag_tx, &actor)
        .expect("tag_ids patch should apply");

    let attached: Vec<i32> = ticket_tags::table
        .filter(ticket_tags::ticket_id.eq(ticket_id))
        .select(ticket_tags::tag_id)
        .load(&mut conn)
        .unwrap();
    assert!(
        attached.contains(&tag_id),
        "tag should be attached via the sync-push path"
    );

    // A `watcher_uuids` patch is refused — watch is a per-user toggle.
    let watch_tx = PushTransaction {
        tx_id: Uuid::now_v7().to_string(),
        aggregate: SyncAggregate::Ticket,
        model_id: ticket_id.to_string(),
        op: SyncOp::Update,
        patch: json!({ "watcher_uuids": [admin.uuid.to_string()] }),
        base_sync_id: None,
    };
    let (reason, _) = super::push::apply_transaction_for_test(&mut conn, &watch_tx, &actor)
        .expect_err("watcher_uuids must be refused on the sync-push path");
    assert_eq!(reason, "unsupported_field");
}

/// The regression behind the notification outbox: an assignment made through
/// the sync push path used to notify nobody, because `TicketAssigned` was
/// raised only by the REST handler. The emitted row now carries the
/// before/after pair, the trigger enqueues it, and the deriver turns it into a
/// `TicketAssigned` payload for the assignee.
#[test]
fn push_assignment_reaches_the_notification_outbox() {
    use super::push::PushTransaction;
    use crate::schema::notification_outbox;
    use crate::services::notifications::deriver::{derive, resolve, Intent, SyncActionRow};
    use crate::services::notifications::types::NotificationTypeCode;

    let mut conn = setup_test_connection();
    let admin = TestFixtures::create_user(&mut conn, "sync_push_assign_actor", "admin");
    let agent = TestFixtures::create_user(&mut conn, "sync_push_assign_agent", "technician");
    let actor = ActorContext::user(admin.uuid, None).with_workspace(1);
    let ticket_id = conn
        .transaction::<_, diesel::result::Error, _>(|conn| {
            session::set_actor(conn, &actor)?;
            Ok(TestFixtures::create_ticket(conn, "assign me", Some(admin.uuid), None).id)
        })
        .unwrap();

    let tx = PushTransaction {
        tx_id: Uuid::now_v7().to_string(),
        aggregate: SyncAggregate::Ticket,
        model_id: ticket_id.to_string(),
        op: SyncOp::Update,
        patch: json!({ "assignee_uuid": agent.uuid.to_string() }),
        base_sync_id: None,
    };
    let sync_id = super::push::apply_transaction_for_test(&mut conn, &tx, &actor)
        .expect("assignment patch should apply");

    // The trigger enqueued it.
    let enqueued: i64 = notification_outbox::table
        .filter(notification_outbox::sync_id.eq(sync_id))
        .count()
        .get_result(&mut conn)
        .unwrap();
    assert_eq!(enqueued, 1, "the assignment row is in the outbox");

    // And the row derives to exactly one assignment for the agent, by the admin.
    let (workspace_id, event_type, data, actor_uuid, actor_kind, occurred_at) = sync_actions::table
        .filter(sync_actions::sync_id.eq(sync_id))
        .select((
            sync_actions::workspace_id,
            sync_actions::event_type,
            sync_actions::data,
            sync_actions::actor_uuid,
            sync_actions::actor_kind,
            sync_actions::occurred_at,
        ))
        .first::<(
            i32,
            String,
            serde_json::Value,
            Option<Uuid>,
            String,
            chrono::DateTime<chrono::Utc>,
        )>(&mut conn)
        .unwrap();
    let row = SyncActionRow {
        sync_id,
        workspace_id,
        event_type,
        data,
        actor_uuid,
        actor_kind,
        occurred_at,
    };
    let intents = derive(&row);
    assert_eq!(intents.len(), 1);
    assert!(matches!(intents[0], Intent::Assigned { assignee, .. } if assignee == agent.uuid));

    let payloads = resolve(&mut conn, &row, intents[0].clone()).unwrap();
    assert_eq!(payloads.len(), 1);
    assert_eq!(
        payloads[0].notification_type,
        NotificationTypeCode::TicketAssigned
    );
    assert_eq!(payloads[0].recipient_uuid, agent.uuid);
    assert_eq!(payloads[0].actor.uuid, admin.uuid);
    assert_eq!(payloads[0].source_sync_id, Some(sync_id));

    // Re-pushing the same assignee is a no-op for notifications.
    let again = PushTransaction {
        tx_id: Uuid::now_v7().to_string(),
        aggregate: SyncAggregate::Ticket,
        model_id: ticket_id.to_string(),
        op: SyncOp::Update,
        patch: json!({ "assignee_uuid": agent.uuid.to_string() }),
        base_sync_id: None,
    };
    let again_id = super::push::apply_transaction_for_test(&mut conn, &again, &actor).unwrap();
    let enqueued_again: i64 = notification_outbox::table
        .filter(notification_outbox::sync_id.eq(again_id))
        .count()
        .get_result(&mut conn)
        .unwrap();
    assert_eq!(enqueued_again, 0, "an unchanged assignee enqueues nothing");
}

/// Push enforces what the REST routes do: someone who is not staff may only
/// retitle a ticket they can see, and may not touch projects.
/// A staff push carries only the ticket columns a client owns: the server sets
/// `closed_at` from the state, and provenance, triage and the spam flag aren't
/// the client's. Assignees and categories are checked as on the REST PATCH.
#[test]
fn push_takes_only_the_ticket_columns_a_client_owns() {
    use super::push::PushTransaction;
    use crate::models::WorkflowStateCategory;
    use crate::schema::{tickets, workflow_states};

    let mut conn = setup_test_connection();
    let admin = TestFixtures::create_user(&mut conn, "sync_push_columns_admin", "admin");
    let member = TestFixtures::create_user(&mut conn, "sync_push_columns_member", "user");
    let ticket = TestFixtures::create_ticket(&mut conn, "Printer jammed", Some(member.uuid), None);
    let actor = ActorContext::user(admin.uuid, None).with_workspace(1);
    let push = |conn: &mut crate::db::DbConnection, patch| {
        let tx = PushTransaction {
            tx_id: Uuid::now_v7().to_string(),
            aggregate: SyncAggregate::Ticket,
            model_id: ticket.id.to_string(),
            op: SyncOp::Update,
            patch,
            base_sync_id: None,
        };
        super::push::apply_transaction_for_test(conn, &tx, &actor)
            .map(|_| "applied")
            .unwrap_or_else(|(reason, _)| reason)
    };

    for patch in [
        json!({ "closed_at": "2020-01-01T00:00:00" }),
        json!({ "verification_state": "verified" }),
        json!({ "origin_channel_id": 1 }),
        json!({ "triage_state": "triaged" }),
        json!({ "recurrence_template_id": 1 }),
        json!({ "spam_suspected": true }),
    ] {
        assert_eq!(
            push(&mut conn, patch.clone()),
            "unsupported_field",
            "{patch}"
        );
    }
    // "Not spam" is the one server column a client may set, and it lands.
    diesel::update(tickets::table.find(ticket.id))
        .set(tickets::spam_suspected.eq(true))
        .execute(&mut conn)
        .expect("flag it");
    assert_eq!(
        push(&mut conn, json!({ "spam_suspected": false })),
        "applied"
    );
    let flagged: bool = tickets::table
        .find(ticket.id)
        .select(tickets::spam_suspected)
        .first(&mut conn)
        .expect("reload ticket");
    assert!(!flagged, "not spam clears the flag");
    assert_eq!(
        push(
            &mut conn,
            json!({ "assignee_uuid": member.uuid.to_string() })
        ),
        "invalid_assignee",
        "a member can't be assigned"
    );

    let state_in = |conn: &mut crate::db::DbConnection, category: WorkflowStateCategory| -> i32 {
        workflow_states::table
            .filter(workflow_states::workspace_id.eq(1))
            .filter(workflow_states::category.eq(category))
            .select(workflow_states::id)
            .first(conn)
            .expect("a seeded state in the category")
    };
    let closed_at = |conn: &mut crate::db::DbConnection| -> Option<chrono::DateTime<chrono::Utc>> {
        tickets::table
            .find(ticket.id)
            .select(tickets::closed_at)
            .first(conn)
            .expect("reload ticket")
    };
    let done = state_in(&mut conn, WorkflowStateCategory::Done);
    let backlog = state_in(&mut conn, WorkflowStateCategory::Backlog);
    assert_eq!(
        push(&mut conn, json!({ "workflow_state_id": done })),
        "applied"
    );
    assert!(closed_at(&mut conn).is_some(), "closing records when");
    assert_eq!(
        push(&mut conn, json!({ "workflow_state_id": backlog })),
        "applied"
    );
    assert!(closed_at(&mut conn).is_none(), "reopening clears it");
}

/// Only a change of assignee is checked: a ticket whose assignee has since
/// become a requester can still be retitled and moved through push.
#[test]
fn push_edits_a_ticket_whose_assignee_was_demoted() {
    use super::push::PushTransaction;
    use crate::models::WorkflowStateCategory;
    use crate::schema::{tickets, workflow_states};

    let mut conn = setup_test_connection();
    let admin = TestFixtures::create_user(&mut conn, "sync_push_demoted_admin", "admin");
    let former = TestFixtures::create_user(&mut conn, "sync_push_demoted_agent", "user");
    let ticket = TestFixtures::create_ticket(&mut conn, "Printer jammed", Some(admin.uuid), None);
    diesel::update(tickets::table.find(ticket.id))
        .set(tickets::assignee_uuid.eq(former.uuid))
        .execute(&mut conn)
        .expect("assigned before they were demoted");
    let done: i32 = workflow_states::table
        .filter(workflow_states::workspace_id.eq(1))
        .filter(workflow_states::category.eq(WorkflowStateCategory::Done))
        .select(workflow_states::id)
        .first(&mut conn)
        .expect("a done state");

    let actor = ActorContext::user(admin.uuid, None).with_workspace(1);
    for patch in [
        json!({ "title": "Printer still jammed" }),
        json!({ "workflow_state_id": done }),
        json!({ "assignee_uuid": former.uuid.to_string(), "title": "Printer fixed" }),
    ] {
        let tx = PushTransaction {
            tx_id: Uuid::now_v7().to_string(),
            aggregate: SyncAggregate::Ticket,
            model_id: ticket.id.to_string(),
            op: SyncOp::Update,
            patch: patch.clone(),
            base_sync_id: None,
        };
        assert!(
            super::push::apply_transaction_for_test(&mut conn, &tx, &actor).is_ok(),
            "{patch}"
        );
    }
    let (title, state, assignee): (String, i32, Option<Uuid>) = tickets::table
        .find(ticket.id)
        .select((
            tickets::title,
            tickets::workflow_state_id,
            tickets::assignee_uuid,
        ))
        .first(&mut conn)
        .expect("reload ticket");
    assert_eq!(title, "Printer fixed");
    assert_eq!(state, done);
    assert_eq!(assignee, Some(former.uuid));
}

/// Closing a recurring ticket through push creates its next occurrence, as the
/// REST PATCH does.
#[test]
fn push_closing_a_recurring_ticket_creates_the_next_one() {
    use super::push::PushTransaction;
    use crate::models::WorkflowStateCategory;
    use crate::schema::{tickets, workflow_states};

    let mut conn = setup_test_connection();
    let admin = TestFixtures::create_user(&mut conn, "sync_push_recur_admin", "admin");
    let ticket =
        TestFixtures::create_ticket(&mut conn, "Check the backups", Some(admin.uuid), None);
    let due = ticket.created_at + chrono::Duration::days(1);
    diesel::update(tickets::table.find(ticket.id))
        .set((
            tickets::recurrence_rule.eq("FREQ=WEEKLY"),
            tickets::due_date.eq(due),
        ))
        .execute(&mut conn)
        .expect("make it recur");
    let done: i32 = workflow_states::table
        .filter(workflow_states::workspace_id.eq(1))
        .filter(workflow_states::category.eq(WorkflowStateCategory::Done))
        .select(workflow_states::id)
        .first(&mut conn)
        .expect("a done state");

    let actor = ActorContext::user(admin.uuid, None).with_workspace(1);
    let tx = PushTransaction {
        tx_id: Uuid::now_v7().to_string(),
        aggregate: SyncAggregate::Ticket,
        model_id: ticket.id.to_string(),
        op: SyncOp::Update,
        patch: json!({ "workflow_state_id": done }),
        base_sync_id: None,
    };
    super::push::apply_transaction_for_test(&mut conn, &tx, &actor).expect("close it");

    let next: Vec<Option<chrono::NaiveDateTime>> = tickets::table
        .filter(tickets::recurrence_template_id.eq(ticket.id))
        .select(tickets::due_date)
        .load(&mut conn)
        .expect("next occurrence");
    assert_eq!(next.len(), 1, "one next occurrence");
    assert!(next[0].is_some_and(|d| d > due));
}

/// A category change through push runs the assignment rules for an
/// unassigned ticket, as the REST PATCH does.
#[test]
fn push_category_change_runs_the_assignment_rules() {
    use super::push::PushTransaction;
    use crate::models::{AssignmentMethod, NewAssignmentRule};
    use crate::schema::{ticket_categories, tickets};

    let mut conn = setup_test_connection();
    let admin = TestFixtures::create_user(&mut conn, "sync_push_route_admin", "admin");
    let agent = TestFixtures::create_user(&mut conn, "sync_push_route_agent", "technician");
    let ticket =
        TestFixtures::create_ticket(&mut conn, "Laptop won't boot", Some(admin.uuid), None);
    let hardware: i32 = diesel::insert_into(ticket_categories::table)
        .values(ticket_categories::name.eq("Hardware (push routing)"))
        .returning(ticket_categories::id)
        .get_result(&mut conn)
        .expect("category");
    crate::repository::assignment_rules::create_rule(
        &mut conn,
        NewAssignmentRule {
            name: "Hardware to the desk".to_string(),
            description: None,
            priority: 1,
            is_active: true,
            method: AssignmentMethod::DirectUser,
            target_user_uuid: Some(agent.uuid),
            target_group_id: None,
            trigger_on_create: false,
            trigger_on_category_change: true,
            category_id: Some(hardware),
            conditions: None,
            created_by: None,
        },
    )
    .expect("rule");

    let actor = ActorContext::user(admin.uuid, None).with_workspace(1);
    let tx = PushTransaction {
        tx_id: Uuid::now_v7().to_string(),
        aggregate: SyncAggregate::Ticket,
        model_id: ticket.id.to_string(),
        op: SyncOp::Update,
        patch: json!({ "category_id": hardware }),
        base_sync_id: None,
    };
    super::push::apply_transaction_for_test(&mut conn, &tx, &actor).expect("recategorise");

    let assignee: Option<Uuid> = tickets::table
        .find(ticket.id)
        .select(tickets::assignee_uuid)
        .first(&mut conn)
        .expect("reload");
    assert_eq!(assignee, Some(agent.uuid));
}

#[test]
fn push_limits_non_staff_to_retitling_their_own_ticket() {
    use super::push::PushTransaction;
    let mut conn = setup_test_connection();
    let member = TestFixtures::create_user(&mut conn, "sync_push_member", "user");
    let other = TestFixtures::create_user(&mut conn, "sync_push_other", "user");
    let mine = TestFixtures::create_ticket(&mut conn, "Mine", Some(member.uuid), None);
    let theirs = TestFixtures::create_ticket(&mut conn, "Theirs", Some(other.uuid), None);
    let actor = ActorContext::user(member.uuid, None);
    let tx = |aggregate, id: i32, patch| PushTransaction {
        tx_id: Uuid::now_v7().to_string(),
        aggregate,
        model_id: id.to_string(),
        op: SyncOp::Update,
        patch,
        base_sync_id: None,
    };
    let push =
        |conn: &mut _, t| super::push::apply_transaction_as_non_staff_for_test(conn, &t, &actor);

    let reason =
        |r: Result<i64, (&'static str, String)>| r.map(|_| "applied").unwrap_or_else(|(r, _)| r);
    assert_eq!(
        reason(push(
            &mut conn,
            tx(SyncAggregate::Ticket, mine.id, json!({"title": "Renamed"}))
        )),
        "applied"
    );
    assert_eq!(
        reason(push(
            &mut conn,
            tx(
                SyncAggregate::Ticket,
                mine.id,
                json!({"priority": "urgent"})
            )
        )),
        "forbidden"
    );
    assert_eq!(
        reason(push(
            &mut conn,
            tx(
                SyncAggregate::Ticket,
                theirs.id,
                json!({"title": "Mine now"})
            )
        )),
        "forbidden"
    );
    assert_eq!(
        reason(push(
            &mut conn,
            tx(SyncAggregate::Ticket, mine.id, json!({"tag_ids": []}))
        )),
        "forbidden"
    );
    assert_eq!(
        reason(push(
            &mut conn,
            tx(SyncAggregate::Project, 1, json!({"name": "x"}))
        )),
        "forbidden"
    );
}
