//! Notification API handlers
//!
//! Endpoints for managing user notifications and preferences.

use actix_web::{web, HttpMessage, HttpRequest, HttpResponse};
use chrono::{DateTime, Utc};

use crate::errors::{self, ApiError};
use crate::handlers::helpers;
use serde::Deserialize;

use crate::db::Pool;
use crate::middleware::request_context::RequestContext;
use crate::models::WorkspaceRole;
use crate::services::notifications::{
    NotificationChannel, NotificationFrequency, NotificationService, NotificationTypeCode,
};
use crate::utils::rbac::{require_auth, require_workspace_role};

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
        // Per-user origin-based interrupt setting ("humans only").
        .route(
            "/notifications/interrupt-preferences",
            web::get().to(get_interrupt_preferences),
        )
        .route(
            "/notifications/interrupt-preferences",
            web::put().to(set_interrupt_preferences),
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
        // Workspace push content level (detailed vs private). Admin-gated.
        .route(
            "/admin/notification-content",
            web::get().to(get_notification_content_level),
        )
        .route(
            "/admin/notification-content",
            web::put().to(set_notification_content_level),
        )
        // Push device registration (per authenticated user).
        .route(
            "/notifications/devices",
            web::post().to(register_push_device),
        )
        .route(
            "/notifications/devices/{token}",
            web::delete().to(unregister_push_device),
        )
        .route(
            "/notifications/delete",
            web::post().to(delete_notifications),
        );
}

/// Request body to register a push device.
#[derive(Debug, Deserialize)]
pub struct RegisterDeviceRequest {
    /// `ios` | `android` | `web`.
    pub platform: String,
    /// The APNs/FCM device token.
    pub token: String,
    #[serde(default)]
    pub app_version: Option<String>,
}

/// POST /api/notifications/devices — register/refresh this user's push device.
pub async fn register_push_device(
    req: HttpRequest,
    pool: web::Data<Pool>,
    notification_service: web::Data<NotificationService>,
    body: web::Json<RegisterDeviceRequest>,
) -> Result<HttpResponse, ApiError> {
    let (user_uuid, workspace_id) = caller(&req)?;
    if !matches!(body.platform.as_str(), "ios" | "android" | "web") {
        return Err(ApiError::BadRequest(format!(
            "Invalid platform: {}",
            body.platform
        )));
    }
    let token = body.token.trim();
    if token.is_empty() {
        return Err(ApiError::BadRequest("Missing device token".into()));
    }

    let mut conn = errors::db_conn(&pool)?;
    let actor =
        crate::sync::actor::ActorContext::user(user_uuid, None).with_workspace(workspace_id);
    let res = crate::sync::session::with_actor_bypass_context(&mut conn, &actor, |conn| {
        crate::repository::push_devices::register(
            conn,
            user_uuid,
            workspace_id,
            &body.platform,
            token,
            body.app_version.as_deref(),
        )?;
        Ok::<(), diesel::result::Error>(())
    });
    match res {
        Ok(_) => {
            // Push's default depends on having a device, so the cached
            // resolution from before this call is now wrong.
            notification_service
                .preferences()
                .invalidate_for_device_change()
                .await;
            Ok(HttpResponse::Ok().json(serde_json::json!({ "success": true })))
        }
        Err(e) => Err(ApiError::Database(e)),
    }
}

/// DELETE /api/notifications/devices/{token} — revoke this user's device token.
pub async fn unregister_push_device(
    req: HttpRequest,
    pool: web::Data<Pool>,
    notification_service: web::Data<NotificationService>,
    path: web::Path<String>,
) -> Result<HttpResponse, ApiError> {
    let (user_uuid, workspace_id) = caller(&req)?;
    let token = path.into_inner();

    let mut conn = errors::db_conn(&pool)?;
    let actor =
        crate::sync::actor::ActorContext::user(user_uuid, None).with_workspace(workspace_id);
    let res = crate::sync::session::with_actor_bypass_context(&mut conn, &actor, |conn| {
        crate::repository::push_devices::revoke(conn, user_uuid, &token)
    });
    match res {
        Ok(_) => {
            // Revoking the last device flips push back to `off` by default.
            notification_service
                .preferences()
                .invalidate_for_device_change()
                .await;
            Ok(HttpResponse::Ok().json(serde_json::json!({ "success": true })))
        }
        Err(e) => Err(ApiError::Database(e)),
    }
}

/// Current caller's workspace (from the actor context pinned by auth middleware).
fn actor_workspace_id(req: &HttpRequest) -> Option<i32> {
    req.extensions()
        .get::<RequestContext>()
        .map(|c| c.actor.workspace_id)
        .unwrap_or(None)
}

/// The authenticated caller's uuid. A signed token whose subject is not a
/// uuid is our bug, not the caller's, hence 500. Preference handlers use
/// this alone: preferences are global per user, not pinned to a workspace.
fn caller_uuid(req: &HttpRequest) -> Result<uuid::Uuid, ApiError> {
    let claims = require_auth(req)?;
    uuid::Uuid::parse_str(&claims.sub).map_err(|_| ApiError::Internal("Invalid user UUID".into()))
}

/// The caller's uuid plus the workspace the request is pinned to, for the
/// inbox handlers, which are per-workspace.
fn caller(req: &HttpRequest) -> Result<(uuid::Uuid, i32), ApiError> {
    let user_uuid = caller_uuid(req)?;
    let workspace_id = actor_workspace_id(req)
        .ok_or_else(|| ApiError::Unauthorized("Authentication required".into()))?;
    Ok((user_uuid, workspace_id))
}

/// The service reports failures as strings that name tables and columns;
/// those go to the log, and the client gets `what` failed.
fn service_failed(what: &'static str) -> impl Fn(String) -> ApiError {
    move |e| {
        tracing::error!(error = %e, "{what}");
        ApiError::Internal(what.into())
    }
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
) -> Result<HttpResponse, ApiError> {
    require_workspace_role(&req, WorkspaceRole::Admin)?;
    let Some(workspace_id) = actor_workspace_id(&req) else {
        return Err(ApiError::Unauthorized("Authentication required".into()));
    };

    match notification_service
        .preferences()
        .get_workspace_defaults(workspace_id)
        .await
    {
        Ok(defaults) => Ok(HttpResponse::Ok().json(defaults)),
        Err(e) => Err(service_failed("Failed to load notification defaults")(e)),
    }
}

/// PUT /api/admin/notification-defaults — set one workspace default cell.
/// Admin-only.
pub async fn update_workspace_notification_default(
    req: HttpRequest,
    notification_service: web::Data<NotificationService>,
    body: web::Json<UpdateWorkspaceDefaultRequest>,
) -> Result<HttpResponse, ApiError> {
    require_workspace_role(&req, WorkspaceRole::Admin)?;
    let Some(workspace_id) = actor_workspace_id(&req) else {
        return Err(ApiError::Unauthorized("Authentication required".into()));
    };

    let notification_type = match NotificationTypeCode::from_str(&body.notification_type) {
        Some(t) => t,
        None => {
            return Err(ApiError::BadRequest(format!(
                "Invalid notification type: {}",
                body.notification_type
            )))
        }
    };
    let channel = match NotificationChannel::from_str(&body.channel) {
        Some(c) => c,
        None => {
            return Err(ApiError::BadRequest(format!(
                "Invalid channel: {}",
                body.channel
            )))
        }
    };
    let frequency = match NotificationFrequency::from_str(&body.frequency) {
        Some(f) => f,
        None => {
            return Err(ApiError::BadRequest(format!(
                "Invalid frequency: {}",
                body.frequency
            )))
        }
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
        Ok(_) => Ok(HttpResponse::Ok().json(serde_json::json!({ "success": true }))),
        Err(e) => Err(service_failed("Failed to update notification default")(e)),
    }
}

/// Request body for setting the workspace push content level.
#[derive(Debug, Deserialize)]
pub struct UpdateContentLevelRequest {
    /// `detailed` (rich context) | `private` ("tap to view").
    pub detail: String,
}

/// GET /api/admin/notification-content — the workspace's push content level.
/// Admin-only.
pub async fn get_notification_content_level(
    req: HttpRequest,
    pool: web::Data<Pool>,
) -> Result<HttpResponse, ApiError> {
    require_workspace_role(&req, WorkspaceRole::Admin)?;
    let Some(workspace_id) = actor_workspace_id(&req) else {
        return Err(ApiError::Unauthorized("Authentication required".into()));
    };
    let mut conn = errors::db_conn(&pool)?;
    match crate::repository::workspaces::get_notification_push_detail(&mut conn, workspace_id) {
        Ok(detailed) => Ok(HttpResponse::Ok().json(serde_json::json!({
            "detail": if detailed { "detailed" } else { "private" }
        }))),
        Err(e) => Err(ApiError::Database(e)),
    }
}

/// PUT /api/admin/notification-content — set the workspace push content level.
/// Admin-only.
pub async fn set_notification_content_level(
    req: HttpRequest,
    pool: web::Data<Pool>,
    body: web::Json<UpdateContentLevelRequest>,
) -> Result<HttpResponse, ApiError> {
    require_workspace_role(&req, WorkspaceRole::Admin)?;
    let detailed = match body.detail.as_str() {
        "detailed" => true,
        "private" => false,
        other => return Err(ApiError::BadRequest(format!("Invalid detail: {other}"))),
    };
    let (user_uuid, workspace_id) = caller(&req)?;
    let mut conn = errors::db_conn(&pool)?;
    let actor =
        crate::sync::actor::ActorContext::user(user_uuid, None).with_workspace(workspace_id);
    let res = crate::sync::session::with_actor_bypass_context(&mut conn, &actor, |conn| {
        crate::repository::workspaces::set_notification_push_detail(conn, workspace_id, detailed)
    });
    match res {
        Ok(_) => Ok(HttpResponse::Ok().json(serde_json::json!({ "success": true }))),
        Err(e) => Err(ApiError::Database(e)),
    }
}

/// Get user's notifications
///
/// GET /api/notifications
pub async fn get_notifications(
    req: HttpRequest,
    notification_service: web::Data<NotificationService>,
    query: web::Query<NotificationQuery>,
) -> Result<HttpResponse, ApiError> {
    let (user_uuid, workspace_id) = caller(&req)?;

    let limit = helpers::clamp_limit(query.limit);
    let offset = helpers::clamp_offset(query.offset);
    let unread_only = query.unread_only.unwrap_or(false);

    let result = if unread_only {
        notification_service
            .get_unread(&user_uuid, workspace_id, limit)
            .await
    } else {
        notification_service
            .get_all(&user_uuid, workspace_id, limit, offset)
            .await
    };

    match result {
        Ok(notifications) => Ok(HttpResponse::Ok().json(notifications)),
        Err(e) => Err(service_failed("Failed to load notifications")(e)),
    }
}

/// Get unread notification count
///
/// GET /api/notifications/count
pub async fn get_unread_count(
    req: HttpRequest,
    notification_service: web::Data<NotificationService>,
) -> Result<HttpResponse, ApiError> {
    let (user_uuid, workspace_id) = caller(&req)?;
    match notification_service
        .get_unread_count(&user_uuid, workspace_id)
        .await
    {
        Ok(count) => Ok(HttpResponse::Ok().json(serde_json::json!({ "count": count }))),
        Err(e) => Err(service_failed("Failed to count unread notifications")(e)),
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
) -> Result<HttpResponse, ApiError> {
    let (user_uuid, workspace_id) = caller(&req)?;
    match notification_service
        .get_unseen_count(&user_uuid, workspace_id)
        .await
    {
        Ok(count) => Ok(HttpResponse::Ok().json(serde_json::json!({ "count": count }))),
        Err(e) => Err(service_failed("Failed to count unseen notifications")(e)),
    }
}

/// Mark all of the user's notifications as seen (badge clear on
/// panel/inbox open).
///
/// POST /api/notifications/seen
pub async fn mark_all_seen(
    req: HttpRequest,
    notification_service: web::Data<NotificationService>,
) -> Result<HttpResponse, ApiError> {
    let (user_uuid, workspace_id) = caller(&req)?;
    match notification_service
        .mark_all_seen(&user_uuid, workspace_id)
        .await
    {
        Ok(count) => Ok(HttpResponse::Ok().json(serde_json::json!({
            "success": true,
            "count": count
        }))),
        Err(e) => Err(service_failed("Failed to mark notifications seen")(e)),
    }
}

/// Mark notifications unread (inverse of read)
///
/// POST /api/notifications/unread
pub async fn mark_notifications_unread(
    req: HttpRequest,
    notification_service: web::Data<NotificationService>,
    body: web::Json<MarkReadRequest>,
) -> Result<HttpResponse, ApiError> {
    let (user_uuid, workspace_id) = caller(&req)?;
    match notification_service
        .mark_unread(&user_uuid, workspace_id, &body.notification_ids)
        .await
    {
        Ok(count) => Ok(HttpResponse::Ok().json(serde_json::json!({
            "success": true,
            "count": count
        }))),
        Err(e) => Err(service_failed("Failed to mark notifications unread")(e)),
    }
}

/// Archive notifications (reversible; hides from the active inbox)
///
/// POST /api/notifications/archive
pub async fn archive_notifications(
    req: HttpRequest,
    notification_service: web::Data<NotificationService>,
    body: web::Json<MarkReadRequest>,
) -> Result<HttpResponse, ApiError> {
    let (user_uuid, workspace_id) = caller(&req)?;
    match notification_service
        .set_archived(&user_uuid, workspace_id, &body.notification_ids, true)
        .await
    {
        Ok(count) => Ok(HttpResponse::Ok().json(serde_json::json!({
            "success": true,
            "count": count
        }))),
        Err(e) => Err(service_failed("Failed to archive notifications")(e)),
    }
}

/// Unarchive notifications (restore to the active inbox)
///
/// POST /api/notifications/unarchive
pub async fn unarchive_notifications(
    req: HttpRequest,
    notification_service: web::Data<NotificationService>,
    body: web::Json<MarkReadRequest>,
) -> Result<HttpResponse, ApiError> {
    let (user_uuid, workspace_id) = caller(&req)?;
    match notification_service
        .set_archived(&user_uuid, workspace_id, &body.notification_ids, false)
        .await
    {
        Ok(count) => Ok(HttpResponse::Ok().json(serde_json::json!({
            "success": true,
            "count": count
        }))),
        Err(e) => Err(service_failed("Failed to unarchive notifications")(e)),
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
) -> Result<HttpResponse, ApiError> {
    let (user_uuid, workspace_id) = caller(&req)?;
    match notification_service
        .snooze(
            &user_uuid,
            workspace_id,
            &body.notification_ids,
            body.until.naive_utc(),
        )
        .await
    {
        Ok(count) => Ok(HttpResponse::Ok().json(serde_json::json!({
            "success": true,
            "count": count
        }))),
        Err(e) => Err(service_failed("Failed to snooze notifications")(e)),
    }
}

/// Mark notifications as read
///
/// POST /api/notifications/read
pub async fn mark_notifications_read(
    req: HttpRequest,
    notification_service: web::Data<NotificationService>,
    body: web::Json<MarkReadRequest>,
) -> Result<HttpResponse, ApiError> {
    let (user_uuid, workspace_id) = caller(&req)?;
    match notification_service
        .mark_read(&user_uuid, workspace_id, &body.notification_ids)
        .await
    {
        Ok(count) => Ok(HttpResponse::Ok().json(serde_json::json!({
            "success": true,
            "count": count
        }))),
        Err(e) => Err(service_failed("Failed to mark notifications read")(e)),
    }
}

/// Mark all notifications as read
///
/// POST /api/notifications/read-all
pub async fn mark_all_notifications_read(
    req: HttpRequest,
    notification_service: web::Data<NotificationService>,
) -> Result<HttpResponse, ApiError> {
    let (user_uuid, workspace_id) = caller(&req)?;
    match notification_service
        .mark_all_read(&user_uuid, workspace_id)
        .await
    {
        Ok(count) => Ok(HttpResponse::Ok().json(serde_json::json!({
            "success": true,
            "count": count
        }))),
        Err(e) => Err(service_failed("Failed to mark notifications read")(e)),
    }
}

/// Get user's notification preferences
///
/// GET /api/notifications/preferences
pub async fn get_preferences(
    req: HttpRequest,
    notification_service: web::Data<NotificationService>,
) -> Result<HttpResponse, ApiError> {
    let user_uuid = caller_uuid(&req)?;

    match notification_service
        .preferences()
        .get_all_preferences(&user_uuid)
        .await
    {
        Ok(prefs) => Ok(HttpResponse::Ok().json(prefs)),
        Err(e) => Err(service_failed("Failed to load notification preferences")(e)),
    }
}

/// Update a notification preference
///
/// PUT /api/notifications/preferences
pub async fn update_preference(
    req: HttpRequest,
    notification_service: web::Data<NotificationService>,
    body: web::Json<UpdatePreferenceRequest>,
) -> Result<HttpResponse, ApiError> {
    let user_uuid = caller_uuid(&req)?;

    let notification_type = match NotificationTypeCode::from_str(&body.notification_type) {
        Some(t) => t,
        None => {
            return Err(ApiError::BadRequest(format!(
                "Invalid notification type: {}",
                body.notification_type
            )));
        }
    };

    let channel = match NotificationChannel::from_str(&body.channel) {
        Some(c) => c,
        None => {
            return Err(ApiError::BadRequest(format!(
                "Invalid channel: {}",
                body.channel
            )))
        }
    };

    // Prefer `frequency`; fall back to a legacy `enabled` bool if that's all the
    // client sent. Reject an unrecognised frequency string.
    let frequency = match body.frequency.as_deref() {
        Some(f) => match NotificationFrequency::from_str(f) {
            Some(freq) => freq,
            None => return Err(ApiError::BadRequest(format!("Invalid frequency: {f}"))),
        },
        None => match body.enabled {
            Some(true) => NotificationFrequency::Instant,
            Some(false) => NotificationFrequency::Off,
            None => {
                return Err(ApiError::BadRequest(
                    "Missing `frequency` (instant|digest|off)".into(),
                ));
            }
        },
    };

    match notification_service
        .preferences()
        .set_preference(&user_uuid, &notification_type, channel, frequency)
        .await
    {
        Ok(_) => Ok(HttpResponse::Ok().json(serde_json::json!({ "success": true }))),
        Err(e) => Err(service_failed("Failed to update notification preference")(
            e,
        )),
    }
}

/// Get the user's origin-based interrupt setting.
///
/// GET /api/notifications/interrupt-preferences → `{ "human_only": bool }`
pub async fn get_interrupt_preferences(
    req: HttpRequest,
    notification_service: web::Data<NotificationService>,
) -> Result<HttpResponse, ApiError> {
    let user_uuid = caller_uuid(&req)?;

    match notification_service
        .preferences()
        .interrupt_human_only(&user_uuid)
        .await
    {
        Ok(human_only) => {
            Ok(HttpResponse::Ok().json(serde_json::json!({ "human_only": human_only })))
        }
        Err(e) => Err(service_failed("Failed to load interrupt preferences")(e)),
    }
}

/// Request body for the origin-based interrupt setting.
#[derive(Debug, Deserialize)]
pub struct InterruptPreferencesRequest {
    /// When true, only human-originated notifications interrupt (toast /
    /// desktop); system/automation-triggered ones land quietly in the bell.
    pub human_only: bool,
}

/// Set the user's origin-based interrupt setting.
///
/// PUT /api/notifications/interrupt-preferences  body: `{ "human_only": bool }`
pub async fn set_interrupt_preferences(
    req: HttpRequest,
    notification_service: web::Data<NotificationService>,
    body: web::Json<InterruptPreferencesRequest>,
) -> Result<HttpResponse, ApiError> {
    let user_uuid = caller_uuid(&req)?;

    match notification_service
        .preferences()
        .set_interrupt_human_only(&user_uuid, body.human_only)
        .await
    {
        Ok(_) => Ok(HttpResponse::Ok().json(serde_json::json!({ "success": true }))),
        Err(e) => Err(service_failed("Failed to update interrupt preferences")(e)),
    }
}

/// Delete notifications
///
/// POST /api/notifications/delete
pub async fn delete_notifications(
    req: HttpRequest,
    notification_service: web::Data<NotificationService>,
    body: web::Json<DeleteNotificationsRequest>,
) -> Result<HttpResponse, ApiError> {
    let (user_uuid, workspace_id) = caller(&req)?;
    match notification_service
        .delete_notifications(&user_uuid, workspace_id, &body.notification_ids)
        .await
    {
        Ok(count) => Ok(HttpResponse::Ok().json(serde_json::json!({
            "success": true,
            "count": count
        }))),
        Err(e) => Err(service_failed("Failed to delete notifications")(e)),
    }
}
