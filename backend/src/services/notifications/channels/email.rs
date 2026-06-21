//! Email notification channel
//!
//! Delivers notifications via email using the existing SMTP infrastructure.
//! Includes rate limiting to prevent spam for rapid updates.

use async_trait::async_trait;
use chrono::Utc;
use diesel::prelude::*;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock as TokioRwLock;
use uuid::Uuid;

use super::{ChannelError, ChannelResult, NotificationDeliveryChannel};
use crate::db::Pool;
use crate::services::notifications::types::{
    DeliverableNotification, NotificationChannel, NotificationTypeCode,
};
use crate::utils::email::EmailService;
use crate::utils::email_branding::get_email_branding;

/// Rate limit duration in seconds (5 minutes)
const RATE_LIMIT_SECONDS: i64 = 300;

/// Email notification channel with rate limiting
pub struct EmailChannel {
    email_service: Arc<EmailService>,
    pool: Pool,
    base_url: String,
    /// Shared cache: notification_type_code -> notification_type_id
    type_id_cache: Arc<TokioRwLock<HashMap<String, i32>>>,
}

impl EmailChannel {
    pub fn new(
        email_service: Arc<EmailService>,
        pool: Pool,
        base_url: String,
        type_id_cache: Arc<TokioRwLock<HashMap<String, i32>>>,
    ) -> Self {
        Self {
            email_service,
            pool,
            base_url,
            type_id_cache,
        }
    }

    /// Generate email subject based on notification type. The
    /// recipient's effective locale picks the catalogue; bracketed
    /// `$app` plus an entity field interpolate the rest.
    fn generate_subject(
        &self,
        notification: &DeliverableNotification,
        app_name: &str,
        locale: &unic_langid::LanguageIdentifier,
    ) -> String {
        let entity_title = match &notification.payload.entity {
            crate::services::notifications::types::NotificationEntity::Ticket { title, .. } => {
                title.clone()
            }
            crate::services::notifications::types::NotificationEntity::Comment {
                ticket_title,
                ..
            } => ticket_title.clone(),
            crate::services::notifications::types::NotificationEntity::DocumentationPage {
                title,
                ..
            } => title.clone(),
            crate::services::notifications::types::NotificationEntity::Asset { name, .. } => {
                name.clone()
            }
        };

        let key = match notification.payload.notification_type {
            NotificationTypeCode::TicketAssigned => "notif-ticket-assigned",
            NotificationTypeCode::TicketStatusChanged => "notif-ticket-status-changed",
            NotificationTypeCode::CommentAdded => "notif-comment-added",
            NotificationTypeCode::Mentioned => "notif-mentioned",
            NotificationTypeCode::TicketCreatedRequester => "notif-ticket-created-requester",
            NotificationTypeCode::DocPageUpdated => "notif-doc-page-updated",
            NotificationTypeCode::AssetLowStock => "notif-asset-low-stock",
            NotificationTypeCode::SlaBreached => "notif-sla-breached",
            NotificationTypeCode::LoanDueSoon => "notif-loan-due-soon",
            NotificationTypeCode::LoanOverdue => "notif-loan-overdue",
        };

        // Pass every possible arg; Fluent silently ignores unused
        // ones, which keeps the per-variant match above small.
        crate::utils::i18n::tr_with(
            locale,
            key,
            &[
                ("app", app_name.to_string().into()),
                ("title", entity_title.into()),
                ("actor", notification.payload.actor.name.clone().into()),
            ],
        )
    }

    /// Build the deep link for a notification. `base_url` is the recipient's
    /// surface base (resolved per notification by `notification_link_base`):
    /// the agent app for an agent, the per-tenant portal for a customer, so the
    /// link lands where that recipient can actually open the entity.
    fn generate_entity_url(
        &self,
        notification: &DeliverableNotification,
        base_url: &str,
    ) -> String {
        match &notification.payload.entity {
            crate::services::notifications::types::NotificationEntity::DocumentationPage {
                slug,
                ..
            } => {
                format!("{base_url}/documentation/{slug}")
            }
            crate::services::notifications::types::NotificationEntity::Asset { id, .. } => {
                format!("{base_url}/assets/{id}")
            }
            _ => {
                let ticket_id = notification.payload.entity.ticket_id();
                format!("{base_url}/tickets/{ticket_id}")
            }
        }
    }

    /// Get the recipient's primary email address
    async fn get_recipient_email(&self, recipient_uuid: &Uuid) -> ChannelResult<String> {
        use crate::schema::user_emails::dsl::{email, is_primary, user_emails, user_uuid};

        let mut conn = self
            .pool
            .get()
            .map_err(|e| ChannelError::DatabaseError(e.to_string()))?;

        let email_result: Option<String> = user_emails
            .filter(user_uuid.eq(recipient_uuid))
            .filter(is_primary.eq(true))
            .select(email)
            .first(&mut conn)
            .optional()
            .map_err(|e| ChannelError::DatabaseError(e.to_string()))?;

        email_result.ok_or_else(|| {
            ChannelError::InvalidRecipient(format!("No primary email for user {recipient_uuid}"))
        })
    }

    /// Update rate limit tracking
    async fn update_rate_limit(
        &self,
        user_uuid_val: &Uuid,
        type_id: i32,
        entity_type_val: &str,
        entity_id_val: i32,
    ) -> ChannelResult<()> {
        use crate::schema::notification_rate_limits::dsl::{
            entity_id, entity_type, last_notified_at, notification_rate_limits,
            notification_type_id, user_uuid,
        };

        let mut conn = self
            .pool
            .get()
            .map_err(|e| ChannelError::DatabaseError(e.to_string()))?;

        let now = Utc::now().naive_utc();

        // Use InsertableValue to avoid type complexity
        let new_record = crate::models::NewNotificationRateLimit {
            user_uuid: *user_uuid_val,
            notification_type_id: type_id,
            entity_type: entity_type_val.to_string(),
            entity_id: entity_id_val,
        };

        diesel::insert_into(notification_rate_limits)
            .values(&new_record)
            .on_conflict((user_uuid, notification_type_id, entity_type, entity_id))
            .do_update()
            .set(last_notified_at.eq(now))
            .execute(&mut conn)
            .map_err(|e| ChannelError::DatabaseError(e.to_string()))?;

        Ok(())
    }

    /// Get notification type ID from code (with shared cache)
    async fn get_notification_type_id(&self, type_code: &str) -> ChannelResult<i32> {
        // Check cache first
        {
            let cache = self.type_id_cache.read().await;
            if let Some(&cached_id) = cache.get(type_code) {
                return Ok(cached_id);
            }
        }

        // Query database
        use crate::schema::notification_types::dsl::{code, id as id_col, notification_types};

        let mut conn = self
            .pool
            .get()
            .map_err(|e| ChannelError::DatabaseError(e.to_string()))?;

        let type_id: i32 = notification_types
            .filter(code.eq(type_code))
            .select(id_col)
            .first(&mut conn)
            .map_err(|e| {
                ChannelError::DatabaseError(format!("Notification type not found: {e}"))
            })?;

        // Update cache
        {
            let mut cache = self.type_id_cache.write().await;
            cache.insert(type_code.to_string(), type_id);
        }

        Ok(type_id)
    }
}

#[async_trait]
impl NotificationDeliveryChannel for EmailChannel {
    fn channel_type(&self) -> NotificationChannel {
        NotificationChannel::Email
    }

    async fn deliver(&self, notification: &DeliverableNotification) -> ChannelResult<()> {
        // Get recipient email
        let recipient_email = self
            .get_recipient_email(&notification.payload.recipient_uuid)
            .await?;

        // Load the link base, workspace branding, and recipient locale in one
        // connection. Branding feeds every transactional surface (logo, primary
        // color); the locale picks which message catalogue formats the subject.
        // Pinned to the notification's workspace: the branding read is
        // RLS-isolated site_settings, and delivery runs from a background
        // dispatcher with no request context. Without the pin the read
        // falls back to default branding (or, under the hosted role, sees
        // nothing).
        let workspace_id = notification.payload.workspace_id;
        let recipient_uuid = notification.payload.recipient_uuid;
        let fallback_base = self.base_url.clone();
        let (base_url, branding, recipient_locale) = crate::sync::session::run_in_workspace(
            &self.pool,
            "background:notification_email_prep",
            workspace_id,
            move |conn| {
                // Deep links point to the RECIPIENT's surface. In hosted Model
                // C an agent views the entity in the agent app (central origin
                // + slug-in-path); a customer/baseline recipient views it in
                // the per-tenant portal (`<slug>.<NOSDESK_TENANT_DOMAIN>` or a
                // custom domain). Self-host has one origin for everyone.
                let base_url = match crate::repository::workspaces::find_by_id(conn, workspace_id) {
                    Ok(Some(ws)) => {
                        let recipient_is_agent =
                            crate::repository::users::find_active_by_uuid(&recipient_uuid, conn)
                                .map(|u| {
                                    crate::repository::user_helpers::user_can_handle_tickets(
                                        conn, &u,
                                    )
                                })
                                .unwrap_or(false);
                        notification_link_base(
                            crate::middleware::workspace_context::selection_resolution_enabled(),
                            recipient_is_agent,
                            &ws.slug,
                            crate::utils::tenant_origin::workspace_origin(&ws).as_deref(),
                            &fallback_base,
                        )
                    }
                    _ => fallback_base.clone(),
                };
                let branding = get_email_branding(conn, &base_url);
                let locale =
                    crate::repository::user_locale::resolve_effective_locale(conn, recipient_uuid);
                Ok((base_url, branding, locale))
            },
        )
        .map_err(|e| ChannelError::DatabaseError(e.to_string()))?;

        let subject = self.generate_subject(notification, &branding.app_name, &recipient_locale);
        let entity_url = self.generate_entity_url(notification, &base_url);
        let body_text = match notification.payload.body.as_deref() {
            Some(text) if !text.is_empty() => text.to_string(),
            _ => crate::utils::i18n::tr_with(&recipient_locale, "notif-body-fallback", &[]),
        };
        let title = notification.payload.title.clone();
        let actor_name = notification.payload.actor.name.clone();

        // Get notification type ID for rate limit tracking
        let type_id = self
            .get_notification_type_id(notification.payload.notification_type.as_str())
            .await?;

        // Enqueue rather than fire-and-forget. The outbound worker
        // retries with backoff if SMTP burps, honours the suppression
        // list, and respects the circuit breaker. The idempotency
        // key combines the notification uuid with the recipient so
        // retries of the same logical event don't deliver twice but
        // multi-recipient fanout still produces one row per
        // watcher.
        let event_id = notification.uuid.to_string();
        let recipient_uuid_str = notification.payload.recipient_uuid.to_string();
        // Enqueue pinned to the notification's workspace: outbound_emails is
        // workspace-isolated with a workspace_id default from app.workspace_id,
        // so an unpinned insert writes NULL and is rejected.
        let enqueue = crate::sync::session::run_in_workspace(
            &self.pool,
            "background:notification_email_enqueue",
            notification.payload.workspace_id,
            |conn| {
                crate::services::transactional_email::enqueue_notification(
                    conn,
                    self.email_service.as_ref(),
                    &branding,
                    &recipient_email,
                    &subject,
                    &title,
                    &body_text,
                    &actor_name,
                    &entity_url,
                    &event_id,
                    &recipient_uuid_str,
                    &recipient_locale,
                )
            },
        );
        match enqueue {
            Ok(row) => tracing::debug!(
                queue_id = row.id,
                notification_uuid = %notification.uuid,
                recipient = %recipient_email,
                "Notification email enqueued"
            ),
            Err(e) => tracing::error!(
                notification_uuid = %notification.uuid,
                recipient = %recipient_email,
                error = ?e,
                "Failed to enqueue notification email"
            ),
        }

        // Update rate limit tracking
        self.update_rate_limit(
            &notification.payload.recipient_uuid,
            type_id,
            notification.payload.entity.entity_type(),
            notification.payload.entity.entity_id(),
        )
        .await?;

        Ok(())
    }

    fn is_available(&self) -> bool {
        // Email channel is always available if we have an email service
        true
    }

    async fn check_rate_limit(
        &self,
        user_uuid_val: &Uuid,
        notification_type: &str,
        entity_type_val: &str,
        entity_id_val: i32,
    ) -> bool {
        use crate::schema::notification_rate_limits::dsl::{
            entity_id, entity_type, id as rate_limit_id, last_notified_at,
            notification_rate_limits, notification_type_id, user_uuid,
        };

        // Get notification type ID from shared cache
        let type_id: i32 = match self.get_notification_type_id(notification_type).await {
            Ok(id) => id,
            Err(_) => return true, // Don't rate limit on errors
        };

        let mut conn = match self.pool.get() {
            Ok(c) => c,
            Err(_) => return true, // Don't rate limit on DB errors
        };

        // Check if we've sent an email for this entity recently
        let cutoff = Utc::now().naive_utc() - chrono::Duration::seconds(RATE_LIMIT_SECONDS);

        let recent: Option<i32> = notification_rate_limits
            .filter(user_uuid.eq(user_uuid_val))
            .filter(notification_type_id.eq(type_id))
            .filter(entity_type.eq(entity_type_val))
            .filter(entity_id.eq(entity_id_val))
            .filter(last_notified_at.gt(cutoff))
            .select(rate_limit_id)
            .first(&mut conn)
            .optional()
            .unwrap_or(None);

        // Return true if NOT rate limited (no recent notification found)
        recent.is_none()
    }
}

/// Base URL for a notification deep link, chosen by the recipient's surface.
///
/// In hosted Model C the agent app and the customer portal are different
/// origins: an AGENT recipient views the entity in the agent app (the central
/// `agent_origin` with the workspace slug in the path), a customer/baseline
/// recipient in the per-tenant `portal_origin`. Outside selection mode
/// (self-host) there is a single origin for everyone, so role doesn't matter.
fn notification_link_base(
    selection_enabled: bool,
    recipient_is_agent: bool,
    slug: &str,
    portal_origin: Option<&str>,
    agent_origin: &str,
) -> String {
    if selection_enabled && recipient_is_agent {
        format!("{}/{}", agent_origin.trim_end_matches('/'), slug)
    } else {
        portal_origin
            .map(str::to_owned)
            .unwrap_or_else(|| agent_origin.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::notification_link_base;

    #[test]
    fn routes_notification_links_by_recipient_surface() {
        let portal = Some("https://acme.nosdesk.au");
        let agent = "https://app.nosdesk.com";

        // Hosted, agent recipient -> agent app, slug in the path.
        assert_eq!(
            notification_link_base(true, true, "acme", portal, agent),
            "https://app.nosdesk.com/acme"
        );
        // Hosted, customer recipient -> the per-tenant portal origin.
        assert_eq!(
            notification_link_base(true, false, "acme", portal, agent),
            "https://acme.nosdesk.au"
        );
        // Self-host (no selection): single origin, regardless of role.
        assert_eq!(
            notification_link_base(false, true, "default", None, "https://help.example.com"),
            "https://help.example.com"
        );
        assert_eq!(
            notification_link_base(false, false, "default", None, "https://help.example.com"),
            "https://help.example.com"
        );
    }
}
