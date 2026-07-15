//! Push delivery channel (mobile/web, APNs/FCM).
//!
//! Provider-agnostic: the concrete sender is a [`PushSender`] wired in a later
//! step. Until one is configured the channel is registered but `is_available`
//! is false, so push preferences are visible in settings yet inert (nothing is
//! silently dropped — `notify()` filters out unavailable channels).
//!
//! **Privacy:** the payload is minimal — a generic title from the notification
//! type + an entity ref for the deep-link, never the ticket subject/body. The
//! app fetches real content after the tap, so Apple/Google/FCM never see
//! customer data.

use async_trait::async_trait;
use std::sync::Arc;

use super::super::types::{DeliverableNotification, NotificationChannel};
use super::{ChannelError, ChannelResult, NotificationDeliveryChannel};
use crate::db::Pool;

/// A minimal, PII-free push payload.
#[derive(Debug, Clone)]
pub struct PushPayload {
    /// Generic, derived from the notification type (e.g. "Assigned to Ticket").
    pub title: String,
    pub notification_type: String,
    pub entity_type: String,
    pub entity_id: i32,
    /// Deep-link target ticket, or 0 for "no ticket link".
    pub ticket_id: i32,
}

/// One device to send to.
#[derive(Debug, Clone)]
pub struct PushTarget {
    pub platform: String,
    pub token: String,
}

/// Sends a push and reports tokens the provider rejected as unregistered (so
/// the channel prunes them). APNs/FCM impls land in a later step;
/// [`NoopPushSender`] is the placeholder.
#[async_trait]
pub trait PushSender: Send + Sync {
    /// True once real credentials are configured — gates the channel.
    fn is_configured(&self) -> bool;
    /// Send `payload` to each target; return permanently-invalid tokens to revoke.
    async fn send(&self, targets: &[PushTarget], payload: &PushPayload) -> Vec<String>;
}

/// Placeholder sender: not configured, sends nothing. Replaced by APNs/FCM.
pub struct NoopPushSender;

#[async_trait]
impl PushSender for NoopPushSender {
    fn is_configured(&self) -> bool {
        false
    }
    async fn send(&self, _targets: &[PushTarget], _payload: &PushPayload) -> Vec<String> {
        Vec::new()
    }
}

/// The push channel.
pub struct PushChannel {
    pool: Pool,
    sender: Arc<dyn PushSender>,
}

impl PushChannel {
    pub fn new(pool: Pool, sender: Arc<dyn PushSender>) -> Self {
        Self { pool, sender }
    }
}

#[async_trait]
impl NotificationDeliveryChannel for PushChannel {
    fn channel_type(&self) -> NotificationChannel {
        NotificationChannel::Push
    }

    fn is_available(&self) -> bool {
        self.sender.is_configured()
    }

    async fn deliver(&self, notification: &DeliverableNotification) -> ChannelResult<()> {
        let recipient = notification.payload.recipient_uuid;

        // Active device tokens, loaded under bypass: this is a background
        // dispatcher, and push targets a user across all their devices, so we
        // don't want RLS to filter by a single workspace pin.
        let targets: Vec<PushTarget> = crate::sync::session::background_run(
            &self.pool,
            "background:push_load_devices",
            |conn| crate::repository::push_devices::active_tokens_for_user(conn, recipient),
        )
        .map_err(|e| ChannelError::DatabaseError(e.to_string()))?
        .into_iter()
        .map(|(platform, token)| PushTarget { platform, token })
        .collect();

        if targets.is_empty() {
            return Ok(());
        }

        let payload = PushPayload {
            title: notification.payload.notification_type.title().to_string(),
            notification_type: notification.payload.notification_type.as_str().to_string(),
            entity_type: notification.payload.entity.entity_type().to_string(),
            entity_id: notification.payload.entity.entity_id(),
            ticket_id: notification.payload.entity.ticket_id(),
        };

        let invalid = self.sender.send(&targets, &payload).await;
        if !invalid.is_empty() {
            let _ = crate::sync::session::background_run(
                &self.pool,
                "background:push_prune_tokens",
                |conn| crate::repository::push_devices::revoke_tokens(conn, &invalid),
            );
        }
        Ok(())
    }
}
