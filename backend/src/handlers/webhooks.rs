//! Webhook Handlers
//!
//! Admin endpoints for managing webhooks for external integrations.

use actix_web::{web, HttpMessage, HttpRequest, HttpResponse, Responder};
use diesel::result::Error as DieselError;
use serde::Deserialize;
use tracing::{error, info};
use uuid::Uuid;

use crate::db::Pool;
use crate::handlers::helpers;
use crate::handlers::errors;
use crate::models::{
    Claims, CreateWebhookRequest, UpdateWebhookRequest, WebhookCreatedResponse,
    WebhookDeliveryResponse, WebhookResponse, WebhookUpdate,
};
use crate::repository::webhooks as webhook_repo;
use crate::services::webhooks::{generate_secret, WebhookEventType, WebhookService};
use crate::utils::rbac::require_admin;

/// Query parameters for pagination
#[derive(Debug, Deserialize)]
pub struct PaginationQuery {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

// =============================================================================
// Helper Functions (DRY)
// =============================================================================

/// Validate webhook name
fn validate_name(name: &str) -> Result<String, HttpResponse> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(errors::bad_request("Webhook name is required"));
    }
    if trimmed.len() > 255 {
        return Err(errors::bad_request("Webhook name must be 255 characters or less"));
    }
    Ok(trimmed.to_string())
}

/// Validate webhook URL
fn validate_url(url: &str) -> Result<(), HttpResponse> {
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err(errors::bad_request("URL must start with http:// or https://"));
    }
    Ok(())
}

/// Validate event types
fn validate_events(events: &[String]) -> Result<(), HttpResponse> {
    if events.is_empty() {
        return Err(errors::bad_request("At least one event type is required"));
    }
    let valid_events = WebhookEventType::all();
    if let Some(invalid) = events.iter().find(|e| !valid_events.contains(&e.as_str())) {
        return Err(errors::bad_request(format!("Invalid event type: {invalid}")));
    }
    Ok(())
}

// =============================================================================
// Handlers
// =============================================================================

/// List all webhooks (admin only)
pub async fn list_webhooks(req: HttpRequest, pool: web::Data<Pool>) -> impl Responder {
    if let Err(e) = require_admin(&req) {
        return e;
    }

    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    match webhook_repo::list_all_webhooks(&mut conn) {
        Ok(webhooks) => {
            let response: Vec<WebhookResponse> = webhooks.into_iter().map(Into::into).collect();
            HttpResponse::Ok().json(response)
        }
        Err(e) => {
            error!("Failed to list webhooks: {}", e);
            errors::internal("Failed to list webhooks")
        }
    }
}

/// Create a new webhook (admin only)
pub async fn create_webhook(
    req: HttpRequest,
    pool: web::Data<Pool>,
    body: web::Json<CreateWebhookRequest>,
) -> impl Responder {
    if let Err(e) = require_admin(&req) {
        return e;
    }

    let claims = match req.extensions().get::<Claims>() {
        Some(claims) => claims.clone(),
        None => return errors::unauthorized("Authentication required"),
    };

    let created_by = Uuid::parse_str(&claims.sub).ok();

    // Validate inputs
    let name = match validate_name(&body.name) {
        Ok(n) => n,
        Err(e) => return e,
    };
    if let Err(e) = validate_url(&body.url) {
        return e;
    }
    if let Err(e) = validate_events(&body.events) {
        return e;
    }

    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let secret = generate_secret();

    match webhook_repo::create_webhook(
        &mut conn,
        name,
        body.url.clone(),
        secret.clone(),
        body.events.clone(),
        body.headers.clone(),
        created_by,
    ) {
        Ok(webhook) => {
            info!(
                "Webhook created: {} ({}) by {:?}",
                webhook.uuid, webhook.name, created_by
            );
            HttpResponse::Created().json(WebhookCreatedResponse {
                uuid: webhook.uuid,
                name: webhook.name,
                url: webhook.url,
                secret, // Only shown once!
                events: body.events.clone(),
            })
        }
        Err(e) => {
            error!("Failed to create webhook: {}", e);
            errors::internal("Failed to create webhook")
        }
    }
}

/// Get available event types
pub async fn get_event_types(req: HttpRequest) -> impl Responder {
    if let Err(e) = require_admin(&req) {
        return e;
    }

    HttpResponse::Ok().json(WebhookEventType::all())
}

/// Get a single webhook by UUID (admin only)
pub async fn get_webhook(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<Uuid>,
) -> impl Responder {
    if let Err(e) = require_admin(&req) {
        return e;
    }

    let webhook_uuid = path.into_inner();

    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    match webhook_repo::get_webhook_by_uuid(&mut conn, webhook_uuid) {
        Ok(webhook) => HttpResponse::Ok().json(WebhookResponse::from(webhook)),
        Err(DieselError::NotFound) => errors::not_found_msg("Webhook not found"),
        Err(e) => {
            error!("Failed to get webhook: {}", e);
            errors::internal("Failed to get webhook")
        }
    }
}

/// Update a webhook (admin only)
pub async fn update_webhook(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<Uuid>,
    body: web::Json<UpdateWebhookRequest>,
) -> impl Responder {
    if let Err(e) = require_admin(&req) {
        return e;
    }

    let webhook_uuid = path.into_inner();

    // Validate optional fields if provided
    let validated_name = if let Some(ref name) = body.name {
        match validate_name(name) {
            Ok(n) => Some(n),
            Err(e) => return e,
        }
    } else {
        None
    };

    if let Some(ref url) = body.url {
        if let Err(e) = validate_url(url) {
            return e;
        }
    }

    if let Some(ref events) = body.events {
        if let Err(e) = validate_events(events) {
            return e;
        }
    }

    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    // Build update
    let mut update = WebhookUpdate::default();
    update.name = validated_name;
    update.url = body.url.clone();
    update.enabled = body.enabled;
    update.headers = body.headers.clone();

    if let Some(ref events) = body.events {
        update.events = Some(events.iter().map(|e| Some(e.clone())).collect());
    }

    // Regenerate secret if requested
    if body.regenerate_secret == Some(true) {
        update.secret = Some(generate_secret());
    }

    // Reset failure count if re-enabling
    if body.enabled == Some(true) {
        update.failure_count = Some(0);
        update.disabled_reason = Some(None);
    }

    match webhook_repo::update_webhook_by_uuid(&mut conn, webhook_uuid, update) {
        Ok(webhook) => {
            info!("Webhook updated: {} ({})", webhook.uuid, webhook.name);
            HttpResponse::Ok().json(WebhookResponse::from(webhook))
        }
        Err(DieselError::NotFound) => errors::not_found_msg("Webhook not found"),
        Err(e) => {
            error!("Failed to update webhook: {}", e);
            errors::internal("Failed to update webhook")
        }
    }
}

/// Delete a webhook (admin only)
pub async fn delete_webhook(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<Uuid>,
) -> impl Responder {
    if let Err(e) = require_admin(&req) {
        return e;
    }

    let webhook_uuid = path.into_inner();

    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    match webhook_repo::delete_webhook_by_uuid(&mut conn, webhook_uuid) {
        Ok(count) if count > 0 => {
            info!("Webhook deleted: {}", webhook_uuid);
            HttpResponse::NoContent().finish()
        }
        Ok(_) => errors::not_found_msg("Webhook not found"),
        Err(e) => {
            error!("Failed to delete webhook: {}", e);
            errors::internal("Failed to delete webhook")
        }
    }
}

/// Get delivery history for a webhook (admin only)
pub async fn get_deliveries(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<Uuid>,
    query: web::Query<PaginationQuery>,
) -> impl Responder {
    if let Err(e) = require_admin(&req) {
        return e;
    }

    let webhook_uuid = path.into_inner();
    let limit = helpers::clamp_limit(query.limit);
    let offset = helpers::clamp_offset(query.offset);

    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    // Get webhook by UUID first
    let webhook = match webhook_repo::get_webhook_by_uuid(&mut conn, webhook_uuid) {
        Ok(w) => w,
        Err(DieselError::NotFound) => return errors::not_found_msg("Webhook not found"),
        Err(e) => {
            error!("Failed to get webhook: {}", e);
            return errors::internal("Failed to get webhook");
        }
    };

    match webhook_repo::get_deliveries_for_webhook(&mut conn, webhook.id, limit, offset) {
        Ok(deliveries) => {
            let response: Vec<WebhookDeliveryResponse> = deliveries
                .into_iter()
                .map(|d| WebhookDeliveryResponse {
                    uuid: d.uuid,
                    event_type: d.event_type,
                    response_status: d.response_status,
                    duration_ms: d.duration_ms,
                    error_message: d.error_message,
                    delivered_at: d.delivered_at,
                    created_at: d.created_at,
                    attempt_number: d.attempt_number,
                })
                .collect();
            HttpResponse::Ok().json(response)
        }
        Err(e) => {
            error!("Failed to get deliveries: {}", e);
            errors::internal("Failed to get deliveries")
        }
    }
}

/// Send a test event to a webhook (admin only)
pub async fn test_webhook(
    req: HttpRequest,
    pool: web::Data<Pool>,
    webhook_service: web::Data<WebhookService>,
    path: web::Path<Uuid>,
) -> impl Responder {
    if let Err(e) = require_admin(&req) {
        return e;
    }

    let webhook_uuid = path.into_inner();

    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    // Get webhook by UUID first
    let webhook = match webhook_repo::get_webhook_by_uuid(&mut conn, webhook_uuid) {
        Ok(w) => w,
        Err(DieselError::NotFound) => return errors::not_found_msg("Webhook not found"),
        Err(e) => {
            error!("Failed to get webhook: {}", e);
            return errors::internal("Failed to get webhook");
        }
    };

    match webhook_service.send_test_event(webhook.id).await {
        Ok(_) => {
            info!("Test event sent to webhook: {} ({})", webhook.uuid, webhook.name);
            HttpResponse::Ok().json(serde_json::json!({
                "message": "Test event queued for delivery"
            }))
        }
        Err(e) => {
            error!("Failed to send test event: {}", e);
            errors::internal(format!("Failed to send test event: {e}"))
        }
    }
}
