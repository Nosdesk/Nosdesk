//! Ticket watch / unwatch + watcher list handlers.
//!
//! Watch / unwatch operate on the AUTHENTICATED user — there's
//! no admin endpoint to add another user as a watcher in V1
//! (we'd want that for power users; defer to V1.1). The
//! authenticated user's uuid comes from the JWT, not the body,
//! so the user can't accidentally watch on someone else's
//! behalf.

use actix_web::{web, HttpResponse, Responder};
use serde::Deserialize;
use tracing::error;

use crate::extractors::AuthContext;
use crate::handlers::{errors, helpers};
use crate::handlers::helpers::with_actor;
use crate::repository::ticket_visibility::{self, VisibilityContext};
use crate::repository::ticket_watchers as repo;
use crate::sync::actor::ActorContext;

/// Watcher uuids for a ticket. Returned as a flat list of uuid
/// strings so the frontend can do a single fetch and resolve
/// each uuid through the directory composable.
pub async fn list_watchers(
    pool: web::Data<crate::db::Pool>,
    params: web::Path<i32>,
    auth: AuthContext,
) -> impl Responder {
    let ticket_id = params.into_inner();
    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };
    let vis = VisibilityContext::from_auth(&auth);
    match ticket_visibility::can_view_ticket(&mut conn, &vis, ticket_id) {
        Ok(true) => {}
        Ok(false) => return errors::not_found_msg("Ticket not found"),
        Err(e) => {
            error!(error = %e, ticket_id, "list_watchers visibility check failed");
            return errors::internal("Failed to load watchers");
        }
    }
    match repo::watcher_uuids(&mut conn, ticket_id) {
        Ok(uuids) => HttpResponse::Ok().json(serde_json::json!({ "watcher_uuids": uuids })),
        Err(e) => {
            error!(error = %e, ticket_id, "list_watchers failed");
            errors::internal("Failed to load watchers")
        }
    }
}

pub async fn watch_ticket(
    pool: web::Data<crate::db::Pool>,
    params: web::Path<i32>,
    auth: AuthContext,
) -> impl Responder {
    let ticket_id = params.into_inner();
    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };
    // Visibility gate: without this, a User could self-add as a
    // watcher on any ticket and gain read access via the visibility
    // predicate (privilege escalation, OWASP A01:2021).
    let vis = VisibilityContext::from_auth(&auth);
    match ticket_visibility::can_view_ticket(&mut conn, &vis, ticket_id) {
        Ok(true) => {}
        Ok(false) => return errors::not_found_msg("Ticket not found"),
        Err(e) => {
            error!(error = %e, ticket_id, "watch_ticket visibility check failed");
            return errors::internal("Failed to watch ticket");
        }
    }
    let actor_ctx = ActorContext::user(auth.user_uuid, None);
    match with_actor(&mut conn, &actor_ctx, |conn| {
        repo::add_watcher(conn, ticket_id, auth.user_uuid, false)
    }) {
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
    pool: web::Data<crate::db::Pool>,
    params: web::Path<i32>,
    auth: AuthContext,
) -> impl Responder {
    let ticket_id = params.into_inner();
    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };
    let actor_ctx = ActorContext::user(auth.user_uuid, None);
    match with_actor(&mut conn, &actor_ctx, |conn| {
        repo::remove_watcher(conn, ticket_id, &auth.user_uuid)
    }) {
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
pub async fn my_watch_state(
    pool: web::Data<crate::db::Pool>,
    params: web::Path<i32>,
    auth: AuthContext,
) -> impl Responder {
    let ticket_id = params.into_inner();
    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };
    let vis = VisibilityContext::from_auth(&auth);
    match ticket_visibility::can_view_ticket(&mut conn, &vis, ticket_id) {
        Ok(true) => {}
        Ok(false) => return errors::not_found_msg("Ticket not found"),
        Err(e) => {
            error!(error = %e, ticket_id, "my_watch_state visibility check failed");
            return errors::internal("Failed to load watch state");
        }
    }
    match repo::get_watch(&mut conn, ticket_id, &auth.user_uuid) {
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
    pool: web::Data<crate::db::Pool>,
    params: web::Path<i32>,
    body: web::Json<WatchPreferencesBody>,
    auth: AuthContext,
) -> impl Responder {
    let ticket_id = params.into_inner();
    let Some(notify) = body.notify_on_internal_notes else {
        return errors::bad_request("notify_on_internal_notes is required");
    };
    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };
    let actor_ctx = ActorContext::user(auth.user_uuid, None);
    match with_actor(&mut conn, &actor_ctx, |conn| {
        repo::set_notify_on_internal_notes(conn, ticket_id, &auth.user_uuid, notify)
    }) {
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
