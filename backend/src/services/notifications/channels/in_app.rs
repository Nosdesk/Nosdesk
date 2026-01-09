//! In-app notification channel via SSE
//!
//! Delivers notifications to the frontend in real-time using the existing
//! Server-Sent Events infrastructure.

use async_trait::async_trait;
use std::sync::Arc;

use super::{ChannelError, ChannelResult, NotificationDeliveryChannel};
use crate::handlers::sse::SseState;
use crate::services::notifications::types::{
    DeliverableNotification, NotificationChannel, NotificationEvent,
};

/// In-app notification channel that broadcasts via SSE
pub struct InAppChannel {
    sse_state: Arc<SseState>,
}

impl InAppChannel {
    pub fn new(sse_state: Arc<SseState>) -> Self {
        Self { sse_state }
    }
}

#[async_trait]
impl NotificationDeliveryChannel for InAppChannel {
    fn channel_type(&self) -> NotificationChannel {
        NotificationChannel::InApp
    }

    async fn deliver(&self, notification: &DeliverableNotification) -> ChannelResult<()> {
        let event: NotificationEvent = notification.into();
        let recipient_uuid = notification.payload.recipient_uuid.to_string();

        // Broadcast the notification via SSE
        // The frontend will filter to show only to the target recipient
        self.sse_state
            .broadcast_notification(recipient_uuid, event)
            .await;

        Ok(())
    }

    fn is_available(&self) -> bool {
        true // SSE is always available
    }
}
