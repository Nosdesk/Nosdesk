//! HTTP handlers for the Knowledge Gaps queue (Phase 2a).
//!
//! Routes (registered in main.rs):
//!   POST   /api/tickets/{id}/flag-as-gap
//!   DELETE /api/tickets/{id}/flag-as-gap
//!   GET    /api/knowledge-gaps
//!   GET    /api/knowledge-gaps/{id}
//!   POST   /api/knowledge-gaps/{id}/dismiss
//!   POST   /api/knowledge-gaps/{id}/resolve
//!
//! Read responses are slim DTOs hydrated with the per-signal
//! source data the UI needs to render evidence rows (ticket title
//! + status for ticket-typed signals). The Phase 3 LLM will read
//! richer data straight from the source rows; the HTTP surface
//! only carries what the queue UI actually shows.

use actix_web::{web, HttpRequest, HttpResponse, Responder};
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use tracing::error;

use crate::db::{DbConnection, Pool};
use crate::handlers::errors;
use crate::handlers::helpers;
use crate::handlers::sse::{SseEvent, SseState};
use crate::models::{KnowledgeGap, KnowledgeGapSignal, UserInfoWithAvatar};
use crate::repository::{self, knowledge_gaps};
use crate::utils::rbac::is_technician_or_admin;

// ---------------------------------------------------------------
// Response DTOs
// ---------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct KnowledgeGapResponse {
    #[serde(flatten)]
    pub gap: KnowledgeGap,
    /// Hydrated signal evidence. Only populated on the detail
    /// endpoint; the list endpoint returns the gap header alone.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signals: Option<Vec<KnowledgeGapSignalResponse>>,
}

#[derive(Debug, Serialize)]
pub struct KnowledgeGapSignalResponse {
    #[serde(flatten)]
    pub signal: KnowledgeGapSignal,
    /// Hydrated source data. Only ticket-typed signals carry a
    /// hydrated source today; other signal types render from
    /// `source_ref` + `payload` directly.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ticket_title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ticket_status: Option<String>,
    /// Hydrated detector user (for manual_flag signals: this is
    /// who flagged the ticket; for AI-suggested signals it stays
    /// None). Lets the queue UI render "Flagged by Kyle" without
    /// chasing a separate user lookup.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detected_by_user: Option<UserInfoWithAvatar>,
}

fn hydrate_signal(
    conn: &mut DbConnection,
    signal: KnowledgeGapSignal,
) -> KnowledgeGapSignalResponse {
    let detected_by_user = signal.detected_by.and_then(|uuid| {
        repository::get_user_by_uuid(&uuid, conn)
            .ok()
            .map(|u| UserInfoWithAvatar {
                uuid: u.uuid,
                name: u.name,
                avatar_url: u.avatar_url,
                avatar_thumb: u.avatar_thumb,
            })
    });

    if signal.source_kind == knowledge_gaps::SOURCE_TICKET {
        if let Ok(ticket_id) = signal.source_ref.parse::<i32>() {
            use crate::schema::tickets;
            let row: Option<(String, i32)> = tickets::table
                .find(ticket_id)
                .select((tickets::title, tickets::workflow_state_id))
                .first(conn)
                .optional()
                .ok()
                .flatten();
            if let Some((title, ws_id)) = row {
                let cat = crate::repository::workflow_states::category_of(conn, ws_id)
                    .ok()
                    .flatten()
                    .unwrap_or(crate::models::WorkflowStateCategory::Backlog);
                return KnowledgeGapSignalResponse {
                    signal,
                    ticket_title: Some(title),
                    ticket_status: Some(cat.legacy_status().to_string()),
                    detected_by_user,
                };
            }
        }
    }
    KnowledgeGapSignalResponse {
        signal,
        ticket_title: None,
        ticket_status: None,
        detected_by_user,
    }
}

// ---------------------------------------------------------------
// POST /api/tickets/{id}/flag-as-gap
// ---------------------------------------------------------------

#[derive(Debug, Deserialize, Default)]
pub struct FlagTicketBody {
    pub reason: Option<String>,
}

pub async fn flag_ticket_as_gap(
    req: HttpRequest,
    pool: web::Data<Pool>,
    sse_state: web::Data<SseState>,
    path: web::Path<i32>,
    body: web::Json<FlagTicketBody>,
) -> impl Responder {
    let (claims, user_uuid, mut conn) = match helpers::auth_conn(&req, &pool) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if !is_technician_or_admin(&claims) {
        return errors::forbidden("Technician or admin role required");
    }
    let ticket_id = path.into_inner();

    // Need the ticket title for the gap headline. Cheap join.
    use crate::schema::tickets;
    let ticket_title: String = match tickets::table
        .find(ticket_id)
        .select(tickets::title)
        .first(&mut conn)
    {
        Ok(t) => t,
        Err(_) => return errors::not_found("Ticket"),
    };

    match knowledge_gaps::flag_ticket(
        &mut conn,
        ticket_id,
        &ticket_title,
        user_uuid,
        body.into_inner().reason,
    ) {
        Ok((gap, _signal, was_created)) => {
            if was_created {
                sse_state
                    .broadcast_event(SseEvent::KnowledgeGapDetected {
                        gap_id: gap.id,
                        signal_type: knowledge_gaps::SIGNAL_MANUAL_FLAG.to_string(),
                        timestamp: chrono::Utc::now(),
                    })
                    .await;
            }
            HttpResponse::Ok().json(KnowledgeGapResponse { gap, signals: None })
        }
        Err(e) => {
            error!(error = ?e, ticket_id, "Failed to flag ticket as gap");
            errors::internal("Failed to flag ticket")
        }
    }
}

pub async fn unflag_ticket_as_gap(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<i32>,
) -> impl Responder {
    let (claims, user_uuid, mut conn) = match helpers::auth_conn(&req, &pool) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if !is_technician_or_admin(&claims) {
        return errors::forbidden("Technician or admin role required");
    }
    let ticket_id = path.into_inner();

    match knowledge_gaps::unflag_ticket(&mut conn, ticket_id, user_uuid) {
        Ok(Some(gap)) => HttpResponse::Ok().json(KnowledgeGapResponse { gap, signals: None }),
        Ok(None) => HttpResponse::NoContent().finish(),
        Err(e) => {
            error!(error = ?e, ticket_id, "Failed to unflag ticket");
            errors::internal("Failed to unflag ticket")
        }
    }
}

// ---------------------------------------------------------------
// GET /api/knowledge-gaps
// ---------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct ListGapsQuery {
    /// Comma-separated list of statuses. Defaults to "open,drafting".
    pub status: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

pub async fn list_knowledge_gaps(
    req: HttpRequest,
    pool: web::Data<Pool>,
    query: web::Query<ListGapsQuery>,
) -> impl Responder {
    let (claims, _user_uuid, mut conn) = match helpers::auth_conn(&req, &pool) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if !is_technician_or_admin(&claims) {
        return errors::forbidden("Technician or admin role required");
    }

    let q = query.into_inner();
    let statuses = q
        .status
        .as_deref()
        .map(|s| {
            s.split(',')
                .map(|t| t.trim().to_string())
                .filter(|t| !t.is_empty())
                .collect()
        })
        .unwrap_or_default();

    match knowledge_gaps::list_gaps(
        &mut conn,
        knowledge_gaps::GapListFilter {
            statuses,
            limit: helpers::clamp_limit(q.limit),
            offset: helpers::clamp_offset(q.offset),
        },
    ) {
        Ok(gaps) => HttpResponse::Ok().json(
            gaps.into_iter()
                .map(|g| KnowledgeGapResponse {
                    gap: g,
                    signals: None,
                })
                .collect::<Vec<_>>(),
        ),
        Err(e) => {
            error!(error = ?e, "Failed to list knowledge gaps");
            errors::internal("Failed to list gaps")
        }
    }
}

// ---------------------------------------------------------------
// GET /api/knowledge-gaps/{id}
// ---------------------------------------------------------------

pub async fn get_knowledge_gap(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<i64>,
) -> impl Responder {
    let (claims, _user_uuid, mut conn) = match helpers::auth_conn(&req, &pool) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if !is_technician_or_admin(&claims) {
        return errors::forbidden("Technician or admin role required");
    }
    let gap_id = path.into_inner();

    let gap = match knowledge_gaps::get_gap(&mut conn, gap_id) {
        Ok(g) => g,
        Err(diesel::result::Error::NotFound) => return errors::not_found("Gap"),
        Err(e) => {
            error!(error = ?e, gap_id, "Failed to load gap");
            return errors::db_error(&e);
        }
    };

    let signals = match knowledge_gaps::list_signals_for_gap(&mut conn, gap.id) {
        Ok(s) => s,
        Err(e) => {
            error!(error = ?e, gap_id, "Failed to load signals");
            return errors::internal("Failed to load signals");
        }
    };

    let hydrated: Vec<KnowledgeGapSignalResponse> = signals
        .into_iter()
        .map(|s| hydrate_signal(&mut conn, s))
        .collect();

    HttpResponse::Ok().json(KnowledgeGapResponse {
        gap,
        signals: Some(hydrated),
    })
}

// ---------------------------------------------------------------
// POST /api/knowledge-gaps/{id}/dismiss
// ---------------------------------------------------------------

pub async fn dismiss_knowledge_gap(
    req: HttpRequest,
    pool: web::Data<Pool>,
    sse_state: web::Data<SseState>,
    path: web::Path<i64>,
) -> impl Responder {
    let (claims, user_uuid, mut conn) = match helpers::auth_conn(&req, &pool) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if !is_technician_or_admin(&claims) {
        return errors::forbidden("Technician or admin role required");
    }
    let gap_id = path.into_inner();

    match knowledge_gaps::dismiss_gap(&mut conn, gap_id, user_uuid) {
        Ok(gap) => {
            sse_state
                .broadcast_event(SseEvent::KnowledgeGapResolved {
                    gap_id: gap.id,
                    resolved_page_id: None,
                    timestamp: chrono::Utc::now(),
                })
                .await;
            HttpResponse::Ok().json(KnowledgeGapResponse { gap, signals: None })
        }
        Err(e) => {
            error!(error = ?e, gap_id, "Failed to dismiss gap");
            errors::internal("Failed to dismiss gap")
        }
    }
}

// ---------------------------------------------------------------
// POST /api/knowledge-gaps/detect-clusters
// ---------------------------------------------------------------

#[derive(Debug, Deserialize, Default)]
pub struct DetectClustersBody {
    /// How far back to look. Defaults to 30 days.
    pub days: Option<i32>,
    /// Minimum cluster size. Defaults to 2 (singletons aren't gaps).
    pub min_size: Option<usize>,
}

#[derive(Debug, serde::Serialize)]
pub struct DetectClustersResponse {
    pub clusters_detected: usize,
    pub gaps_created: usize,
    pub gaps_updated: usize,
}

pub async fn detect_clusters(
    req: HttpRequest,
    pool: web::Data<Pool>,
    sse_state: web::Data<SseState>,
    body: web::Json<DetectClustersBody>,
) -> impl Responder {
    let (claims, user_uuid, mut conn) = match helpers::auth_conn(&req, &pool) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if !is_technician_or_admin(&claims) {
        return errors::forbidden("Technician or admin role required");
    }
    let req_body = body.into_inner();
    let days = req_body.days.unwrap_or(30);
    let min_size = req_body.min_size.unwrap_or(2).max(2);

    match knowledge_gaps::run_cluster_detection(&mut conn, Some(user_uuid), days, min_size) {
        Ok(stats) => {
            for gap_id in &stats.new_gap_ids {
                sse_state
                    .broadcast_event(SseEvent::KnowledgeGapDetected {
                        gap_id: *gap_id,
                        signal_type: knowledge_gaps::SIGNAL_TICKET_CLUSTER.to_string(),
                        timestamp: chrono::Utc::now(),
                    })
                    .await;
            }
            HttpResponse::Ok().json(DetectClustersResponse {
                clusters_detected: stats.clusters_detected,
                gaps_created: stats.gaps_created,
                gaps_updated: stats.gaps_updated,
            })
        }
        Err(e) => {
            error!(error = ?e, "Cluster detection failed");
            errors::internal("Cluster detection failed")
        }
    }
}

// ---------------------------------------------------------------
// POST /api/knowledge-gaps/detect-failed-searches
// ---------------------------------------------------------------

#[derive(Debug, Deserialize, Default)]
pub struct DetectFailedSearchesBody {
    pub days: Option<i32>,
    pub min_count: Option<i64>,
}

pub async fn detect_failed_searches(
    req: HttpRequest,
    pool: web::Data<Pool>,
    sse_state: web::Data<SseState>,
    body: web::Json<DetectFailedSearchesBody>,
) -> impl Responder {
    let (claims, user_uuid, mut conn) = match helpers::auth_conn(&req, &pool) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if !is_technician_or_admin(&claims) {
        return errors::forbidden("Technician or admin role required");
    }
    let req_body = body.into_inner();
    let days = req_body.days.unwrap_or(30);
    let min_count = req_body.min_count.unwrap_or(2).max(1);

    match knowledge_gaps::run_failed_search_detection(&mut conn, Some(user_uuid), days, min_count) {
        Ok(stats) => {
            for gap_id in &stats.new_gap_ids {
                sse_state
                    .broadcast_event(SseEvent::KnowledgeGapDetected {
                        gap_id: *gap_id,
                        signal_type: knowledge_gaps::SIGNAL_FAILED_SEARCH.to_string(),
                        timestamp: chrono::Utc::now(),
                    })
                    .await;
            }
            HttpResponse::Ok().json(DetectClustersResponse {
                clusters_detected: stats.clusters_detected,
                gaps_created: stats.gaps_created,
                gaps_updated: stats.gaps_updated,
            })
        }
        Err(e) => {
            error!(error = ?e, "Failed-search detection failed");
            errors::internal("Failed-search detection failed")
        }
    }
}

// ---------------------------------------------------------------
// POST /api/knowledge-gaps/detect-stale-docs
// ---------------------------------------------------------------

#[derive(Debug, Deserialize, Default)]
pub struct DetectStaleDocsBody {
    /// How recent a 'resolves'-linked ticket must have closed
    /// before it counts as evidence (default 30).
    pub recent_ticket_days: Option<i32>,
    /// Minimum recently-closed tickets per stale page before it
    /// becomes a gap (default 1).
    pub min_recent_tickets: Option<usize>,
}

pub async fn detect_stale_docs(
    req: HttpRequest,
    pool: web::Data<Pool>,
    sse_state: web::Data<SseState>,
    body: web::Json<DetectStaleDocsBody>,
) -> impl Responder {
    let (claims, user_uuid, mut conn) = match helpers::auth_conn(&req, &pool) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if !is_technician_or_admin(&claims) {
        return errors::forbidden("Technician or admin role required");
    }
    let req_body = body.into_inner();
    let recent_ticket_days = req_body.recent_ticket_days.unwrap_or(30);
    let min_recent_tickets = req_body.min_recent_tickets.unwrap_or(1).max(1);

    match knowledge_gaps::run_stale_doc_detection(
        &mut conn,
        Some(user_uuid),
        recent_ticket_days,
        min_recent_tickets,
    ) {
        Ok(stats) => {
            for gap_id in &stats.new_gap_ids {
                sse_state
                    .broadcast_event(SseEvent::KnowledgeGapDetected {
                        gap_id: *gap_id,
                        signal_type: knowledge_gaps::SIGNAL_STALE_DOC.to_string(),
                        timestamp: chrono::Utc::now(),
                    })
                    .await;
            }
            HttpResponse::Ok().json(DetectClustersResponse {
                clusters_detected: stats.clusters_detected,
                gaps_created: stats.gaps_created,
                gaps_updated: stats.gaps_updated,
            })
        }
        Err(e) => {
            error!(error = ?e, "Stale-doc detection failed");
            errors::internal("Stale-doc detection failed")
        }
    }
}

// ---------------------------------------------------------------
// POST /api/knowledge-gaps/{id}/resolve
// ---------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct ResolveGapBody {
    pub page_id: i32,
}

pub async fn resolve_knowledge_gap(
    req: HttpRequest,
    pool: web::Data<Pool>,
    sse_state: web::Data<SseState>,
    path: web::Path<i64>,
    body: web::Json<ResolveGapBody>,
) -> impl Responder {
    let (claims, user_uuid, mut conn) = match helpers::auth_conn(&req, &pool) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if !is_technician_or_admin(&claims) {
        return errors::forbidden("Technician or admin role required");
    }
    let gap_id = path.into_inner();
    let req_body = body.into_inner();

    match knowledge_gaps::resolve_gap(&mut conn, gap_id, req_body.page_id, user_uuid) {
        Ok(gap) => {
            sse_state
                .broadcast_event(SseEvent::KnowledgeGapResolved {
                    gap_id: gap.id,
                    resolved_page_id: Some(req_body.page_id),
                    timestamp: chrono::Utc::now(),
                })
                .await;
            HttpResponse::Ok().json(KnowledgeGapResponse { gap, signals: None })
        }
        Err(e) => {
            error!(error = ?e, gap_id, "Failed to resolve gap");
            errors::internal("Failed to resolve gap")
        }
    }
}
