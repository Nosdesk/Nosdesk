//! `/api/notices`: staff post known-issue notices (agents and up, since the
//! person handling an outage is usually an agent). The portal and guest pages
//! show the live one; see [`public_notice`].

use actix_web::{web, HttpRequest, HttpResponse};
use chrono::{Duration, Utc};
use serde::Serialize;
use serde_json::json;

use crate::errors::ApiError;
use crate::extractors::{AuthContext, TenantConn};
use crate::models::{NoticeFields, WorkspaceNotice, WorkspaceRole};
use crate::repository::workspace_notices;
use crate::utils::rbac::require_workspace_role;

/// Longest a notice may run. An outage notice that outlives the outage is
/// worse than none, so an end time is always required and bounded.
const MAX_NOTICE_DAYS: i64 = 30;

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.route("/notices", web::get().to(list_notices))
        .route("/notices", web::post().to(create_notice))
        .route("/notices/{id}", web::put().to(update_notice))
        .route("/notices/{id}/end", web::post().to(end_notice));
}

/// A notice as requesters see it (no author, no internal ids beyond the
/// incident link's presence).
#[derive(Debug, Serialize)]
pub struct PublicNotice {
    pub id: i32,
    pub title: String,
    pub body: Option<String>,
    pub severity: String,
    /// Changes when the notice is edited, so a dismissed notice comes back.
    pub updated_at: chrono::DateTime<Utc>,
    /// Whether signed-in requesters can follow the issue.
    pub followable: bool,
}

impl From<WorkspaceNotice> for PublicNotice {
    fn from(n: WorkspaceNotice) -> Self {
        Self {
            id: n.id,
            title: n.title,
            body: n.body,
            severity: n.severity,
            updated_at: n.updated_at,
            followable: n.incident_ticket_id.is_some(),
        }
    }
}

/// The live notice for the workspace the connection is pinned to, as
/// requesters see it.
pub fn public_notice(
    conn: &mut crate::db::DbConnection,
) -> diesel::QueryResult<Option<PublicNotice>> {
    Ok(workspace_notices::active(conn, Utc::now())?.map(PublicNotice::from))
}

fn validate(fields: &mut NoticeFields) -> Result<(), ApiError> {
    fields.title = fields.title.trim().to_string();
    fields.body = fields
        .body
        .take()
        .map(|b| b.trim().to_string())
        .filter(|b| !b.is_empty());
    if fields.title.is_empty() || fields.title.chars().count() > 120 {
        return Err(ApiError::BadRequest(
            "Give the notice a title of up to 120 characters".into(),
        ));
    }
    if fields
        .body
        .as_ref()
        .is_some_and(|b| b.chars().count() > 1000)
    {
        return Err(ApiError::BadRequest(
            "Keep the notice to 1000 characters".into(),
        ));
    }
    if !matches!(fields.severity.as_str(), "info" | "degraded" | "outage") {
        return Err(ApiError::BadRequest("Unknown severity".into()));
    }
    if fields.ends_at <= fields.starts_at {
        return Err(ApiError::BadRequest(
            "The notice has to end after it starts".into(),
        ));
    }
    if fields.ends_at - fields.starts_at > Duration::days(MAX_NOTICE_DAYS) {
        return Err(ApiError::BadRequest(format!(
            "A notice can run for at most {MAX_NOTICE_DAYS} days"
        )));
    }
    Ok(())
}

/// The incident ticket must exist in this workspace.
fn check_incident(
    conn: &mut crate::db::DbConnection,
    fields: &NoticeFields,
) -> Result<(), diesel::result::Error> {
    if let Some(id) = fields.incident_ticket_id {
        crate::repository::tickets::get_ticket_by_id(conn, id)?;
    }
    Ok(())
}

fn internal(e: diesel::result::Error, what: &str) -> ApiError {
    match e {
        diesel::result::Error::NotFound => ApiError::NotFoundMsg("Not found".into()),
        e => {
            tracing::error!(error = ?e, "notices: {what} failed");
            ApiError::Internal(format!("Failed to {what}"))
        }
    }
}

pub async fn list_notices(mut tc: TenantConn, req: HttpRequest) -> Result<HttpResponse, ApiError> {
    require_workspace_role(&req, WorkspaceRole::Agent)?;
    let notices = tc
        .run(workspace_notices::list)
        .map_err(|e| internal(e, "load notices"))?;
    Ok(HttpResponse::Ok().json(json!({ "notices": notices })))
}

pub async fn create_notice(
    mut tc: TenantConn,
    req: HttpRequest,
    auth: AuthContext,
    body: web::Json<NoticeFields>,
) -> Result<HttpResponse, ApiError> {
    require_workspace_role(&req, WorkspaceRole::Agent)?;
    let mut fields = body.into_inner();
    validate(&mut fields)?;
    let author = auth.user_uuid;
    let notice = tc
        .run(|conn| {
            check_incident(conn, &fields)?;
            workspace_notices::create(conn, &fields, author)
        })
        .map_err(|e| internal(e, "post the notice"))?;
    Ok(HttpResponse::Created().json(notice))
}

pub async fn update_notice(
    mut tc: TenantConn,
    req: HttpRequest,
    path: web::Path<i32>,
    body: web::Json<NoticeFields>,
) -> Result<HttpResponse, ApiError> {
    require_workspace_role(&req, WorkspaceRole::Agent)?;
    let id = path.into_inner();
    let mut fields = body.into_inner();
    validate(&mut fields)?;
    let notice = tc
        .run(|conn| {
            check_incident(conn, &fields)?;
            workspace_notices::update(conn, id, &fields)
        })
        .map_err(|e| internal(e, "update the notice"))?;
    Ok(HttpResponse::Ok().json(notice))
}

/// End a notice now (it stops showing at once).
pub async fn end_notice(
    mut tc: TenantConn,
    req: HttpRequest,
    path: web::Path<i32>,
) -> Result<HttpResponse, ApiError> {
    require_workspace_role(&req, WorkspaceRole::Agent)?;
    let id = path.into_inner();
    let notice = tc
        .run(|conn| workspace_notices::end_now(conn, id))
        .map_err(|e| internal(e, "end the notice"))?;
    Ok(HttpResponse::Ok().json(notice))
}
