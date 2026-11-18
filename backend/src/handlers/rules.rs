//! HTTP handlers for the rules engine. Phase 1 surface: admin CRUD
//! on the `rules` resource, the recent-applications log, the agent
//! picker endpoint, and the self-referential save linter (a check
//! that runs at create / update time, not a separate endpoint).
//!
//! The apply endpoint (`POST /api/rules/{id}/apply`) lives in Wave
//! 6 alongside the atomic lifecycle in `repository::rules::apply`.
//! Routes are wired in `main.rs` so the literal `/rules/{id}/apply`
//! and `/rules/{id}/state` register before the wildcard
//! `/rules/{id}` PATCH/DELETE per memory note
//! `project_actix_route_shadowing`.
//!
//! Workspace isolation is via RLS + `require_workspace_role`. The
//! repository layer doesn't add explicit `workspace_id = ?` filters
//! to its queries; each handler runs them through `TenantConn`, which
//! pins the request's workspace (a pooled connection starts with none,
//! and row security then shows and accepts nothing).

use std::sync::Arc;

use actix_web::{web, HttpMessage, HttpRequest, HttpResponse};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::db::Pool;
use crate::errors::{self, ApiError};
use crate::extractors::TenantConn;
use crate::middleware::request_context::RequestContext;
use crate::models::{
    NewRule, Rule, RuleApplicationStatus, RuleState, RuleTriggerKind, RuleUpdate, RuleVersion,
    WorkspaceRole,
};
use crate::repository::rules;
use crate::repository::tickets::TicketUpdatedObserver;
use crate::services::rules::conditions::{self, ConditionError, Predicate};
use crate::services::search::SearchService;
use crate::services::ticket_updates::{ActorConn, InWorkspace};
use crate::utils::rbac::require_workspace_role;

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.route(
        "/admin/rule-starters",
        web::get().to(crate::handlers::rules::list_starter_catalog),
    )
    .route(
        "/admin/rule-starters/{id}",
        web::post().to(crate::handlers::rules::add_starter),
    )
    .route("/rules", web::get().to(crate::handlers::rules::list_rules))
    .route(
        "/rules",
        web::post().to(crate::handlers::rules::create_rule),
    )
    // Literal, so before `/rules/{id}`, which would take it and answer 405.
    .route(
        "/rules/preview",
        web::post().to(crate::handlers::rules::preview_rule),
    )
    // Literal too: `GET /rules/{id}` would take it and answer 404.
    .route(
        "/rules/conflicts",
        web::get().to(crate::handlers::rules::list_rule_conflicts),
    )
    .route(
        "/rules/{id}/apply",
        web::post().to(crate::handlers::rules::apply_rule),
    )
    .route(
        "/rules/{id}/state",
        web::patch().to(crate::handlers::rules::transition_state),
    )
    .route(
        "/rules/{id}/restore",
        web::post().to(crate::handlers::rules::restore_rule),
    )
    .route(
        "/rules/{id}/versions",
        web::get().to(crate::handlers::rules::list_rule_versions),
    )
    .route(
        "/rules/{rule_id}/versions/{version}",
        web::get().to(crate::handlers::rules::get_rule_version),
    )
    .route(
        "/rules/{id}",
        web::get().to(crate::handlers::rules::get_rule),
    )
    .route(
        "/rules/{id}",
        web::put().to(crate::handlers::rules::update_rule),
    )
    .route(
        "/rules/{id}",
        web::delete().to(crate::handlers::rules::delete_rule),
    )
    .route(
        "/rule-applications",
        web::get().to(crate::handlers::rules::list_rule_applications),
    )
    .route(
        "/rule-applications/{id}",
        web::get().to(crate::handlers::rules::get_rule_application),
    );
}

// =====================================================================
// Request / response DTOs.
// =====================================================================

/// Body of `POST /api/rules`. Only admins call this. `reads_set` /
/// `writes_set` are derived server-side; clients don't send them.
#[derive(Debug, Deserialize)]
pub struct CreateRuleRequest {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub trigger_kind: RuleTriggerKind,
    #[serde(default = "empty_object")]
    pub trigger_config: Value,
    #[serde(default = "empty_array")]
    pub conditions: Value,
    pub actions: Value,
    #[serde(default = "default_priority")]
    pub priority: i32,
    /// Optional override for the self-referential save linter. The
    /// editor sets this to `true` only when the admin clicks the
    /// "I understand this rule may loop" checkbox. The server still
    /// computes `reads_set` / `writes_set` and stores them; the flag
    /// just changes whether an intersection blocks the save.
    #[serde(default)]
    pub override_self_reference: bool,
}

fn empty_object() -> Value {
    Value::Object(serde_json::Map::new())
}
fn empty_array() -> Value {
    Value::Array(Vec::new())
}
fn default_priority() -> i32 {
    100
}

/// Body of `PUT /api/rules/{id}`. Every field is Option-wrapped so
/// the editor's partial PATCH-equivalent round-trips without
/// clobbering unsent fields. Same `override_self_reference` knob as
/// create.
#[derive(Debug, Deserialize)]
pub struct UpdateRuleRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<Option<String>>,
    #[serde(default)]
    pub trigger_kind: Option<RuleTriggerKind>,
    #[serde(default)]
    pub trigger_config: Option<Value>,
    #[serde(default)]
    pub conditions: Option<Value>,
    #[serde(default)]
    pub actions: Option<Value>,
    #[serde(default)]
    pub priority: Option<i32>,
    #[serde(default)]
    pub override_self_reference: bool,
}

/// Body of `PATCH /api/rules/{id}/state`. The state machine is
/// validated server-side; illegal transitions return 409 with the
/// `RULE_STATE_TRANSITION` code so the editor can surface the
/// allowed next states.
#[derive(Debug, Deserialize)]
pub struct StateTransitionRequest {
    pub state: RuleState,
}

/// Body of `POST /api/rules/preview`: the conditions a rule being
/// edited would have, checked as a save checks them.
#[derive(Debug, Deserialize)]
pub struct PreviewRuleRequest {
    pub trigger_kind: RuleTriggerKind,
    #[serde(default = "empty_array")]
    pub conditions: Value,
}

/// Filters for `GET /api/rules`. Pulled from query string.
#[derive(Debug, Default, Deserialize)]
pub struct ListRulesQuery {
    pub trigger_kind: Option<RuleTriggerKind>,
    pub state: Option<RuleState>,
    pub q: Option<String>,
    #[serde(default)]
    pub include_archived: bool,
}

/// Filters for `GET /api/rule-applications`. Date bounds are
/// optional RFC-3339 strings; the repo converts to `chrono`.
#[derive(Debug, Default, Deserialize)]
pub struct ListApplicationsQuery {
    pub rule_id: Option<i32>,
    pub ticket_id: Option<i32>,
    pub status: Option<RuleApplicationStatus>,
    pub actor_uuid: Option<Uuid>,
    pub from: Option<chrono::DateTime<Utc>>,
    pub to: Option<chrono::DateTime<Utc>>,
    #[serde(default = "default_application_limit")]
    pub limit: i64,
}

fn default_application_limit() -> i64 {
    50
}

/// Query-string knob on `DELETE /api/rules/{id}`. `?hard=true`
/// permanently deletes; otherwise the row is soft-archived (sets
/// `archived_at`). Decision 32 in the plan: hard delete is only
/// permitted on a row that's already archived.
#[derive(Debug, Default, Deserialize)]
pub struct DeleteRuleQuery {
    #[serde(default)]
    pub hard: bool,
}

/// `Rule` as it appears in API responses. Strips the
/// `Vec<Option<String>>` schema-boundary on `reads_set` /
/// `writes_set` (Diesel's nullable-elements view) into a flat
/// `Vec<String>` for clients.
#[derive(Debug, Serialize)]
pub struct RuleDto {
    pub id: i32,
    pub workspace_id: i32,
    pub name: String,
    pub description: Option<String>,
    pub trigger_kind: RuleTriggerKind,
    pub trigger_config: Value,
    pub conditions: Value,
    pub actions: Value,
    pub reads_set: Vec<String>,
    pub writes_set: Vec<String>,
    pub state: RuleState,
    pub priority: i32,
    pub last_fired_at: Option<chrono::DateTime<Utc>>,
    pub fire_count: i32,
    pub created_by: Option<Uuid>,
    pub created_at: chrono::DateTime<Utc>,
    pub updated_at: chrono::DateTime<Utc>,
    pub archived_at: Option<chrono::DateTime<Utc>>,
    /// Events turned away before any rule ran (the ticket was held for
    /// approval, or merged), which leave no application row.
    pub skipped_preflight_count: i64,
    /// Events a live rule skipped because the ticket didn't meet its
    /// conditions, which leave no application row.
    pub skipped_condition_unmet_count: i64,
}

impl From<Rule> for RuleDto {
    fn from(r: Rule) -> Self {
        Self {
            id: r.id,
            workspace_id: r.workspace_id,
            name: r.name,
            description: r.description,
            trigger_kind: r.trigger_kind,
            trigger_config: r.trigger_config,
            conditions: r.conditions,
            actions: r.actions,
            reads_set: r.reads_set.into_iter().flatten().collect(),
            writes_set: r.writes_set.into_iter().flatten().collect(),
            state: r.state,
            priority: r.priority,
            last_fired_at: r.last_fired_at,
            fire_count: r.fire_count,
            created_by: r.created_by,
            created_at: r.created_at,
            updated_at: r.updated_at,
            archived_at: r.archived_at,
            skipped_preflight_count: r.skipped_preflight_count,
            skipped_condition_unmet_count: r.skipped_condition_unmet_count,
        }
    }
}

/// A rule as create and update return it: the rule, plus how many recent
/// tickets its conditions match (the plan's save-time check).
#[derive(Debug, Serialize)]
pub struct SavedRuleDto {
    #[serde(flatten)]
    pub rule: RuleDto,
    pub preview: rules::PreviewMatch,
}

impl SavedRuleDto {
    fn new(rule: Rule, preview: rules::PreviewMatch) -> Self {
        Self {
            rule: RuleDto::from(rule),
            preview,
        }
    }
}

/// `RuleVersion` as it appears in API responses. Same flat shape as
/// `RuleDto` for the columns it shares; `reads_set` / `writes_set`
/// aren't on `rule_versions` because the migration's trigger only
/// snapshots the user-facing fields.
#[derive(Debug, Serialize)]
pub struct RuleVersionDto {
    pub id: i32,
    pub rule_id: i32,
    pub workspace_id: i32,
    pub version: i32,
    pub name: String,
    pub description: Option<String>,
    pub trigger_kind: RuleTriggerKind,
    pub trigger_config: Value,
    pub conditions: Value,
    pub actions: Value,
    pub state: RuleState,
    pub priority: i32,
    pub saved_by: Option<Uuid>,
    pub saved_at: chrono::DateTime<Utc>,
}

impl From<RuleVersion> for RuleVersionDto {
    fn from(v: RuleVersion) -> Self {
        Self {
            id: v.id,
            rule_id: v.rule_id,
            workspace_id: v.workspace_id,
            version: v.version,
            name: v.name,
            description: v.description,
            trigger_kind: v.trigger_kind,
            trigger_config: v.trigger_config,
            conditions: v.conditions,
            actions: v.actions,
            state: v.state,
            priority: v.priority,
            saved_by: v.saved_by,
            saved_at: v.saved_at,
        }
    }
}

// =====================================================================
// Self-referential save linter (decision 8 in the plan / unit-11).
// Lives in this module because it's a save-time HTTP-layer check, not
// a repository invariant.
// =====================================================================

#[derive(Debug)]
struct SelfReferentialError {
    fields: Vec<String>,
}

/// Returns `Err` when the rule's `reads_set ∩ writes_set ≠ ∅` AND
/// the caller didn't opt out via `override_self_reference`. The
/// repository's `derive_*` helpers produce `Vec<Option<String>>`
/// because the column is `Array<Nullable<Text>>`; we strip the
/// `Option` wrapper at this boundary.
fn check_self_referential(
    reads: &[Option<String>],
    writes: &[Option<String>],
    override_flag: bool,
) -> Result<(), SelfReferentialError> {
    if override_flag {
        return Ok(());
    }
    let writes_set: std::collections::HashSet<&str> =
        writes.iter().filter_map(|w| w.as_deref()).collect();
    let overlap: Vec<String> = reads
        .iter()
        .filter_map(|r| r.as_deref())
        .filter(|r| writes_set.contains(r))
        .map(|s| s.to_string())
        .collect();
    if overlap.is_empty() {
        Ok(())
    } else {
        Err(SelfReferentialError { fields: overlap })
    }
}

// =====================================================================
// Condition checks (plan §5.2). Compiling is pure; whether the ids a
// tree names exist is a read in the request's workspace.
// =====================================================================

/// The conditions compiled for evaluation, or the 400 saying why they
/// can't be saved.
fn compile_conditions(
    trigger_kind: RuleTriggerKind,
    conditions: &Value,
) -> Result<Predicate, Box<HttpResponse>> {
    conditions::compile(conditions, trigger_kind).map_err(|e| Box::new(condition_refused(&e)))
}

/// 400 `RULE_CONDITION_INVALID`, with the node's path and the leaf's
/// field, operator and field kind in `details`.
fn condition_refused(e: &ConditionError) -> HttpResponse {
    errors::bad_request_with_details(
        e.to_string(),
        "RULE_CONDITION_INVALID",
        serde_json::json!({
            "path": e.path,
            "field": e.field.map(|f| f.as_str()),
            "op": e.op.map(|o| o.as_str()),
            "field_kind": e.field_kind().map(|k| k.as_str()),
        }),
    )
}

/// The 400 for a category, status or tag id the conditions name that the
/// workspace doesn't have, if there is one.
fn unknown_reference(
    conn: &mut crate::db::DbConnection,
    predicate: &Predicate,
) -> diesel::QueryResult<Option<HttpResponse>> {
    let refs = predicate.references();
    Ok(
        rules::unknown_condition_reference(conn, &refs)?.map(|(field, id)| {
            errors::bad_request_with_details(
                format!("{}: {id} isn't in this workspace", field.as_str()),
                "RULE_CONDITION_INVALID",
                serde_json::json!({
                    "path": null,
                    "field": field.as_str(),
                    "op": null,
                    "field_kind": field.kind().as_str(),
                    "id": id,
                }),
            )
        }),
    )
}

/// A plugin event rule's settings as saved: the plugin's uuid, in the form
/// its events are recorded with, and an event name that isn't a host
/// event's. Other keys are dropped. Whether the plugin declares the event is
/// [`unknown_plugin_event`]'s check.
fn plugin_trigger_config(config: &Value) -> Result<Value, &'static str> {
    let plugin_uuid = config
        .get("plugin_uuid")
        .and_then(Value::as_str)
        .and_then(|u| Uuid::parse_str(u).ok())
        .ok_or("a plugin event rule must name the plugin it runs on")?;
    let event_type = config
        .get("event_type")
        .and_then(Value::as_str)
        .filter(|e| {
            use crate::services::plugins::manifest_validate::{
                plugin_event_name_problem, PluginEventNameProblem, PLUGIN_EVENT_NAME_MAX,
            };
            !e.is_empty()
                && e.len() <= PLUGIN_EVENT_NAME_MAX
                && plugin_event_name_problem("", e) != Some(PluginEventNameProblem::Reserved)
        })
        .ok_or("a plugin event rule must name one of the plugin's events")?;
    Ok(serde_json::json!({
        "plugin_uuid": plugin_uuid.to_string(),
        "event_type": event_type,
    }))
}

/// A plugin event rule's plugin is installed in this workspace and its
/// manifest declares the event it names, namespaced to it. Whether the plugin is enabled is checked when an
/// event comes in, not here.
fn unknown_plugin_event(
    conn: &mut crate::db::DbConnection,
    config: &Value,
) -> diesel::QueryResult<Option<HttpResponse>> {
    let (Some(plugin_uuid), Some(event_type)) = (
        config
            .get("plugin_uuid")
            .and_then(Value::as_str)
            .and_then(|u| Uuid::parse_str(u).ok()),
        config.get("event_type").and_then(Value::as_str),
    ) else {
        return Ok(None);
    };
    let plugin = match crate::repository::plugins::get_plugin_by_uuid(conn, plugin_uuid) {
        Ok(plugin) => plugin,
        Err(diesel::result::Error::NotFound) => {
            return Ok(Some(errors::bad_request_with_code(
                "the plugin the rule runs on isn't installed in this workspace",
                "RULE_VALIDATION",
            )))
        }
        Err(e) => return Err(e),
    };
    let emits = plugin
        .parse_manifest()
        .is_ok_and(|m| m.events.iter().any(|e| e == event_type))
        && crate::services::plugins::manifest_validate::plugin_event_name_problem(
            &plugin.name,
            event_type,
        )
        .is_none();
    Ok((!emits).then(|| {
        errors::bad_request_with_code(
            format!("the plugin doesn't emit {event_type}"),
            "RULE_VALIDATION",
        )
    }))
}

fn manual_conditions_refused() -> HttpResponse {
    errors::bad_request_with_code(
        "manual rules must have an empty conditions list",
        "RULE_VALIDATION",
    )
}

// =====================================================================
// Helpers.
// =====================================================================

fn actor_workspace_id(req: &HttpRequest) -> Option<i32> {
    req.extensions()
        .get::<RequestContext>()
        .map(|c| c.actor.workspace_id)
        .unwrap_or(None)
}

/// 409 `RULE_ARCHIVED`: see [`Rule::is_archived`].
fn archived_conflict(id: i32) -> HttpResponse {
    errors::conflict_with_code(format!("rule {id} is archived"), "RULE_ARCHIVED")
}

fn actor_uuid(req: &HttpRequest) -> Option<Uuid> {
    req.extensions()
        .get::<RequestContext>()
        .and_then(|c| c.actor.uuid)
}

/// Cheap "this transition is one of the legal ones" check. The
/// state machine: draft ↔ paused ↔ live → archived, plus dry_run
/// (shadow runs) between them for a rule with an automated trigger;
/// a manual rule has nothing to shadow. From archived the row stays
/// put; restore-to-draft is its own endpoint.
fn legal_state_transition(kind: RuleTriggerKind, from: RuleState, to: RuleState) -> bool {
    use RuleState::*;
    let common = matches!(
        (from, to),
        (Draft, Paused)
            | (Draft, Live)
            | (Draft, Archived)
            | (Paused, Live)
            | (Paused, Draft)
            | (Paused, Archived)
            | (Live, Paused)
            | (Live, Archived)
    );
    let shadow = kind != RuleTriggerKind::Manual
        && matches!(
            (from, to),
            (Draft, DryRun)
                | (Paused, DryRun)
                | (Live, DryRun)
                | (DryRun, Live)
                | (DryRun, Paused)
                | (DryRun, Draft)
                | (DryRun, Archived)
        );
    common || shadow
}

// =====================================================================
// Handlers.
// =====================================================================

/// `POST /api/rules` (admin). Create a new rule. Manual rules
/// require `conditions = []`; the `rules_manual_no_conditions`
/// CHECK enforces this at the DB but we early-reject here with a
/// clearer error.
pub async fn create_rule(
    req: HttpRequest,
    body: web::Json<CreateRuleRequest>,
    mut tc: TenantConn,
) -> Result<HttpResponse, ApiError> {
    require_workspace_role(&req, WorkspaceRole::Admin)?;
    let Some(workspace_id) = actor_workspace_id(&req) else {
        return Err(ApiError::Unauthorized("Authentication required".into()));
    };
    let created_by = actor_uuid(&req);
    let body = body.into_inner();
    created_response(tc.run_result(|conn| insert_rule(conn, workspace_id, created_by, body)))
}

/// Why [`insert_rule`] didn't save the rule.
enum InsertRuleError {
    /// The rule itself is invalid; the response says why.
    Refused(Box<HttpResponse>),
    Db(diesel::result::Error),
}

fn refused(resp: HttpResponse) -> InsertRuleError {
    InsertRuleError::Refused(Box::new(resp))
}

impl From<diesel::result::Error> for InsertRuleError {
    fn from(e: diesel::result::Error) -> Self {
        Self::Db(e)
    }
}

/// Validate and save a new draft rule, and preview its conditions. Shared
/// by `create_rule` and `add_starter`, so a starter is held to the same
/// checks as a rule written in the editor. A refusal is an `Err`, so the
/// caller's transaction rolls back anything it wrote first.
fn insert_rule(
    conn: &mut crate::db::DbConnection,
    workspace_id: i32,
    created_by: Option<Uuid>,
    body: CreateRuleRequest,
) -> Result<(Rule, rules::PreviewMatch), InsertRuleError> {
    let CreateRuleRequest {
        name,
        description,
        trigger_kind,
        mut trigger_config,
        conditions,
        actions,
        priority,
        override_self_reference,
    } = body;

    if name.trim().is_empty() {
        return Err(refused(errors::bad_request_with_code(
            "name must not be empty",
            "RULE_VALIDATION",
        )));
    }
    if let Err(msg) = validate_actions(&actions) {
        return Err(refused(errors::bad_request_with_code(
            msg,
            "RULE_VALIDATION",
        )));
    }
    if trigger_kind == RuleTriggerKind::Manual && !is_empty_conditions(&conditions) {
        return Err(refused(manual_conditions_refused()));
    }
    let predicate =
        compile_conditions(trigger_kind, &conditions).map_err(InsertRuleError::Refused)?;
    if let Some(resp) = unknown_reference(conn, &predicate)? {
        return Err(refused(resp));
    }
    if let Some(problem) =
        rules::step_person_problem(conn, workspace_id, trigger_kind, &actions, created_by)?
    {
        return Err(refused(step_person_refused(problem)));
    }
    if trigger_kind == RuleTriggerKind::PluginEvent {
        trigger_config = plugin_trigger_config(&trigger_config)
            .map_err(|msg| refused(errors::bad_request_with_code(msg, "RULE_VALIDATION")))?;
        if let Some(resp) = unknown_plugin_event(conn, &trigger_config)? {
            return Err(refused(resp));
        }
    }

    let reads = rules::reads_beyond_guards(&conditions);
    let writes = rules::derive_writes_set(&actions);

    if let Err(e) = check_self_referential(&reads, &writes, override_self_reference) {
        return Err(refused(errors::conflict_with_code(
            format!(
                "rule reads and writes the same fields: {} (set override_self_reference=true to bypass)",
                e.fields.join(", ")
            ),
            "RULE_SELF_REFERENTIAL",
        )));
    }

    let new = NewRule {
        workspace_id,
        name,
        description,
        trigger_kind,
        trigger_config,
        conditions,
        actions,
        // derive_* output is what gets persisted; the repo's
        // create() recomputes from the JSONB trees so passing it
        // again here is redundant but explicit.
        reads_set: reads,
        writes_set: writes,
        state: RuleState::Draft,
        priority,
        created_by,
    };
    let rule = rules::create(conn, new)?;
    let preview = rules::preview_match(conn, &predicate, rules::PREVIEW_SCAN)?;
    Ok((rule, preview))
}

/// 201 with the new rule, or the reason it wasn't saved.
fn created_response(
    result: Result<(Rule, rules::PreviewMatch), InsertRuleError>,
) -> Result<HttpResponse, ApiError> {
    match result {
        Ok((rule, preview)) => Ok(HttpResponse::Created().json(SavedRuleDto::new(rule, preview))),
        Err(InsertRuleError::Refused(resp)) => Ok(*resp),
        Err(InsertRuleError::Db(e)) => {
            tracing::error!(error = ?e, "create_rule: repo error");
            Err(ApiError::Database(e))
        }
    }
}

/// `GET /api/rules` (admin). List rules in the caller's workspace.
pub async fn list_rules(
    req: HttpRequest,
    query: web::Query<ListRulesQuery>,
    mut tc: TenantConn,
) -> Result<HttpResponse, ApiError> {
    require_workspace_role(&req, WorkspaceRole::Admin)?;
    let q = query.into_inner();
    let filter = rules::ListFilter {
        trigger_kind: q.trigger_kind,
        state: q.state,
        include_archived: q.include_archived,
        name_query: q.q,
    };
    match tc.run(|conn| rules::list(conn, filter)) {
        Ok(rs) => {
            Ok(HttpResponse::Ok().json(rs.into_iter().map(RuleDto::from).collect::<Vec<_>>()))
        }
        Err(e) => Err(ApiError::Database(e)),
    }
}

/// `GET /api/rules/{id}` (admin). Single rule.
pub async fn get_rule(
    req: HttpRequest,
    path: web::Path<i32>,
    mut tc: TenantConn,
) -> Result<HttpResponse, ApiError> {
    require_workspace_role(&req, WorkspaceRole::Admin)?;
    let id = path.into_inner();
    match tc.run(|conn| rules::find(conn, id)) {
        Ok(Some(rule)) => Ok(HttpResponse::Ok().json(RuleDto::from(rule))),
        Ok(None) => Err(ApiError::NotFoundMsg(format!("rule {id} not found"))),
        Err(e) => Err(ApiError::Database(e)),
    }
}

/// `PUT /api/rules/{id}` (admin). Apply a partial update; the
/// migration's BEFORE UPDATE trigger writes a `rule_versions` row
/// and bumps `updated_at`. `reads_set` / `writes_set` are
/// recomputed by the repo when `conditions` or `actions` move. An
/// archived rule can't be edited (409 `RULE_ARCHIVED`).
pub async fn update_rule(
    req: HttpRequest,
    path: web::Path<i32>,
    body: web::Json<UpdateRuleRequest>,
    mut tc: TenantConn,
) -> Result<HttpResponse, ApiError> {
    require_workspace_role(&req, WorkspaceRole::Admin)?;
    let id = path.into_inner();
    let UpdateRuleRequest {
        name,
        description,
        trigger_kind,
        mut trigger_config,
        conditions,
        actions,
        priority,
        override_self_reference,
    } = body.into_inner();

    // An archived rule takes no edits, valid or not. The self-ref check
    // below also needs the post-update conditions / actions, which means
    // the existing row when either is omitted.
    let existing = match tc.run(|conn| rules::find(conn, id)) {
        Ok(Some(r)) => r,
        Ok(None) => return Err(ApiError::NotFoundMsg(format!("rule {id} not found"))),
        Err(e) => return Err(ApiError::Database(e)),
    };
    if existing.is_archived() {
        return Ok(archived_conflict(id));
    }

    if let Some(ref n) = name {
        if n.trim().is_empty() {
            return Ok(errors::bad_request_with_code(
                "name must not be empty",
                "RULE_VALIDATION",
            ));
        }
    }
    if let Some(ref a) = actions {
        if let Err(msg) = validate_actions(a) {
            return Ok(errors::bad_request_with_code(msg, "RULE_VALIDATION"));
        }
    }
    let effective_conditions = conditions.clone().unwrap_or(existing.conditions);
    let effective_actions = actions.clone().unwrap_or(existing.actions);
    let effective_trigger_kind = trigger_kind.unwrap_or(existing.trigger_kind);
    // A dry run shadows an automated trigger (`rules_manual_no_dry_run`).
    if effective_trigger_kind == RuleTriggerKind::Manual && existing.state == RuleState::DryRun {
        return Ok(errors::conflict_with_code(
            "a rule in dry run can't switch to the manual trigger; pause it first",
            "RULE_STATE_TRANSITION",
        ));
    }
    if effective_trigger_kind == RuleTriggerKind::Manual
        && !is_empty_conditions(&effective_conditions)
    {
        return Ok(manual_conditions_refused());
    }
    let predicate = match compile_conditions(effective_trigger_kind, &effective_conditions) {
        Ok(predicate) => predicate,
        Err(resp) => return Ok(*resp),
    };
    // A plugin event rule's settings are checked when the trigger or its
    // settings change. The plugin is looked up only when they name another
    // plugin or event than the rule has: one that has since lost its plugin
    // can still be renamed or have its steps edited.
    let mut plugin_trigger = effective_trigger_kind == RuleTriggerKind::PluginEvent
        && (trigger_kind.is_some() || trigger_config.is_some());
    if plugin_trigger {
        let config = trigger_config.as_ref().unwrap_or(&existing.trigger_config);
        match plugin_trigger_config(config) {
            Ok(config) => {
                plugin_trigger = existing.trigger_kind != RuleTriggerKind::PluginEvent
                    || config != existing.trigger_config;
                trigger_config = Some(config);
            }
            Err(msg) => return Ok(errors::bad_request_with_code(msg, "RULE_VALIDATION")),
        }
    }
    let reads = rules::reads_beyond_guards(&effective_conditions);
    let writes = rules::derive_writes_set(&effective_actions);
    if let Err(e) = check_self_referential(&reads, &writes, override_self_reference) {
        return Ok(errors::conflict_with_code(
            format!(
                "rule reads and writes the same fields: {} (set override_self_reference=true to bypass)",
                e.fields.join(", ")
            ),
            "RULE_SELF_REFERENTIAL",
        ));
    }

    let change = RuleUpdate {
        name,
        description,
        trigger_kind,
        trigger_config,
        conditions,
        actions,
        reads_set: None,
        writes_set: None,
        state: None,
        priority,
        last_fired_at: None,
        fire_count: None,
        archived_at: None,
    };

    let plugin_config = change.trigger_config.clone().filter(|_| plugin_trigger);
    let saved = tc.run_result(|conn| {
        if let Some(resp) = unknown_reference(conn, &predicate)? {
            return Err(UpdateRuleError::Refused(Box::new(resp)));
        }
        // A reply with no agent of its own sends as the rule's author.
        if let Some(problem) = rules::step_person_problem(
            conn,
            existing.workspace_id,
            effective_trigger_kind,
            &effective_actions,
            existing.created_by,
        )? {
            return Err(UpdateRuleError::Refused(Box::new(step_person_refused(
                problem,
            ))));
        }
        if let Some(config) = &plugin_config {
            if let Some(resp) = unknown_plugin_event(conn, config)? {
                return Err(UpdateRuleError::Refused(Box::new(resp)));
            }
        }
        let rule = rules::update(conn, id, change).map_err(UpdateRuleError::Write)?;
        let preview = rules::preview_match(conn, &predicate, rules::PREVIEW_SCAN)?;
        Ok((rule, preview))
    });
    match saved {
        Ok((rule, preview)) => Ok(HttpResponse::Ok().json(SavedRuleDto::new(rule, preview))),
        Err(UpdateRuleError::Refused(resp)) => Ok(*resp),
        Err(UpdateRuleError::Write(rules::WriteError::NotFound(_))) => {
            Err(ApiError::NotFoundMsg(format!("rule {id} not found")))
        }
        Err(UpdateRuleError::Write(rules::WriteError::Db(e))) => Err(ApiError::Database(e)),
        Err(UpdateRuleError::Write(rules::WriteError::NotArchived(_))) => {
            // Not reachable from update(); only hard_delete returns
            // NotArchived. Defensive 500 if it ever leaks.
            Err(ApiError::Internal(
                "unexpected NotArchived from update".into(),
            ))
        }
    }
}

/// Why `update_rule` didn't save the change.
enum UpdateRuleError {
    /// The change is invalid; the response says why.
    Refused(Box<HttpResponse>),
    Write(rules::WriteError),
}

impl From<diesel::result::Error> for UpdateRuleError {
    fn from(e: diesel::result::Error) -> Self {
        Self::Write(rules::WriteError::Db(e))
    }
}

/// `POST /api/rules/preview` (admin). Checks the conditions of a rule
/// being edited as saving would, and returns how many recent tickets they
/// match, for the editor's live count.
pub async fn preview_rule(
    req: HttpRequest,
    body: web::Json<PreviewRuleRequest>,
    mut tc: TenantConn,
) -> Result<HttpResponse, ApiError> {
    require_workspace_role(&req, WorkspaceRole::Admin)?;
    let PreviewRuleRequest {
        trigger_kind,
        conditions,
    } = body.into_inner();
    if trigger_kind == RuleTriggerKind::Manual && !is_empty_conditions(&conditions) {
        return Ok(manual_conditions_refused());
    }
    let predicate = match compile_conditions(trigger_kind, &conditions) {
        Ok(predicate) => predicate,
        Err(resp) => return Ok(*resp),
    };
    let previewed = tc.run_result(|conn| {
        if let Some(resp) = unknown_reference(conn, &predicate)? {
            return Err(refused(resp));
        }
        Ok(rules::preview_match(conn, &predicate, rules::PREVIEW_SCAN)?)
    });
    match previewed {
        Ok(preview) => Ok(HttpResponse::Ok().json(preview)),
        Err(InsertRuleError::Refused(resp)) => Ok(*resp),
        Err(InsertRuleError::Db(e)) => Err(ApiError::Database(e)),
    }
}

/// Query string of `GET /api/rules/conflicts`: how many days back to look
/// for rules that ran on the same ticket. 7 by default, 1 to 90.
#[derive(Debug, Default, Deserialize)]
pub struct RuleConflictsQuery {
    pub days: Option<i64>,
}

/// `GET /api/rules/conflicts` (admin). Live and dry run automated rules
/// where one rule's steps change a field another rule's conditions check,
/// and pairs of them that ran on the same ticket in the last `days` days.
pub async fn list_rule_conflicts(
    req: HttpRequest,
    query: web::Query<RuleConflictsQuery>,
    mut tc: TenantConn,
) -> Result<HttpResponse, ApiError> {
    require_workspace_role(&req, WorkspaceRole::Admin)?;
    let days = query.days.unwrap_or(7).clamp(1, 90);
    let since = Utc::now() - chrono::Duration::days(days);
    match tc.run(|conn| crate::repository::rule_conflicts::find(conn, since)) {
        Ok(conflicts) => Ok(HttpResponse::Ok().json(conflicts)),
        Err(e) => Err(ApiError::Database(e)),
    }
}

/// `PATCH /api/rules/{id}/state` (admin). State transition with
/// the legal-transition table; everything else returns 409
/// `RULE_STATE_TRANSITION`, and any change to an archived rule 409
/// `RULE_ARCHIVED`.
pub async fn transition_state(
    req: HttpRequest,
    path: web::Path<i32>,
    body: web::Json<StateTransitionRequest>,
    mut tc: TenantConn,
) -> Result<HttpResponse, ApiError> {
    require_workspace_role(&req, WorkspaceRole::Admin)?;
    let id = path.into_inner();
    let target = body.into_inner().state;

    let existing = match tc.run(|conn| rules::find(conn, id)) {
        Ok(Some(r)) => r,
        Ok(None) => return Err(ApiError::NotFoundMsg(format!("rule {id} not found"))),
        Err(e) => return Err(ApiError::Database(e)),
    };
    if existing.is_archived() {
        // Archiving again changes nothing; anything else would bring it back.
        return Ok(if target == RuleState::Archived {
            HttpResponse::Ok().json(RuleDto::from(existing))
        } else {
            archived_conflict(id)
        });
    }
    if existing.state == target {
        return Ok(HttpResponse::Ok().json(RuleDto::from(existing)));
    }
    if !legal_state_transition(existing.trigger_kind, existing.state, target) {
        return Ok(errors::conflict_with_code(
            format!(
                "cannot transition rule from {} to {}",
                existing.state.as_str(),
                target.as_str()
            ),
            "RULE_STATE_TRANSITION",
        ));
    }
    // Someone a step names may have left since the rule was saved: it can
    // be parked and edited, but not run.
    if matches!(target, RuleState::Live | RuleState::DryRun) {
        match tc.run(|conn| {
            rules::step_person_problem(
                conn,
                existing.workspace_id,
                existing.trigger_kind,
                &existing.actions,
                existing.created_by,
            )
        }) {
            Ok(Some(problem)) => return Ok(step_person_refused(problem)),
            Ok(None) => {}
            Err(e) => return Err(ApiError::Database(e)),
        }
    }
    // Archive transitions stamp archived_at so the
    // archived-vs-live filter in list_pickable_manual stays
    // consistent with the state column. The reverse (un-archive)
    // isn't a v1 transition; out-of-scope per decision 32.
    let archived_at = match target {
        RuleState::Archived => Some(Some(Utc::now())),
        _ => None,
    };
    let change = RuleUpdate {
        state: Some(target),
        archived_at,
        ..Default::default()
    };
    match tc.run_result(|conn| rules::update(conn, id, change)) {
        Ok(rule) => Ok(HttpResponse::Ok().json(RuleDto::from(rule))),
        Err(rules::WriteError::NotFound(_)) => {
            Err(ApiError::NotFoundMsg(format!("rule {id} not found")))
        }
        Err(rules::WriteError::Db(e)) => Err(ApiError::Database(e)),
        Err(rules::WriteError::NotArchived(_)) => Err(ApiError::Internal(
            "unexpected NotArchived from state transition".into(),
        )),
    }
}

/// `DELETE /api/rules/{id}` (admin). Soft-archive by default;
/// `?hard=true` permanently deletes but only if `archived_at` is
/// already set. Returns 204 on hard delete, 200 with the archived
/// rule on soft (unchanged if it was already archived).
pub async fn delete_rule(
    req: HttpRequest,
    path: web::Path<i32>,
    query: web::Query<DeleteRuleQuery>,
    mut tc: TenantConn,
) -> Result<HttpResponse, ApiError> {
    require_workspace_role(&req, WorkspaceRole::Admin)?;
    let id = path.into_inner();
    if query.hard {
        match tc.run_result(|conn| rules::hard_delete(conn, id)) {
            Ok(()) => Ok(HttpResponse::NoContent().finish()),
            Err(rules::WriteError::NotFound(_)) => {
                Err(ApiError::NotFoundMsg(format!("rule {id} not found")))
            }
            Err(rules::WriteError::NotArchived(_)) => Ok(errors::conflict_with_code(
                "rule must be archived before hard delete",
                "RULE_NOT_ARCHIVED",
            )),
            Err(rules::WriteError::Db(e)) => Err(ApiError::Database(e)),
        }
    } else {
        // Archiving an archived rule changes nothing: no new archived_at, no
        // new version.
        match tc.run(|conn| rules::find(conn, id)) {
            Ok(Some(existing)) if existing.is_archived() => {
                return Ok(HttpResponse::Ok().json(RuleDto::from(existing)));
            }
            Ok(Some(_)) => {}
            Ok(None) => return Err(ApiError::NotFoundMsg(format!("rule {id} not found"))),
            Err(e) => return Err(ApiError::Database(e)),
        }
        match tc.run_result(|conn| rules::archive(conn, id, Utc::now())) {
            Ok(rule) => Ok(HttpResponse::Ok().json(RuleDto::from(rule))),
            Err(rules::WriteError::NotFound(_)) => {
                Err(ApiError::NotFoundMsg(format!("rule {id} not found")))
            }
            Err(rules::WriteError::Db(e)) => Err(ApiError::Database(e)),
            Err(rules::WriteError::NotArchived(_)) => Err(ApiError::Internal(
                "unexpected NotArchived from archive".into(),
            )),
        }
    }
}

/// `POST /api/rules/{id}/restore` (admin). Brings an archived rule back as
/// a draft; a rule that isn't archived comes back unchanged.
pub async fn restore_rule(
    req: HttpRequest,
    path: web::Path<i32>,
    mut tc: TenantConn,
) -> Result<HttpResponse, ApiError> {
    require_workspace_role(&req, WorkspaceRole::Admin)?;
    let id = path.into_inner();
    match tc.run_result(|conn| rules::restore(conn, id)) {
        Ok(rule) => Ok(HttpResponse::Ok().json(RuleDto::from(rule))),
        Err(rules::WriteError::NotFound(_)) => {
            Err(ApiError::NotFoundMsg(format!("rule {id} not found")))
        }
        Err(rules::WriteError::Db(e)) => Err(ApiError::Database(e)),
        Err(rules::WriteError::NotArchived(_)) => Err(ApiError::Internal(
            "unexpected NotArchived from restore".into(),
        )),
    }
}

/// `GET /api/rules/{id}/versions` (admin). The activity-feed
/// inspector deep-links into a specific version; the editor's
/// "version history" sidebar reads this.
pub async fn list_rule_versions(
    req: HttpRequest,
    path: web::Path<i32>,
    mut tc: TenantConn,
) -> Result<HttpResponse, ApiError> {
    require_workspace_role(&req, WorkspaceRole::Admin)?;
    let rule_id = path.into_inner();
    match tc.run(|conn| rules::list_versions(conn, rule_id)) {
        Ok(rs) => {
            Ok(HttpResponse::Ok()
                .json(rs.into_iter().map(RuleVersionDto::from).collect::<Vec<_>>()))
        }
        Err(e) => Err(ApiError::Database(e)),
    }
}

/// `GET /api/rules/{id}/versions/{version}` (admin). The single-
/// version snapshot the inspector renders.
pub async fn get_rule_version(
    req: HttpRequest,
    path: web::Path<(i32, i32)>,
    mut tc: TenantConn,
) -> Result<HttpResponse, ApiError> {
    require_workspace_role(&req, WorkspaceRole::Admin)?;
    let (rule_id, version) = path.into_inner();
    match tc.run(|conn| rules::find_version(conn, rule_id, version)) {
        Ok(Some(v)) => Ok(HttpResponse::Ok().json(RuleVersionDto::from(v))),
        Ok(None) => Err(ApiError::NotFoundMsg(format!(
            "rule {rule_id} version {version} not found"
        ))),
        Err(e) => Err(ApiError::Database(e)),
    }
}

// =====================================================================
// Recent-activity log (unit-09).
// =====================================================================

/// `GET /api/rule-applications` (admin). The workspace-wide
/// recent-activity feed. Default page size 50, max 500 (the repo
/// clamps).
pub async fn list_rule_applications(
    req: HttpRequest,
    query: web::Query<ListApplicationsQuery>,
    mut tc: TenantConn,
) -> Result<HttpResponse, ApiError> {
    require_workspace_role(&req, WorkspaceRole::Admin)?;
    let q = query.into_inner();
    let filter = rules::ApplicationFilter {
        rule_id: q.rule_id,
        ticket_id: q.ticket_id,
        status: q.status,
        actor_uuid: q.actor_uuid,
        from: q.from,
        to: q.to,
        limit: q.limit,
    };
    match tc.run(|conn| rules::list_applications(conn, filter)) {
        Ok(rows) => Ok(HttpResponse::Ok().json(rows)),
        Err(e) => Err(ApiError::Database(e)),
    }
}

/// `GET /api/rule-applications/{id}` (admin). Full row with the
/// `condition_evaluation` / `actions_taken` / `actions_skipped` /
/// `failure_reason` payloads the inspector renders.
pub async fn get_rule_application(
    req: HttpRequest,
    path: web::Path<i64>,
    mut tc: TenantConn,
) -> Result<HttpResponse, ApiError> {
    require_workspace_role(&req, WorkspaceRole::Admin)?;
    let id = path.into_inner();
    match tc.run(|conn| rules::find_application(conn, id)) {
        Ok(Some(row)) => Ok(HttpResponse::Ok().json(row)),
        Ok(None) => Err(ApiError::NotFoundMsg(format!(
            "rule_application {id} not found"
        ))),
        Err(e) => Err(ApiError::Database(e)),
    }
}

/// `GET /api/tickets/{ticket_id}/rule-applications` (agent). The
/// per-ticket activity slice the open-ticket-view's history panel
/// reads. Agent-readable so the agent can see "rules that fired on
/// this ticket" without needing admin access to the workspace-wide
/// log.
pub async fn list_ticket_rule_applications(
    req: HttpRequest,
    path: web::Path<i32>,
    mut tc: TenantConn,
) -> Result<HttpResponse, ApiError> {
    require_workspace_role(&req, WorkspaceRole::Agent)?;
    let ticket_id = path.into_inner();
    match tc.run(|conn| rules::list_applications_for_ticket(conn, ticket_id)) {
        Ok(rows) => Ok(HttpResponse::Ok().json(rows)),
        Err(e) => Err(ApiError::Database(e)),
    }
}

// =====================================================================
// Manual apply (Wave 6 / unit-08). Agent role; the lifecycle in
// repository::rules::apply_manual owns the transaction.
// =====================================================================

#[derive(Debug, Deserialize)]
pub struct ApplyRuleRequest {
    pub ticket_id: i32,
    #[serde(default)]
    pub overrides: ApplyRuleOverrides,
}

#[derive(Debug, Default, Deserialize)]
pub struct ApplyRuleOverrides {
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub suppress_actions: Vec<usize>,
}

#[derive(Debug, Serialize)]
pub struct ApplyRuleResponse {
    pub rule: RuleDto,
    pub application_id: i64,
    pub correlation_id: Option<Uuid>,
    pub comment_id: Option<i32>,
    pub actions_executed: usize,
    pub actions_suppressed: usize,
}

// tenant-read-exempt: the post-apply ticket reload runs through ActorConn::run, which pins via with_actor_context (invisible to the scanner).
/// `POST /api/rules/{id}/apply`. Agent role minimum. The route is
/// registered before the wildcard `/rules/{id}` PUT/DELETE in
/// main.rs to dodge actix's route-shadowing footgun.
///
/// Post-commit side-effects (channel relay for public replies, search
/// reindex, recurring-ticket follow-up) live in this handler, not in the
/// apply lifecycle, so a slow downstream cannot roll back a committed
/// rule fire. Errors here are logged and swallowed.
pub async fn apply_rule(
    req: HttpRequest,
    path: web::Path<i32>,
    body: web::Json<ApplyRuleRequest>,
    pool: web::Data<Pool>,
    search: Option<web::Data<Arc<SearchService>>>,
) -> Result<HttpResponse, ApiError> {
    require_workspace_role(&req, WorkspaceRole::Agent)?;
    let Some(actor) = req
        .extensions()
        .get::<RequestContext>()
        .map(|c| c.actor.clone())
    else {
        return Err(ApiError::Unauthorized("Authentication required".into()));
    };
    let rule_id = path.into_inner();
    let mut conn = errors::db_conn(&pool)?;
    let body = body.into_inner();
    let ticket_id = body.ticket_id;
    let input = rules::ApplyInput {
        rule_id,
        ticket_id,
        overrides: rules::ApplyOverrides {
            body: body.overrides.body,
            suppress_actions: body.overrides.suppress_actions,
        },
    };

    // A step that closes a recurring ticket indexes its next occurrence.
    let observer = search
        .as_ref()
        .map(|s| s.get_ref() as &dyn crate::repository::tickets::TicketUpdatedObserver);
    let outcome = match rules::apply_manual(&mut conn, input, &actor, observer) {
        Ok(o) => o,
        Err(e) => return Ok(map_apply_error(e)),
    };

    // Channel relay: when the apply produced a public-visibility
    // reply, enqueue the outbound message the same way the regular
    // comment-create handler does. enqueue_for_comment spawns a
    // background task, so the response returns immediately. Internal
    // notes never relay.
    if let (Some(cid), Some(workspace_id)) = (outcome.comment_id, actor.workspace_id) {
        if let Err(e) =
            crate::services::rules::engine::relay_reply(&pool, workspace_id, ticket_id, cid)
        {
            tracing::warn!(error = %e, comment_id = cid, "rule reply channel relay enqueue failed");
        }
    }

    // Rule-applied field changes (status / assignee / priority / tags)
    // reach clients through the sync pool: each step goes through the
    // tickets or tags repository and emits its own ticket.* sync action,
    // and ticket.rule_applied drives the activity feed. The ticket is
    // reindexed for search here, once for all the steps. (A recurring ticket
    // a step closed got its next occurrence in that step's write.)
    if outcome.actions_executed > 0 {
        let search = search.as_ref().map(|s| s.get_ref());
        let mut db = ActorConn {
            conn: &mut conn,
            actor: &actor,
        };
        match db.run(|c| crate::repository::tickets::get_ticket_by_id(c, ticket_id)) {
            Ok(ticket) => {
                if let Some(search) = search {
                    let article = db
                        .run(|c| {
                            crate::repository::article_content::get_article_content_by_ticket_id(
                                c, ticket_id,
                            )
                        })
                        .ok();
                    search.ticket_updated(&ticket, article.as_ref());
                }
            }
            Err(e) => tracing::warn!(error = %e, ticket_id, "rule apply: post-save reload failed"),
        }
    }

    Ok(HttpResponse::Ok().json(ApplyRuleResponse {
        rule: RuleDto::from(outcome.rule),
        application_id: outcome.application.id,
        correlation_id: outcome.application.correlation_id,
        comment_id: outcome.comment_id,
        actions_executed: outcome.actions_executed,
        actions_suppressed: outcome.actions_suppressed,
    }))
}

fn map_apply_error(err: rules::ApplyError) -> HttpResponse {
    let message = err.to_string();
    use rules::ApplyError::*;
    match err {
        NotFound(_) | TicketNotFound(_) => errors::not_found_msg(message),
        NotLive(..) => errors::conflict_with_code(message, "RULE_NOT_LIVE"),
        NotManual(_) => errors::bad_request_with_code(message, "RULE_NOT_MANUAL"),
        // Only an automated run meets this; an agent's apply gets NotManual.
        WrongTrigger(..) => errors::bad_request_with_code(message, "RULE_WRONG_TRIGGER"),
        TicketMerged(_) => errors::bad_request_with_code(message, "RULE_TICKET_MERGED"),
        InvalidOverrideIndex(..) => {
            errors::bad_request_with_code(message, "MANUAL_APPLY_VALIDATION")
        }
        UnsupportedActionPhase1 { .. } => {
            errors::bad_request_with_code(message, "RULE_ACTION_UNSUPPORTED")
        }
        ActionFailed { .. } => errors::internal_with_code(message, "RULE_ACTION_FAILED"),
        // Soft-deleted / revoked agents land here distinctly from a
        // transient DB error so the frontend can clear stale auth
        // state and prompt for re-login instead of showing a generic
        // 500.
        AgentRevoked(_) => errors::unauthorized_with_code(message, "ACTOR_REVOKED"),
        MissingWorkspace => errors::internal("missing workspace context"),
        IneligibleAssignee(_) | NoAssignableTeamMember { .. } => {
            errors::bad_request_with_code(message, "INVALID_ASSIGNEE")
        }
        NoRequester { .. } => errors::bad_request_with_code(message, "RULE_NO_REQUESTER"),
        AssetRefused { .. } => errors::conflict_with_code(message, "RULE_ASSET_REFUSED"),
        // Only the engine's runs carry a budget or an email source.
        AlreadyRan { .. } | RanInChain | ChainCap | AutomatedSource => {
            errors::conflict_with_code(message, "RULE_SUPPRESSED")
        }
        Db(e) => errors::db_error(&e),
    }
}

// =====================================================================
// Starter catalog (Wave 8 / unit-19). Admin-only browse of the
// pre-built rules baked into the binary.
// =====================================================================

#[derive(Debug, Serialize)]
pub struct StarterRuleDto {
    pub id: String,
    pub name: String,
    pub description: String,
    pub trigger_kind: String,
    pub conditions: Value,
    pub actions: Value,
}

/// The caller's locale from `Accept-Language` (its first tag), or en.
fn request_locale(req: &HttpRequest) -> String {
    req.headers()
        .get(actix_web::http::header::ACCEPT_LANGUAGE)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.split(',').next())
        .map(|s| s.split(';').next().unwrap_or(s).trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "en".to_string())
}

/// `GET /api/admin/rule-starters`. Admin only. Returns the
/// localised catalog the rules-page "Browse starters" affordance
/// renders. Locale comes from `Accept-Language`; falls back to
/// English when the requested locale isn't represented.
///
/// Named statuses and tags are resolved in the caller's workspace (see
/// `services::starter_catalog`). A tag step lists the ids of the named
/// tags that exist and every name, so the dialog can name a tag that
/// adding the starter will create.
pub async fn list_starter_catalog(
    req: HttpRequest,
    mut tc: TenantConn,
) -> Result<HttpResponse, ApiError> {
    use crate::services::starter_catalog::{self, Purpose, ResolveError, WorkspaceRefs};

    require_workspace_role(&req, WorkspaceRole::Admin)?;
    let locale = request_locale(&req);
    let dtos = tc
        .run_result(|conn| {
            let mut refs = WorkspaceRefs::default();
            starter_catalog::list()
                .iter()
                .map(|r| {
                    Ok(StarterRuleDto {
                        id: r.id.clone(),
                        name: r.name_for(&locale).to_string(),
                        description: r.description_for(&locale).to_string(),
                        trigger_kind: r.trigger_kind.clone(),
                        conditions: r.conditions.clone(),
                        actions: refs.resolve_actions(conn, r, &locale, Purpose::Browse)?,
                    })
                })
                .collect::<Result<Vec<_>, ResolveError>>()
        })
        .map_err(|e| match e {
            ResolveError::Db(e) => ApiError::Database(e),
            // Browsing leaves a step without a status instead of refusing.
            ResolveError::NoStatusIn(_) => ApiError::Internal("starter status unresolved".into()),
        })?;
    Ok(HttpResponse::Ok().json(dtos))
}

/// `POST /api/admin/rule-starters/{id}`. Admin only. Adds a starter
/// to the workspace as a draft manual rule, named and described in the
/// `Accept-Language` locale, and returns it like `POST /api/rules`.
/// Named statuses and tags resolve in the workspace, and a named tag
/// the workspace doesn't have is created, all in one transaction with
/// the rule. The rule goes through the same checks as one written in
/// the editor. A starter that moves tickets to a status category the
/// workspace has no state in (none, or all archived) is refused with
/// `STARTER_STATUS_MISSING` before anything is written.
pub async fn add_starter(
    req: HttpRequest,
    path: web::Path<String>,
    mut tc: TenantConn,
) -> Result<HttpResponse, ApiError> {
    use crate::services::starter_catalog::{self, Purpose, ResolveError, WorkspaceRefs};

    require_workspace_role(&req, WorkspaceRole::Admin)?;
    let Some(workspace_id) = actor_workspace_id(&req) else {
        return Err(ApiError::Unauthorized("Authentication required".into()));
    };
    let Some(starter) = starter_catalog::find(&path.into_inner()) else {
        return Ok(errors::not_found("Starter rule"));
    };
    let locale = request_locale(&req);
    let created_by = actor_uuid(&req);
    created_response(tc.run_result(|conn| {
        let actions = WorkspaceRefs::default()
            .resolve_actions(conn, starter, &locale, Purpose::Add)
            .map_err(|e| match e {
                ResolveError::Db(e) => InsertRuleError::Db(e),
                ResolveError::NoStatusIn(category) => {
                    refused(starter_status_missing(&req, category))
                }
            })?;
        let description = starter.description_for(&locale);
        insert_rule(
            conn,
            workspace_id,
            created_by,
            CreateRuleRequest {
                name: starter.name_for(&locale).to_string(),
                description: (!description.is_empty()).then(|| description.to_string()),
                trigger_kind: RuleTriggerKind::Manual,
                trigger_config: empty_object(),
                conditions: starter.conditions.clone(),
                actions,
                priority: default_priority(),
                override_self_reference: false,
            },
        )
    }))
}

/// 400 for a starter that moves tickets to `category` in a workspace with
/// no state there, naming the category as the Workflow page does.
fn starter_status_missing(
    req: &HttpRequest,
    category: crate::models::WorkflowStateCategory,
) -> HttpResponse {
    use crate::utils::{i18n, locale};

    let locale = locale::request_locale(req);
    let label_key = format!("workflow-category-{}", category.as_str().replace('_', "-"));
    let label = i18n::tr(&locale, &label_key);
    errors::bad_request_with_code(
        i18n::tr_with(
            &locale,
            "backend-error-starter-status-missing",
            &[("category", label.into())],
        ),
        "STARTER_STATUS_MISSING",
    )
}

// =====================================================================
// Agent picker (unit-10).
// =====================================================================

/// `GET /api/tickets/{id}/applicable-actions` (Agent role minimum).
/// Returns the live manual rules in the caller's workspace. Per
/// decision 30, manual rules have empty conditions so we don't
/// per-ticket-condition-evaluate; the picker is category-organised
/// in the UI, but the endpoint returns the unfiltered manual-rules
/// list and the frontend groups it.
pub async fn list_applicable_actions(
    req: HttpRequest,
    _path: web::Path<i32>,
    mut tc: TenantConn,
) -> Result<HttpResponse, ApiError> {
    require_workspace_role(&req, WorkspaceRole::Agent)?;
    match tc.run(rules::list_pickable_manual) {
        Ok(rs) => {
            Ok(HttpResponse::Ok().json(rs.into_iter().map(RuleDto::from).collect::<Vec<_>>()))
        }
        Err(e) => Err(ApiError::Database(e)),
    }
}

// =====================================================================
// Local validation helpers used by create / update.
// =====================================================================

/// Surface-level sanity check on the actions JSONB. Full per-action
/// config validation lives with the executor in Wave 5; here we
/// just confirm the array shape and that every element has a
/// recognised `kind` so the editor's typo'd kind doesn't reach the
/// engine.
fn validate_actions(actions: &Value) -> Result<(), String> {
    let Some(arr) = actions.as_array() else {
        return Err("actions must be a JSON array".to_string());
    };
    if arr.is_empty() {
        return Err("actions must contain at least one entry".to_string());
    }
    // The steps `repository::rules::apply_manual` runs. A rule is saved only
    // with these, so applying it never fails on a step it can't run.
    const RUNNABLE: &[&str] = &[
        "reply",
        "set_status",
        "assign",
        "unassign",
        "add_tags",
        "remove_tags",
        "set_priority",
        "assign_assets_to_requester",
        "lend_assets",
        "return_requester_assets",
        "notify",
        "stop_processing",
    ];
    // Named in the rule model but not run yet.
    const NOT_YET: &[&str] = &["apply_macro_template", "webhook"];
    for (i, action) in arr.iter().enumerate() {
        let Some(obj) = action.as_object() else {
            return Err(format!("actions[{i}] must be a JSON object"));
        };
        let kind = obj
            .get("kind")
            .and_then(|k| k.as_str())
            .ok_or_else(|| format!("actions[{i}] missing kind"))?;
        if NOT_YET.contains(&kind) {
            return Err(format!(
                "Step {}: this kind of step can't run yet, so remove it",
                i + 1
            ));
        }
        if !RUNNABLE.contains(&kind) {
            return Err(format!("actions[{i}] has unknown kind: {kind}"));
        }
        validate_step_config(kind, obj.get("config").unwrap_or(&Value::Null))
            .map_err(|problem| format!("Step {}: {problem}", i + 1))?;
    }
    Ok(())
}

/// 400 for a step that names someone it can't send as or email, each
/// with its own code.
fn step_person_refused(problem: rules::StepPersonProblem) -> HttpResponse {
    match problem {
        rules::StepPersonProblem::SendAs(step) => errors::bad_request_with_code(
            format!(
                "Step {step}: the agent this reply sends as can't work tickets in this workspace, so pick another"
            ),
            "RULE_SEND_AS_INELIGIBLE",
        ),
        rules::StepPersonProblem::NotifyRecipient(step) => errors::bad_request_with_code(
            format!(
                "Step {step}: the person to email isn't an active member of this workspace with an email address, so pick another"
            ),
            "RULE_NOTIFY_RECIPIENT_INVALID",
        ),
    }
}

/// A step's settings must be complete when the rule is saved, so an agent
/// never applies a rule that fails on a missing value.
fn validate_step_config(kind: &str, config: &Value) -> Result<(), &'static str> {
    let positive_int = |key: &str| {
        config
            .get(key)
            .and_then(Value::as_i64)
            .is_some_and(|n| n > 0)
    };
    let text = |key: &str| config.get(key).and_then(Value::as_str).unwrap_or("");
    let uuid_or_absent = |key: &str| match config.get(key) {
        None | Some(Value::Null) => true,
        Some(v) => v.as_str().is_some_and(|s| Uuid::parse_str(s).is_ok()),
    };
    match kind {
        "reply" => {
            if text("body").trim().is_empty() {
                return Err("write the reply");
            }
            // Absent, the rule's author.
            if !uuid_or_absent("send_as_uuid") {
                return Err("pick an agent to send the reply as");
            }
        }
        "notify" => {
            match config.get("recipient").and_then(Value::as_str) {
                Some("requester") => {}
                Some("user") => {
                    if Uuid::parse_str(text("user_uuid")).is_err() {
                        return Err("pick a person to email");
                    }
                }
                _ => return Err("email the requester or a person"),
            }
            if text("subject").trim().is_empty() {
                return Err("write the email's subject");
            }
            if text("body").trim().is_empty() {
                return Err("write the email");
            }
        }
        "set_status" if !positive_int("workflow_state_id") => return Err("pick a status"),
        "assign" => match config
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or("direct")
        {
            "direct" => {
                let uuid = config
                    .get("user_uuid")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                if Uuid::parse_str(uuid).is_err() {
                    return Err("pick a person to assign");
                }
            }
            "group" if !positive_int("group_id") => return Err("pick a team to assign"),
            "group" => {}
            _ => return Err("assign to a person or a team"),
        },
        "add_tags" | "remove_tags" => {
            let tags = config.get("tag_ids").and_then(Value::as_array);
            if !tags.is_some_and(|t| {
                !t.is_empty() && t.iter().all(|v| v.as_i64().is_some_and(|n| n > 0))
            }) {
                return Err("pick at least one tag");
            }
        }
        "set_priority" => {
            let priority = config.get("priority").and_then(Value::as_str).unwrap_or("");
            if crate::models::TicketPriority::parse(priority).is_none() {
                return Err("pick a priority");
            }
        }
        // The loan's length is optional; without it the loan is open ended.
        "lend_assets" => match config.get("due_in_days") {
            None | Some(Value::Null) => {}
            Some(days) => {
                if !days
                    .as_i64()
                    .is_some_and(|d| (1..=rules::MAX_LOAN_DAYS).contains(&d))
                {
                    return Err("set the loan to between 1 and 3650 days, or leave it open");
                }
            }
        },
        _ => {}
    }
    Ok(())
}

fn is_empty_conditions(value: &Value) -> bool {
    matches!(value, Value::Array(a) if a.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legal_transitions_match_state_machine() {
        use RuleState::*;
        for kind in [RuleTriggerKind::Manual, RuleTriggerKind::TicketCreated] {
            assert!(legal_state_transition(kind, Draft, Live));
            assert!(legal_state_transition(kind, Live, Paused));
            assert!(legal_state_transition(kind, Paused, Live));
            assert!(legal_state_transition(kind, Paused, Draft));
            assert!(legal_state_transition(kind, Live, Archived));
            // Unarchive isn't a transition; restore is its own endpoint.
            assert!(!legal_state_transition(kind, Archived, Draft));
            assert!(!legal_state_transition(kind, Archived, Live));
            assert!(!legal_state_transition(kind, Archived, Paused));
            assert!(!legal_state_transition(kind, Archived, DryRun));
        }
        // Shadow runs are for automated triggers only.
        let manual = RuleTriggerKind::Manual;
        assert!(!legal_state_transition(manual, Draft, DryRun));
        assert!(!legal_state_transition(manual, Live, DryRun));
        assert!(!legal_state_transition(manual, Paused, DryRun));
        let event = RuleTriggerKind::TicketUpdated;
        assert!(legal_state_transition(event, Draft, DryRun));
        assert!(legal_state_transition(event, DryRun, Live));
        assert!(legal_state_transition(event, Live, DryRun));
        assert!(legal_state_transition(event, DryRun, Paused));
    }

    #[test]
    fn self_ref_check_flags_overlap() {
        let reads = vec![Some("ticket.priority".to_string())];
        let writes = vec![
            Some("ticket.priority".to_string()),
            Some("ticket.updated_at".to_string()),
        ];
        let err = check_self_referential(&reads, &writes, false).unwrap_err();
        assert_eq!(err.fields, vec!["ticket.priority"]);
    }

    #[test]
    fn self_ref_check_passes_with_override() {
        let reads = vec![Some("ticket.priority".to_string())];
        let writes = vec![Some("ticket.priority".to_string())];
        assert!(check_self_referential(&reads, &writes, true).is_ok());
    }

    #[test]
    fn self_ref_check_passes_when_disjoint() {
        let reads = vec![Some("ticket.title".to_string())];
        let writes = vec![Some("ticket.priority".to_string())];
        assert!(check_self_referential(&reads, &writes, false).is_ok());
    }

    /// "Assignee is empty, then assign" guards on the field it writes; its
    /// write can't meet its conditions again, so it saves without the
    /// override. Reading the assignee any other way still needs it.
    #[test]
    fn self_ref_check_passes_an_is_empty_guard() {
        let guard =
            serde_json::json!({ "kind": "leaf", "field": "ticket.assignee_uuid", "op": "is_null" });
        let writes = rules::derive_writes_set(&serde_json::json!([
            { "kind": "assign", "config": { "method": "group", "group_id": 3 } },
            { "kind": "stop_processing" },
        ]));
        let guarded = serde_json::json!({ "kind": "and", "children": [guard] });
        assert!(
            check_self_referential(&rules::reads_beyond_guards(&guarded), &writes, false).is_ok()
        );
        let not_empty = serde_json::json!({ "kind": "not", "child": guard });
        assert!(
            check_self_referential(&rules::reads_beyond_guards(&not_empty), &writes, false)
                .is_err()
        );
    }

    #[test]
    fn validate_actions_rejects_empty_array() {
        let actions = serde_json::json!([]);
        assert!(validate_actions(&actions).is_err());
    }

    #[test]
    fn validate_actions_rejects_unknown_kind() {
        let actions = serde_json::json!([{ "kind": "obliterate" }]);
        let err = validate_actions(&actions).unwrap_err();
        assert!(err.contains("unknown kind"));
    }

    /// A rule saves only with steps applying it can run: macro templates and
    /// webhooks don't run yet, so a rule carrying one would fail every time
    /// an agent applied it.
    #[test]
    fn validate_actions_refuses_steps_that_dont_run_yet() {
        for kind in ["apply_macro_template", "webhook"] {
            let actions = serde_json::json!([
                { "kind": "set_priority", "config": { "priority": "high" } },
                { "kind": kind, "config": {} }
            ]);
            let err = validate_actions(&actions).expect_err(kind);
            assert!(err.starts_with("Step 2: "), "{kind}: {err}");
        }
    }

    #[test]
    fn validate_actions_names_the_incomplete_step() {
        let actions = serde_json::json!([
            { "kind": "reply", "config": { "visibility": "public", "body": "hi" } },
            { "kind": "set_status", "config": { "workflow_state_id": 0 } }
        ]);
        assert_eq!(
            validate_actions(&actions).unwrap_err(),
            "Step 2: pick a status"
        );
    }

    #[test]
    fn validate_actions_checks_each_step_kind() {
        let ok = serde_json::json!([
            { "kind": "assign", "config": { "method": "direct", "user_uuid": "7b5f2c1e-3f0a-4c55-9f1d-0d1e2f3a4b5c" } },
            { "kind": "assign", "config": { "method": "group", "group_id": 3 } },
            { "kind": "add_tags", "config": { "tag_ids": [1, 2] } },
            { "kind": "set_priority", "config": { "priority": "normal" } },
            { "kind": "unassign" }
        ]);
        assert!(validate_actions(&ok).is_ok());
        for bad in [
            serde_json::json!([{ "kind": "reply", "config": { "body": "  " } }]),
            serde_json::json!([{ "kind": "assign", "config": { "method": "direct", "user_uuid": "nope" } }]),
            serde_json::json!([{ "kind": "assign", "config": { "method": "round_robin" } }]),
            serde_json::json!([{ "kind": "remove_tags", "config": { "tag_ids": [] } }]),
            serde_json::json!([{ "kind": "set_priority", "config": { "priority": "asap" } }]),
        ] {
            assert!(validate_actions(&bad).is_err(), "{bad} should be refused");
        }
    }

    /// The asset steps save with no settings; a loan's length is optional
    /// and, when set, a whole number of days.
    #[test]
    fn validate_actions_takes_the_asset_steps() {
        let ok = serde_json::json!([
            { "kind": "assign_assets_to_requester", "config": {} },
            { "kind": "lend_assets", "config": {} },
            { "kind": "lend_assets", "config": { "due_in_days": 14 } },
            { "kind": "lend_assets", "config": { "due_in_days": null } },
            { "kind": "return_requester_assets" }
        ]);
        assert!(validate_actions(&ok).is_ok());
        for days in [
            serde_json::json!(0),
            serde_json::json!(3651),
            serde_json::json!("7"),
        ] {
            let bad =
                serde_json::json!([{ "kind": "lend_assets", "config": { "due_in_days": days } }]);
            assert!(validate_actions(&bad).is_err(), "{bad} should be refused");
        }
    }

    #[test]
    fn validate_actions_accepts_known_kinds() {
        let actions = serde_json::json!([
            { "kind": "reply", "config": { "visibility": "public", "body": "hi" } },
            { "kind": "stop_processing" }
        ]);
        assert!(validate_actions(&actions).is_ok());
    }

    /// A notify step emails the requester or a named person, with a subject
    /// and a message. A reply's agent to send as is optional.
    #[test]
    fn validate_actions_checks_notify_and_send_as() {
        let person = "7b5f2c1e-3f0a-4c55-9f1d-0d1e2f3a4b5c";
        let ok = serde_json::json!([
            { "kind": "notify", "config": { "recipient": "requester", "subject": "Closing soon", "body": "Hi" } },
            { "kind": "notify", "config": { "recipient": "user", "user_uuid": person, "subject": "Escalated", "body": "See #{{ticket_id}}" } },
            { "kind": "reply", "config": { "body": "hi", "send_as_uuid": person } },
            { "kind": "reply", "config": { "body": "hi", "send_as_uuid": null } }
        ]);
        assert!(validate_actions(&ok).is_ok());
        for (bad, why) in [
            (
                serde_json::json!({ "kind": "notify", "config": { "subject": "s", "body": "b" } }),
                "email the requester or a person",
            ),
            (
                serde_json::json!({ "kind": "notify", "config": { "recipient": "user", "subject": "s", "body": "b" } }),
                "pick a person to email",
            ),
            (
                serde_json::json!({ "kind": "notify", "config": { "recipient": "requester", "subject": " ", "body": "b" } }),
                "write the email's subject",
            ),
            (
                serde_json::json!({ "kind": "notify", "config": { "recipient": "requester", "subject": "s" } }),
                "write the email",
            ),
            (
                serde_json::json!({ "kind": "reply", "config": { "body": "hi", "send_as_uuid": "nope" } }),
                "pick an agent to send the reply as",
            ),
        ] {
            let err = validate_actions(&serde_json::json!([bad])).expect_err(why);
            assert_eq!(err, format!("Step 1: {why}"));
        }
    }

    #[test]
    fn a_plugin_trigger_names_a_plugin_and_one_of_its_events() {
        let plugin = Uuid::now_v7();
        let saved = plugin_trigger_config(&serde_json::json!({
            "plugin_uuid": plugin.to_string().to_uppercase(),
            "event_type": "github:issue_linked",
            "reply_kind": "any",
        }))
        .expect("valid");
        assert_eq!(
            saved,
            serde_json::json!({ "plugin_uuid": plugin.to_string(), "event_type": "github:issue_linked" }),
            "the uuid as events record it, and nothing else"
        );
        let long = format!("github:{}", "e".repeat(64));
        for config in [
            serde_json::json!({}),
            serde_json::json!({ "event_type": "github:issue_linked" }),
            serde_json::json!({ "plugin_uuid": "nope", "event_type": "github:issue_linked" }),
            serde_json::json!({ "plugin_uuid": plugin.to_string() }),
            serde_json::json!({ "plugin_uuid": plugin.to_string(), "event_type": "" }),
            serde_json::json!({ "plugin_uuid": plugin.to_string(), "event_type": long }),
            // A host event is never a plugin's.
            serde_json::json!({ "plugin_uuid": plugin.to_string(), "event_type": "ticket:updated" }),
        ] {
            assert!(plugin_trigger_config(&config).is_err(), "{config}");
        }
    }

    #[test]
    fn is_empty_conditions_matches_array_only() {
        assert!(is_empty_conditions(&serde_json::json!([])));
        assert!(!is_empty_conditions(&serde_json::json!({})));
        assert!(!is_empty_conditions(&serde_json::json!({"kind": "leaf"})));
        assert!(!is_empty_conditions(&serde_json::json!([{"x": 1}])));
    }
}
