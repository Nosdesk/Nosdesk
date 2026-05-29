//! Canned-response CRUD. Reads open to any authenticated user so the
//! ticket composer's picker works for all techs; writes restricted
//! to admins so the template library has a single source of truth.
//!
//! `{{variable}}` validation lives in
//! `utils::template_variables`; the canned-response surface uses
//! the `CANNED_RESPONSE_VARIABLES` allow-list, which includes
//! ticket-scoped tokens (`{{ticket_id}}`, `{{customer_name}}`)
//! that don't apply to signatures. Keep the frontend
//! `cannedResponsesService.renderTemplate` in sync with that list.

use actix_web::{web, HttpMessage, HttpRequest, HttpResponse};
use serde::Deserialize;
use tracing::{error, info};

use crate::extractors::TenantConn;
use crate::handlers::errors;
use crate::models::{CannedResponse, CannedResponseUpdate, NewCannedResponse};
use crate::repository::canned_responses as repo;
use crate::utils::rbac::require_admin;
use crate::utils::template_variables::{unknown_variables, CANNED_RESPONSE_VARIABLES};

/// Body for `POST /api/canned-responses`. Validation is trivial —
/// title + body both required and non-empty — and happens inline.
#[derive(Debug, Deserialize)]
pub struct CreateRequest {
    pub title: String,
    pub body: String,
}

#[derive(Debug, Deserialize, Default)]
pub struct UpdateRequest {
    pub title: Option<String>,
    pub body: Option<String>,
}

// ---------- Small response helpers ----------

fn server_error(msg: &str) -> HttpResponse {
    errors::internal(msg)
}

fn bad_request(msg: impl Into<String>) -> HttpResponse {
    errors::bad_request(msg)
}

// ---------- Routes ----------

/// GET /api/canned-responses — available to any authenticated user so
/// the reply composer can show the picker.
pub async fn list_canned(mut tc: TenantConn, _req: HttpRequest) -> HttpResponse {
    // Auth is enforced upstream by the JWT middleware that populates
    // RequestContext; TenantConn's extractor refuses without it. Read
    // is open to all techs / admins so the reply composer's picker
    // works for any logged-in user.
    match tc.run(|conn| repo::list(conn)) {
        Ok(rows) => HttpResponse::Ok().json(rows),
        Err(e) => {
            error!(error = %e, "failed to list canned_responses");
            server_error("Failed to list canned responses")
        }
    }
}

/// POST /api/admin/canned-responses — admin only.
pub async fn create_canned(
    mut tc: TenantConn,
    body: web::Json<CreateRequest>,
    req: HttpRequest,
) -> HttpResponse {
    if let Err(resp) = require_admin(&req) {
        return resp;
    }
    let creator = req
        .extensions()
        .get::<crate::models::Claims>()
        .and_then(|c| uuid::Uuid::parse_str(&c.sub).ok());

    let title = body.title.trim();
    let content = body.body.trim();
    if title.is_empty() {
        return bad_request("Title is required");
    }
    if content.is_empty() {
        return bad_request("Body is required");
    }
    let unknown = unknown_variables(content, CANNED_RESPONSE_VARIABLES);
    if !unknown.is_empty() {
        return bad_request(format!(
            "Unknown template variables: {}. Supported: {}.",
            unknown.join(", "),
            CANNED_RESPONSE_VARIABLES.join(", ")
        ));
    }

    let new = NewCannedResponse {
        title: title.to_string(),
        body: content.to_string(),
        created_by: creator,
    };

    match tc.run(|conn| repo::create(conn, new)) {
        Ok(created) => {
            info!(id = created.id, "canned response created");
            HttpResponse::Created().json(created)
        }
        Err(e) => {
            error!(error = %e, "failed to create canned_response");
            server_error("Failed to create canned response")
        }
    }
}

/// PATCH /api/admin/canned-responses/{id} — admin only.
pub async fn update_canned(
    mut tc: TenantConn,
    path: web::Path<i32>,
    body: web::Json<UpdateRequest>,
    req: HttpRequest,
) -> HttpResponse {
    if let Err(resp) = require_admin(&req) {
        return resp;
    }
    let id = path.into_inner();

    if let Some(t) = body.title.as_deref() {
        if t.trim().is_empty() {
            return bad_request("Title must not be empty");
        }
    }
    if let Some(b) = body.body.as_deref() {
        let trimmed = b.trim();
        if trimmed.is_empty() {
            return bad_request("Body must not be empty");
        }
        let unknown = unknown_variables(trimmed, CANNED_RESPONSE_VARIABLES);
        if !unknown.is_empty() {
            return bad_request(format!(
                "Unknown template variables: {}. Supported: {}.",
                unknown.join(", "),
                CANNED_RESPONSE_VARIABLES.join(", ")
            ));
        }
    }

    let change = CannedResponseUpdate {
        title: body.title.clone().map(|s| s.trim().to_string()),
        body: body.body.clone().map(|s| s.trim().to_string()),
        ..Default::default()
    };

    match tc.run(|conn| repo::update(conn, id, change)) {
        Ok(updated) => {
            let updated: CannedResponse = updated;
            HttpResponse::Ok().json(updated)
        }
        Err(diesel::result::Error::NotFound) => HttpResponse::NotFound().finish(),
        Err(e) => {
            error!(error = %e, "failed to update canned_response");
            server_error("Failed to update canned response")
        }
    }
}

/// DELETE /api/admin/canned-responses/{id} — admin only.
pub async fn delete_canned(
    mut tc: TenantConn,
    path: web::Path<i32>,
    req: HttpRequest,
) -> HttpResponse {
    if let Err(resp) = require_admin(&req) {
        return resp;
    }
    let id = path.into_inner();
    match tc.run(|conn| repo::delete(conn, id)) {
        Ok(0) => HttpResponse::NotFound().finish(),
        Ok(_) => {
            info!(id, "canned response deleted");
            HttpResponse::NoContent().finish()
        }
        Err(e) => {
            error!(error = %e, "failed to delete canned_response");
            server_error("Failed to delete canned response")
        }
    }
}

