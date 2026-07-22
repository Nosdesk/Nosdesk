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
//! The aggregate must be one of the registered `SyncAggregate`
//! variants. We deliberately don't let plugins invent new
//! aggregates: every aggregate needs a `sync_aggregate` enum value,
//! a manifest in `backend/sync-models/`, and downstream consumer
//! awareness, none of which a runtime emit can synthesize. Plugins
//! extend behaviour through the `event_type` string instead, which
//! is free-form per call. (The architecture doc § 6 references this
//! constraint as part of the manifest design.)

use actix_web::{web, HttpMessage, HttpRequest, HttpResponse, Responder};
use serde::Deserialize;
use serde_json::Value;
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::db::Pool;
use crate::handlers::{errors, helpers};
use crate::middleware::RequestContext;
use crate::models::{SyncAggregate, SyncOp};
use crate::repository::plugins as plugin_repo;
use crate::sync::actor::ActorContext;
use crate::sync::emit::{self, SyncEmit};
use crate::sync::groups;
use crate::sync::session;

#[derive(Debug, Deserialize)]
pub struct PluginEventBody {
    pub aggregate: SyncAggregate,
    pub aggregate_id: String,
    pub op: SyncOp,
    pub event_type: String,
    pub data: Value,
    #[serde(default)]
    pub causation_id: Option<Uuid>,
}

const PLUGIN_EVENT_TYPE_MAX: usize = 64;
/// The recorded `aggregate_id` is metadata on the row (fan-out is workspace-
/// scoped host-side, not driven by this value); bound it anyway.
const PLUGIN_AGGREGATE_ID_MAX: usize = 128;
/// Cap the event payload. The row fans out to every workspace SSE client and,
/// by `event_type`, to external webhook subscribers, so an oversized body is an
/// amplification vector.
const PLUGIN_EVENT_DATA_MAX: usize = 32 * 1024;
/// Per (workspace, plugin) emission budget, bounds how fast any one member can
/// drive plugin-attributed fan-out.
const PLUGIN_EVENT_RATE_MAX: u32 = 120;
const PLUGIN_EVENT_RATE_WINDOW_SECS: u64 = 60;

/// Pure, DB-free bounds on the event body. Returns the client error message on
/// rejection. (The signature-visible half of the B6 hardening; the group and
/// rate constraints live in the handler because they need request context.)
fn validate_event_body(body: &PluginEventBody) -> Result<(), &'static str> {
    if body.event_type.trim().is_empty() || body.event_type.len() > PLUGIN_EVENT_TYPE_MAX {
        return Err("event_type must be 1 to 64 characters");
    }
    if body.aggregate_id.trim().is_empty() {
        return Err("aggregate_id is required");
    }
    if body.aggregate_id.len() > PLUGIN_AGGREGATE_ID_MAX {
        return Err("aggregate_id is too long");
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
) -> impl Responder {
    let plugin_uuid = path.into_inner();

    let body = body.into_inner();
    if let Err(msg) = validate_event_body(&body) {
        return errors::bad_request(msg);
    }

    // Caller must be authenticated (plugins run inside the user's
    // iframe, so the user's JWT proxies the plugin call). The actor
    // recorded on the row is the plugin, not the user — but the
    // user's UUID flows through actor_ref so an audit can answer
    // "who triggered the plugin?".
    let (claims, _user_uuid, mut conn) = match helpers::auth_conn(&req, &pool) {
        Ok(v) => v,
        Err(e) => return e,
    };

    // Plugin must exist before we accept its events. `plugins` is
    // RLS-isolated; the read is scoped because `auth_conn` above pinned
    // the request's workspace on this connection (the runtime role is
    // NOBYPASSRLS, so an unpinned read would 404 every plugin).
    let plugin = match plugin_repo::get_plugin_by_uuid(&mut conn, plugin_uuid) {
        Ok(p) => p,
        Err(diesel::result::Error::NotFound) => {
            return errors::not_found_msg("Plugin not found");
        }
        Err(e) => {
            error!(error = %e, plugin_uuid = %plugin_uuid, "failed to look up plugin");
            return errors::internal("Failed to look up plugin");
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
        return errors::forbidden("Plugin is not active");
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
                return errors::forbidden("event_type is not declared in the plugin manifest");
            }
        }
        Err(e) => {
            error!(error = %e, plugin_uuid = %plugin_uuid, "plugin manifest failed to parse");
            return errors::internal("Plugin manifest is invalid");
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
    // (SSE + external webhooks). Fail open on a Redis outage: this is
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
                return errors::too_many_requests(
                    "Too many plugin events",
                    PLUGIN_EVENT_RATE_WINDOW_SECS,
                );
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
                SyncEmit {
                    aggregate,
                    aggregate_id: body.aggregate_id,
                    op: body.op,
                    event_type: &event_type_owned,
                    data: body.data,
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
            HttpResponse::Created().json(serde_json::json!({ "sync_id": sync_id }))
        }
        Err(e) => {
            warn!(
                error = %e,
                plugin_uuid = %plugin_uuid,
                event_type = %event_type_owned,
                "failed to record plugin event"
            );
            errors::internal("Failed to record plugin event")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn body(event_type: &str, aggregate_id: &str, data: Value) -> PluginEventBody {
        PluginEventBody {
            aggregate: SyncAggregate::Ticket,
            aggregate_id: aggregate_id.to_string(),
            op: SyncOp::Update,
            event_type: event_type.to_string(),
            data,
            causation_id: None,
        }
    }

    #[test]
    fn accepts_a_reasonable_event() {
        assert!(validate_event_body(&body("x.done", "42", json!({ "a": 1 }))).is_ok());
    }

    #[test]
    fn rejects_empty_or_overlong_event_type() {
        assert!(validate_event_body(&body("   ", "42", Value::Null)).is_err());
        let long = "e".repeat(PLUGIN_EVENT_TYPE_MAX + 1);
        assert!(validate_event_body(&body(&long, "42", Value::Null)).is_err());
    }

    #[test]
    fn rejects_empty_or_overlong_aggregate_id() {
        assert!(validate_event_body(&body("x", "", Value::Null)).is_err());
        let long = "1".repeat(PLUGIN_AGGREGATE_ID_MAX + 1);
        assert!(validate_event_body(&body("x", &long, Value::Null)).is_err());
    }

    #[test]
    fn rejects_oversized_data() {
        let big = json!({ "blob": "x".repeat(PLUGIN_EVENT_DATA_MAX) });
        assert!(validate_event_body(&body("x", "42", big)).is_err());
    }

    /// B6 structural guard: a caller cannot supply `groups`. Unknown fields are
    /// ignored on deserialize, so an injected topic list is dropped and fan-out
    /// is always host-derived (`groups::workspace()`).
    #[test]
    fn caller_supplied_groups_are_dropped() {
        let parsed: PluginEventBody = serde_json::from_value(json!({
            "aggregate": "ticket",
            "aggregate_id": "42",
            "op": "U",
            "event_type": "x.done",
            "data": {},
            "groups": ["user:00000000-0000-0000-0000-000000000000", "ticket:99"],
        }))
        .expect("body parses, ignoring the injected groups");
        // There is no `groups` field to carry the injected topics; the handler
        // always emits to the workspace group.
        assert_eq!(parsed.aggregate_id, "42");
        assert_eq!(parsed.event_type, "x.done");
    }
}
