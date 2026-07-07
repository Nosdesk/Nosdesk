//! Integration tests for the sync substrate.

use diesel::prelude::*;
use serde_json::json;
use uuid::Uuid;

use crate::models::{SyncAggregate, SyncOp};
use crate::schema::sync_actions;
use crate::sync::actor::ActorContext;
use crate::sync::emit::{self, SyncEmit};
use crate::sync::session;
use crate::sync::system_meta;
use crate::test_helpers::{setup_test_connection, TestFixtures};

#[test]
fn record_inserts_a_row_with_the_actor_attached() {
    let mut conn = setup_test_connection();
    let user = TestFixtures::create_user(&mut conn, "sync_emit_user", "admin");
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
    assert_eq!(
        actor_ref,
        Some("scheduler:partition_provisioner".to_string())
    );
}

#[test]
fn record_resolves_the_workspace_group_placeholder_from_the_pin() {
    let mut conn = setup_test_connection();
    let user = TestFixtures::create_user(&mut conn, "sync_ws_group_user", "admin");

    // A second workspace, so the assertion can't pass by echoing the
    // ambient test default (workspace 1). sync_actions.workspace_id
    // has an FK to workspaces, so the row must exist — created under
    // the admin role, since the registry table isn't app-writable.
    let ws_id: i32 = session::with_actor_bypass_context(
        &mut conn,
        &ActorContext::system("test:workspace_fixture"),
        |conn| {
            diesel::insert_into(crate::schema::workspaces::table)
                .values((
                    crate::schema::workspaces::slug.eq("groups-resolve"),
                    crate::schema::workspaces::name.eq("Groups Resolve"),
                ))
                .returning(crate::schema::workspaces::id)
                .get_result::<i32>(conn)
        },
    )
    .expect("create second workspace");

    let actor = ActorContext::user(user.uuid, None).with_workspace(ws_id);
    // Read back inside the same pinned transaction — sync_actions is
    // RLS-scoped, so the row is only visible under this pin.
    let (groups, row_workspace_id) = conn
        .transaction::<(Vec<Option<String>>, i32), diesel::result::Error, _>(|conn| {
            session::set_actor(conn, &actor)?;
            let sync_id = emit::record(
                conn,
                SyncEmit {
                    aggregate: SyncAggregate::WorkflowState,
                    aggregate_id: "1".to_string(),
                    op: SyncOp::Update,
                    event_type: "workflow_state.renamed",
                    data: json!({}),
                    groups: vec![
                        crate::sync::groups::WORKSPACE_GROUP.to_string(),
                        "ticket:42".to_string(),
                    ],
                    causation_id: None,
                },
            )?;
            sync_actions::table
                .filter(sync_actions::sync_id.eq(sync_id))
                .select((sync_actions::groups, sync_actions::workspace_id))
                .first(conn)
        })
        .expect("record + readback should succeed");

    // The placeholder resolved to the pinned workspace, the literal
    // ticket group passed through untouched, and the resolved group
    // agrees with the workspace_id column stamped by the same GUC.
    assert_eq!(
        groups,
        vec![
            Some(format!("workspace:{}", ws_id)),
            Some("ticket:42".to_string()),
        ]
    );
    assert_eq!(row_workspace_id, ws_id);
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
    let user = TestFixtures::create_user(&mut conn, "session_actor_user", "user");
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
