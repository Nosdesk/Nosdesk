//! Integration tests for the sync substrate.

use diesel::prelude::*;
use serde_json::json;
use uuid::Uuid;

use crate::models::{SyncAggregate, SyncOp, UserRole};
use crate::schema::sync_actions;
use crate::sync::actor::ActorContext;
use crate::sync::emit::{self, SyncEmit};
use crate::sync::session;
use crate::sync::system_meta;
use crate::test_helpers::{setup_test_connection, TestFixtures};

#[test]
fn record_inserts_a_row_with_the_actor_attached() {
    let mut conn = setup_test_connection();
    let user = TestFixtures::create_user(&mut conn, "sync_emit_user", UserRole::Admin);
    let actor = ActorContext::user(user.uuid, None);

    let sync_id = conn
        .transaction::<i64, diesel::result::Error, _>(|conn| {
            session::set_actor(conn, &actor)?;
            emit::record(
                conn,
                SyncEmit {
                    aggregate: SyncAggregate::WorkflowState,
                    aggregate_id: "1".to_string(),
                    op: SyncOp::Update,
                    event_type: "workflow_state.renamed",
                    data: json!({ "name": "In Progress" }),
                    groups: vec!["workspace:1".to_string()],
                    causation_id: None,
                },
            )
        })
        .expect("record should insert");

    let (event_type, op, actor_uuid, actor_kind): (String, SyncOp, Option<Uuid>, String) =
        sync_actions::table
            .filter(sync_actions::sync_id.eq(sync_id))
            .select((
                sync_actions::event_type,
                sync_actions::op,
                sync_actions::actor_uuid,
                sync_actions::actor_kind,
            ))
            .first(&mut conn)
            .expect("inserted row should be readable back");

    assert_eq!(event_type, "workflow_state.renamed");
    assert_eq!(op, SyncOp::Update);
    assert_eq!(actor_uuid, Some(user.uuid));
    assert_eq!(actor_kind, "user");
}

#[test]
fn system_actor_records_with_null_uuid_and_a_reference() {
    let mut conn = setup_test_connection();
    let actor = ActorContext::system("scheduler:partition_provisioner");

    let sync_id = conn
        .transaction::<i64, diesel::result::Error, _>(|conn| {
            session::set_actor(conn, &actor)?;
            emit::record(
                conn,
                SyncEmit {
                    aggregate: SyncAggregate::WorkflowState,
                    aggregate_id: "1".to_string(),
                    op: SyncOp::Update,
                    event_type: "workflow_state.system_change",
                    data: json!({}),
                    groups: vec!["workspace:1".to_string()],
                    causation_id: None,
                },
            )
        })
        .expect("record should insert");

    let (actor_uuid, actor_kind, actor_ref): (Option<Uuid>, String, Option<String>) =
        sync_actions::table
            .filter(sync_actions::sync_id.eq(sync_id))
            .select((
                sync_actions::actor_uuid,
                sync_actions::actor_kind,
                sync_actions::actor_ref,
            ))
            .first(&mut conn)
            .expect("inserted row should be readable back");

    assert_eq!(actor_uuid, None);
    assert_eq!(actor_kind, "system");
    assert_eq!(actor_ref, Some("scheduler:partition_provisioner".to_string()));
}

#[test]
fn system_meta_schema_hash_round_trip() {
    let mut conn = setup_test_connection();
    system_meta::set_schema_hash(&mut conn, "abc123").expect("write should succeed");
    let read = system_meta::schema_hash(&mut conn).expect("read should succeed");
    assert_eq!(read, "abc123");
}

#[test]
fn session_set_actor_runs_within_a_transaction() {
    let mut conn = setup_test_connection();
    let user = TestFixtures::create_user(&mut conn, "session_actor_user", UserRole::User);
    let actor = ActorContext::user(user.uuid, None);

    // Should not error. The trigger isn't attached to any table yet
    // so we can't observe the side effect; this test asserts the SQL
    // statement parses and executes against a real connection.
    conn.transaction::<_, diesel::result::Error, _>(|conn| {
        session::set_actor(conn, &actor)?;
        Ok(())
    })
    .expect("set_actor should run inside a tx");
}
