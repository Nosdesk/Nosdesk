//! Admin handler for the email suppression list (J Pass 2.2b).
//!
//! Three endpoints:
//!   GET    /api/admin/email-suppressions          list + counts
//!   POST   /api/admin/email-suppressions          add manually
//!   DELETE /api/admin/email-suppressions/{email}  remove one
//!
//! Admin-gated via `rbac::require_admin`; no per-tech access (a tech
//! seeing the suppression list could reveal email addresses they
//! shouldn't know about).

use actix_web::{web, HttpRequest, HttpResponse, Responder};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tracing::warn;

use crate::db::Pool;
use crate::handlers::{errors, helpers};
use crate::utils::rbac;
use crate::models::{email_suppression_reason, EmailSuppression, NewEmailSuppression};
use crate::repository::email_suppressions as repo;

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    /// ISO-8601 timestamp; rows older than this are returned. Used
    /// for cursor-style pagination (`before = oldest row in last
    /// page.created_at`).
    pub before: Option<DateTime<Utc>>,
    pub limit: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct ListResponse {
    pub rows: Vec<RowResponse>,
    pub total: i64,
    pub next_cursor: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize)]
pub struct RowResponse {
    pub email: String,
    pub reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bounce_diagnostic: Option<String>,
    pub bounce_count: i32,
    pub created_at: DateTime<Utc>,
    pub last_seen_at: DateTime<Utc>,
}

impl From<EmailSuppression> for RowResponse {
    fn from(s: EmailSuppression) -> Self {
        Self {
            email: s.email,
            reason: s.reason,
            bounce_diagnostic: s.bounce_diagnostic,
            bounce_count: s.bounce_count,
            created_at: s.created_at,
            last_seen_at: s.last_seen_at,
        }
    }
}

pub async fn list(
    req: HttpRequest,
    db_pool: web::Data<Pool>,
    query: web::Query<ListQuery>,
) -> impl Responder {
    if let Err(resp) = rbac::require_admin(&req) {
        return resp;
    }
    let mut conn = match helpers::db_conn(&db_pool) {
        Ok(c) => c,
        Err(e) => return e,
    };
    let limit = query.limit.unwrap_or(50).clamp(1, 200);
    let rows = match repo::list(&mut conn, limit, query.before) {
        Ok(r) => r,
        Err(e) => {
            warn!(error = ?e, "Failed to list email suppressions");
            return errors::internal("Failed to list email suppressions");
        }
    };
    let total = match repo::count(&mut conn) {
        Ok(n) => n,
        Err(e) => {
            warn!(error = ?e, "Failed to count email suppressions");
            return errors::internal("Failed to count email suppressions");
        }
    };
    // The next cursor is the created_at of the last row returned;
    // the client passes this as `before` to fetch the next page.
    // `None` when we returned fewer rows than the limit (last page).
    let next_cursor = if rows.len() as i64 == limit {
        rows.last().map(|r| r.created_at)
    } else {
        None
    };
    HttpResponse::Ok().json(ListResponse {
        rows: rows.into_iter().map(Into::into).collect(),
        total,
        next_cursor,
    })
}

#[derive(Debug, Deserialize)]
pub struct CreateBody {
    pub email: String,
    /// Optional admin note explaining why. Falls through to the
    /// `metadata` JSONB on the suppression row for audit.
    pub note: Option<String>,
}

pub async fn create(
    req: HttpRequest,
    db_pool: web::Data<Pool>,
    body: web::Json<CreateBody>,
) -> impl Responder {
    if let Err(resp) = rbac::require_admin(&req) {
        return resp;
    }
    let email = body.email.trim().to_string();
    if email.is_empty() || !email.contains('@') {
        return errors::bad_request("Email must look like an address");
    }
    let mut conn = match helpers::db_conn(&db_pool) {
        Ok(c) => c,
        Err(e) => return e,
    };
    let new = NewEmailSuppression {
        email,
        reason: email_suppression_reason::MANUAL.to_string(),
        bounce_diagnostic: body.note.clone(),
    };
    match repo::upsert(&mut conn, new) {
        Ok(row) => HttpResponse::Ok().json(RowResponse::from(row)),
        Err(e) => {
            warn!(error = ?e, "Failed to add email suppression");
            errors::internal("Failed to add email suppression")
        }
    }
}

pub async fn delete(
    req: HttpRequest,
    db_pool: web::Data<Pool>,
    path: web::Path<String>,
) -> impl Responder {
    if let Err(resp) = rbac::require_admin(&req) {
        return resp;
    }
    let email = path.into_inner();
    let mut conn = match helpers::db_conn(&db_pool) {
        Ok(c) => c,
        Err(e) => return e,
    };
    match repo::remove(&mut conn, &email) {
        Ok(0) => errors::not_found_msg("Address is not on the suppression list"),
        Ok(_) => HttpResponse::NoContent().finish(),
        Err(e) => {
            warn!(error = ?e, "Failed to remove email suppression");
            errors::internal("Failed to remove email suppression")
        }
    }
}
