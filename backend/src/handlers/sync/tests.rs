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
    let agent = TestFixtures::create_user(&mut conn, "sync_push_assign_agent", "agent");
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
