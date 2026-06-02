//! CRUD for the unified rules engine. Phase 1 surface only: list,
//! find, create, update, archive, hard-delete, plus the version /
//! application read paths and the application insert path. The
//! atomic apply lifecycle lives in `repository::rules::apply` (Wave
//! 5); this module is the data access layer the lifecycle and the
//! HTTP handlers call into.
//!
//! Workspace isolation is enforced by Postgres RLS (the
//! `rules_workspace_isolation` and sibling policies in the
//! migration), so the queries below don't add explicit
//! `workspace_id = ?` filters — the session's `app.workspace_id`
//! GUC scopes every read and the `WITH CHECK` clause rejects any
//! write to another workspace. Callers in handler code already
//! resolve the GUC via the request actor context; the helpers here
//! just need the workspace_id to populate `NewRule.workspace_id`
//! on insert.
//!
//! The `reads_set` / `writes_set` derivation tables in plan §11.2
//! live in this module so the self-referential save linter (Wave
//! 4) reads them off a single source of truth.

use diesel::prelude::*;
use diesel::QueryResult;
use serde_json::Value;
use uuid::Uuid;

use crate::db::DbConnection;
use crate::models::{
    NewRule, NewRuleApplication, Rule, RuleApplication, RuleApplicationStatus, RuleState,
    RuleTriggerKind, RuleUpdate, RuleVersion,
};

// =====================================================================
// reads_set / writes_set derivation (plan §11.2). Pure helpers; the
// repository calls them on every save so the columns stay in sync
// with the conditions and actions trees.
// =====================================================================

/// Walk `conditions` and return the sorted, deduplicated list of
/// `ConditionField` strings the rule reads. Manual rules pass `[]`
/// and get an empty `reads_set` back. Nested-field expansion
/// (`ticket.requester.email` adding both itself and
/// `ticket.requester_uuid`) is applied per the plan's derivation
/// table.
///
/// The return type is `Vec<Option<String>>` because Postgres TEXT[]
/// columns reach Rust through Diesel with nullable elements. The
/// derivation only ever produces real strings; the inner Option is
/// the schema boundary, not a semantic null.
pub fn derive_reads_set(conditions: &Value) -> Vec<Option<String>> {
    let mut out = std::collections::BTreeSet::new();
    collect_condition_fields(conditions, &mut out);
    out.into_iter().map(Some).collect()
}

fn collect_condition_fields(node: &Value, out: &mut std::collections::BTreeSet<String>) {
    // Manual rules: conditions is `[]`. Nothing to walk.
    if node.is_array() && node.as_array().is_some_and(|a| a.is_empty()) {
        return;
    }
    let Some(obj) = node.as_object() else { return };
    let kind = obj.get("kind").and_then(|k| k.as_str()).unwrap_or("");
    match kind {
        "leaf" => {
            if let Some(field) = obj.get("field").and_then(|f| f.as_str()) {
                for expanded in expand_condition_field(field) {
                    out.insert(expanded);
                }
            }
        }
        "and" | "or" => {
            if let Some(children) = obj.get("children").and_then(|c| c.as_array()) {
                for child in children {
                    collect_condition_fields(child, out);
                }
            }
        }
        "not" => {
            if let Some(child) = obj.get("child") {
                collect_condition_fields(child, out);
            }
        }
        _ => {}
    }
}

/// Plan §11.2 reads_set derivation table. Each entry returns the
/// concrete `reads_set` keys for one `ConditionField`. Status and
/// workflow_state_id alias the same underlying column so a rule
/// conditioning on either reads both. Nested requester fields also
/// add `ticket.requester_uuid` because mutating the requester (FK)
/// implicitly changes those values.
fn expand_condition_field(field: &str) -> Vec<String> {
    match field {
        "ticket.status" => vec!["ticket.workflow_state.category".into()],
        "ticket.workflow_state_id" => vec![
            "ticket.workflow_state_id".into(),
            "ticket.workflow_state.category".into(),
        ],
        "ticket.workflow_state.category" => vec![
            "ticket.workflow_state.category".into(),
            "ticket.workflow_state_id".into(),
        ],
        "ticket.requester.email" => vec![
            "ticket.requester.email".into(),
            "ticket.requester_uuid".into(),
        ],
        "ticket.requester.organization_id" => vec![
            "ticket.requester.organization_id".into(),
            "ticket.requester_uuid".into(),
        ],
        // Event / clock metadata is not ticket state, so no
        // reads_set contribution.
        "event.changed_fields" | "reply.kind" | "reply.author_role" | "clock.minutes_since" => {
            vec![]
        }
        // Everything else maps to itself.
        other => vec![other.to_string()],
    }
}

/// Walk `actions` and return the sorted, deduplicated list of
/// ticket-state fields the rule writes. Plan §11.2 derivation
/// table; the synthetic `ticket.comments` key flags reply-style
/// actions so Phase 2's `ticket_replied` trigger can detect
/// rule-replies-to-its-own-reply self-loops.
///
/// Same `Vec<Option<String>>` schema-boundary shape as
/// [`derive_reads_set`].
pub fn derive_writes_set(actions: &Value) -> Vec<Option<String>> {
    let mut out = std::collections::BTreeSet::new();
    let Some(arr) = actions.as_array() else {
        return Vec::new();
    };
    for action in arr {
        let Some(obj) = action.as_object() else { continue };
        let Some(kind) = obj.get("kind").and_then(|k| k.as_str()) else {
            continue;
        };
        for w in writes_for_action_kind(kind) {
            out.insert(w.to_string());
        }
    }
    out.into_iter().map(Some).collect()
}

fn writes_for_action_kind(kind: &str) -> &'static [&'static str] {
    match kind {
        "reply" => &[
            "ticket.first_response_at",
            "ticket.updated_at",
            "ticket.comments",
        ],
        "set_status" => &[
            "ticket.workflow_state_id",
            "ticket.workflow_state.category",
            "ticket.resolved_at",
            "ticket.updated_at",
        ],
        "assign" => &["ticket.assignee_uuid", "ticket.updated_at"],
        "unassign" => &["ticket.assignee_uuid", "ticket.updated_at"],
        "add_tags" => &["ticket.tag_ids", "ticket.updated_at"],
        "remove_tags" => &["ticket.tag_ids", "ticket.updated_at"],
        "set_priority" => &["ticket.priority", "ticket.updated_at"],
        "apply_macro_template" => &[
            "ticket.first_response_at",
            "ticket.updated_at",
            "ticket.comments",
        ],
        // notify, webhook, stop_processing have no ticket-state
        // writes (outbound side-effects only or engine control).
        _ => &[],
    }
}

// =====================================================================
// List / find. Workspace scoping is via RLS — the queries below
// rely on the session's app.workspace_id GUC and don't repeat it
// in WHERE clauses (matching the existing tenant-table pattern).
// =====================================================================

/// Filter shape for [`list`]. The handler unpacks query params into
/// this; the picker call site uses [`list_pickable_manual`] which
/// hardcodes its own filter.
#[derive(Debug, Default, Clone)]
pub struct ListFilter {
    pub trigger_kind: Option<RuleTriggerKind>,
    pub state: Option<RuleState>,
    pub include_archived: bool,
    pub name_query: Option<String>,
}

/// List rules visible in the current workspace, ordered by priority
/// then name. `archived_at IS NULL` unless `include_archived` is
/// explicitly set (the admin "show archived" toggle).
pub fn list(conn: &mut DbConnection, filter: ListFilter) -> QueryResult<Vec<Rule>> {
    use crate::schema::rules::dsl;
    let mut query = dsl::rules.into_boxed();
    if !filter.include_archived {
        query = query.filter(dsl::archived_at.is_null());
    }
    if let Some(kind) = filter.trigger_kind {
        query = query.filter(dsl::trigger_kind.eq(kind));
    }
    if let Some(state) = filter.state {
        query = query.filter(dsl::state.eq(state));
    }
    if let Some(q) = filter.name_query {
        // `ILIKE %q%` style match for the admin search box. The
        // bound % wrappers go through Diesel's escape path so a
        // literal `%` in the query is treated as literal.
        let pattern = format!("%{}%", q.replace('%', "\\%").replace('_', "\\_"));
        query = query.filter(dsl::name.ilike(pattern));
    }
    query
        .order((dsl::priority.asc(), dsl::name.asc()))
        .load(conn)
}

/// Live, non-archived, manual-trigger rules for the agent toolbar
/// picker (plan §13.4). Phase 1 has no per-ticket condition
/// evaluation since manual rules carry `conditions = []` by the
/// `rules_manual_no_conditions` CHECK.
pub fn list_pickable_manual(conn: &mut DbConnection) -> QueryResult<Vec<Rule>> {
    use crate::schema::rules::dsl;
    dsl::rules
        .filter(dsl::archived_at.is_null())
        .filter(dsl::state.eq(RuleState::Live))
        .filter(dsl::trigger_kind.eq(RuleTriggerKind::Manual))
        .order((dsl::priority.asc(), dsl::name.asc()))
        .load(conn)
}

pub fn find(conn: &mut DbConnection, id: i32) -> QueryResult<Option<Rule>> {
    use crate::schema::rules::dsl;
    dsl::rules.find(id).first(conn).optional()
}

// =====================================================================
// Create / update / archive / hard-delete.
// =====================================================================

/// Result type for write paths that need to distinguish "row not
/// found" and "row not in the right state for this op" from a
/// transient DB error.
#[derive(Debug, thiserror::Error)]
pub enum WriteError {
    #[error("rule {0} not found")]
    NotFound(i32),
    #[error("rule {0} must be archived before hard delete")]
    NotArchived(i32),
    #[error("database error: {0}")]
    Db(#[from] diesel::result::Error),
}

// sync-pending-wire: needs sync aggregate wiring (rules.created on Insert)
/// Insert a new rule. `reads_set` and `writes_set` are computed
/// from `conditions` / `actions` before insert so the columns stay
/// in sync with the JSONB trees they derive from. Manual rules
/// pass `conditions = []` and get an empty `reads_set` populated.
pub fn create(conn: &mut DbConnection, mut new: NewRule) -> QueryResult<Rule> {
    use crate::schema::rules::dsl;
    new.reads_set = derive_reads_set(&new.conditions);
    new.writes_set = derive_writes_set(&new.actions);
    diesel::insert_into(dsl::rules)
        .values(&new)
        .get_result(conn)
}

// sync-pending-wire: needs sync aggregate wiring (rules.updated on Update)
/// Apply a partial update. If `conditions` or `actions` move,
/// `reads_set` / `writes_set` are recomputed and overwritten on
/// the change set; any explicit set the caller passed is ignored.
/// The migration's BEFORE UPDATE trigger writes a `rule_versions`
/// snapshot and bumps `updated_at`.
pub fn update(
    conn: &mut DbConnection,
    id: i32,
    mut change: RuleUpdate,
) -> Result<Rule, WriteError> {
    use crate::schema::rules::dsl;
    if let Some(ref conditions) = change.conditions {
        change.reads_set = Some(derive_reads_set(conditions));
    }
    if let Some(ref actions) = change.actions {
        change.writes_set = Some(derive_writes_set(actions));
    }
    diesel::update(dsl::rules.find(id))
        .set(&change)
        .get_result::<Rule>(conn)
        .map_err(|e| match e {
            diesel::result::Error::NotFound => WriteError::NotFound(id),
            other => WriteError::Db(other),
        })
}

// sync-pending-wire: needs sync aggregate wiring (rules.archived on Update)
/// Soft-archive. Sets `archived_at = NOW()` and is the only path
/// the admin "Delete" button drives; hard delete requires
/// archived_at to already be set per decision 32 in the plan.
pub fn archive(conn: &mut DbConnection, id: i32, when: chrono::DateTime<chrono::Utc>) -> Result<Rule, WriteError> {
    use crate::schema::rules::dsl;
    diesel::update(dsl::rules.find(id))
        .set(dsl::archived_at.eq(Some(when)))
        .get_result::<Rule>(conn)
        .map_err(|e| match e {
            diesel::result::Error::NotFound => WriteError::NotFound(id),
            other => WriteError::Db(other),
        })
}

// sync-pending-wire: needs sync aggregate wiring (rules.deleted on Delete)
/// Permanently delete a rule. The caller must have already
/// archived it; we return `NotArchived` rather than implicitly
/// soft-archiving so the admin can't bypass the "review recent
/// activity before destroying it" UX gate by hitting the API
/// directly. Cascades into `rule_versions` and `rule_applications`
/// via the FK.
pub fn hard_delete(conn: &mut DbConnection, id: i32) -> Result<(), WriteError> {
    use crate::schema::rules::dsl;
    let existing: Rule = dsl::rules
        .find(id)
        .first(conn)
        .map_err(|e| match e {
            diesel::result::Error::NotFound => WriteError::NotFound(id),
            other => WriteError::Db(other),
        })?;
    if existing.archived_at.is_none() {
        return Err(WriteError::NotArchived(id));
    }
    diesel::delete(dsl::rules.find(id)).execute(conn)?;
    Ok(())
}

// =====================================================================
// Versions (read-only; the migration's INSERT/UPDATE triggers do
// the writes).
// =====================================================================

pub fn list_versions(conn: &mut DbConnection, rule_id: i32) -> QueryResult<Vec<RuleVersion>> {
    use crate::schema::rule_versions::dsl;
    dsl::rule_versions
        .filter(dsl::rule_id.eq(rule_id))
        .order(dsl::version.desc())
        .load(conn)
}

pub fn find_version(
    conn: &mut DbConnection,
    rule_id: i32,
    version: i32,
) -> QueryResult<Option<RuleVersion>> {
    use crate::schema::rule_versions::dsl;
    dsl::rule_versions
        .filter(dsl::rule_id.eq(rule_id))
        .filter(dsl::version.eq(version))
        .first(conn)
        .optional()
}

// =====================================================================
// Applications (the audit / recent-activity log).
// =====================================================================

/// Filter shape for [`list_applications`]. The recent-activity
/// admin endpoint unpacks query params into this.
#[derive(Debug, Default, Clone)]
pub struct ApplicationFilter {
    pub rule_id: Option<i32>,
    pub ticket_id: Option<i32>,
    pub status: Option<RuleApplicationStatus>,
    pub actor_uuid: Option<Uuid>,
    pub from: Option<chrono::DateTime<chrono::Utc>>,
    pub to: Option<chrono::DateTime<chrono::Utc>>,
    pub limit: i64,
}

pub fn list_applications(
    conn: &mut DbConnection,
    filter: ApplicationFilter,
) -> QueryResult<Vec<RuleApplication>> {
    use crate::schema::rule_applications::dsl;
    let mut query = dsl::rule_applications.into_boxed();
    if let Some(rid) = filter.rule_id {
        query = query.filter(dsl::rule_id.eq(rid));
    }
    if let Some(tid) = filter.ticket_id {
        query = query.filter(dsl::ticket_id.eq(tid));
    }
    if let Some(status) = filter.status {
        query = query.filter(dsl::status.eq(status));
    }
    if let Some(actor) = filter.actor_uuid {
        query = query.filter(dsl::actor_uuid.eq(actor));
    }
    if let Some(from) = filter.from {
        query = query.filter(dsl::applied_at.ge(from));
    }
    if let Some(to) = filter.to {
        query = query.filter(dsl::applied_at.le(to));
    }
    let limit = filter.limit.clamp(1, 500);
    query
        .order(dsl::applied_at.desc())
        .limit(limit)
        .load(conn)
}

pub fn find_application(
    conn: &mut DbConnection,
    id: i64,
) -> QueryResult<Option<RuleApplication>> {
    use crate::schema::rule_applications::dsl;
    dsl::rule_applications.find(id).first(conn).optional()
}

pub fn list_applications_for_ticket(
    conn: &mut DbConnection,
    ticket_id: i32,
) -> QueryResult<Vec<RuleApplication>> {
    use crate::schema::rule_applications::dsl;
    dsl::rule_applications
        .filter(dsl::ticket_id.eq(ticket_id))
        .order(dsl::applied_at.desc())
        .load(conn)
}

// sync-audit-only: rule-application audit row, no entity sync
/// Insert one application row. Called from inside the apply
/// lifecycle's transaction so the row's `correlation_id` matches
/// the audit_log + sync_actions rows the same apply produced.
pub fn record_application(
    conn: &mut DbConnection,
    new: NewRuleApplication,
) -> QueryResult<RuleApplication> {
    use crate::schema::rule_applications::dsl;
    diesel::insert_into(dsl::rule_applications)
        .values(&new)
        .get_result(conn)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Strip the Option wrapper so test assertions stay readable.
    /// Production code keeps the Option<String> shape because that's
    /// what Diesel hands back for Array<Nullable<Text>> columns.
    fn flat(v: Vec<Option<String>>) -> Vec<String> {
        v.into_iter().flatten().collect()
    }

    #[test]
    fn empty_conditions_derive_empty_reads_set() {
        let conditions = json!([]);
        assert!(derive_reads_set(&conditions).is_empty());
    }

    #[test]
    fn single_leaf_condition_picks_one_field() {
        let conditions = json!({
            "kind": "leaf",
            "field": "ticket.priority",
            "op": "eq",
            "value": "high"
        });
        assert_eq!(flat(derive_reads_set(&conditions)), vec!["ticket.priority"]);
    }

    #[test]
    fn and_tree_collects_every_leaf() {
        let conditions = json!({
            "kind": "and",
            "children": [
                {"kind": "leaf", "field": "ticket.priority", "op": "eq", "value": "high"},
                {"kind": "leaf", "field": "ticket.category_id", "op": "eq", "value": 12}
            ]
        });
        let reads = flat(derive_reads_set(&conditions));
        assert_eq!(reads, vec!["ticket.category_id", "ticket.priority"]);
    }

    #[test]
    fn not_wraps_and_traversal_continues() {
        let conditions = json!({
            "kind": "not",
            "child": {
                "kind": "or",
                "children": [
                    {"kind": "leaf", "field": "ticket.title", "op": "contains", "value": "spam"},
                    {"kind": "leaf", "field": "ticket.tag_ids", "op": "has_any", "value": [1]}
                ]
            }
        });
        let reads = flat(derive_reads_set(&conditions));
        assert_eq!(reads, vec!["ticket.tag_ids", "ticket.title"]);
    }

    #[test]
    fn status_alias_expands_to_category() {
        let conditions = json!({
            "kind": "leaf",
            "field": "ticket.status",
            "op": "eq",
            "value": "open"
        });
        let reads = flat(derive_reads_set(&conditions));
        // ticket.status aliases the category column; conditioning
        // on either flags both for the linter so a rule that
        // mutates workflow_state_id while reading status is
        // caught as self-referential.
        assert_eq!(reads, vec!["ticket.workflow_state.category"]);
    }

    #[test]
    fn workflow_state_id_and_category_alias_each_other() {
        let conditions = json!({
            "kind": "leaf",
            "field": "ticket.workflow_state_id",
            "op": "eq",
            "value": 4
        });
        let reads = flat(derive_reads_set(&conditions));
        assert_eq!(
            reads,
            vec![
                "ticket.workflow_state.category",
                "ticket.workflow_state_id"
            ]
        );
    }

    #[test]
    fn requester_email_expands_to_parent_uuid() {
        let conditions = json!({
            "kind": "leaf",
            "field": "ticket.requester.email",
            "op": "ends_with",
            "value": "@vip.example"
        });
        let reads = flat(derive_reads_set(&conditions));
        assert_eq!(
            reads,
            vec!["ticket.requester.email", "ticket.requester_uuid"]
        );
    }

    #[test]
    fn event_and_clock_fields_do_not_count_as_ticket_reads() {
        // These read event / engine metadata, not ticket state.
        // The self-referential linter only cares about state-vs-
        // state intersections.
        let conditions = json!({
            "kind": "and",
            "children": [
                {"kind": "leaf", "field": "event.changed_fields", "op": "has_any", "value": ["status"]},
                {"kind": "leaf", "field": "clock.minutes_since", "op": "gt", "value": 60}
            ]
        });
        assert!(derive_reads_set(&conditions).is_empty());
    }

    #[test]
    fn writes_set_from_set_status_action() {
        let actions = json!([
            { "kind": "set_status", "config": { "workflow_state_id": 7 } }
        ]);
        let writes = flat(derive_writes_set(&actions));
        assert_eq!(
            writes,
            vec![
                "ticket.resolved_at",
                "ticket.updated_at",
                "ticket.workflow_state.category",
                "ticket.workflow_state_id"
            ]
        );
    }

    #[test]
    fn writes_set_dedupes_across_actions() {
        let actions = json!([
            { "kind": "add_tags", "config": { "tag_ids": [1, 2] } },
            { "kind": "remove_tags", "config": { "tag_ids": [3] } }
        ]);
        let writes = flat(derive_writes_set(&actions));
        assert_eq!(writes, vec!["ticket.tag_ids", "ticket.updated_at"]);
    }

    #[test]
    fn reply_action_includes_synthetic_comments_key() {
        let actions = json!([
            { "kind": "reply", "config": { "visibility": "public", "body": "hi" } }
        ]);
        let writes = flat(derive_writes_set(&actions));
        assert!(writes.iter().any(|s| s == "ticket.comments"));
    }

    #[test]
    fn notify_action_has_no_ticket_state_writes() {
        let actions = json!([
            { "kind": "notify", "config": { "recipient": "requester", "subject": "x", "body": "y" } }
        ]);
        assert!(derive_writes_set(&actions).is_empty());
    }

    #[test]
    fn stop_processing_action_contributes_nothing() {
        let actions = json!([
            { "kind": "stop_processing" }
        ]);
        assert!(derive_writes_set(&actions).is_empty());
    }
}
