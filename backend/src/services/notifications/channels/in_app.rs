//! In-app notification channel.
//!
//! Real-time in-app delivery is the `notification` sync aggregate emitted
//! from `NotificationService::persist_notification` (cross-machine via
//! Postgres LISTEN/NOTIFY), gated on this channel being one of the
//! deliverable channels. This channel stays registered so the in-app
//! preference and rate limiting still participate in channel selection;
//! its `deliver` is a no-op because the persist-time emit is the actual
//! delivery.

use async_trait::async_trait;

use super::{ChannelResult, NotificationDeliveryChannel};
use crate::services::notifications::types::{DeliverableNotification, NotificationChannel};

/// In-app channel. Carries no state: delivery happens via the sync emit
/// in `persist_notification`; this type exists for channel selection.
pub struct InAppChannel;

impl InAppChannel {
    pub fn new() -> Self {
        Self
    }
}

impl Default for InAppChannel {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl NotificationDeliveryChannel for InAppChannel {
    fn channel_type(&self) -> NotificationChannel {
        NotificationChannel::InApp
    }

    async fn deliver(&self, _notification: &DeliverableNotification) -> ChannelResult<()> {
        // No-op: in-app delivery is the `notification` sync-action emit in
        // persist_notification, gated on this channel being enabled.
        Ok(())
    }

    fn is_available(&self) -> bool {
        true
    }
}
