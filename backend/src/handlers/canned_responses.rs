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
use crate::models::{
    CannedResponse, CannedResponseStarter, CannedResponseUpdate, NewCannedResponse,
    NewCannedResponseInsertion, WorkspaceRole,
};
use crate::repository::canned_responses as repo;
use crate::utils::rbac::require_workspace_role;
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

/// Body for `POST /api/canned-responses/{id}/insertions`. The
/// `ticket_id` is informational so the admin page can correlate
/// which templates are inserted on which tickets; the user comes
/// from the JWT claims (we don't trust caller-supplied user ids
/// on a usage-log endpoint). Both fields are optional because the
/// picker may eventually open in contexts without a ticket bound
/// (preview surfaces, draft composer).
#[derive(Debug, Deserialize, Default)]
pub struct RecordInsertionRequest {
    pub ticket_id: Option<i32>,
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
    // works for any logged-in user. The 30-day insertion counter
    // tags along on every row; the picker ignores it, the admin
    // page surfaces it as a column.
    match tc.run(repo::list_with_insert_counts) {
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
    if let Err(resp) = require_workspace_role(&req, WorkspaceRole::Admin) {
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
    if let Err(resp) = require_workspace_role(&req, WorkspaceRole::Admin) {
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
    if let Err(resp) = require_workspace_role(&req, WorkspaceRole::Admin) {
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

/// POST /api/canned-responses/{id}/insertions — usage-log endpoint
/// the composer hits fire-and-forget on every insert. Workspace-
/// local: rows stay inside this operator's database, no external
/// transmission. Body is optional; an empty `{}` is fine and means
/// "no ticket context." Any authenticated user. Failures log but
/// return 200 anyway so a logging hiccup doesn't break the user-
/// facing insert flow.
pub async fn record_insertion(
    mut tc: TenantConn,
    path: web::Path<i32>,
    body: Option<web::Json<RecordInsertionRequest>>,
    req: HttpRequest,
) -> HttpResponse {
    let canned_response_id = path.into_inner();
    let user_uuid = req
        .extensions()
        .get::<crate::models::Claims>()
        .and_then(|c| uuid::Uuid::parse_str(&c.sub).ok());
    let workspace_id = req
        .extensions()
        .get::<crate::middleware::RequestContext>()
        .and_then(|ctx| ctx.actor.workspace_id);

    // No workspace pin means the auth middleware didn't run a
    // workspace-aware path. A usage row without a workspace can't
    // satisfy the RLS policy and would just trip the WITH CHECK,
    // so drop it silently.
    let Some(workspace_id) = workspace_id else {
        return HttpResponse::Ok().finish();
    };

    let ticket_id = body.and_then(|b| b.ticket_id);
    let new = NewCannedResponseInsertion {
        canned_response_id,
        user_uuid,
        ticket_id,
        workspace_id,
    };

    // FK violation on canned_response_id (template deleted between
    // composer fetch and insert) is the most likely error path and
    // doesn't deserve a 5xx. Log and return 200.
    if let Err(e) = tc.run(|conn| repo::record_insertion(conn, new)) {
        info!(error = %e, canned_response_id, "insertion log skipped");
    }
    HttpResponse::Ok().finish()
}

/// GET /api/admin/canned-response-starters — admin-only, static
/// catalog. Returns a curated list of starter templates the admin
/// can pick from when creating a new canned response. No DB reads;
/// no writes; selecting a starter only pre-fills the editor on the
/// frontend so every saved row remains the admin's choice. Lives
/// on its own path (not nested under `/admin/canned-responses/`)
/// to avoid being shadowed by the sibling `{id}` route under
/// Actix's `.route()` chain ordering.
pub async fn starter_catalog(req: HttpRequest) -> HttpResponse {
    if let Err(resp) = require_workspace_role(&req, WorkspaceRole::Admin) {
        return resp;
    }
    HttpResponse::Ok().json(starters::CATALOG)
}

/// Curated starter templates served by `starter_catalog`. The
/// catalog is intentionally small (eight items) so the picker
/// modal stays scannable. Bodies use only the canned-response
/// allow-list variables, validated by the test at the bottom of
/// this module to keep "ships a template the create handler would
/// reject" out of the design space.
pub mod starters {
    use super::CannedResponseStarter;

    /// Pragmatic defaults: each body works as a standalone reply
    /// chunk (greeting + middle + no signoff, the agent appends
    /// their own). Help Scout's "modular puzzle pieces" philosophy
    /// is a stretch goal admins can pursue when they rewrite for
    /// their own voice; the defaults prioritise "usable in one
    /// click" over voice purity.
    pub const CATALOG: &[CannedResponseStarter] = &[
        CannedResponseStarter {
            slug: "ticket-acknowledged",
            title: "Ticket acknowledged",
            body: "Hi {{customer_name}}, thanks for reaching out. I've opened ticket #{{ticket_id}} for this and I'm looking into it now. I'll get back to you shortly with next steps.",
        },
        CannedResponseStarter {
            slug: "request-more-details",
            title: "Request more details",
            body: "Hi {{customer_name}}, to help me investigate this, could you share:\n\n1. What you were doing when the issue happened\n2. Any error messages you saw (a screenshot helps)\n3. Which device or browser you're on\n\nThanks, that'll let me dig in faster.",
        },
        CannedResponseStarter {
            slug: "status-update",
            title: "Status update",
            body: "Hi {{customer_name}}, quick update on ticket #{{ticket_id}}: I'm still looking into this and should have something for you within a few hours. I'll reach out as soon as I have more information.",
        },
        CannedResponseStarter {
            slug: "try-this-fix",
            title: "Try this fix",
            body: "Hi {{customer_name}}, could you try the following and let me know if it resolves the issue:\n\n1. [Step one]\n2. [Step two]\n3. [Step three]\n\nIf it's still not working after that, let me know and we'll dig deeper.",
        },
        CannedResponseStarter {
            slug: "resolved-closing",
            title: "Resolved, closing",
            body: "Thanks for confirming, {{customer_name}}. I'll mark ticket #{{ticket_id}} as resolved. If anything similar comes up, feel free to reach out and reference this ticket so we have the context to hand.",
        },
        CannedResponseStarter {
            slug: "escalation",
            title: "Escalation",
            body: "Hi {{customer_name}}, I'm passing ticket #{{ticket_id}} to a colleague who specialises in this area. They'll be in touch shortly. The full context of our conversation has been included so you won't need to repeat yourself.",
        },
        CannedResponseStarter {
            slug: "follow-up-check",
            title: "Follow-up check",
            body: "Hi {{customer_name}}, just checking in on ticket #{{ticket_id}}. Did the steps I shared resolve the issue, or are you still seeing the problem? Happy to keep digging if needed.",
        },
        CannedResponseStarter {
            slug: "outside-scope",
            title: "Outside our scope",
            body: "Hi {{customer_name}}, thanks for raising this. Unfortunately what you're describing is outside what {{app_name}} can help with from this team. [Pointer to the right resource]. I'll close ticket #{{ticket_id}} for now, but please reach out again if anything changes.",
        },
    ];
}

#[cfg(test)]
mod tests {
    //! The starter catalog is shipped content, not user input, so it
    //! gets a build-time guard rather than a save-time validator: if
    //! a future commit introduces a typo'd variable in a starter, the
    //! test below catches it before it lands in production. Same
    //! contract the live create/update handlers enforce, applied to
    //! every starter body.

    use super::starters::CATALOG;
    use crate::utils::template_variables::{unknown_variables, CANNED_RESPONSE_VARIABLES};
    use std::collections::HashSet;

    #[test]
    fn starter_bodies_only_reference_allowlisted_variables() {
        for starter in CATALOG {
            let unknown = unknown_variables(starter.body, CANNED_RESPONSE_VARIABLES);
            assert!(
                unknown.is_empty(),
                "starter `{}` references unknown variables: {:?}",
                starter.slug,
                unknown
            );
        }
    }

    #[test]
    fn starter_slugs_are_unique() {
        let slugs: HashSet<&str> = CATALOG.iter().map(|s| s.slug).collect();
        assert_eq!(slugs.len(), CATALOG.len(), "duplicate slug in starter pack");
    }

    #[test]
    fn starter_titles_and_bodies_are_non_empty() {
        for starter in CATALOG {
            assert!(
                !starter.title.trim().is_empty(),
                "starter `{}` has empty title",
                starter.slug
            );
            assert!(
                !starter.body.trim().is_empty(),
                "starter `{}` has empty body",
                starter.slug
            );
        }
    }
}
