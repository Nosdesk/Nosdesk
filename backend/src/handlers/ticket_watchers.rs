//! Ticket watch / unwatch + watcher list handlers.
//!
//! Watch / unwatch operate on the AUTHENTICATED user — there's
//! no admin endpoint to add another user as a watcher in V1
//! (we'd want that for power users; defer to V1.1). The
//! authenticated user's uuid comes from the JWT, not the body,
//! so the user can't accidentally watch on someone else's
//! behalf.

use actix_web::{web, HttpResponse, Responder};
use tracing::error;

use crate::extractors::AuthContext;
use crate::handlers::{errors, helpers};
use crate::handlers::helpers::with_actor;
use crate::repository::ticket_watchers as repo;
use crate::sync::actor::ActorContext;

/// Watcher uuids for a ticket. Returned as a flat list of uuid
/// strings so the frontend can do a single fetch and resolve
/// each uuid through the directory composable.
pub async fn list_watchers(
    pool: web::Data<crate::db::Pool>,
    params: web::Path<i32>,
    _auth: AuthContext,
) -> impl Responder {
    let ticket_id = params.into_inner();
    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };
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
