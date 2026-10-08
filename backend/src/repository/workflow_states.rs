//! Workflow state lookups.
//!
//! Every read goes to the database, so it follows the caller's workspace and
//! sees edits made through any server process. A workspace has a small set
//! (typically 6 to ~20 rows); a caller resolving many states at once uses
//! [`categories`] for a single query.
//!
//! The one thing kept in memory is each state's category by id, for hot paths
//! without a connection ([`category_of_cached`]). That is safe to share across
//! workspaces and processes: a state's category is fixed when it is created,
//! and ids are never reused.

use std::collections::HashMap;
use std::sync::RwLock;

use chrono::Utc;
use diesel::prelude::*;
use once_cell::sync::Lazy;
use serde_json::json;
use uuid::Uuid;

use super::pinned_workspace;
use crate::db::DbConnection;
use crate::models::{
    NewWorkflowState, SyncAggregate, SyncOp, WorkflowState, WorkflowStateCategory,
    WorkflowStateUpdate,
};
use crate::schema::workflow_states;
use crate::sync::emit::{self, SyncEmit};
use crate::sync::groups;

/// The category of every state this process has read, by id.
static CATEGORIES: Lazy<RwLock<HashMap<i32, WorkflowStateCategory>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));

fn remember(categories: impl IntoIterator<Item = (i32, WorkflowStateCategory)>) {
    if let Ok(mut map) = CATEGORIES.write() {
        map.extend(categories);
    }
}

/// The pinned workspace's states, by category then position.
fn workspace_states(conn: &mut DbConnection) -> QueryResult<Vec<WorkflowState>> {
    let rows: Vec<WorkflowState> = workflow_states::table
        .filter(workflow_states::workspace_id.eq(pinned_workspace()))
        .order((workflow_states::category, workflow_states::position))
        .load(conn)?;
    remember(rows.iter().map(|s| (s.id, s.category)));
    Ok(rows)
}

/// Every state in the pinned workspace, archived ones included (the listing
/// endpoint filters those out), ordered by category then position. Archived
/// states stay resolvable by `find_by_id` so historical tickets keep their
/// state.
pub fn list_all(conn: &mut DbConnection) -> QueryResult<Vec<WorkflowState>> {
    let mut rows = workspace_states(conn)?;
    rows.sort_by(|a, b| {
        a.category
            .as_str()
            .cmp(b.category.as_str())
            .then_with(|| a.position.cmp(&b.position))
    });
    Ok(rows)
}

/// A state by id, as the caller's row security allows: on a tenant connection,
/// a state in another workspace is `None`.
pub fn find_by_id(conn: &mut DbConnection, id: i32) -> QueryResult<Option<WorkflowState>> {
    let row: Option<WorkflowState> = workflow_states::table.find(id).first(conn).optional()?;
    remember(row.iter().map(|s| (s.id, s.category)));
    Ok(row)
}

/// A state's category, as visible as in [`find_by_id`]. Always read from the
/// database, never from memory, since the id may come from a request.
pub fn category_of(conn: &mut DbConnection, id: i32) -> QueryResult<Option<WorkflowStateCategory>> {
    let category: Option<WorkflowStateCategory> = workflow_states::table
        .find(id)
        .select(workflow_states::category)
        .first(conn)
        .optional()?;
    remember(category.map(|c| (id, c)));
    Ok(category)
}

/// The pinned workspace's categories by state id, in one query, for a caller
/// resolving many states at once.
pub fn categories(conn: &mut DbConnection) -> QueryResult<HashMap<i32, WorkflowStateCategory>> {
    let rows: Vec<(i32, WorkflowStateCategory)> = workflow_states::table
        .filter(workflow_states::workspace_id.eq(pinned_workspace()))
        .select((workflow_states::id, workflow_states::category))
        .load(conn)?;
    remember(rows.iter().copied());
    Ok(rows.into_iter().collect())
}

/// Read the category of every state the connection can see (on an elevated
/// connection, every workspace's), so [`category_of_cached`] knows them.
pub fn remember_visible_categories(conn: &mut DbConnection) -> QueryResult<()> {
    let rows: Vec<(i32, WorkflowStateCategory)> = workflow_states::table
        .select((workflow_states::id, workflow_states::category))
        .load(conn)?;
    remember(rows);
    Ok(())
}

/// A state's category from memory, for hot paths without a connection. Known
/// for any state this process has read; `None` otherwise, and the caller falls
/// back. It answers for any workspace, so pass only an id read from a row the
/// caller may see, never one taken from a request.
pub fn category_of_cached(id: i32) -> Option<WorkflowStateCategory> {
    CATEGORIES.read().ok()?.get(&id).copied()
}

/// The pinned workspace's default state. There is exactly one row with
/// `is_default = TRUE` (enforced by a partial unique index); fall back to
/// the first Backlog state if the invariant is broken in test data.
pub fn default_state(conn: &mut DbConnection) -> QueryResult<WorkflowState> {
    let states = workspace_states(conn)?;
    let live = || states.iter().filter(|s| s.archived_at.is_none());
    live()
        .find(|s| s.is_default)
        .or_else(|| {
            live()
                .filter(|s| s.category == WorkflowStateCategory::Backlog)
                .min_by_key(|s| s.position)
        })
        .or_else(|| states.first())
        .cloned()
        // Empty means the pinned workspace has no states: either it is
        // unseeded or (the common case under selection mode) the request
        // reached here with no workspace pinned. Fail closed with NotFound so
        // callers return a clean error instead of panicking the worker.
        .ok_or(diesel::result::Error::NotFound)
}

/// Lowest-position non-archived state in the given category. Used by the
/// legacy status writer paths that say "set ticket to in-progress" without
/// naming a specific state.
pub fn first_in_category(
    conn: &mut DbConnection,
    category: WorkflowStateCategory,
) -> QueryResult<WorkflowState> {
    workspace_states(conn)?
        .into_iter()
        .filter(|s| s.category == category && s.archived_at.is_none())
        .min_by_key(|s| s.position)
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
/// (`tests/it/sync_emit_lint.rs`) sees the emit, and so an admin script
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
    Ok(row)
}

/// Bring an archived state back. It goes to the end of its category, so it
/// doesn't take the place of a state added since it was archived. A state
/// that isn't archived comes back as it is, with nothing recorded. `NotFound`
/// for a state the connection can't see.
pub fn restore(conn: &mut DbConnection, id: i32) -> QueryResult<WorkflowState> {
    conn.transaction(|conn| {
        let current: WorkflowState = workflow_states::table.find(id).first(conn)?;
        if current.archived_at.is_none() {
            return Ok(current);
        }
        let position = workflow_states::table
            .filter(workflow_states::workspace_id.eq(current.workspace_id))
            .filter(workflow_states::category.eq(current.category))
            .filter(workflow_states::archived_at.is_null())
            .select(diesel::dsl::max(workflow_states::position))
            .first::<Option<i32>>(conn)?
            .map_or(0, |p| p + 1);
        let row: WorkflowState = diesel::update(workflow_states::table.find(id))
            .set((
                workflow_states::archived_at.eq(None::<chrono::DateTime<Utc>>),
                workflow_states::position.eq(position),
            ))
            .get_result(conn)?;
        emit::record(
            conn,
            SyncEmit {
                aggregate: SyncAggregate::WorkflowState,
                aggregate_id: row.id.to_string(),
                op: SyncOp::Update,
                event_type: "workflow_state.restored",
                data: json!({
                    "id": row.id,
                    "name": row.name,
                    "category": row.category.as_str(),
                    "color": row.color,
                    "position": row.position,
                    "is_default": row.is_default,
                }),
                groups: groups::workspace(),
                causation_id: None,
            },
        )?;
        Ok(row)
    })
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
        let s = default_state(&mut conn).unwrap();
        assert_eq!(s.category, WorkflowStateCategory::Backlog);
        assert!(s.is_default);
    }

    #[test]
    fn create_emits_a_sync_action_with_actor_from_session() {
        let mut conn = setup_test_connection();
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
