//! Plugin event emission endpoint.
//!
//! POST `/api/plugins/{plugin_uuid}/events` — accepts an event from a
//! running plugin and records it in `sync_actions` with
//! `actor_kind = 'plugin'`. The endpoint reuses the caller's
//! authenticated session for authorization (the plugin runs inside
//! an iframe in the user's UI, so the user's JWT is what gates
//! access to plugin paths). The actor_uuid is the plugin's UUID,
//! not the user's, so consumers can distinguish plugin-emitted
//! events from user-emitted ones.
//!
//! The aggregate must be `plugin`. Plugins can't invent aggregates
//! (every aggregate needs a `sync_aggregate` enum value, a manifest in
//! `backend/sync-models/`, and downstream consumer awareness, none of
//! which a runtime emit can synthesize), and the core aggregates are
//! rows clients apply as server-written. Plugins extend behaviour
//! through the `event_type` string instead. (The architecture doc § 6
//! references this constraint as part of the manifest design.)
//!
//! A plugin event is an event, never a row. `plugin` is a pooled aggregate:
//! clients upsert or delete a pool row for any `plugin` action whose payload
//! carries an `id` or `uuid`. So the server sets what decides that: the
//! aggregate id is the emitting plugin's uuid, the op is always an update, and
//! the caller's data is wrapped as `{ "event": ... }`, which has no row key, so
//! clients treat it as a side event (observers still see it). The caller's
//! `aggregate_id` and `op` are accepted for compatibility and ignored.

use actix_web::{web, HttpMessage, HttpRequest, HttpResponse};
use serde::Deserialize;
use serde_json::Value;
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::db::Pool;
use crate::errors::{self, ApiError};
use crate::handlers::helpers;
use crate::middleware::RequestContext;
use crate::models::{SyncAggregate, SyncOp};
use crate::repository::plugins as plugin_repo;
use crate::services::plugins::manifest_validate::KNOWN_EVENTS;
use crate::sync::actor::ActorContext;
use crate::sync::emit::{self, SyncEmit};
use crate::sync::groups;
use crate::sync::session;

#[derive(Debug, Deserialize)]
pub struct PluginEventBody {
    pub aggregate: SyncAggregate,
    /// Ignored: the recorded aggregate id is the emitting plugin's uuid.
    #[serde(default)]
    pub aggregate_id: Option<String>,
    /// Ignored: a plugin event is always recorded as an update.
    #[serde(default)]
    pub op: Option<SyncOp>,
    pub event_type: String,
    pub data: Value,
    #[serde(default)]
    pub causation_id: Option<Uuid>,
}

const PLUGIN_EVENT_TYPE_MAX: usize = 64;
/// Cap the event payload. The row fans out to every workspace SSE client, so an
/// oversized body is an amplification vector. (Plugin events don't reach
/// webhooks: their names are colon-named plugin events, and webhooks deliver
/// dot-named host events only.)
const PLUGIN_EVENT_DATA_MAX: usize = 32 * 1024;
/// Per (workspace, plugin) emission budget, bounds how fast any one member can
/// drive plugin-attributed fan-out.
const PLUGIN_EVENT_RATE_MAX: u32 = 120;
const PLUGIN_EVENT_RATE_WINDOW_SECS: u64 = 60;

/// Pure, DB-free bounds on the event body. Returns the client error message on
/// rejection. (The signature-visible half of the B6 hardening; the group and
/// rate constraints live in the handler because they need request context.)
fn validate_event_body(body: &PluginEventBody) -> Result<(), &'static str> {
    if !matches!(body.aggregate, SyncAggregate::Plugin) {
        return Err("plugin events must use the plugin aggregate");
    }
    if body.event_type.trim().is_empty() || body.event_type.len() > PLUGIN_EVENT_TYPE_MAX {
        return Err("event_type must be 1 to 64 characters");
    }
    // Only a known plugin event, as a manifest may declare (the manifest check
    // in the handler holds a plugin to the ones it declares).
    if !KNOWN_EVENTS.contains(&body.event_type.as_str()) {
        return Err("event_type is not a known plugin event");
    }
    if serde_json::to_vec(&body.data)
        .map(|v| v.len())
        .unwrap_or(usize::MAX)
        > PLUGIN_EVENT_DATA_MAX
    {
        return Err("event data exceeds the size limit");
    }
    Ok(())
}

/// POST /api/plugins/{plugin_uuid}/events
pub async fn emit_plugin_event(
    pool: web::Data<Pool>,
    path: web::Path<Uuid>,
    body: web::Json<PluginEventBody>,
    req: HttpRequest,
) -> Result<HttpResponse, ApiError> {
    let plugin_uuid = path.into_inner();

    let body = body.into_inner();
    if let Err(msg) = validate_event_body(&body) {
        return Err(ApiError::BadRequest(msg.into()));
    }

    // Caller must be authenticated (plugins run inside the user's
    // iframe, so the user's JWT proxies the plugin call). The actor
    // recorded on the row is the plugin, not the user — but the
    // user's UUID flows through actor_ref so an audit can answer
    // "who triggered the plugin?".
    let (claims, _user_uuid, mut conn) = helpers::auth_conn(&req, &pool)?;

    // Plugin must exist before we accept its events. `plugins` is
    // RLS-isolated; the read is scoped because `auth_conn` above pinned
    // the request's workspace on this connection (the runtime role is
    // NOBYPASSRLS, so an unpinned read would 404 every plugin).
    let plugin = match plugin_repo::get_plugin_by_uuid(&mut conn, plugin_uuid) {
        Ok(p) => p,
        Err(diesel::result::Error::NotFound) => {
            return Err(ApiError::NotFoundMsg("Plugin not found".into()));
        }
        Err(e) => {
            error!(error = %e, plugin_uuid = %plugin_uuid, "failed to look up plugin");
            return Err(ApiError::Internal("Failed to look up plugin".into()));
        }
    };

    // Only an active (installed) plugin may emit. A disabled / failed / pending
    // plugin emitting events would let a member forge activity through a plugin
    // that is not actually running.
    if !plugin.is_active() {
        warn!(
            plugin_uuid = %plugin_uuid,
            state = ?plugin.state,
            "rejected event from a non-active plugin"
        );
        return Err(ApiError::Forbidden("Plugin is not active".into()));
    }

    // The event_type must be one the plugin's manifest declares. The manifest
    // `events` list is an allowlist (see PluginManifest::events); enforcing it
    // here stops a caller from injecting arbitrary event types under the
    // plugin's identity, bounding a plugin to its own declared event surface.
    match plugin.parse_manifest() {
        Ok(manifest) => {
            if !manifest.events.iter().any(|e| e == &body.event_type) {
                warn!(
                    plugin_uuid = %plugin_uuid,
                    event_type = %body.event_type,
                    "rejected undeclared plugin event type"
                );
                return Err(ApiError::Forbidden(
                    "event_type is not declared in the plugin manifest".into(),
                ));
            }
        }
        Err(e) => {
            error!(error = %e, plugin_uuid = %plugin_uuid, "plugin manifest failed to parse");
            return Err(ApiError::Internal("Plugin manifest is invalid".into()));
        }
    }

    // The actor here is `Plugin`-kind (not `User`), so we can't delegate to
    // `TenantConn` which would build a User actor; instead we copy the
    // workspace_id off the RequestContext's actor and pair it with the
    // plugin actor for the emit below.
    let workspace_id = req
        .extensions()
        .get::<RequestContext>()
        .and_then(|ctx| ctx.actor.workspace_id);

    // Bound the rate any single member can drive plugin-attributed fan-out
    // to the workspace's SSE clients. Fail open on a Redis outage: this is
    // abuse-limiting, not an auth gate, so a limiter outage must not break
    // plugin events, but log it.
    {
        let redis_url = crate::utils::rate_limit::get_redis_url();
        let key = format!(
            "plugin_events:{}:{}",
            workspace_id.unwrap_or(0),
            plugin_uuid
        );
        match crate::utils::rate_limit::RateLimiter::check_rate_limit(
            &redis_url,
            &key,
            PLUGIN_EVENT_RATE_MAX,
            PLUGIN_EVENT_RATE_WINDOW_SECS,
        )
        .await
        {
            Ok(true) => {}
            Ok(false) => {
                warn!(plugin_uuid = %plugin_uuid, "plugin event rate limit exceeded");
                return Ok(errors::too_many_requests(
                    "Too many plugin events",
                    PLUGIN_EVENT_RATE_WINDOW_SECS,
                ));
            }
            Err(e) => warn!(error = %e, "plugin event rate limiter unavailable; allowing"),
        }
    }

    let user_ref = format!("plugin:{} via user:{}", plugin.name, claims.sub);
    let actor = ActorContext {
        kind: crate::sync::actor::ActorKind::Plugin,
        uuid: Some(plugin.uuid),
        reference: Some(user_ref),
        correlation_id: None,
        client_tx_id: None,
        // Workspace pin sourced from RequestContext (Phase 2d
        // delivery). If absent (no workspace middleware match,
        // tests bypassing middleware), the GUC stays unset and
        // the strict RLS policy returns zero rows — preferable
        // to a silent cross-tenant write.
        workspace_id,
    };

    // Fan-out is workspace-scoped host-side. We deliberately do NOT accept a
    // caller-supplied `groups` list: it would let any member target arbitrary
    // SSE topics (another user's stream, an arbitrary ticket) with a forged,
    // plugin-attributed event. Per-entity scoping returns with the sandbox
    // redesign, behind a real access check. (B6)
    let groups = groups::workspace();
    let aggregate = body.aggregate;
    let event_type_owned = body.event_type.clone();

    // `with_actor_context` opens a transaction, primes the actor +
    // workspace GUCs, then runs the closure inside it. Same
    // mechanism `TenantConn::run` uses internally; we call it
    // directly here because the actor is a Plugin actor, not the
    // User actor TenantConn would synthesize.
    let result =
        session::with_actor_context::<_, diesel::result::Error>(&mut conn, &actor, |conn| {
            emit::record(
                conn,
                // Event, not row: see the module docs.
                SyncEmit {
                    aggregate,
                    aggregate_id: plugin.uuid.to_string(),
                    op: SyncOp::Update,
                    event_type: &event_type_owned,
                    data: serde_json::json!({ "event": body.data }),
                    groups,
                    causation_id: body.causation_id,
                },
            )
        });

    match result {
        Ok(sync_id) => {
            info!(
                plugin_uuid = %plugin_uuid,
                event_type = %event_type_owned,
                sync_id,
                "plugin emitted event"
            );
            Ok(HttpResponse::Created().json(serde_json::json!({ "sync_id": sync_id })))
        }
        Err(e) => {
            warn!(
                error = %e,
                plugin_uuid = %plugin_uuid,
                event_type = %event_type_owned,
                "failed to record plugin event"
            );
            Err(ApiError::Internal("Failed to record plugin event".into()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn body(event_type: &str, data: Value) -> PluginEventBody {
        PluginEventBody {
            aggregate: SyncAggregate::Plugin,
            aggregate_id: None,
            op: None,
            event_type: event_type.to_string(),
            data,
            causation_id: None,
        }
    }

    #[test]
    fn accepts_a_reasonable_event() {
        assert!(validate_event_body(&body("ticket:created", json!({ "a": 1 }))).is_ok());
    }

    #[test]
    fn rejects_a_core_aggregate() {
        for aggregate in [
            SyncAggregate::Ticket,
            SyncAggregate::Comment,
            SyncAggregate::User,
        ] {
            let event = PluginEventBody {
                aggregate,
                ..body("ticket:created", json!({}))
            };
            assert!(validate_event_body(&event).is_err());
        }
    }

    #[test]
    fn rejects_empty_or_overlong_event_type() {
        assert!(validate_event_body(&body("   ", Value::Null)).is_err());
        let long = "e".repeat(PLUGIN_EVENT_TYPE_MAX + 1);
        assert!(validate_event_body(&body(&long, Value::Null)).is_err());
    }

    /// Only a known plugin event: a made-up name, or a host event's dot name,
    /// is refused even if a manifest lists it.
    #[test]
    fn rejects_an_unknown_event_type() {
        for name in ["report:ready", "ticket.created", "x.done"] {
            assert!(
                validate_event_body(&body(name, json!({}))).is_err(),
                "{name}"
            );
        }
    }

    /// The caller's row-shaping fields are optional and ignored, so a body
    /// without them is accepted.
    #[test]
    fn aggregate_id_and_op_are_optional() {
        let parsed: PluginEventBody = serde_json::from_value(json!({
            "aggregate": "plugin",
            "event_type": "ticket:created",
            "data": {},
        }))
        .expect("body parses without aggregate_id and op");
        assert!(validate_event_body(&parsed).is_ok());
    }

    #[test]
    fn rejects_oversized_data() {
        let big = json!({ "blob": "x".repeat(PLUGIN_EVENT_DATA_MAX) });
        assert!(validate_event_body(&body("ticket:created", big)).is_err());
    }

    /// B6 structural guard: a caller cannot supply `groups`. Unknown fields are
    /// ignored on deserialize, so an injected topic list is dropped and fan-out
    /// is always host-derived (`groups::workspace()`).
    #[test]
    fn caller_supplied_groups_are_dropped() {
        let parsed: PluginEventBody = serde_json::from_value(json!({
            "aggregate": "plugin",
            "aggregate_id": "42",
            "op": "U",
            "event_type": "x.done",
            "data": {},
            "groups": ["user:00000000-0000-0000-0000-000000000000", "ticket:99"],
        }))
        .expect("body parses, ignoring the injected groups");
        // There is no `groups` field to carry the injected topics; the handler
        // always emits to the workspace group.
        assert_eq!(parsed.event_type, "x.done");
    }
}
