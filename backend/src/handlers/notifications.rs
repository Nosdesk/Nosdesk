//! Notification API handlers
//!
//! Endpoints for managing user notifications and preferences.

use actix_web::{web, HttpMessage, HttpRequest, HttpResponse};
use serde::Deserialize;

use crate::models::Claims;
use crate::services::notifications::{NotificationChannel, NotificationService, NotificationTypeCode};

/// Query parameters for fetching notifications
#[derive(Debug, Deserialize)]
pub struct NotificationQuery {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    pub unread_only: Option<bool>,
}

/// Request body for marking notifications as read
#[derive(Debug, Deserialize)]
pub struct MarkReadRequest {
    pub notification_ids: Vec<i32>,
}

/// Request body for deleting notifications
#[derive(Debug, Deserialize)]
pub struct DeleteNotificationsRequest {
    pub notification_ids: Vec<i32>,
}

/// Request body for updating a preference
#[derive(Debug, Deserialize)]
pub struct UpdatePreferenceRequest {
    pub notification_type: String,
    pub channel: String,
    pub enabled: bool,
}

/// Get user's notifications
///
/// GET /api/notifications
pub async fn get_notifications(
    req: HttpRequest,
    notification_service: web::Data<NotificationService>,
    query: web::Query<NotificationQuery>,
) -> HttpResponse {
    let claims = match req.extensions().get::<Claims>() {
        Some(c) => c.clone(),
        None => return HttpResponse::Unauthorized().finish(),
    };

    let user_uuid = match uuid::Uuid::parse_str(&claims.sub) {
        Ok(u) => u,
        Err(_) => return HttpResponse::BadRequest().json("Invalid user UUID"),
    };

    let limit = query.limit.unwrap_or(20).min(100);
    let offset = query.offset.unwrap_or(0);
    let unread_only = query.unread_only.unwrap_or(false);

    let result = if unread_only {
        notification_service.get_unread(&user_uuid, limit).await
    } else {
        notification_service.get_all(&user_uuid, limit, offset).await
    };

    match result {
        Ok(notifications) => HttpResponse::Ok().json(notifications),
        Err(e) => HttpResponse::InternalServerError().json(serde_json::json!({
            "error": e
        })),
    }
}

/// Get unread notification count
///
/// GET /api/notifications/count
pub async fn get_unread_count(
    req: HttpRequest,
    notification_service: web::Data<NotificationService>,
) -> HttpResponse {
    let claims = match req.extensions().get::<Claims>() {
        Some(c) => c.clone(),
        None => return HttpResponse::Unauthorized().finish(),
    };

    let user_uuid = match uuid::Uuid::parse_str(&claims.sub) {
        Ok(u) => u,
        Err(_) => return HttpResponse::BadRequest().json("Invalid user UUID"),
    };

    match notification_service.get_unread_count(&user_uuid).await {
        Ok(count) => HttpResponse::Ok().json(serde_json::json!({ "count": count })),
        Err(e) => HttpResponse::InternalServerError().json(serde_json::json!({
            "error": e
        })),
    }
}

/// Mark notifications as read
///
/// POST /api/notifications/read
pub async fn mark_notifications_read(
    req: HttpRequest,
    notification_service: web::Data<NotificationService>,
    body: web::Json<MarkReadRequest>,
) -> HttpResponse {
    let claims = match req.extensions().get::<Claims>() {
        Some(c) => c.clone(),
        None => return HttpResponse::Unauthorized().finish(),
    };

    let user_uuid = match uuid::Uuid::parse_str(&claims.sub) {
        Ok(u) => u,
        Err(_) => return HttpResponse::BadRequest().json("Invalid user UUID"),
    };

    match notification_service
        .mark_read(&user_uuid, &body.notification_ids)
        .await
    {
        Ok(count) => HttpResponse::Ok().json(serde_json::json!({
            "success": true,
            "count": count
        })),
        Err(e) => HttpResponse::InternalServerError().json(serde_json::json!({
            "error": e
        })),
    }
}

/// Mark all notifications as read
///
/// POST /api/notifications/read-all
pub async fn mark_all_notifications_read(
    req: HttpRequest,
    notification_service: web::Data<NotificationService>,
) -> HttpResponse {
    let claims = match req.extensions().get::<Claims>() {
        Some(c) => c.clone(),
        None => return HttpResponse::Unauthorized().finish(),
    };

    let user_uuid = match uuid::Uuid::parse_str(&claims.sub) {
        Ok(u) => u,
        Err(_) => return HttpResponse::BadRequest().json("Invalid user UUID"),
    };

    match notification_service.mark_all_read(&user_uuid).await {
        Ok(count) => HttpResponse::Ok().json(serde_json::json!({
            "success": true,
            "count": count
        })),
        Err(e) => HttpResponse::InternalServerError().json(serde_json::json!({
            "error": e
        })),
    }
}

/// Get user's notification preferences
///
/// GET /api/notifications/preferences
pub async fn get_preferences(
    req: HttpRequest,
    notification_service: web::Data<NotificationService>,
) -> HttpResponse {
    let claims = match req.extensions().get::<Claims>() {
        Some(c) => c.clone(),
        None => return HttpResponse::Unauthorized().finish(),
    };

    let user_uuid = match uuid::Uuid::parse_str(&claims.sub) {
        Ok(u) => u,
        Err(_) => return HttpResponse::BadRequest().json("Invalid user UUID"),
    };

    match notification_service
        .preferences()
        .get_all_preferences(&user_uuid)
        .await
    {
        Ok(prefs) => HttpResponse::Ok().json(prefs),
        Err(e) => HttpResponse::InternalServerError().json(serde_json::json!({
            "error": e
        })),
    }
}

/// Update a notification preference
///
/// PUT /api/notifications/preferences
pub async fn update_preference(
    req: HttpRequest,
    notification_service: web::Data<NotificationService>,
    body: web::Json<UpdatePreferenceRequest>,
) -> HttpResponse {
    let claims = match req.extensions().get::<Claims>() {
        Some(c) => c.clone(),
        None => return HttpResponse::Unauthorized().finish(),
    };

    let user_uuid = match uuid::Uuid::parse_str(&claims.sub) {
        Ok(u) => u,
        Err(_) => return HttpResponse::BadRequest().json("Invalid user UUID"),
    };

    let notification_type = match NotificationTypeCode::from_str(&body.notification_type) {
        Some(t) => t,
        None => {
            return HttpResponse::BadRequest().json(serde_json::json!({
                "error": format!("Invalid notification type: {}", body.notification_type)
            }))
        }
    };

    let channel = match NotificationChannel::from_str(&body.channel) {
        Some(c) => c,
        None => {
            return HttpResponse::BadRequest().json(serde_json::json!({
                "error": format!("Invalid channel: {}", body.channel)
            }))
        }
    };

    match notification_service
        .preferences()
        .set_preference(&user_uuid, &notification_type, channel, body.enabled)
        .await
    {
        Ok(_) => HttpResponse::Ok().json(serde_json::json!({ "success": true })),
        Err(e) => HttpResponse::InternalServerError().json(serde_json::json!({
            "error": e
        })),
    }
}

/// Delete notifications
///
/// POST /api/notifications/delete
pub async fn delete_notifications(
    req: HttpRequest,
    notification_service: web::Data<NotificationService>,
    body: web::Json<DeleteNotificationsRequest>,
) -> HttpResponse {
    let claims = match req.extensions().get::<Claims>() {
        Some(c) => c.clone(),
        None => return HttpResponse::Unauthorized().finish(),
    };

    let user_uuid = match uuid::Uuid::parse_str(&claims.sub) {
        Ok(u) => u,
        Err(_) => return HttpResponse::BadRequest().json("Invalid user UUID"),
    };

    match notification_service
        .delete_notifications(&user_uuid, &body.notification_ids)
        .await
    {
        Ok(count) => HttpResponse::Ok().json(serde_json::json!({
            "success": true,
            "count": count
        })),
        Err(e) => HttpResponse::InternalServerError().json(serde_json::json!({
            "error": e
        })),
    }
}
