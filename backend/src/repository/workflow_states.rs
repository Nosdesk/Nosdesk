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

use crate::db::DbConnection;
use crate::models::{NewWorkflowState, WorkflowState, WorkflowStateCategory, WorkflowStateUpdate};
use crate::schema::workflow_states;

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

pub fn legacy_status_of(conn: &mut DbConnection, id: i32) -> QueryResult<&'static str> {
    let cat = category_of(conn, id)?.unwrap_or(WorkflowStateCategory::Backlog);
    Ok(cat.legacy_status())
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
                    .filter(|s| s.category == WorkflowStateCategory::Backlog && s.archived_at.is_none())
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

/// Map a legacy status string (`open` / `in-progress` / `closed`) to a
/// concrete workflow state id. Picks the lowest-position state in the
/// canonical category for that bucket: `open → backlog`, `in-progress →
/// active`, `closed → done`.
pub fn state_for_legacy_status(conn: &mut DbConnection, status: &str) -> QueryResult<WorkflowState> {
    let category = match status {
        "open" => WorkflowStateCategory::Backlog,
        "in-progress" => WorkflowStateCategory::Active,
        "closed" => WorkflowStateCategory::Done,
        _ => WorkflowStateCategory::Backlog,
    };
    first_in_category(conn, category)
}

pub fn create(conn: &mut DbConnection, new: NewWorkflowState) -> QueryResult<WorkflowState> {
    let row = diesel::insert_into(workflow_states::table)
        .values(&new)
        .get_result(conn)?;
    invalidate_cache();
    Ok(row)
}

pub fn update(
    conn: &mut DbConnection,
    id: i32,
    patch: WorkflowStateUpdate,
) -> QueryResult<WorkflowState> {
    let row = diesel::update(workflow_states::table.find(id))
        .set(&patch)
        .get_result(conn)?;
    invalidate_cache();
    Ok(row)
}

pub fn archive(conn: &mut DbConnection, id: i32) -> QueryResult<WorkflowState> {
    let row = diesel::update(workflow_states::table.find(id))
        .set(workflow_states::archived_at.eq(Some(Utc::now())))
        .get_result(conn)?;
    invalidate_cache();
    Ok(row)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::setup_test_connection;

    #[test]
    fn seeded_states_are_present() {
        let mut conn = setup_test_connection();
        invalidate_cache();
        let states = list_all(&mut conn).unwrap();
        assert_eq!(states.len(), 6);
        assert!(states.iter().any(|s| s.name == "Backlog" && s.is_default));
        assert!(states
            .iter()
            .any(|s| s.category == WorkflowStateCategory::Done));
    }

    #[test]
    fn legacy_status_mapping_buckets_correctly() {
        let mut conn = setup_test_connection();
        invalidate_cache();
        let backlog = state_for_legacy_status(&mut conn, "open").unwrap();
        assert_eq!(backlog.category, WorkflowStateCategory::Backlog);
        let active = state_for_legacy_status(&mut conn, "in-progress").unwrap();
        assert_eq!(active.category, WorkflowStateCategory::Active);
        let done = state_for_legacy_status(&mut conn, "closed").unwrap();
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
}
