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

/// Push enforces what the REST routes do: someone who is not staff may only
/// change their own request's title, priority, category and due date, and
/// may not touch projects.
#[test]
fn push_limits_non_staff_to_their_own_requests_details() {
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
        |conn: &mut _, t| super::push::apply_transaction_as_requester_for_test(conn, &t, &actor);
    let reason =
        |r: Result<i64, (&'static str, String)>| r.map(|_| "applied").unwrap_or_else(|(r, _)| r);

    let ticket = |id, patch| tx(SyncAggregate::Ticket, id, patch);
    assert_eq!(
        reason(push(
            &mut conn,
            ticket(mine.id, json!({"title": "Renamed"}))
        )),
        "applied"
    );
    assert_eq!(
        reason(push(
            &mut conn,
            ticket(mine.id, json!({"priority": "high"}))
        )),
        "applied"
    );
    for patch in [
        json!({"workflow_state_id": 1}),
        json!({"requester_uuid": other.uuid}),
        json!({"assignee_uuid": member.uuid}),
        json!({"title": "Sneaky", "verification_state": "verified"}),
    ] {
        assert_eq!(
            reason(push(&mut conn, ticket(mine.id, patch.clone()))),
            "forbidden",
            "{patch}"
        );
    }
    assert_eq!(
        reason(push(
            &mut conn,
            ticket(theirs.id, json!({"title": "Mine now"}))
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
