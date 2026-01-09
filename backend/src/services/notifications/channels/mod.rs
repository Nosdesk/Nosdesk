//! Notification delivery channels
//!
//! This module defines the trait for notification delivery channels
//! and provides implementations for in-app (SSE) and email channels.

pub mod in_app;
pub mod email;

use async_trait::async_trait;
use std::fmt;
use uuid::Uuid;

use super::types::{DeliverableNotification, NotificationChannel};

/// Result type for channel delivery operations
pub type ChannelResult<T> = Result<T, ChannelError>;

/// Errors that can occur during channel delivery
#[derive(Debug)]
pub enum ChannelError {
    /// Delivery failed with reason
    DeliveryFailed(String),
    /// User has been rate limited for this channel
    RateLimited,
    /// Channel is not configured/available
    ChannelDisabled,
    /// Invalid recipient (e.g., no email address)
    InvalidRecipient(String),
    /// Database error
    DatabaseError(String),
}

impl fmt::Display for ChannelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DeliveryFailed(msg) => write!(f, "Delivery failed: {}", msg),
            Self::RateLimited => write!(f, "Rate limited"),
            Self::ChannelDisabled => write!(f, "Channel disabled"),
            Self::InvalidRecipient(msg) => write!(f, "Invalid recipient: {}", msg),
            Self::DatabaseError(msg) => write!(f, "Database error: {}", msg),
        }
    }
}

impl std::error::Error for ChannelError {}

/// Trait for notification delivery channels
///
/// Implementing this trait allows adding new delivery channels
/// without modifying the core notification service.
#[async_trait]
pub trait NotificationDeliveryChannel: Send + Sync {
    /// Returns the channel type this implementation handles
    fn channel_type(&self) -> NotificationChannel;

    /// Deliver a single notification
    async fn deliver(&self, notification: &DeliverableNotification) -> ChannelResult<()>;

    /// Check if channel is available/configured
    fn is_available(&self) -> bool;

    /// Check rate limiting for this channel/user/entity
    /// Returns true if the notification should be sent (not rate limited)
    async fn check_rate_limit(
        &self,
        _user_uuid: &Uuid,
        _notification_type: &str,
        _entity_type: &str,
        _entity_id: i32,
    ) -> bool {
        true // Default: no rate limiting
    }
}
