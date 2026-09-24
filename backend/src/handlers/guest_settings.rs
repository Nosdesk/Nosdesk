//! Handlers for the guest-access (public portal) settings in `site_settings`:
//! the admin flags at `/api/admin/guest-settings`, and `/api/workspace/portal`,
//! which tells any member where the workspace's portal is and what it offers.

use actix_web::{web, HttpRequest, HttpResponse};
use serde::Deserialize;
use tracing::error;

use crate::errors::ApiError;
use crate::extractors::TenantConn;
use crate::models::{SiteSettingsResponse, UpdateSiteSettings};
use crate::repository::site_settings;
use crate::utils;

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.route(
        "/admin/guest-settings",
        web::get().to(crate::handlers::guest_settings::get_guest_settings),
    )
    .route(
        "/admin/guest-settings",
        web::patch().to(crate::handlers::guest_settings::update_guest_settings),
    )
    .route(
        "/workspace/portal",
        web::get().to(crate::handlers::guest_settings::get_portal),
    );
}

/// Where this workspace's public portal lives and what it offers.
#[derive(Debug, serde::Serialize)]
pub struct PortalInfo {
    /// The portal's origin: the workspace's own host (subdomain or custom
    /// domain), else `FRONTEND_URL`. `None` when neither is known, and the
    /// client uses its own origin (self-hosted, same host).
    pub portal_url: Option<String>,
    pub request_form_enabled: bool,
    pub public_docs_enabled: bool,
    pub help_page_enabled: bool,
}

/// GET /api/workspace/portal — for links to the portal from the agent app.
///
/// On hosted, the agent app runs on one shared origin, so a link built from
/// the page's own origin (`/submit-ticket`, `/docs`) lands on the agent app,
/// not the workspace's portal, and 404s. The portal's public endpoints resolve
/// the workspace from the host, so they can't answer this for the agent app
/// either; this authenticated read does.
pub async fn get_portal(
    mut tc: TenantConn,
    req: HttpRequest,
    ws: crate::extractors::WorkspaceContext,
) -> Result<HttpResponse, ApiError> {
    crate::utils::rbac::require_workspace_role(&req, crate::models::WorkspaceRole::Member)?;
    let settings = tc.run(site_settings::get_site_settings).map_err(|e| {
        error!(error = ?e, "Failed to load site_settings for portal info");
        ApiError::Internal("Failed to load settings".into())
    })?;
    Ok(HttpResponse::Ok().json(PortalInfo {
        portal_url: crate::utils::tenant_origin::email_link_base(ws.canonical_origin()),
        request_form_enabled: settings.guest_tickets_enabled,
        public_docs_enabled: settings.guest_public_docs_enabled,
        help_page_enabled: settings.guest_help_page_enabled,
    }))
}

/// Partial update payload for the guest-access admin settings. Any field
/// left `None` is untouched. `Option<Option<String>>` fields use the
/// outer `Some(None)` to explicitly clear a previously-set value.
#[derive(Debug, Deserialize)]
pub struct UpdateGuestSettingsRequest {
    pub guest_tickets_enabled: Option<bool>,
    pub guest_public_docs_enabled: Option<bool>,
    pub guest_kb_search_enabled: Option<bool>,
    pub guest_ticket_lookup_enabled: Option<bool>,
    pub guest_help_page_enabled: Option<bool>,
    pub guest_ticket_default_priority: Option<Option<String>>,
    pub guest_ticket_rate_limit_per_hour: Option<i32>,
    pub guest_ticket_email_verification: Option<bool>,
    pub guest_ticket_attachments_enabled: Option<bool>,
    pub guest_ticket_intro_message: Option<Option<String>>,
}

pub async fn get_guest_settings(
    mut tc: TenantConn,
    req: HttpRequest,
) -> Result<HttpResponse, ApiError> {
    // Per-workspace guest config (site_settings, RLS-isolated via TenantConn),
    // so a workspace admin owns it. The read was ungated while the write
    // demanded platform-admin; both are now workspace-admin. The public portal
    // reads guest config through handlers/guest.rs, not this admin endpoint.
    crate::utils::rbac::require_workspace_role(&req, crate::models::WorkspaceRole::Admin)?;
    match tc.run(site_settings::get_site_settings) {
        Ok(settings) => {
            let response: SiteSettingsResponse = settings.into();
            Ok(HttpResponse::Ok().json(response))
        }
        Err(e) => {
            error!(error = ?e, "Failed to load site_settings for guest admin view");
            Err(ApiError::Internal("Failed to load settings".into()))
        }
    }
}

pub async fn update_guest_settings(
    mut tc: TenantConn,
    req: HttpRequest,
    body: web::Json<UpdateGuestSettingsRequest>,
) -> Result<HttpResponse, ApiError> {
    // Per-workspace guest config, so a workspace admin owns it. Was
    // platform-admin, which dead-ended every tenant admin on save.
    let claims =
        crate::utils::rbac::require_workspace_role(&req, crate::models::WorkspaceRole::Admin)?;

    let user_uuid = match utils::parse_uuid(&claims.sub) {
        Ok(u) => u,
        Err(_) => return Err(ApiError::BadRequest("Bad request".into())),
    };

    if let Some(n) = body.guest_ticket_rate_limit_per_hour {
        if !(1..=1000).contains(&n) {
            return Err(ApiError::BadRequest(
                "Rate limit must be between 1 and 1000".into(),
            ));
        }
    }
    if let Some(Some(ref p)) = body.guest_ticket_default_priority {
        if !["low", "medium", "high"].contains(&p.as_str()) {
            return Err(ApiError::BadRequest("Invalid default priority".into()));
        }
    }

    // Intro message: plain text only, bounded to 500 chars. Rendered with
    // preserved line breaks but no markdown/HTML on the frontend, so this
    // is just a size cap — no HTML escaping or sanitation needed here
    // (that happens at render time).
    if let Some(Some(ref m)) = body.guest_ticket_intro_message {
        if m.chars().count() > 500 {
            return Err(ApiError::BadRequest(
                "Intro message must be 500 characters or fewer".into(),
            ));
        }
    }

    // Normalize: whitespace-only strings collapse to null so the frontend
    // renders no intro at all.
    let intro_update = body.guest_ticket_intro_message.clone().map(|outer| {
        outer.and_then(|s| {
            let trimmed = s.trim().to_string();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            }
        })
    });

    let update = UpdateSiteSettings {
        updated_by: Some(user_uuid),
        guest_tickets_enabled: body.guest_tickets_enabled,
        guest_public_docs_enabled: body.guest_public_docs_enabled,
        guest_kb_search_enabled: body.guest_kb_search_enabled,
        guest_ticket_lookup_enabled: body.guest_ticket_lookup_enabled,
        guest_help_page_enabled: body.guest_help_page_enabled,
        guest_ticket_default_priority: body.guest_ticket_default_priority.clone(),
        guest_ticket_rate_limit_per_hour: body.guest_ticket_rate_limit_per_hour,
        guest_ticket_email_verification: body.guest_ticket_email_verification,
        guest_ticket_attachments_enabled: body.guest_ticket_attachments_enabled,
        guest_ticket_intro_message: intro_update,
        ..Default::default()
    };

    match tc.run(|conn| site_settings::update_site_settings(conn, update)) {
        Ok(settings) => {
            let response: SiteSettingsResponse = settings.into();
            Ok(HttpResponse::Ok().json(response))
        }
        Err(e) => {
            error!(error = ?e, "Failed to update guest settings");
            Err(ApiError::Internal("Failed to update settings".into()))
        }
    }
}
