//! Notification API handlers
//!
//! Endpoints for managing user notifications and preferences.

use actix_web::{web, HttpMessage, HttpRequest, HttpResponse};
use chrono::{DateTime, Utc};

use crate::handlers::{errors, helpers};
use serde::Deserialize;

use crate::middleware::request_context::RequestContext;
use crate::models::{Claims, WorkspaceRole};
use crate::services::notifications::{
    NotificationChannel, NotificationFrequency, NotificationService, NotificationTypeCode,
};
use crate::utils::rbac::require_workspace_role;

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

/// Request body for snoozing notifications until a given time.
#[derive(Debug, Deserialize)]
pub struct SnoozeRequest {
    pub notification_ids: Vec<i32>,
    /// ISO-8601 instant; the items stay hidden from the active inbox
    /// until this time, then auto-unsnooze.
    pub until: DateTime<Utc>,
}

/// Request body for updating a preference cell. `frequency` is
/// `instant` | `digest` | `off` (replaces the former `enabled` bool). For
/// backward compatibility with any client still sending `enabled`, a missing
/// `frequency` falls back to it (`true → instant`, `false → off`).
#[derive(Debug, Deserialize)]
pub struct UpdatePreferenceRequest {
    pub notification_type: String,
    pub channel: String,
    #[serde(default)]
    pub frequency: Option<String>,
    #[serde(default)]
    pub enabled: Option<bool>,
}

/// Notification routes, mounted inside the authenticated `/api` scope in main.rs.
pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.route("/notifications", web::get().to(get_notifications))
        .route("/notifications/count", web::get().to(get_unread_count))
        .route(
            "/notifications/unseen-count",
            web::get().to(get_unseen_count),
        )
        .route("/notifications/seen", web::post().to(mark_all_seen))
        .route(
            "/notifications/unread",
            web::post().to(mark_notifications_unread),
        )
        .route(
            "/notifications/archive",
            web::post().to(archive_notifications),
        )
        .route(
            "/notifications/unarchive",
            web::post().to(unarchive_notifications),
        )
        .route(
            "/notifications/snooze",
            web::post().to(snooze_notifications),
        )
        .route(
            "/notifications/read",
            web::post().to(mark_notifications_read),
        )
        .route(
            "/notifications/read-all",
            web::post().to(mark_all_notifications_read),
        )
        .route("/notifications/preferences", web::get().to(get_preferences))
        .route(
            "/notifications/preferences",
            web::put().to(update_preference),
        )
        // Workspace-admin defaults (the middle inheritance layer). Admin-gated.
        .route(
            "/admin/notification-defaults",
            web::get().to(get_workspace_notification_defaults),
        )
        .route(
            "/admin/notification-defaults",
            web::put().to(update_workspace_notification_default),
        )
        .route(
            "/notifications/delete",
            web::post().to(delete_notifications),
        );
}

/// Current caller's workspace (from the actor context pinned by auth middleware).
fn actor_workspace_id(req: &HttpRequest) -> Option<i32> {
    req.extensions()
        .get::<RequestContext>()
        .map(|c| c.actor.workspace_id)
        .unwrap_or(None)
}

/// Request body for setting a workspace notification default cell.
#[derive(Debug, Deserialize)]
pub struct UpdateWorkspaceDefaultRequest {
    pub notification_type: String,
    pub channel: String,
    pub frequency: String,
    #[serde(default)]
    pub locked: bool,
}

/// GET /api/admin/notification-defaults — the workspace's default matrix.
/// Admin-only (manages workspace settings).
pub async fn get_workspace_notification_defaults(
    req: HttpRequest,
    notification_service: web::Data<NotificationService>,
) -> HttpResponse {
    if let Err(e) = require_workspace_role(&req, WorkspaceRole::Admin) {
        return e;
    }
    let Some(workspace_id) = actor_workspace_id(&req) else {
        return errors::unauthorized("Authentication required");
    };

    match notification_service
        .preferences()
        .get_workspace_defaults(workspace_id)
        .await
    {
        Ok(defaults) => HttpResponse::Ok().json(defaults),
        Err(e) => HttpResponse::InternalServerError().json(serde_json::json!({ "error": e })),
    }
}

/// PUT /api/admin/notification-defaults — set one workspace default cell.
/// Admin-only.
pub async fn update_workspace_notification_default(
    req: HttpRequest,
    notification_service: web::Data<NotificationService>,
    body: web::Json<UpdateWorkspaceDefaultRequest>,
) -> HttpResponse {
    if let Err(e) = require_workspace_role(&req, WorkspaceRole::Admin) {
        return e;
    }
    let Some(workspace_id) = actor_workspace_id(&req) else {
        return errors::unauthorized("Authentication required");
    };

    let notification_type = match NotificationTypeCode::from_str(&body.notification_type) {
        Some(t) => t,
        None => {
            return errors::bad_request(format!(
                "Invalid notification type: {}",
                body.notification_type
            ))
        }
    };
    let channel = match NotificationChannel::from_str(&body.channel) {
        Some(c) => c,
        None => return errors::bad_request(format!("Invalid channel: {}", body.channel)),
    };
    let frequency = match NotificationFrequency::from_str(&body.frequency) {
        Some(f) => f,
        None => return errors::bad_request(format!("Invalid frequency: {}", body.frequency)),
    };

    match notification_service
        .preferences()
        .set_workspace_default(
            workspace_id,
            &notification_type,
            channel,
            frequency,
            body.locked,
        )
        .await
    {
        Ok(_) => HttpResponse::Ok().json(serde_json::json!({ "success": true })),
        Err(e) => HttpResponse::InternalServerError().json(serde_json::json!({ "error": e })),
    }
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
        Err(_) => return errors::bad_request("Invalid user UUID"),
    };

    let limit = helpers::clamp_limit(query.limit);
    let offset = helpers::clamp_offset(query.offset);
    let unread_only = query.unread_only.unwrap_or(false);

    let result = if unread_only {
        notification_service.get_unread(&user_uuid, limit).await
    } else {
        notification_service
            .get_all(&user_uuid, limit, offset)
            .await
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
        Err(_) => return errors::bad_request("Invalid user UUID"),
    };

    match notification_service.get_unread_count(&user_uuid).await {
        Ok(count) => HttpResponse::Ok().json(serde_json::json!({ "count": count })),
        Err(e) => HttpResponse::InternalServerError().json(serde_json::json!({
            "error": e
        })),
    }
}

/// Get unseen notification count (drives the bell badge; unlike the
/// unread count, opening the panel clears this without marking items
/// read).
///
/// GET /api/notifications/unseen-count
pub async fn get_unseen_count(
    req: HttpRequest,
    notification_service: web::Data<NotificationService>,
) -> HttpResponse {
    let claims = match req.extensions().get::<Claims>() {
        Some(c) => c.clone(),
        None => return HttpResponse::Unauthorized().finish(),
    };

    let user_uuid = match uuid::Uuid::parse_str(&claims.sub) {
        Ok(u) => u,
        Err(_) => return errors::bad_request("Invalid user UUID"),
    };

    match notification_service.get_unseen_count(&user_uuid).await {
        Ok(count) => HttpResponse::Ok().json(serde_json::json!({ "count": count })),
        Err(e) => HttpResponse::InternalServerError().json(serde_json::json!({
            "error": e
        })),
    }
}

/// Mark all of the user's notifications as seen (badge clear on
/// panel/inbox open).
///
/// POST /api/notifications/seen
pub async fn mark_all_seen(
    req: HttpRequest,
    notification_service: web::Data<NotificationService>,
) -> HttpResponse {
    let claims = match req.extensions().get::<Claims>() {
        Some(c) => c.clone(),
        None => return HttpResponse::Unauthorized().finish(),
    };

    let user_uuid = match uuid::Uuid::parse_str(&claims.sub) {
        Ok(u) => u,
        Err(_) => return errors::bad_request("Invalid user UUID"),
    };

    match notification_service.mark_all_seen(&user_uuid).await {
        Ok(count) => HttpResponse::Ok().json(serde_json::json!({
            "success": true,
            "count": count
        })),
        Err(e) => HttpResponse::InternalServerError().json(serde_json::json!({
            "error": e
        })),
    }
}

/// Mark notifications unread (inverse of read)
///
/// POST /api/notifications/unread
pub async fn mark_notifications_unread(
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
        Err(_) => return errors::bad_request("Invalid user UUID"),
    };

    match notification_service
        .mark_unread(&user_uuid, &body.notification_ids)
        .await
    {
        Ok(count) => HttpResponse::Ok().json(serde_json::json!({
            "success": true,
            "count": count
        })),
        Err(e) => HttpResponse::InternalServerError().json(serde_json::json!({ "error": e })),
    }
}

/// Archive notifications (reversible; hides from the active inbox)
///
/// POST /api/notifications/archive
pub async fn archive_notifications(
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
        Err(_) => return errors::bad_request("Invalid user UUID"),
    };

    match notification_service
        .set_archived(&user_uuid, &body.notification_ids, true)
        .await
    {
        Ok(count) => HttpResponse::Ok().json(serde_json::json!({
            "success": true,
            "count": count
        })),
        Err(e) => HttpResponse::InternalServerError().json(serde_json::json!({ "error": e })),
    }
}

/// Unarchive notifications (restore to the active inbox)
///
/// POST /api/notifications/unarchive
pub async fn unarchive_notifications(
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
        Err(_) => return errors::bad_request("Invalid user UUID"),
    };

    match notification_service
        .set_archived(&user_uuid, &body.notification_ids, false)
        .await
    {
        Ok(count) => HttpResponse::Ok().json(serde_json::json!({
            "success": true,
            "count": count
        })),
        Err(e) => HttpResponse::InternalServerError().json(serde_json::json!({ "error": e })),
    }
}

/// Snooze notifications until a given time (hides them from the active
/// inbox until then; they auto-unsnooze).
///
/// POST /api/notifications/snooze
pub async fn snooze_notifications(
    req: HttpRequest,
    notification_service: web::Data<NotificationService>,
    body: web::Json<SnoozeRequest>,
) -> HttpResponse {
    let claims = match req.extensions().get::<Claims>() {
        Some(c) => c.clone(),
        None => return HttpResponse::Unauthorized().finish(),
    };

    let user_uuid = match uuid::Uuid::parse_str(&claims.sub) {
        Ok(u) => u,
        Err(_) => return errors::bad_request("Invalid user UUID"),
    };

    match notification_service
        .snooze(&user_uuid, &body.notification_ids, body.until.naive_utc())
        .await
    {
        Ok(count) => HttpResponse::Ok().json(serde_json::json!({
            "success": true,
            "count": count
        })),
        Err(e) => HttpResponse::InternalServerError().json(serde_json::json!({ "error": e })),
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
        Err(_) => return errors::bad_request("Invalid user UUID"),
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
        Err(_) => return errors::bad_request("Invalid user UUID"),
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
        Err(_) => return errors::bad_request("Invalid user UUID"),
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
        Err(_) => return errors::bad_request("Invalid user UUID"),
    };

    let notification_type = match NotificationTypeCode::from_str(&body.notification_type) {
        Some(t) => t,
        None => {
            return errors::bad_request(format!(
                "Invalid notification type: {}",
                body.notification_type
            ))
        }
    };

    let channel = match NotificationChannel::from_str(&body.channel) {
        Some(c) => c,
        None => return errors::bad_request(format!("Invalid channel: {}", body.channel)),
    };

    // Prefer `frequency`; fall back to a legacy `enabled` bool if that's all the
    // client sent. Reject an unrecognised frequency string.
    let frequency = match body.frequency.as_deref() {
        Some(f) => match NotificationFrequency::from_str(f) {
            Some(freq) => freq,
            None => return errors::bad_request(format!("Invalid frequency: {f}")),
        },
        None => match body.enabled {
            Some(true) => NotificationFrequency::Instant,
            Some(false) => NotificationFrequency::Off,
            None => {
                return errors::bad_request("Missing `frequency` (instant|digest|off)");
            }
        },
    };

    match notification_service
        .preferences()
        .set_preference(&user_uuid, &notification_type, channel, frequency)
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
        Err(_) => return errors::bad_request("Invalid user UUID"),
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
