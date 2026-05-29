//! Canned-response CRUD. Reads open to any authenticated user so the
//! ticket composer's picker works for all techs; writes restricted
//! to admins so the template library has a single source of truth.

use actix_web::{web, HttpMessage, HttpRequest, HttpResponse};
use once_cell::sync::Lazy;
use regex::Regex;
use serde::Deserialize;
use std::collections::BTreeSet;
use tracing::{error, info};

use crate::extractors::TenantConn;
use crate::handlers::errors;
use crate::models::{CannedResponse, CannedResponseUpdate, NewCannedResponse};
use crate::repository::canned_responses as repo;
use crate::utils::rbac::require_admin;

/// Variables the renderer (frontend `cannedResponsesService.renderTemplate`)
/// substitutes at insert time. Keep in sync with that file's table of
/// supported tokens; the validation below rejects any `{{...}}` in a
/// template body that isn't on this list, so an admin typo like
/// `{{custmer_name}}` fails on save rather than landing in a customer
/// reply verbatim.
const ALLOWED_VARIABLES: &[&str] = &[
    "ticket_id",
    "ticket_title",
    "customer_name",
    "tech_name",
    "app_name",
];

static VARIABLE_TOKEN_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\{\{\s*([A-Za-z_][A-Za-z0-9_]*)\s*\}\}").expect("valid regex"));

/// Return the sorted set of unknown `{{token}}` names in `body`.
/// Empty when every token is recognised. Captures dedup automatically
/// via the BTreeSet so the error message lists each typo once.
fn unknown_variables(body: &str) -> Vec<String> {
    let allowed: BTreeSet<&str> = ALLOWED_VARIABLES.iter().copied().collect();
    let mut out: BTreeSet<String> = BTreeSet::new();
    for cap in VARIABLE_TOKEN_RE.captures_iter(body) {
        let name = &cap[1];
        if !allowed.contains(name) {
            out.insert(name.to_string());
        }
    }
    out.into_iter().collect()
}

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
    let unknown = unknown_variables(content);
    if !unknown.is_empty() {
        return bad_request(format!(
            "Unknown template variables: {}. Supported: {}.",
            unknown.join(", "),
            ALLOWED_VARIABLES.join(", ")
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
        let unknown = unknown_variables(trimmed);
        if !unknown.is_empty() {
            return bad_request(format!(
                "Unknown template variables: {}. Supported: {}.",
                unknown.join(", "),
                ALLOWED_VARIABLES.join(", ")
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_variables_returns_empty_for_clean_body() {
        let body =
            "Hi {{customer_name}}, your ticket {{ticket_id}} is being looked at by {{tech_name}}.";
        assert!(unknown_variables(body).is_empty());
    }

    #[test]
    fn unknown_variables_flags_typos() {
        let body = "Hi {{custmer_name}}, ticket {{ticket_id}} from {{app_naem}}.";
        let mut flagged = unknown_variables(body);
        flagged.sort();
        assert_eq!(flagged, vec!["app_naem", "custmer_name"]);
    }

    #[test]
    fn unknown_variables_dedups_repeated_unknowns() {
        let body = "{{foo}} {{foo}} {{foo}}";
        assert_eq!(unknown_variables(body), vec!["foo"]);
    }

    #[test]
    fn unknown_variables_ignores_plain_braces() {
        let body = "Use { single } braces or {{ticket_id}} tokens — but not {{nope}}.";
        assert_eq!(unknown_variables(body), vec!["nope"]);
    }

    #[test]
    fn unknown_variables_tolerates_whitespace_inside_braces() {
        // The frontend renderer matches `\{\{\s*name\s*\}\}` too, so
        // a body with `{{ ticket_id }}` rendered fine. Validation must
        // not reject it.
        let body = "Ticket #{{ ticket_id }} opened.";
        assert!(unknown_variables(body).is_empty());
    }
}
