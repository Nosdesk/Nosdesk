//! Search API handlers

use actix_web::{web, HttpMessage, HttpRequest, HttpResponse, Responder};
use serde_json::json;
use std::sync::Arc;
use tracing::{debug, error, info, warn};

use crate::models::Claims;
use crate::services::search::{SearchQuery, SearchService};

/// Search across all indexed entities
///
/// GET /api/search?q=<query>&limit=20&types=ticket,documentation
pub async fn search(
    query: web::Query<SearchQuery>,
    search_service: web::Data<Arc<SearchService>>,
    req: HttpRequest,
) -> impl Responder {
    // Verify authentication
    let claims = match req.extensions().get::<Claims>() {
        Some(claims) => claims.clone(),
        None => return HttpResponse::Unauthorized().json(json!({"error": "Authentication required"})),
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
        return HttpResponse::BadRequest().json(json!({
            "error": "Search query cannot be empty"
        }));
    }

    if query_str.len() > 500 {
        return HttpResponse::BadRequest().json(json!({
            "error": "Search query too long (max 500 characters)"
        }));
    }

    // Execute search
    match search_service.search(&query.into_inner()) {
        Ok(response) => {
            debug!(
                query = %response.query,
                results = response.results.len(),
                total = response.total,
                took_ms = response.took_ms,
                "Search completed"
            );
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
        None => return HttpResponse::Unauthorized().json(json!({"error": "Authentication required"})),
    };

    // Check if user is admin
    if claims.role != "admin" {
        warn!(user = %claims.sub, "Non-admin user attempted to rebuild search index");
        return HttpResponse::Forbidden().json(json!({
            "error": "Admin access required"
        }));
    }

    // Check if already rebuilding
    if search_service.is_rebuilding() {
        return HttpResponse::Conflict().json(json!({
            "error": "Index rebuild already in progress"
        }));
    }

    info!(user = %claims.sub, "Starting search index rebuild");

    // Get database connection
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(e) => {
            error!(error = ?e, "Database connection error");
            return HttpResponse::InternalServerError().json(json!({
                "error": "Database connection error"
            }));
        }
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
        None => return HttpResponse::Unauthorized().json(json!({"error": "Authentication required"})),
    };

    // Check if user is admin
    if claims.role != "admin" {
        return HttpResponse::Forbidden().json(json!({
            "error": "Admin access required"
        }));
    }

    match search_service.get_stats() {
        Ok(stats) => HttpResponse::Ok().json(stats),
        Err(e) => {
            error!(error = ?e, "Failed to get index stats");
            HttpResponse::InternalServerError().json(json!({
                "error": "Failed to get index statistics"
            }))
        }
    }
}
