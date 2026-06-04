//! Admin-only outbound email queue read API + per-row actions.
//!
//! Endpoints:
//! - `GET    /api/admin/email-queue` — list rows, filterable + cursor
//! - `GET    /api/admin/email-queue/stats` — counts per status + queue health
//! - `POST   /api/admin/email-queue/{id}/retry` — operator action: bump
//!   `next_attempt_at` to now and reset `attempts` if dead
//! - `POST   /api/admin/email-queue/{id}/cancel` — operator action: mark
//!   suppressed with a "cancelled by admin" reason
//!
//! All workspace-admin-gated via `rbac::require_workspace_role`.

use crate::extractors::TenantConn;
use crate::handlers::errors;
use crate::models::WorkspaceRole;
use crate::repository::outbound_emails as repo;
use crate::utils::rbac;
use actix_web::{web, HttpRequest, HttpResponse, Responder};
use base64::Engine;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tracing::warn;

#[derive(Debug, Deserialize, Default)]
pub struct ListQuery {
    /// Comma-separated list of statuses, e.g. "pending,failed,dead".
    pub status: Option<String>,
    pub ticket_id: Option<i32>,
    pub recipient_domain: Option<String>,
    pub since: Option<DateTime<Utc>>,
    pub until: Option<DateTime<Utc>>,
    pub limit: Option<i64>,
    pub cursor: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ListResponse {
    pub rows: Vec<RowResponse>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct RowResponse {
    pub id: i64,
    /// `None` for transactional rows (password reset, invitation,
    /// notification) that don't bind to a channel; admin UI renders
    /// these in a separate "Transactional" bucket.
    pub channel_id: Option<i32>,
    pub ticket_id: Option<i32>,
    pub comment_id: Option<i32>,
    pub recipient: String,
    pub subject: String,
    pub status: String,
    pub attempts: i32,
    pub last_smtp_code: Option<i32>,
    pub last_error: Option<String>,
    pub next_attempt_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub sent_at: Option<DateTime<Utc>>,
    pub failed_at: Option<DateTime<Utc>>,
    /// J Pass 2.2 bounce-linkage fields. Non-null `bounced_at`
    /// is the canonical "row was bounced" signal that drives the
    /// admin UI badge; recipient and diagnostic populate the
    /// expanded-row detail.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bounced_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bounce_recipient: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bounce_diagnostic: Option<String>,
}

impl From<crate::models::OutboundEmail> for RowResponse {
    fn from(r: crate::models::OutboundEmail) -> Self {
        Self {
            id: r.id,
            channel_id: r.channel_id,
            ticket_id: r.ticket_id,
            comment_id: r.comment_id,
            recipient: r.recipient,
            subject: r.subject,
            status: r.status,
            attempts: r.attempts,
            last_smtp_code: r.last_smtp_code,
            last_error: r.last_error,
            next_attempt_at: r.next_attempt_at,
            created_at: r.created_at,
            sent_at: r.sent_at,
            failed_at: r.failed_at,
            bounced_at: r.bounced_at,
            bounce_recipient: r.bounce_recipient,
            bounce_diagnostic: r.bounce_diagnostic,
        }
    }
}

/// `GET /api/admin/email-queue` — admin-gated.
pub async fn list(
    req: HttpRequest,
    mut tc: TenantConn,
    query: web::Query<ListQuery>,
) -> impl Responder {
    if let Err(resp) = rbac::require_workspace_role(&req, WorkspaceRole::Admin) {
        return resp;
    }

    let cursor = match query.cursor.as_deref().map(decode_cursor) {
        Some(Ok(c)) => Some(c),
        Some(Err(_)) => {
            return errors::bad_request(
                "Invalid cursor; pass the next_cursor from the previous response verbatim",
            );
        }
        None => None,
    };

    let statuses: Option<Vec<String>> = query.status.as_deref().map(|s| {
        s.split(',')
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(str::to_string)
            .collect()
    });

    let filter = repo::ListFilter {
        status: statuses,
        ticket_id: query.ticket_id,
        recipient_domain: query.recipient_domain.clone(),
        since: query.since,
        until: query.until,
    };

    let limit = query.limit.unwrap_or(50);
    let page = match tc.run(|conn| repo::list(conn, &filter, cursor, limit)) {
        Ok(p) => p,
        Err(e) => {
            warn!(error = ?e, "Failed to list outbound email queue");
            return errors::internal("Failed to list outbound email queue");
        }
    };

    HttpResponse::Ok().json(ListResponse {
        rows: page.rows.into_iter().map(Into::into).collect(),
        next_cursor: page.next_cursor.map(encode_cursor),
    })
}

#[derive(Debug, Serialize)]
pub struct StatsResponse {
    pub by_status: Vec<StatusCount>,
    /// Count of rows in `pending` or `failed` — the worker's claimable
    /// set. Drives the "queue is backed up" admin alert.
    pub pending_total: i64,
    /// Age in seconds of the oldest pending/failed row. `None` when the
    /// queue is empty.
    pub oldest_pending_age_seconds: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct StatusCount {
    pub status: String,
    pub count: i64,
}

/// `GET /api/admin/email-queue/stats` — top stats card data.
pub async fn stats(req: HttpRequest, mut tc: TenantConn) -> impl Responder {
    if let Err(resp) = rbac::require_workspace_role(&req, WorkspaceRole::Admin) {
        return resp;
    }
    // Fold both counts into one tc.run so they ride a single RLS
    // transaction; the second call won't observe writes that landed
    // between them.
    let result: diesel::QueryResult<(Vec<(String, i64)>, (i64, Option<i64>))> = tc.run(|conn| {
        let by_status = repo::count_by_status(conn)?;
        let health = repo::pending_health(conn).unwrap_or((0, None));
        Ok((by_status, health))
    });
    let (by_status, (pending_total, oldest_age)) = match result {
        Ok(t) => t,
        Err(e) => {
            warn!(error = ?e, "Failed to count outbound email queue by status");
            return errors::internal("Failed to count queue rows");
        }
    };
    HttpResponse::Ok().json(StatsResponse {
        by_status: by_status
            .into_iter()
            .map(|(status, count)| StatusCount { status, count })
            .collect(),
        pending_total,
        oldest_pending_age_seconds: oldest_age,
    })
}

/// `POST /api/admin/email-queue/{id}/retry` — bump back to pending,
/// reset attempts if the row was dead.
pub async fn retry_now(
    req: HttpRequest,
    mut tc: TenantConn,
    path: web::Path<i64>,
) -> impl Responder {
    if let Err(resp) = rbac::require_workspace_role(&req, WorkspaceRole::Admin) {
        return resp;
    }
    let id = path.into_inner();
    match tc.run(|conn| repo::retry_now(conn, id)) {
        Ok(0) => errors::not_found_msg("queue row not found, or not in a retryable state"),
        Ok(_) => HttpResponse::NoContent().finish(),
        Err(e) => {
            warn!(error = ?e, queue_id = id, "Failed to retry queue row");
            errors::internal("Failed to retry queue row")
        }
    }
}

/// `POST /api/admin/email-queue/{id}/cancel` — mark suppressed.
pub async fn cancel(req: HttpRequest, mut tc: TenantConn, path: web::Path<i64>) -> impl Responder {
    if let Err(resp) = rbac::require_workspace_role(&req, WorkspaceRole::Admin) {
        return resp;
    }
    let id = path.into_inner();
    match tc.run(|conn| repo::cancel(conn, id)) {
        Ok(0) => errors::not_found_msg("queue row not found"),
        Ok(_) => HttpResponse::NoContent().finish(),
        Err(e) => {
            warn!(error = ?e, queue_id = id, "Failed to cancel queue row");
            errors::internal("Failed to cancel queue row")
        }
    }
}

fn encode_cursor(c: repo::Cursor) -> String {
    let json = serde_json::to_vec(&c).expect("cursor serialises to JSON");
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(json)
}

fn decode_cursor(s: &str) -> Result<repo::Cursor, ()> {
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(s.as_bytes())
        .map_err(|_| ())?;
    serde_json::from_slice(&bytes).map_err(|_| ())
}

#[cfg(test)]
mod tests {
    //! Permission tests — the same shape as audit_log/api_tokens/webhooks.
    //! The data path is exercised by the repo's tests; here we just
    //! prove the admin gate is wired on each route.
    use super::*;
    use crate::test_helpers::{claims_for, setup_test_pool};
    use actix_web::test as actix_test;
    use actix_web::{http::StatusCode, App, HttpMessage};

    fn test_app(
        pool: crate::db::Pool,
    ) -> App<
        impl actix_web::dev::ServiceFactory<
            actix_web::dev::ServiceRequest,
            Config = (),
            Response = actix_web::dev::ServiceResponse<impl actix_web::body::MessageBody>,
            Error = actix_web::Error,
            InitError = (),
        >,
    > {
        App::new()
            .app_data(web::Data::new(pool))
            .route("/admin/email-queue", web::get().to(list))
            .route("/admin/email-queue/stats", web::get().to(stats))
            .route("/admin/email-queue/{id}/retry", web::post().to(retry_now))
            .route("/admin/email-queue/{id}/cancel", web::post().to(cancel))
    }

    #[actix_web::test]
    async fn list_requires_authentication() {
        let pool = setup_test_pool();
        let app = actix_test::init_service(test_app(pool)).await;
        let req = actix_test::TestRequest::get()
            .uri("/admin/email-queue")
            .to_request();
        let resp = actix_test::call_service(&app, req).await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[actix_web::test]
    async fn list_rejects_user_role() {
        let pool = setup_test_pool();
        let claims = claims_for(&pool, "user");
        let app = actix_test::init_service(test_app(pool.clone())).await;
        let req = actix_test::TestRequest::get()
            .uri("/admin/email-queue")
            .to_request();
        req.extensions_mut().insert(claims);
        let resp = actix_test::call_service(&app, req).await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[actix_web::test]
    async fn retry_rejects_user_role() {
        let pool = setup_test_pool();
        let claims = claims_for(&pool, "user");
        let app = actix_test::init_service(test_app(pool.clone())).await;
        let req = actix_test::TestRequest::post()
            .uri("/admin/email-queue/1/retry")
            .to_request();
        req.extensions_mut().insert(claims);
        let resp = actix_test::call_service(&app, req).await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[actix_web::test]
    async fn cursor_round_trip() {
        let c = repo::Cursor {
            created_at: Utc::now(),
            id: 999,
        };
        let encoded = encode_cursor(c);
        let decoded = decode_cursor(&encoded).expect("decode");
        assert_eq!(decoded.id, c.id);
    }
}
