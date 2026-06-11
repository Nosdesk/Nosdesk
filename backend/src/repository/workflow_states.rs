//! Workflow state lookups.
//!
//! Workflow states are a small, slow-moving set (typically 6 to ~20 rows) and
//! are read on every ticket fetch to derive the legacy status bucket and the
//! category that downstream code reasons in. We cache the full set in memory
//! behind an `RwLock` and bust the cache on writes.

use std::collections::HashMap;
use std::sync::RwLock;

use chrono::Utc;
use diesel::prelude::*;
use once_cell::sync::Lazy;
use serde_json::json;
use uuid::Uuid;

use crate::db::DbConnection;
use crate::models::{
    NewWorkflowState, SyncAggregate, SyncOp, WorkflowState, WorkflowStateCategory,
    WorkflowStateUpdate,
};
use crate::schema::workflow_states;
use crate::sync::emit::{self, SyncEmit};
use crate::sync::groups;

static CACHE: Lazy<RwLock<Option<HashMap<i32, WorkflowState>>>> = Lazy::new(|| RwLock::new(None));

/// Drop the in-memory cache. Call after any write to `workflow_states` so
/// the next read repopulates from Postgres.
pub fn invalidate_cache() {
    let mut guard = CACHE.write().expect("workflow_states cache poisoned");
    *guard = None;
}

fn load_into_cache(conn: &mut DbConnection) -> QueryResult<HashMap<i32, WorkflowState>> {
    let rows: Vec<WorkflowState> = workflow_states::table
        .order((workflow_states::category, workflow_states::position))
        .load(conn)?;
    let map = rows.into_iter().map(|s| (s.id, s)).collect();
    Ok(map)
}

fn with_cache<R>(
    conn: &mut DbConnection,
    f: impl FnOnce(&HashMap<i32, WorkflowState>) -> R,
) -> QueryResult<R> {
    {
        let guard = CACHE.read().expect("workflow_states cache poisoned");
        if let Some(map) = guard.as_ref() {
            return Ok(f(map));
        }
    }
    let map = load_into_cache(conn)?;
    let out = f(&map);
    let mut guard = CACHE.write().expect("workflow_states cache poisoned");
    *guard = Some(map);
    Ok(out)
}

/// Return every non-archived state, ordered by category then position.
/// Archived states are still returned by `find_by_id` so historical tickets
/// keep resolving, but the listing endpoint filters them out separately.
pub fn list_all(conn: &mut DbConnection) -> QueryResult<Vec<WorkflowState>> {
    with_cache(conn, |map| {
        let mut rows: Vec<WorkflowState> = map.values().cloned().collect();
        rows.sort_by(|a, b| {
            a.category
                .as_str()
                .cmp(b.category.as_str())
                .then_with(|| a.position.cmp(&b.position))
        });
        rows
    })
}

pub fn find_by_id(conn: &mut DbConnection, id: i32) -> QueryResult<Option<WorkflowState>> {
    with_cache(conn, |map| map.get(&id).cloned())
}

pub fn category_of(conn: &mut DbConnection, id: i32) -> QueryResult<Option<WorkflowStateCategory>> {
    with_cache(conn, |map| map.get(&id).map(|s| s.category))
}

/// Best-effort sync helper for hot paths that already hold a fresh cache.
/// Returns `None` if the cache is cold; callers must fall back to the
/// async `category_of` path in that case.
pub fn category_of_cached(id: i32) -> Option<WorkflowStateCategory> {
    let guard = CACHE.read().ok()?;
    guard.as_ref()?.get(&id).map(|s| s.category)
}

/// Return the workspace-default state. There is exactly one row with
/// `is_default = TRUE` (enforced by a partial unique index); fall back to
/// the first Backlog state if the invariant is broken in test data.
pub fn default_state(conn: &mut DbConnection) -> QueryResult<WorkflowState> {
    with_cache(conn, |map| {
        map.values()
            .find(|s| s.is_default && s.archived_at.is_none())
            .cloned()
            .or_else(|| {
                map.values()
                    .filter(|s| {
                        s.category == WorkflowStateCategory::Backlog && s.archived_at.is_none()
                    })
                    .min_by_key(|s| s.position)
                    .cloned()
            })
            .or_else(|| map.values().next().cloned())
            .expect("workflow_states must have at least one row after migration")
    })
}

/// Lowest-position non-archived state in the given category. Used by the
/// legacy status writer paths that say "set ticket to in-progress" without
/// naming a specific state.
pub fn first_in_category(
    conn: &mut DbConnection,
    category: WorkflowStateCategory,
) -> QueryResult<WorkflowState> {
    with_cache(conn, move |map| {
        map.values()
            .filter(|s| s.category == category && s.archived_at.is_none())
            .min_by_key(|s| s.position)
            .cloned()
    })?
    .ok_or(diesel::result::Error::NotFound)
}

/// First-run seeder: insert the default workflow-state catalogue for a
/// freshly-provisioned workspace so ticket creation and triage have
/// states to route through. No-ops when the workspace already holds any
/// workflow state, so re-running provisioning never doubles up or trashes
/// admin edits. Mirrors the rows the initial migration hardcodes for the
/// bootstrap workspace.
///
/// Caller must run inside an actor context pinned to the target workspace;
/// `workspace_id` is supplied by the column default reading
/// `app.workspace_id`. Exactly one default (Backlog); `In Progress` is the
/// only state that doesn't pause the SLA clock.
// sync-audit-only: provisioning seed, not a user-driven write
pub fn seed_defaults_if_empty(
    conn: &mut DbConnection,
    created_by: Option<Uuid>,
) -> QueryResult<usize> {
    use diesel::dsl::count_star;

    let existing: i64 = workflow_states::table.select(count_star()).first(conn)?;
    if existing > 0 {
        return Ok(0);
    }

    // (name, category, color, is_default, pauses_sla) in catalogue order.
    let defaults = [
        (
            "Triage",
            WorkflowStateCategory::Triage,
            "slate",
            false,
            true,
        ),
        (
            "Backlog",
            WorkflowStateCategory::Backlog,
            "gray",
            true,
            true,
        ),
        (
            "In Progress",
            WorkflowStateCategory::Active,
            "blue",
            false,
            false,
        ),
        (
            "In Review",
            WorkflowStateCategory::InReview,
            "purple",
            false,
            true,
        ),
        ("Done", WorkflowStateCategory::Done, "green", false, true),
        (
            "Cancelled",
            WorkflowStateCategory::Cancelled,
            "subtle",
            false,
            true,
        ),
        (
            "Merged",
            WorkflowStateCategory::Merged,
            "subtle",
            false,
            true,
        ),
    ];

    let rows: Vec<NewWorkflowState> = defaults
        .into_iter()
        .enumerate()
        .map(
            |(i, (name, category, color, is_default, pauses_sla))| NewWorkflowState {
                name: name.to_string(),
                category,
                color: color.to_string(),
                position: i as i32,
                is_default,
                created_by,
                pauses_sla,
            },
        )
        .collect();

    let inserted = diesel::insert_into(workflow_states::table)
        .values(&rows)
        .execute(conn)?;
    invalidate_cache();
    Ok(inserted)
}

pub fn create(conn: &mut DbConnection, new: NewWorkflowState) -> QueryResult<WorkflowState> {
    let row = conn.transaction(|conn| {
        let row: WorkflowState = diesel::insert_into(workflow_states::table)
            .values(&new)
            .get_result(conn)?;
        emit::record(
            conn,
            SyncEmit {
                aggregate: SyncAggregate::WorkflowState,
                aggregate_id: row.id.to_string(),
                op: SyncOp::Insert,
                event_type: "workflow_state.created",
                data: json!({
                    "id": row.id,
                    "name": row.name,
                    "category": row.category.as_str(),
                    "color": row.color,
                }),
                groups: groups::workspace(),
                causation_id: None,
            },
        )?;
        Ok::<_, diesel::result::Error>(row)
    })?;
    invalidate_cache();
    Ok(row)
}

pub fn update(
    conn: &mut DbConnection,
    id: i32,
    patch: WorkflowStateUpdate,
) -> QueryResult<WorkflowState> {
    let row = conn.transaction(|conn| {
        let row: WorkflowState = diesel::update(workflow_states::table.find(id))
            .set(&patch)
            .get_result(conn)?;
        emit::record(
            conn,
            SyncEmit {
                aggregate: SyncAggregate::WorkflowState,
                aggregate_id: row.id.to_string(),
                op: SyncOp::Update,
                event_type: "workflow_state.updated",
                data: json!({
                    "id": row.id,
                    "name": row.name,
                    "color": row.color,
                    "position": row.position,
                    "is_default": row.is_default,
                }),
                groups: groups::workspace(),
                causation_id: None,
            },
        )?;
        Ok::<_, diesel::result::Error>(row)
    })?;
    invalidate_cache();
    Ok(row)
}

/// Atomically promote `new_default_id` to be the workspace's default
/// workflow state. Demotes the previous default (if any) and emits
/// both the `workflow_state.default_revoked` event for the prior
/// default and a `workflow_state.default_promoted` event for the new
/// one in the same transaction. Other patch fields (name, color,
/// position) ride along — callers that just want to flip the default
/// without renaming pass an otherwise-empty `patch`.
///
/// This sits in the repo layer rather than the handler so the lint
/// (`tests/sync_emit_lint.rs`) sees the emit, and so an admin script
/// or background job can promote a default with the same emit shape
/// the HTTP handler produces.
pub fn promote_default(
    conn: &mut DbConnection,
    new_default_id: i32,
    patch: WorkflowStateUpdate,
) -> QueryResult<WorkflowState> {
    let row = conn.transaction(|conn| {
        // Demote the existing default (if any), and emit a revoked
        // event for it — but only when the prior default isn't the
        // same row we're about to promote (a no-op promotion to the
        // already-default state shouldn't fire a revoked event).
        let previously_default: Option<i32> = workflow_states::table
            .filter(workflow_states::is_default.eq(true))
            .select(workflow_states::id)
            .first(conn)
            .optional()?;
        diesel::update(workflow_states::table.filter(workflow_states::is_default.eq(true)))
            .set(workflow_states::is_default.eq(false))
            .execute(conn)?;
        if let Some(prev_id) = previously_default {
            if prev_id != new_default_id {
                emit::record(
                    conn,
                    SyncEmit {
                        aggregate: SyncAggregate::WorkflowState,
                        aggregate_id: prev_id.to_string(),
                        op: SyncOp::Update,
                        event_type: "workflow_state.default_revoked",
                        data: json!({ "id": prev_id }),
                        groups: groups::workspace(),
                        causation_id: None,
                    },
                )?;
            }
        }

        // Force is_default in the patch so callers can't accidentally
        // promote-without-promoting.
        let mut patch = patch;
        patch.is_default = Some(true);

        let row: WorkflowState = diesel::update(workflow_states::table.find(new_default_id))
            .set(&patch)
            .get_result(conn)?;
        emit::record(
            conn,
            SyncEmit {
                aggregate: SyncAggregate::WorkflowState,
                aggregate_id: row.id.to_string(),
                op: SyncOp::Update,
                event_type: "workflow_state.default_promoted",
                data: json!({
                    "id": row.id,
                    "name": row.name,
                    "color": row.color,
                    "position": row.position,
                    "is_default": row.is_default,
                }),
                groups: groups::workspace(),
                causation_id: None,
            },
        )?;
        Ok::<_, diesel::result::Error>(row)
    })?;
    invalidate_cache();
    Ok(row)
}

pub fn archive(conn: &mut DbConnection, id: i32) -> QueryResult<WorkflowState> {
    let row = conn.transaction(|conn| {
        let row: WorkflowState = diesel::update(workflow_states::table.find(id))
            .set(workflow_states::archived_at.eq(Some(Utc::now())))
            .get_result(conn)?;
        emit::record(
            conn,
            SyncEmit {
                aggregate: SyncAggregate::WorkflowState,
                aggregate_id: row.id.to_string(),
                op: SyncOp::Archive,
                event_type: "workflow_state.archived",
                data: json!({ "id": row.id }),
                groups: groups::workspace(),
                causation_id: None,
            },
        )?;
        Ok::<_, diesel::result::Error>(row)
    })?;
    invalidate_cache();
    Ok(row)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::SyncAggregate;
    use crate::schema::sync_actions;
    use crate::sync::actor::ActorContext;
    use crate::sync::session;
    use crate::test_helpers::{setup_test_connection, TestFixtures};
    use diesel::dsl::count_star;

    #[test]
    fn seeded_states_are_present() {
        let mut conn = setup_test_connection();
        invalidate_cache();
        let states = list_all(&mut conn).unwrap();
        // Six base states + the `Merged` state seeded by the
        // ticket-merge migration. Bump this alongside any new seeded
        // state and add a presence assertion for it below, so the
        // count and the catalogue stay in lockstep.
        assert_eq!(states.len(), 7);
        assert!(states.iter().any(|s| s.name == "Backlog" && s.is_default));
        assert!(states
            .iter()
            .any(|s| s.category == WorkflowStateCategory::Done));
        assert!(states
            .iter()
            .any(|s| s.category == WorkflowStateCategory::Merged));
    }

    #[test]
    fn first_in_category_resolves_seeded_states() {
        let mut conn = setup_test_connection();
        invalidate_cache();
        let backlog = first_in_category(&mut conn, WorkflowStateCategory::Backlog).unwrap();
        assert_eq!(backlog.category, WorkflowStateCategory::Backlog);
        let active = first_in_category(&mut conn, WorkflowStateCategory::Active).unwrap();
        assert_eq!(active.category, WorkflowStateCategory::Active);
        let done = first_in_category(&mut conn, WorkflowStateCategory::Done).unwrap();
        assert_eq!(done.category, WorkflowStateCategory::Done);
    }

    #[test]
    fn default_state_is_backlog() {
        let mut conn = setup_test_connection();
        invalidate_cache();
        let s = default_state(&mut conn).unwrap();
        assert_eq!(s.category, WorkflowStateCategory::Backlog);
        assert!(s.is_default);
    }

    #[test]
    fn create_emits_a_sync_action_with_actor_from_session() {
        let mut conn = setup_test_connection();
        invalidate_cache();
        let user = TestFixtures::create_user(&mut conn, "wf_emit_admin", "admin");
        let actor = ActorContext::user(user.uuid, None);

        let created = conn
            .transaction::<_, diesel::result::Error, _>(|conn| {
                session::set_actor(conn, &actor)?;
                let before: i64 = sync_actions::table
                    .filter(sync_actions::aggregate.eq(SyncAggregate::WorkflowState))
                    .select(count_star())
                    .first(conn)?;
                let new = NewWorkflowState {
                    name: "Investigating".into(),
                    category: WorkflowStateCategory::Active,
                    color: "blue".into(),
                    position: 99,
                    is_default: false,
                    created_by: Some(user.uuid),
                    pauses_sla: false,
                };
                let created = create(conn, new)?;
                let after: i64 = sync_actions::table
                    .filter(sync_actions::aggregate.eq(SyncAggregate::WorkflowState))
                    .filter(sync_actions::aggregate_id.eq(created.id.to_string()))
                    .filter(sync_actions::event_type.eq("workflow_state.created"))
                    .filter(sync_actions::actor_uuid.eq(Some(user.uuid)))
                    .select(count_star())
                    .first(conn)?;
                assert_eq!(after, 1);
                let total_after: i64 = sync_actions::table
                    .filter(sync_actions::aggregate.eq(SyncAggregate::WorkflowState))
                    .select(count_star())
                    .first(conn)?;
                assert_eq!(total_after, before + 1);
                Ok(created)
            })
            .unwrap();
        // Touch `created` so the binding is exercised and can't be
        // accidentally dropped by a future refactor.
        assert!(created.id > 0);
    }
}
