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
    /// Optional explicit groups; defaults to `["workspace:1"]` when
    /// absent. Plugins can scope an event to a ticket / project by
    /// setting groups themselves.
    #[serde(default)]
    pub groups: Option<Vec<String>>,
    #[serde(default)]
    pub causation_id: Option<Uuid>,
}

const PLUGIN_EVENT_TYPE_MAX: usize = 64;

/// POST /api/plugins/{plugin_uuid}/events
pub async fn emit_plugin_event(
    pool: web::Data<Pool>,
    path: web::Path<Uuid>,
    body: web::Json<PluginEventBody>,
    req: HttpRequest,
) -> impl Responder {
    let plugin_uuid = path.into_inner();

    let body = body.into_inner();
    if body.event_type.trim().is_empty() || body.event_type.len() > PLUGIN_EVENT_TYPE_MAX {
        return errors::bad_request("event_type must be 1 to 64 characters");
    }
    if body.aggregate_id.trim().is_empty() {
        return errors::bad_request("aggregate_id is required");
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

    // Plugin must exist before we accept its events. This read
    // happens before we set the workspace GUC; once RLS on `plugins`
    // is the only access layer, the read will be implicitly scoped
    // by the surrounding tx-level GUC (set by the workspace
    // middleware on the outer connection). For now, the runtime DSN
    // still bypasses RLS so the lookup is unscoped here.
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

    // Pull the workspace pin from RequestContext (populated by the
    // auth middleware via WorkspaceContext). Without it the inner
    // emit::record write would land in `sync_actions` with no
    // `app.workspace_id` GUC set, and once Phase 3a-style RLS
    // covers sync_actions any cross-tenant inference would fail.
    // The actor here is `Plugin`-kind (not `User`), so we can't
    // delegate to `TenantConn` which would build a User actor;
    // instead we copy the workspace_id off the RequestContext's
    // actor and pair it with the plugin actor.
    let workspace_id = req
        .extensions()
        .get::<RequestContext>()
        .and_then(|ctx| ctx.actor.workspace_id);

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

    let groups = body.groups.unwrap_or_else(groups::workspace);
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
