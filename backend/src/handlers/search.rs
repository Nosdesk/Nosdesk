//! Search API handlers

use actix_web::{web, HttpMessage, HttpRequest, HttpResponse, Responder};
use serde_json::json;
use std::sync::Arc;
use tracing::{debug, error, info, warn};

use crate::db::Pool;
use crate::extractors::AuthContext;
use crate::extractors::WorkspaceContext;
use crate::handlers::errors;
use crate::handlers::helpers;
use crate::models::Claims;
use crate::repository::search_query_log;
use crate::services::search::{EntityType, SearchQuery, SearchService};
use crate::utils::i18n;
use crate::utils::locale::request_locale;
use crate::utils::rbac::is_platform_admin;

/// Search routes, mounted inside the authenticated `/api` scope in main.rs.
pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.route("/search", web::get().to(search))
        .route("/search/rebuild", web::post().to(rebuild_index))
        .route("/search/stats", web::get().to(get_stats));
}

/// Search across all indexed entities
///
/// GET /api/search?q=<query>&limit=20&types=ticket,documentation

pub async fn search(
    query: web::Query<SearchQuery>,
    search_service: web::Data<Arc<SearchService>>,
    pool: web::Data<Pool>,
    auth: AuthContext,
    ws: WorkspaceContext,
    req: HttpRequest,
) -> impl Responder {
    // Verify authentication
    let claims = match req.extensions().get::<Claims>() {
        Some(claims) => claims.clone(),
        None => return errors::unauthorized("Authentication required"),
    };

    debug!(
        user = %claims.sub,
        query = %query.q,
        limit = query.limit,
        types = ?query.types,
        "Search request"
    );

    // Validate query
    let query_str = query.q.trim();
    if query_str.is_empty() {
        return errors::bad_request("Search query cannot be empty");
    }

    if query_str.len() > 500 {
        return errors::bad_request("Search query too long (max 500 characters)");
    }

    let query = query.into_inner();
    // Log only when documentation was in scope. Ticket-only or
    // device-only searches don't carry KB-demand signal.
    let log_doc_search = match query.entity_types() {
        Some(types) => types.iter().any(|t| matches!(t, EntityType::Documentation)),
        None => true, // unscoped search includes docs
    };
    let logged_query = query.q.clone();

    // Internal-note hits are gated by role. Admin and Technician
    // need them for triage / search-as-you-think workflows; end-
    // users must never see them via full-text search. Unknown /
    // future roles default to staff-equivalent here because the
    // migration adds them to the privileged tier; revisit if
    // non-staff roles expand.
    let is_end_user = !auth.can_handle_tickets();
    let include_internal = !is_end_user;

    match search_service.search(&query, include_internal, ws.workspace_id as i64) {
        Ok(mut response) => {
            // AUD-011: end-users must not learn about tickets they
            // can't read via search. Staff bypass this filter (their
            // visibility predicate matches every ticket). Comment
            // hits are filtered by the parent ticket id parsed out
            // of the result URL (`/tickets/{id}`).
            if is_end_user {
                use crate::repository::ticket_visibility::{self, VisibilityContext};

                let vis_opt = Some(VisibilityContext::from_auth(&auth));
                let candidate_ids: Vec<i32> = response
                    .results
                    .iter()
                    .filter_map(|r| match r.entity_type.as_str() {
                        "ticket" => i32::try_from(r.entity_id).ok(),
                        "comment" => r
                            .url
                            .strip_prefix("/tickets/")
                            .and_then(|s| s.parse::<i32>().ok()),
                        _ => None,
                    })
                    .collect();

                if let (Some(vis), false) = (vis_opt, candidate_ids.is_empty()) {
                    let mut conn = match helpers::db_conn(&pool) {
                        Ok(c) => c,
                        Err(e) => return e,
                    };
                    match ticket_visibility::visible_ticket_ids(&mut conn, &vis, &candidate_ids) {
                        Ok(visible) => {
                            response.results.retain(|r| match r.entity_type.as_str() {
                                "ticket" => i32::try_from(r.entity_id)
                                    .map(|id| visible.contains(&id))
                                    .unwrap_or(false),
                                "comment" => r
                                    .url
                                    .strip_prefix("/tickets/")
                                    .and_then(|s| s.parse::<i32>().ok())
                                    .map(|id| visible.contains(&id))
                                    .unwrap_or(false),
                                _ => true,
                            });
                            response.total = response.results.len();
                        }
                        Err(e) => {
                            error!(error = ?e, "search visibility filter failed");
                            return errors::internal("Search failed");
                        }
                    }
                }
            }

            debug!(
                query = %response.query,
                results = response.results.len(),
                total = response.total,
                took_ms = response.took_ms,
                "Search completed"
            );

            if log_doc_search {
                let doc_hits = response
                    .results
                    .iter()
                    .filter(|r| r.entity_type == "documentation")
                    .count() as i32;
                let pool = pool.clone();
                // Off the response path: a slow log write must not
                // delay the search response. Errors are logged but
                // don't propagate.
                actix_web::rt::spawn(async move {
                    let mut conn = match pool.get() {
                        Ok(c) => c,
                        Err(e) => {
                            warn!(error = ?e, "Search log: db pool acquire failed");
                            return;
                        }
                    };
                    // The spawn has no RequestContext so no workspace
                    // pin. search_query_log is RLS-enabled
                    // (Phase 3c.2 sync/audit/system migration).
                    // Elevate to nosdesk_admin for the write.
                    let bypass_actor =
                        crate::sync::actor::ActorContext::system("background:search_query_log");
                    let result = crate::sync::session::with_actor_bypass_context(
                        &mut conn,
                        &bypass_actor,
                        |conn| search_query_log::log_query(conn, &logged_query, doc_hits),
                    );
                    if let Err(e) = result {
                        warn!(error = ?e, "Search log write failed");
                    }
                });
            }

            HttpResponse::Ok().json(response)
        }
        Err(e) => {
            error!(error = ?e, "Search failed");
            HttpResponse::InternalServerError().json(json!({
                "error": i18n::tr(&request_locale(&req), "backend-error-search-failed"),
                "code": "backend-error-search-failed",
                "details": e.to_string()
            }))
        }
    }
}

/// Rebuild the search index (admin only)
///
/// POST /api/search/rebuild
pub async fn rebuild_index(
    pool: web::Data<crate::db::Pool>,
    search_service: web::Data<Arc<SearchService>>,
    req: HttpRequest,
) -> impl Responder {
    // Verify authentication and admin role
    let claims = match req.extensions().get::<Claims>() {
        Some(claims) => claims.clone(),
        None => return errors::unauthorized("Authentication required"),
    };

    if !is_platform_admin(&claims) {
        warn!(user = %claims.sub, "Non-admin user attempted to rebuild search index");
        return errors::forbidden("Admin access required");
    }

    // Check if already rebuilding
    if search_service.is_rebuilding() {
        return errors::conflict("Index rebuild already in progress");
    }

    info!(user = %claims.sub, "Starting search index rebuild");

    // Get database connection
    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    // Rebuild index
    match search_service.rebuild_index(&mut conn) {
        Ok(stats) => {
            info!(
                tickets = stats.tickets,
                comments = stats.comments,
                documentation = stats.documentation,
                attachments = stats.attachments,
                devices = stats.devices,
                users = stats.users,
                projects = stats.projects,
                total = stats.total(),
                "Search index rebuilt"
            );

            // Commit the changes
            if let Err(e) = search_service.commit() {
                warn!(error = ?e, "Failed to commit index changes");
            }

            HttpResponse::Ok().json(json!({
                "success": true,
                "message": "Index rebuilt successfully",
                "stats": {
                    "tickets": stats.tickets,
                    "comments": stats.comments,
                    "documentation": stats.documentation,
                    "attachments": stats.attachments,
                    "devices": stats.devices,
                    "users": stats.users,
                    "projects": stats.projects,
                    "total": stats.total()
                }
            }))
        }
        Err(e) => {
            error!(error = ?e, "Index rebuild failed");
            HttpResponse::InternalServerError().json(json!({
                "error": i18n::tr(&request_locale(&req), "backend-error-search-rebuild-failed"),
                "code": "backend-error-search-rebuild-failed",
                "details": e.to_string()
            }))
        }
    }
}

/// Get search index statistics (admin only)
///
/// GET /api/search/stats
pub async fn get_stats(
    search_service: web::Data<Arc<SearchService>>,
    req: HttpRequest,
) -> impl Responder {
    // Verify authentication and admin role
    let claims = match req.extensions().get::<Claims>() {
        Some(claims) => claims.clone(),
        None => return errors::unauthorized("Authentication required"),
    };

    if !is_platform_admin(&claims) {
        return errors::forbidden("Admin access required");
    }

    match search_service.get_stats() {
        Ok(stats) => HttpResponse::Ok().json(stats),
        Err(e) => {
            error!(error = ?e, "Failed to get index stats");
            errors::internal("Failed to get index statistics")
        }
    }
}
