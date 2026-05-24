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

use crate::extractors::TenantConn;
use crate::handlers::errors;
use crate::models::{email_suppression_reason, EmailSuppression, NewEmailSuppression};
use crate::repository::email_suppressions as repo;
use crate::utils::rbac;

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
    mut tc: TenantConn,
    query: web::Query<ListQuery>,
) -> impl Responder {
    if let Err(resp) = rbac::require_admin(&req) {
        return resp;
    }
    let limit = query.limit.unwrap_or(50).clamp(1, 200);
    let before = query.before;
    // `email_suppressions` is a platform-global table (no
    // `workspace_id`), so the RLS GUC TenantConn primes is a no-op
    // for it. Wrapping it in `tc.run` still gives us the transaction
    // boundary that pairs the list + count counters consistently.
    let result: diesel::QueryResult<(Vec<EmailSuppression>, i64)> = tc.run(|conn| {
        let rows = repo::list(conn, limit, before)?;
        let total = repo::count(conn)?;
        Ok((rows, total))
    });
    let (rows, total) = match result {
        Ok(t) => t,
        Err(e) => {
            warn!(error = ?e, "Failed to read email suppressions");
            return errors::internal("Failed to read email suppressions");
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
    mut tc: TenantConn,
    body: web::Json<CreateBody>,
) -> impl Responder {
    if let Err(resp) = rbac::require_admin(&req) {
        return resp;
    }
    let email = body.email.trim().to_string();
    if email.is_empty() || !email.contains('@') {
        return errors::bad_request("Email must look like an address");
    }
    let new = NewEmailSuppression {
        email,
        reason: email_suppression_reason::MANUAL.to_string(),
        bounce_diagnostic: body.note.clone(),
    };
    match tc.run(|conn| repo::upsert(conn, new)) {
        Ok(row) => HttpResponse::Ok().json(RowResponse::from(row)),
        Err(e) => {
            warn!(error = ?e, "Failed to add email suppression");
            errors::internal("Failed to add email suppression")
        }
    }
}

pub async fn delete(
    req: HttpRequest,
    mut tc: TenantConn,
    path: web::Path<String>,
) -> impl Responder {
    if let Err(resp) = rbac::require_admin(&req) {
        return resp;
    }
    let email = path.into_inner();
    match tc.run(|conn| repo::remove(conn, &email)) {
        Ok(0) => errors::not_found_msg("Address is not on the suppression list"),
        Ok(_) => HttpResponse::NoContent().finish(),
        Err(e) => {
            warn!(error = ?e, "Failed to remove email suppression");
            errors::internal("Failed to remove email suppression")
        }
    }
}
