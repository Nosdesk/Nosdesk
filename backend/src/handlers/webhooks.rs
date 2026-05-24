//! Webhook Handlers
//!
//! Admin endpoints for managing webhooks for external integrations.

use actix_web::{web, HttpRequest, HttpResponse, Responder};
use diesel::result::Error as DieselError;
use serde::Deserialize;
use tracing::{error, info};
use uuid::Uuid;

use crate::extractors::{AuthContext, TenantConn};
use crate::handlers::errors;
use crate::handlers::helpers;
use crate::models::{
    CreateWebhookRequest, UpdateWebhookRequest, Webhook, WebhookCreatedResponse,
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
        return Err(errors::bad_request(
            "Webhook name must be 255 characters or less",
        ));
    }
    Ok(trimmed.to_string())
}

/// Validate webhook URL
fn validate_url(url: &str) -> Result<(), HttpResponse> {
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err(errors::bad_request(
            "URL must start with http:// or https://",
        ));
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
        return Err(errors::bad_request(format!(
            "Invalid event type: {invalid}"
        )));
    }
    Ok(())
}

// =============================================================================
// Handlers
// =============================================================================

/// List all webhooks (admin only)
pub async fn list_webhooks(req: HttpRequest, mut tc: TenantConn) -> impl Responder {
    if let Err(e) = require_admin(&req) {
        return e;
    }

    match tc.run(|conn| webhook_repo::list_all_webhooks(conn)) {
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
    mut tc: TenantConn,
    auth: AuthContext,
    body: web::Json<CreateWebhookRequest>,
) -> impl Responder {
    if let Err(e) = require_admin(&req) {
        return e;
    }

    let created_by = Some(auth.user_uuid);

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

    let secret = generate_secret();
    let url = body.url.clone();
    let events = body.events.clone();
    let headers = body.headers.clone();
    let secret_for_repo = secret.clone();

    match tc.run(|conn| {
        webhook_repo::create_webhook(
            conn,
            name,
            url,
            secret_for_repo,
            events,
            headers,
            created_by,
        )
    }) {
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
    mut tc: TenantConn,
    path: web::Path<Uuid>,
) -> impl Responder {
    if let Err(e) = require_admin(&req) {
        return e;
    }

    let webhook_uuid = path.into_inner();

    match tc.run(|conn| webhook_repo::get_webhook_by_uuid(conn, webhook_uuid)) {
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
    mut tc: TenantConn,
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

    match tc.run(|conn| webhook_repo::update_webhook_by_uuid(conn, webhook_uuid, update)) {
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
    mut tc: TenantConn,
    path: web::Path<Uuid>,
) -> impl Responder {
    if let Err(e) = require_admin(&req) {
        return e;
    }

    let webhook_uuid = path.into_inner();

    match tc.run(|conn| webhook_repo::delete_webhook_by_uuid(conn, webhook_uuid)) {
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

/// Result variants for the get-deliveries flow.
enum DeliveriesOutcome {
    Ok(Vec<crate::models::WebhookDelivery>),
    NotFound,
}

/// Get delivery history for a webhook (admin only)
pub async fn get_deliveries(
    req: HttpRequest,
    mut tc: TenantConn,
    path: web::Path<Uuid>,
    query: web::Query<PaginationQuery>,
) -> impl Responder {
    if let Err(e) = require_admin(&req) {
        return e;
    }

    let webhook_uuid = path.into_inner();
    let limit = helpers::clamp_limit(query.limit);
    let offset = helpers::clamp_offset(query.offset);

    let outcome = tc.run(|conn| {
        let webhook = match webhook_repo::get_webhook_by_uuid(conn, webhook_uuid) {
            Ok(w) => w,
            Err(DieselError::NotFound) => return Ok(DeliveriesOutcome::NotFound),
            Err(e) => return Err(e),
        };
        let deliveries = webhook_repo::get_deliveries_for_webhook(conn, webhook.id, limit, offset)?;
        Ok(DeliveriesOutcome::Ok(deliveries))
    });

    match outcome {
        Ok(DeliveriesOutcome::Ok(deliveries)) => {
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
        Ok(DeliveriesOutcome::NotFound) => errors::not_found_msg("Webhook not found"),
        Err(e) => {
            error!("Failed to get deliveries: {}", e);
            errors::internal("Failed to get deliveries")
        }
    }
}

/// Result variants for the test-webhook flow's lookup-then-dispatch.
enum TestLookupOutcome {
    Ok(Webhook),
    NotFound,
}

/// Send a test event to a webhook (admin only)
pub async fn test_webhook(
    req: HttpRequest,
    mut tc: TenantConn,
    webhook_service: web::Data<WebhookService>,
    path: web::Path<Uuid>,
) -> impl Responder {
    if let Err(e) = require_admin(&req) {
        return e;
    }

    let webhook_uuid = path.into_inner();

    let lookup = tc.run(
        |conn| match webhook_repo::get_webhook_by_uuid(conn, webhook_uuid) {
            Ok(w) => Ok(TestLookupOutcome::Ok(w)),
            Err(DieselError::NotFound) => Ok(TestLookupOutcome::NotFound),
            Err(e) => Err(e),
        },
    );

    let webhook = match lookup {
        Ok(TestLookupOutcome::Ok(w)) => w,
        Ok(TestLookupOutcome::NotFound) => return errors::not_found_msg("Webhook not found"),
        Err(e) => {
            error!("Failed to get webhook: {}", e);
            return errors::internal("Failed to get webhook");
        }
    };

    match webhook_service.send_test_event(webhook.id).await {
        Ok(_) => {
            info!(
                "Test event sent to webhook: {} ({})",
                webhook.uuid, webhook.name
            );
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

#[cfg(test)]
mod tests {
    //! Permission-boundary tests for the webhooks surface. The gate
    //! itself is unit-tested in utils::rbac; here we prove it's wired
    //! to the list and delete endpoints (the highest-impact actions on
    //! this surface — a non-admin could otherwise silence audit-style
    //! integrations or leak the webhook secret payload).
    use super::*;
    use crate::models::UserRole;
    use crate::test_helpers::{claims_for, setup_test_pool};
    use actix_web::test as actix_test;
    use actix_web::{http::StatusCode, App, HttpMessage};

    fn test_app(
        pool: crate::db::Pool,
    ) -> App<
        impl actix_web::dev::ServiceFactory<
            actix_web::dev::ServiceRequest,
            Config = (),
            Response = actix_web::dev::ServiceResponse<impl actix_web::body::MessageBody>,
            Error = actix_web::Error,
            InitError = (),
        >,
    > {
        App::new()
            .app_data(web::Data::new(pool))
            .route("/admin/webhooks", web::get().to(list_webhooks))
            .route("/admin/webhooks/{id}", web::delete().to(delete_webhook))
    }

    #[actix_web::test]
    async fn list_requires_authentication() {
        let pool = setup_test_pool();
        let app = actix_test::init_service(test_app(pool)).await;
        let req = actix_test::TestRequest::get()
            .uri("/admin/webhooks")
            .to_request();
        let resp = actix_test::call_service(&app, req).await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[actix_web::test]
    async fn list_rejects_user_role() {
        let pool = setup_test_pool();
        let claims = claims_for(&pool, UserRole::User);
        let app = actix_test::init_service(test_app(pool.clone())).await;
        let req = actix_test::TestRequest::get()
            .uri("/admin/webhooks")
            .to_request();
        req.extensions_mut().insert(claims);
        let resp = actix_test::call_service(&app, req).await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[actix_web::test]
    async fn delete_rejects_user_role() {
        // The path extractor types `{id}` as Uuid, so we need a valid
        // uuid to actually dispatch to the handler. An int-shaped path
        // would 404 inside the extractor before the gate fires (which
        // would mask a missing gate).
        let pool = setup_test_pool();
        let claims = claims_for(&pool, UserRole::User);
        let app = actix_test::init_service(test_app(pool.clone())).await;
        let req = actix_test::TestRequest::delete()
            .uri(&format!("/admin/webhooks/{}", uuid::Uuid::now_v7()))
            .to_request();
        req.extensions_mut().insert(claims);
        let resp = actix_test::call_service(&app, req).await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }
}
