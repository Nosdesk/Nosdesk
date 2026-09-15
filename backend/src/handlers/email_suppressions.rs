//! Admin handler for the email suppression list (J Pass 2.2b).
//!
//! Three endpoints:
//!   GET    /api/admin/email-suppressions          list + counts
//!   POST   /api/admin/email-suppressions          add manually
//!   DELETE /api/admin/email-suppressions/{email}  remove one
//!
//! Workspace-admin-gated via `rbac::require_workspace_role`; no
//! per-agent access (an agent seeing the suppression list could
//! reveal email addresses they shouldn't know about). The same reasoning
//! scopes the list per workspace: a peer tenant's admin knows those addresses
//! less than an agent does, and deleting a peer's manual entry would resume
//! mail to someone who asked them to stop.

use actix_web::{web, HttpRequest, HttpResponse};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tracing::warn;

use crate::errors::ApiError;
use crate::extractors::TenantConn;
use crate::models::{
    email_suppression_reason, EmailSuppression, NewEmailSuppression, WorkspaceRole,
};
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
) -> Result<HttpResponse, ApiError> {
    rbac::require_workspace_role(&req, WorkspaceRole::Admin)?;
    let limit = query.limit.unwrap_or(50).clamp(1, 200);
    let before = query.before;
    let Some(workspace_id) = tc.workspace_id() else {
        return Err(ApiError::Forbidden(
            "A resolved workspace is required".into(),
        ));
    };
    // Both queries filter on the workspace explicitly. The table now carries
    // RLS too, but the filter is what makes the query correct on its face;
    // RLS is the backstop for a caller that forgets to pin.
    let result: diesel::QueryResult<(Vec<EmailSuppression>, i64)> = tc.run(|conn| {
        let rows = repo::list(conn, workspace_id, limit, before)?;
        let total = repo::count(conn, workspace_id)?;
        Ok((rows, total))
    });
    let (rows, total) = match result {
        Ok(t) => t,
        Err(e) => {
            warn!(error = ?e, "Failed to read email suppressions");
            return Err(ApiError::Internal(
                "Failed to read email suppressions".into(),
            ));
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
    Ok(HttpResponse::Ok().json(ListResponse {
        rows: rows.into_iter().map(Into::into).collect(),
        total,
        next_cursor,
    }))
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
) -> Result<HttpResponse, ApiError> {
    rbac::require_workspace_role(&req, WorkspaceRole::Admin)?;
    let email = body.email.trim().to_string();
    if email.is_empty() || !email.contains('@') {
        return Err(ApiError::BadRequest(
            "Email must look like an address".into(),
        ));
    }
    let Some(workspace_id) = tc.workspace_id() else {
        return Err(ApiError::Forbidden(
            "A resolved workspace is required".into(),
        ));
    };
    let new = NewEmailSuppression {
        email,
        reason: email_suppression_reason::MANUAL.to_string(),
        bounce_diagnostic: body.note.clone(),
        workspace_id,
    };
    match tc.run(|conn| repo::upsert(conn, new)) {
        Ok(row) => Ok(HttpResponse::Ok().json(RowResponse::from(row))),
        Err(e) => {
            warn!(error = ?e, "Failed to add email suppression");
            Err(ApiError::Internal("Failed to add email suppression".into()))
        }
    }
}

pub async fn delete(
    req: HttpRequest,
    mut tc: TenantConn,
    path: web::Path<String>,
) -> Result<HttpResponse, ApiError> {
    rbac::require_workspace_role(&req, WorkspaceRole::Admin)?;
    let email = path.into_inner();
    let Some(workspace_id) = tc.workspace_id() else {
        return Err(ApiError::Forbidden(
            "A resolved workspace is required".into(),
        ));
    };
    // A peer tenant's entry is now indistinguishable from an address that was
    // never on the list, which is the right shape: no cross-tenant oracle.
    match tc.run(|conn| repo::remove(conn, workspace_id, &email)) {
        Ok(0) => Err(ApiError::NotFoundMsg(
            "Address is not on the suppression list".into(),
        )),
        Ok(_) => Ok(HttpResponse::NoContent().finish()),
        Err(e) => {
            warn!(error = ?e, "Failed to remove email suppression");
            Err(ApiError::Internal(
                "Failed to remove email suppression".into(),
            ))
        }
    }
}
