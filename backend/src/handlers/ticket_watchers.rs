//! Ticket watch / unwatch + watcher list handlers.
//!
//! Every ticket-scoped handler takes `TicketAccess` instead of a
//! raw `web::Path<i32>` + `AuthContext`. The extractor runs the
//! visibility gate during request extraction, so the handler
//! body is only reachable for callers who can already read the
//! ticket. That makes the "user self-adds as watcher to escalate
//! visibility" path impossible by construction — there's nowhere
//! in the source where a handler can take a ticket id without
//! the gate having run first.

use actix_web::{web, HttpResponse, Responder};
use serde::Deserialize;
use tracing::error;

use crate::extractors::{AuthContext, TenantConn, TicketAccess};
use crate::handlers::errors;
use crate::repository::ticket_watchers as repo;

/// Watcher uuids for a ticket. Returned as a flat list of uuid
/// strings so the frontend can do a single fetch and resolve
/// each uuid through the directory composable.
pub async fn list_watchers(mut tc: TenantConn, access: TicketAccess) -> impl Responder {
    let TicketAccess { ticket_id, .. } = access;
    match tc.run(|conn| repo::watcher_uuids(conn, ticket_id)) {
        Ok(uuids) => HttpResponse::Ok().json(serde_json::json!({ "watcher_uuids": uuids })),
        Err(e) => {
            error!(error = %e, ticket_id, "list_watchers failed");
            errors::internal("Failed to load watchers")
        }
    }
}

pub async fn watch_ticket(mut tc: TenantConn, access: TicketAccess) -> impl Responder {
    let TicketAccess { ticket_id, auth } = access;
    match tc.run(|conn| repo::add_watcher(conn, ticket_id, auth.user_uuid, false)) {
        Ok(added) => HttpResponse::Ok().json(serde_json::json!({
            "watching": true,
            "added": added,
        })),
        Err(e) => {
            error!(error = %e, ticket_id, "watch_ticket failed");
            errors::internal("Failed to watch ticket")
        }
    }
}

pub async fn unwatch_ticket(
    mut tc: TenantConn,
    params: web::Path<i32>,
    auth: AuthContext,
) -> impl Responder {
    // Unwatch doesn't need a visibility gate: the user is removing
    // themselves from the watcher list, which only narrows their
    // own access. Continuing to take `web::Path<i32>` here is
    // intentional — escalation isn't a risk for this direction.
    let ticket_id = params.into_inner();
    match tc.run(|conn| repo::remove_watcher(conn, ticket_id, &auth.user_uuid)) {
        Ok(removed) => HttpResponse::Ok().json(serde_json::json!({
            "watching": false,
            "removed": removed,
        })),
        Err(e) => {
            error!(error = %e, ticket_id, "unwatch_ticket failed");
            errors::internal("Failed to unwatch ticket")
        }
    }
}

/// Authenticated user's own watch state for a ticket. Used by the
/// sidebar to render the visibility toggle in the right position
/// (only on the row matching `auth.user_uuid`) and pre-seed the
/// toggle's value. Returns `watching: false` with default prefs
/// when the user isn't watching.
pub async fn my_watch_state(mut tc: TenantConn, access: TicketAccess) -> impl Responder {
    let TicketAccess { ticket_id, auth } = access;
    match tc.run(|conn| repo::get_watch(conn, ticket_id, &auth.user_uuid)) {
        Ok(Some(w)) => HttpResponse::Ok().json(serde_json::json!({
            "watching": true,
            "notify_on_internal_notes": w.notify_on_internal_notes,
            "auto_added": w.auto_added,
        })),
        Ok(None) => HttpResponse::Ok().json(serde_json::json!({
            "watching": false,
            "notify_on_internal_notes": true,
            "auto_added": false,
        })),
        Err(e) => {
            error!(error = %e, ticket_id, "my_watch_state failed");
            errors::internal("Failed to load watch state")
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct WatchPreferencesBody {
    pub notify_on_internal_notes: Option<bool>,
}

/// Update the authenticated user's per-watch preferences. Today
/// the only setting is `notify_on_internal_notes`; the struct is
/// already shaped to accept further flags without an API change.
pub async fn update_my_watch_preferences(
    mut tc: TenantConn,
    access: TicketAccess,
    body: web::Json<WatchPreferencesBody>,
) -> impl Responder {
    let TicketAccess { ticket_id, auth } = access;
    let Some(notify) = body.notify_on_internal_notes else {
        return errors::bad_request("notify_on_internal_notes is required");
    };
    match tc.run(|conn| repo::set_notify_on_internal_notes(conn, ticket_id, &auth.user_uuid, notify))
    {
        Ok(true) => HttpResponse::Ok().json(serde_json::json!({
            "notify_on_internal_notes": notify,
        })),
        Ok(false) => errors::not_found_msg("You aren't watching this ticket"),
        Err(e) => {
            error!(error = %e, ticket_id, "update_my_watch_preferences failed");
            errors::internal("Failed to update watch preferences")
        }
    }
}
