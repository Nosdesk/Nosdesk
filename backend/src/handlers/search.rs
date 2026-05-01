//! Search API handlers

use actix_web::{web, HttpMessage, HttpRequest, HttpResponse, Responder};
use serde_json::json;
use std::sync::Arc;
use tracing::{debug, error, info, warn};

use crate::db::Pool;
use crate::handlers::helpers;
use crate::handlers::errors;
use crate::models::Claims;
use crate::repository::search_query_log;
use crate::services::search::{EntityType, SearchQuery, SearchService};

/// Search across all indexed entities
///
/// GET /api/search?q=<query>&limit=20&types=ticket,documentation
pub async fn search(
    query: web::Query<SearchQuery>,
    search_service: web::Data<Arc<SearchService>>,
    pool: web::Data<Pool>,
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

    match search_service.search(&query) {
        Ok(response) => {
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
                    if let Err(e) = search_query_log::log_query(&mut conn, &logged_query, doc_hits) {
                        warn!(error = ?e, "Search log write failed");
                    }
                });
            }

            HttpResponse::Ok().json(response)
        }
        Err(e) => {
            error!(error = ?e, "Search failed");
            HttpResponse::InternalServerError().json(json!({
                "error": "Search failed",
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

    // Check if user is admin
    if claims.role != "admin" {
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
                    "total": stats.total()
                }
            }))
        }
        Err(e) => {
            error!(error = ?e, "Index rebuild failed");
            HttpResponse::InternalServerError().json(json!({
                "error": "Index rebuild failed",
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

    // Check if user is admin
    if claims.role != "admin" {
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
