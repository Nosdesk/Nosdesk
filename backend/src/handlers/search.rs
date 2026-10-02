//! Search API handlers

use actix_web::{web, HttpMessage, HttpRequest, HttpResponse, Responder};
use serde_json::json;
use std::sync::Arc;
use tracing::{debug, error, info, warn};

use crate::db::Pool;
use crate::errors::{self, ApiError};
use crate::extractors::AuthContext;
use crate::extractors::WorkspaceContext;
use crate::handlers::helpers;
use crate::models::Claims;
use crate::repository::search_query_log;
use crate::services::search::{EntityType, SearchQuery, SearchService};
use crate::utils::i18n;
use crate::utils::locale::request_locale;
use crate::utils::rbac::is_platform_admin;

/// The ticket a ticket, comment or attachment hit belongs to. The index
/// links each to `/tickets/{id}`.
fn hit_ticket_id(r: &crate::services::search::types::SearchResult) -> Option<i32> {
    match r.entity_type.as_str() {
        "ticket" => i32::try_from(r.entity_id).ok(),
        "comment" | "attachment" => r.url.strip_prefix("/tickets/").and_then(|s| s.parse().ok()),
        _ => None,
    }
}

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
) -> Result<HttpResponse, ApiError> {
    // Verify authentication
    let claims = match req.extensions().get::<Claims>() {
        Some(claims) => claims.clone(),
        None => return Err(ApiError::Unauthorized("Authentication required".into())),
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
        return Err(ApiError::BadRequest("Search query cannot be empty".into()));
    }

    if query_str.len() > 500 {
        return Err(ApiError::BadRequest(
            "Search query too long (max 500 characters)".into(),
        ));
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

                // Assets, people and projects are staff views.
                response.results.retain(|r| {
                    matches!(
                        r.entity_type.as_str(),
                        "ticket" | "comment" | "attachment" | "documentation"
                    )
                });

                let vis_opt = Some(VisibilityContext::from_auth(&auth));
                let candidate_ids: Vec<i32> =
                    response.results.iter().filter_map(hit_ticket_id).collect();

                if let (Some(vis), false) = (vis_opt, candidate_ids.is_empty()) {
                    let mut conn = helpers::db_conn(&pool)?;
                    // Pin the request's workspace: the visibility query
                    // runs under RLS, and an unpinned connection (the
                    // pool scrubs the GUC on checkout) resolves ZERO
                    // tickets — which silently dropped every ticket and
                    // comment hit for end-users.
                    helpers::pin_workspace(&mut conn, ws.workspace_id);
                    match ticket_visibility::visible_ticket_ids(&mut conn, &vis, &candidate_ids) {
                        Ok(visible) => {
                            response.results.retain(|r| match r.entity_type.as_str() {
                                "ticket" | "comment" | "attachment" => {
                                    hit_ticket_id(r).is_some_and(|id| visible.contains(&id))
                                }
                                // Documentation is filtered below for everyone.
                                "documentation" => true,
                                // Assets, people and projects are staff views.
                                _ => false,
                            });
                            response.total = response.results.len();
                        }
                        Err(e) => {
                            error!(error = ?e, "search visibility filter failed");
                            return Err(ApiError::Internal("Search failed".into()));
                        }
                    }
                }
            }

            // Documentation hits follow page access (group, user and collection
            // visibility), as opening the page does. Workspace admins see all.
            let doc_ids: Vec<i32> = response
                .results
                .iter()
                .filter(|r| r.entity_type == "documentation")
                .filter_map(|r| i32::try_from(r.entity_id).ok())
                .collect();
            if !doc_ids.is_empty() && !auth.is_workspace_admin() {
                let mut conn = helpers::db_conn(&pool)?;
                helpers::pin_workspace(&mut conn, ws.workspace_id);
                let user = auth.user_uuid;
                let mut allowed = std::collections::HashSet::new();
                for id in doc_ids {
                    match crate::repository::documentation::can_user_access_page(
                        &mut conn, id, &user, false,
                    ) {
                        Ok(true) => {
                            allowed.insert(id);
                        }
                        Ok(false) => {}
                        Err(e) => {
                            error!(error = ?e, "search documentation filter failed");
                            return Err(ApiError::Internal("Search failed".into()));
                        }
                    }
                }
                let before = response.results.len();
                response.results.retain(|r| {
                    r.entity_type != "documentation"
                        || i32::try_from(r.entity_id).is_ok_and(|id| allowed.contains(&id))
                });
                response.total = response
                    .total
                    .saturating_sub(before - response.results.len());
            }

            // The index links a hit to its ticket by id; people know the
            // ticket by its number, so the link carries that.
            let hit_tickets: Vec<i32> = response.results.iter().filter_map(hit_ticket_id).collect();
            if !hit_tickets.is_empty() {
                let mut conn = helpers::db_conn(&pool)?;
                helpers::pin_workspace(&mut conn, ws.workspace_id);
                let numbers = crate::repository::tickets::numbers_of(
                    &mut conn,
                    ws.workspace_id,
                    &hit_tickets,
                )
                .map_err(|e| {
                    error!(error = ?e, "search ticket numbers failed");
                    ApiError::Internal("Search failed".into())
                })?;
                for r in &mut response.results {
                    if let Some(number) = hit_ticket_id(r).and_then(|id| numbers.get(&id)) {
                        r.url = format!("/tickets/{number}");
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
                let workspace_id = ws.workspace_id;
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
                    // The spawn has no RequestContext so no ambient
                    // workspace pin. search_query_log is RLS-enabled
                    // (Phase 3c.2 sync/audit/system migration) and its
                    // workspace_id column defaults from the GUC, so the
                    // actor must carry the request's workspace — without
                    // it every write dies on the NOT NULL default.
                    // Elevate to nosdesk_admin for the write itself.
                    let bypass_actor =
                        crate::sync::actor::ActorContext::system("background:search_query_log")
                            .with_workspace(workspace_id);
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

            Ok(HttpResponse::Ok().json(response))
        }
        Err(e) => {
            error!(error = ?e, "Search failed");
            Ok(errors::internal_with_code(
                i18n::tr(&request_locale(&req), "backend-error-search-failed"),
                "backend-error-search-failed",
            ))
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
) -> Result<HttpResponse, ApiError> {
    // Verify authentication and admin role
    let claims = match req.extensions().get::<Claims>() {
        Some(claims) => claims.clone(),
        None => return Err(ApiError::Unauthorized("Authentication required".into())),
    };

    if !is_platform_admin(&claims) {
        warn!(user = %claims.sub, "Non-admin user attempted to rebuild search index");
        return Err(ApiError::Forbidden("Admin access required".into()));
    }

    // Check if already rebuilding
    if search_service.is_rebuilding() {
        return Err(ApiError::Conflict(
            "Index rebuild already in progress".into(),
        ));
    }

    info!(user = %claims.sub, "Starting search index rebuild");

    // The index holds every workspace on this machine, and the content tables
    // are row-secured, so the rebuild reads elevated, as the startup build does.
    // cross-tenant: a full reindex spans every tenant (platform admin only).
    let rebuilt = crate::sync::session::background_run(&pool, "platform:search_rebuild", |conn| {
        search_service
            .rebuild_index(conn)
            .map_err(|e| diesel::result::Error::QueryBuilderError(e.to_string().into()))
    });
    match rebuilt {
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

            Ok(HttpResponse::Ok().json(json!({
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
            })))
        }
        Err(e) => {
            error!(error = ?e, "Index rebuild failed");
            Ok(errors::internal_with_code(
                i18n::tr(&request_locale(&req), "backend-error-search-rebuild-failed"),
                "backend-error-search-rebuild-failed",
            ))
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
