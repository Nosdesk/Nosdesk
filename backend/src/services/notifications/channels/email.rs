//! Email notification channel
//!
//! Delivers notifications via email using the existing SMTP infrastructure.
//! Includes rate limiting to prevent spam for rapid updates.

use async_trait::async_trait;
use chrono::Utc;
use diesel::prelude::*;
use std::sync::Arc;
use uuid::Uuid;

use super::{ChannelError, ChannelResult, NotificationDeliveryChannel};
use crate::db::Pool;
use crate::services::notifications::types::{
    DeliverableNotification, NotificationChannel, NotificationTypeCode,
};
use crate::utils::email::EmailService;

/// Rate limit duration in seconds (5 minutes)
const RATE_LIMIT_SECONDS: i64 = 300;

/// Email notification channel with rate limiting
pub struct EmailChannel {
    email_service: Arc<EmailService>,
    pool: Pool,
    base_url: String,
    app_name: String,
}

impl EmailChannel {
    pub fn new(
        email_service: Arc<EmailService>,
        pool: Pool,
        base_url: String,
        app_name: String,
    ) -> Self {
        Self {
            email_service,
            pool,
            base_url,
            app_name,
        }
    }

    /// Generate email subject based on notification type
    fn generate_subject(&self, notification: &DeliverableNotification) -> String {
        let entity_title = match &notification.payload.entity {
            crate::services::notifications::types::NotificationEntity::Ticket { title, .. } => {
                title.clone()
            }
            crate::services::notifications::types::NotificationEntity::Comment {
                ticket_title,
                ..
            } => ticket_title.clone(),
        };

        match notification.payload.notification_type {
            NotificationTypeCode::TicketAssigned => {
                format!("[{}] You've been assigned: {}", self.app_name, entity_title)
            }
            NotificationTypeCode::TicketStatusChanged => {
                format!("[{}] Status changed: {}", self.app_name, entity_title)
            }
            NotificationTypeCode::CommentAdded => {
                format!("[{}] New comment on: {}", self.app_name, entity_title)
            }
            NotificationTypeCode::Mentioned => {
                format!(
                    "[{}] {} mentioned you",
                    self.app_name, notification.payload.actor.name
                )
            }
            NotificationTypeCode::TicketCreatedRequester => {
                format!("[{}] Ticket created: {}", self.app_name, entity_title)
            }
        }
    }

    /// Generate the ticket URL for the email
    fn generate_ticket_url(&self, notification: &DeliverableNotification) -> String {
        let ticket_id = notification.payload.entity.ticket_id();
        format!("{}/tickets/{}", self.base_url, ticket_id)
    }

    /// Generate email HTML body
    fn generate_html_body(&self, notification: &DeliverableNotification) -> String {
        let ticket_url = self.generate_ticket_url(notification);
        let body_text = notification
            .payload
            .body
            .as_deref()
            .unwrap_or("You have a new notification.");

        format!(
            r#"<!DOCTYPE html>
<html>
<head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
</head>
<body style="font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; max-width: 600px; margin: 0 auto; padding: 20px; background-color: #f5f5f5;">
    <div style="background-color: white; border-radius: 8px; padding: 24px; box-shadow: 0 1px 3px rgba(0,0,0,0.1);">
        <h2 style="color: #1a1a1a; margin: 0 0 16px 0; font-size: 20px;">
            {}
        </h2>
        <p style="color: #4a4a4a; font-size: 16px; line-height: 1.6; margin: 0 0 16px 0;">
            {}
        </p>
        <p style="color: #6a6a6a; font-size: 14px; margin: 0 0 24px 0;">
            <strong>From:</strong> {}
        </p>
        <a href="{}" style="display: inline-block; padding: 12px 24px; background-color: #0066cc; color: white; text-decoration: none; border-radius: 6px; font-weight: 500;">
            View in {}
        </a>
    </div>
    <p style="color: #888; font-size: 12px; text-align: center; margin-top: 16px;">
        You're receiving this because of your notification preferences.
    </p>
</body>
</html>"#,
            notification.payload.title,
            body_text,
            notification.payload.actor.name,
            ticket_url,
            self.app_name
        )
    }

    /// Get the recipient's primary email address
    async fn get_recipient_email(&self, recipient_uuid: &Uuid) -> ChannelResult<String> {
        use crate::schema::user_emails::dsl::{user_emails, user_uuid, email, is_primary};

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
            notification_rate_limits, user_uuid, notification_type_id, entity_type, entity_id,
            last_notified_at,
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

    /// Get notification type ID from code
    async fn get_notification_type_id(&self, type_code: &str) -> ChannelResult<i32> {
        use crate::schema::notification_types::dsl::{notification_types, code, id as id_col};

        let mut conn = self
            .pool
            .get()
            .map_err(|e| ChannelError::DatabaseError(e.to_string()))?;

        notification_types
            .filter(code.eq(type_code))
            .select(id_col)
            .first(&mut conn)
            .map_err(|e| ChannelError::DatabaseError(format!("Notification type not found: {e}")))
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

        let subject = self.generate_subject(notification);
        let body = self.generate_html_body(notification);

        // Get notification type ID for rate limit tracking
        let type_id = self
            .get_notification_type_id(notification.payload.notification_type.as_str())
            .await?;

        // Send email asynchronously
        let email_service = self.email_service.clone();
        let email = recipient_email.clone();
        let subj = subject.clone();
        let html = body.clone();

        tokio::spawn(async move {
            if let Err(e) = email_service.send_html_email(&email, &subj, &html).await {
                tracing::error!(error = ?e, "Failed to send notification email");
            }
        });

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
            notification_rate_limits, user_uuid, notification_type_id, entity_type, entity_id,
            last_notified_at, id as rate_limit_id,
        };
        use crate::schema::notification_types::dsl::{
            notification_types as nt_table, code as nt_code, id as nt_id,
        };

        let mut conn = match self.pool.get() {
            Ok(c) => c,
            Err(_) => return true, // Don't rate limit on DB errors
        };

        // Get notification type ID
        let type_id: i32 = match nt_table
            .filter(nt_code.eq(notification_type))
            .select(nt_id)
            .first(&mut conn)
        {
            Ok(fetched_id) => fetched_id,
            Err(_) => return true,
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
