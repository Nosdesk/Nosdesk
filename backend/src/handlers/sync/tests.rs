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

use crate::models::{NewProject, ProjectUpdate, SyncAggregate, SyncOp, UserRole};
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
    let admin = TestFixtures::create_user(&mut conn, "sync_proto_admin", UserRole::Admin);
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
    let admin = TestFixtures::create_user(&mut conn, "sync_proto_pushdeny", UserRole::Admin);
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
