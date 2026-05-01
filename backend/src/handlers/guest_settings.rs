//! Admin-only handlers for the guest-access feature flags in `site_settings`.
//! Exposed at `/api/admin/guest-settings`.

use actix_web::{web, HttpMessage, HttpRequest, HttpResponse, Responder};
use serde::Deserialize;
use tracing::error;

use crate::db::Pool;
use crate::handlers::helpers;
use crate::handlers::errors;
use crate::models::{Claims, SiteSettingsResponse, UpdateSiteSettings};
use crate::repository::site_settings;
use crate::utils;

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

pub async fn get_guest_settings(pool: web::Data<Pool>) -> impl Responder {
    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    match site_settings::get_site_settings(&mut conn) {
        Ok(settings) => {
            let response: SiteSettingsResponse = settings.into();
            HttpResponse::Ok().json(response)
        }
        Err(e) => {
            error!(error = ?e, "Failed to load site_settings for guest admin view");
            errors::internal("Failed to load settings")
        }
    }
}

pub async fn update_guest_settings(
    pool: web::Data<Pool>,
    req: HttpRequest,
    body: web::Json<UpdateGuestSettingsRequest>,
) -> impl Responder {
    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let claims = match req.extensions().get::<Claims>() {
        Some(c) => c.clone(),
        None => return HttpResponse::Unauthorized().finish(),
    };

    if !crate::utils::rbac::is_admin(&claims) {
        return errors::forbidden("Admin required");
    }

    let user_uuid = match utils::parse_uuid(&claims.sub) {
        Ok(u) => u,
        Err(_) => return HttpResponse::BadRequest().finish(),
    };

    if let Some(n) = body.guest_ticket_rate_limit_per_hour {
        if !(1..=1000).contains(&n) {
            return errors::bad_request("Rate limit must be between 1 and 1000");
        }
    }
    if let Some(Some(ref p)) = body.guest_ticket_default_priority {
        if !["low", "medium", "high"].contains(&p.as_str()) {
            return errors::bad_request("Invalid default priority");
        }
    }

    // Intro message: plain text only, bounded to 500 chars. Rendered with
    // preserved line breaks but no markdown/HTML on the frontend, so this
    // is just a size cap — no HTML escaping or sanitation needed here
    // (that happens at render time).
    if let Some(Some(ref m)) = body.guest_ticket_intro_message {
        if m.chars().count() > 500 {
            return errors::bad_request("Intro message must be 500 characters or fewer");
        }
    }

    // Normalize: whitespace-only strings collapse to null so the frontend
    // renders no intro at all.
    let intro_update = body.guest_ticket_intro_message.clone().map(|outer| {
        outer.and_then(|s| {
            let trimmed = s.trim().to_string();
            if trimmed.is_empty() { None } else { Some(trimmed) }
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

    match site_settings::update_site_settings(&mut conn, update) {
        Ok(settings) => {
            let response: SiteSettingsResponse = settings.into();
            HttpResponse::Ok().json(response)
        }
        Err(e) => {
            error!(error = ?e, "Failed to update guest settings");
            errors::internal("Failed to update settings")
        }
    }
}
