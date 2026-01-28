//! Core notification service
//!
//! Orchestrates notification creation, preference checking, persistence,
//! and delivery to multiple channels.

use chrono::Utc;
use diesel::prelude::*;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use tokio::sync::RwLock as TokioRwLock;
use uuid::Uuid;

use crate::db::Pool;
use crate::models::{NewNotification, Notification, NotificationResponse};

use super::channels::{ChannelError, NotificationDeliveryChannel};
use super::preferences::PreferenceService;
use super::types::{DeliverableNotification, NotificationChannel, NotificationPayload};

/// Central notification service that orchestrates notification creation and delivery
pub struct NotificationService {
    pool: Pool,
    /// Channels use std::sync::RwLock since operations are quick and don't need async
    channels: RwLock<HashMap<NotificationChannel, Arc<dyn NotificationDeliveryChannel>>>,
    preference_service: Arc<PreferenceService>,
    /// Cache: notification_type_code -> notification_type_id (uses tokio RwLock for async access)
    type_id_cache: TokioRwLock<HashMap<String, i32>>,
}

impl NotificationService {
    pub fn new(pool: Pool) -> Self {
        let preference_service = Arc::new(PreferenceService::new(pool.clone()));
        Self {
            pool,
            channels: RwLock::new(HashMap::new()),
            preference_service,
            type_id_cache: TokioRwLock::new(HashMap::new()),
        }
    }

    /// Get reference to the preference service
    pub fn preferences(&self) -> &Arc<PreferenceService> {
        &self.preference_service
    }

    /// Register a delivery channel (synchronous - uses std::sync::RwLock)
    pub fn register_channel(&self, channel: Arc<dyn NotificationDeliveryChannel>) {
        let channel_type = channel.channel_type();
        let mut channels = self.channels.write().expect("RwLock poisoned");
        channels.insert(channel_type, channel);
        tracing::info!(
            channel = ?channel_type,
            "Registered notification channel"
        );
    }

    /// Create and send a notification
    ///
    /// This is the single entry point for all notifications in the system.
    pub async fn notify(&self, payload: NotificationPayload) -> Result<(), String> {
        // Don't notify the actor themselves
        if payload.recipient_uuid == payload.actor.uuid {
            tracing::debug!(
                recipient = %payload.recipient_uuid,
                "Skipping self-notification"
            );
            return Ok(());
        }

        // 1. Check if user should receive this notification type at all
        let enabled_channels = self
            .preference_service
            .get_enabled_channels(&payload.recipient_uuid, &payload.notification_type)
            .await?;

        if enabled_channels.is_empty() {
            tracing::debug!(
                recipient = %payload.recipient_uuid,
                notification_type = ?payload.notification_type,
                "User has disabled all channels for this notification type"
            );
            return Ok(());
        }

        // 2. Filter channels by rate limiting
        // First, collect the channels we need to check (without holding lock across await)
        let channels_to_check: Vec<(NotificationChannel, Arc<dyn NotificationDeliveryChannel>)> = {
            let channels = self.channels.read().expect("RwLock poisoned");
            enabled_channels
                .iter()
                .filter_map(|channel_type| {
                    channels.get(channel_type).and_then(|channel| {
                        if channel.is_available() {
                            Some((*channel_type, channel.clone()))
                        } else {
                            None
                        }
                    })
                })
                .collect()
        };

        // Now check rate limits without holding the lock
        let mut deliverable_channels = Vec::new();
        for (channel_type, channel) in channels_to_check {
            let should_send = channel
                .check_rate_limit(
                    &payload.recipient_uuid,
                    payload.notification_type.as_str(),
                    payload.entity.entity_type(),
                    payload.entity.entity_id(),
                )
                .await;

            if should_send {
                deliverable_channels.push(channel_type);
            } else {
                tracing::debug!(
                    channel = ?channel_type,
                    recipient = %payload.recipient_uuid,
                    "Rate limited, skipping channel"
                );
            }
        }

        if deliverable_channels.is_empty() {
            tracing::debug!(
                recipient = %payload.recipient_uuid,
                "All channels rate limited or unavailable"
            );
            return Ok(());
        }

        // 3. Persist notification to database
        let notification_id = self
            .persist_notification(&payload, &deliverable_channels)
            .await?;

        // 4. Create deliverable notification
        let deliverable = DeliverableNotification {
            id: Some(notification_id),
            uuid: Uuid::now_v7(),
            payload,
            channels: deliverable_channels.clone(),
        };

        // 5. Deliver to each enabled channel
        // Collect channels to deliver to (without holding lock across await)
        let channels_to_deliver: Vec<(NotificationChannel, Arc<dyn NotificationDeliveryChannel>)> = {
            let channels = self.channels.read().expect("RwLock poisoned");
            deliverable_channels
                .iter()
                .filter_map(|channel_type| {
                    channels.get(channel_type).map(|channel| (*channel_type, channel.clone()))
                })
                .collect()
        };

        for (channel_type, channel) in channels_to_deliver {
            match channel.deliver(&deliverable).await {
                Ok(_) => {
                    tracing::debug!(
                        channel = ?channel_type,
                        notification_id,
                        "Delivered notification"
                    );
                    // Mark channel as delivered
                    if let Err(e) = self
                        .mark_channel_delivered(notification_id, channel_type)
                        .await
                    {
                        tracing::warn!(error = %e, "Failed to mark channel as delivered");
                    }
                }
                Err(ChannelError::RateLimited) => {
                    tracing::debug!(
                        channel = ?channel_type,
                        "Rate limited during delivery"
                    );
                }
                Err(e) => {
                    tracing::warn!(
                        channel = ?channel_type,
                        error = ?e,
                        "Failed to deliver notification"
                    );
                }
            }
        }

        Ok(())
    }

    /// Persist notification to database
    async fn persist_notification(
        &self,
        payload: &NotificationPayload,
        _channels: &[NotificationChannel],
    ) -> Result<i32, String> {
        use crate::schema::notifications;

        let mut conn = self
            .pool
            .get()
            .map_err(|e| format!("Database error: {e}"))?;

        let type_id = self
            .get_notification_type_id(payload.notification_type.as_str())
            .await?;

        // Merge ticket_id into metadata for navigation purposes
        let mut metadata = payload.metadata.clone();
        if let serde_json::Value::Object(ref mut map) = metadata {
            map.insert("ticket_id".to_string(), serde_json::json!(payload.entity.ticket_id()));
        } else {
            metadata = serde_json::json!({
                "ticket_id": payload.entity.ticket_id()
            });
        }

        let new_notification = NewNotification {
            uuid: Uuid::now_v7(),
            user_uuid: payload.recipient_uuid,
            notification_type_id: type_id,
            entity_type: payload.entity.entity_type().to_string(),
            entity_id: payload.entity.entity_id(),
            title: payload.title.clone(),
            body: payload.body.clone(),
            metadata: Some(metadata),
            channels_delivered: serde_json::json!([]),
        };

        let notification: Notification = diesel::insert_into(notifications::table)
            .values(&new_notification)
            .get_result(&mut conn)
            .map_err(|e| format!("Failed to persist notification: {e}"))?;

        Ok(notification.id)
    }

    /// Mark a channel as having delivered the notification
    async fn mark_channel_delivered(
        &self,
        notification_id: i32,
        channel: NotificationChannel,
    ) -> Result<(), String> {
        use crate::schema::notifications::dsl::*;

        let mut conn = self
            .pool
            .get()
            .map_err(|e| format!("Database error: {e}"))?;

        // Get current channels_delivered
        let current: Notification = notifications
            .find(notification_id)
            .first(&mut conn)
            .map_err(|e| format!("Notification not found: {e}"))?;

        // Add new channel to the array
        let mut delivered: Vec<String> = current
            .channels_delivered
            .as_array()
            .unwrap_or(&vec![])
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect();

        let channel_str = channel.as_str().to_string();
        if !delivered.contains(&channel_str) {
            delivered.push(channel_str);
        }

        // Update
        diesel::update(notifications.find(notification_id))
            .set(channels_delivered.eq(serde_json::json!(delivered)))
            .execute(&mut conn)
            .map_err(|e| format!("Failed to update channels_delivered: {e}"))?;

        Ok(())
    }

    /// Get notification type ID from code (with caching)
    async fn get_notification_type_id(&self, type_code: &str) -> Result<i32, String> {
        // Check cache
        {
            let cache = self.type_id_cache.read().await;
            if let Some(cached_id) = cache.get(type_code) {
                return Ok(*cached_id);
            }
        }

        // Query database
        use crate::schema::notification_types::dsl::{notification_types, code, id as id_col};

        let mut conn = self
            .pool
            .get()
            .map_err(|e| format!("Database error: {e}"))?;

        let type_id: i32 = notification_types
            .filter(code.eq(type_code))
            .select(id_col)
            .first(&mut conn)
            .map_err(|e| format!("Notification type '{type_code}' not found: {e}"))?;

        // Update cache
        {
            let mut cache = self.type_id_cache.write().await;
            cache.insert(type_code.to_string(), type_id);
        }

        Ok(type_id)
    }

    /// Get unread notifications for a user
    pub async fn get_unread(
        &self,
        user_uuid_val: &Uuid,
        limit: i64,
    ) -> Result<Vec<NotificationResponse>, String> {
        use crate::schema::notification_types;
        use crate::schema::notifications::dsl::*;

        let mut conn = self
            .pool
            .get()
            .map_err(|e| format!("Database error: {e}"))?;

        let results: Vec<(Notification, String)> = notifications
            .inner_join(notification_types::table)
            .filter(user_uuid.eq(user_uuid_val))
            .filter(is_read.eq(false))
            .order(created_at.desc())
            .limit(limit)
            .select((
                crate::schema::notifications::all_columns,
                notification_types::code,
            ))
            .load(&mut conn)
            .map_err(|e| format!("Query failed: {e}"))?;

        Ok(results
            .into_iter()
            .map(|(n, type_code)| NotificationResponse {
                id: n.id,
                uuid: n.uuid,
                notification_type: type_code,
                entity_type: n.entity_type,
                entity_id: n.entity_id,
                title: n.title,
                body: n.body,
                metadata: n.metadata,
                is_read: n.is_read,
                created_at: n.created_at,
            })
            .collect())
    }

    /// Get all notifications for a user (with pagination)
    pub async fn get_all(
        &self,
        user_uuid_val: &Uuid,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<NotificationResponse>, String> {
        use crate::schema::notification_types;
        use crate::schema::notifications::dsl::*;

        let mut conn = self
            .pool
            .get()
            .map_err(|e| format!("Database error: {e}"))?;

        let results: Vec<(Notification, String)> = notifications
            .inner_join(notification_types::table)
            .filter(user_uuid.eq(user_uuid_val))
            .order(created_at.desc())
            .limit(limit)
            .offset(offset)
            .select((
                crate::schema::notifications::all_columns,
                notification_types::code,
            ))
            .load(&mut conn)
            .map_err(|e| format!("Query failed: {e}"))?;

        Ok(results
            .into_iter()
            .map(|(n, type_code)| NotificationResponse {
                id: n.id,
                uuid: n.uuid,
                notification_type: type_code,
                entity_type: n.entity_type,
                entity_id: n.entity_id,
                title: n.title,
                body: n.body,
                metadata: n.metadata,
                is_read: n.is_read,
                created_at: n.created_at,
            })
            .collect())
    }

    /// Get unread notification count for a user
    pub async fn get_unread_count(&self, user_uuid_val: &Uuid) -> Result<i64, String> {
        use crate::schema::notifications::dsl::*;

        let mut conn = self
            .pool
            .get()
            .map_err(|e| format!("Database error: {e}"))?;

        notifications
            .filter(user_uuid.eq(user_uuid_val))
            .filter(is_read.eq(false))
            .count()
            .get_result(&mut conn)
            .map_err(|e| format!("Query failed: {e}"))
    }

    /// Mark notifications as read
    pub async fn mark_read(
        &self,
        user_uuid_val: &Uuid,
        notification_ids: &[i32],
    ) -> Result<usize, String> {
        use crate::schema::notifications::dsl::*;

        let mut conn = self
            .pool
            .get()
            .map_err(|e| format!("Database error: {e}"))?;

        let count = diesel::update(
            notifications
                .filter(user_uuid.eq(user_uuid_val))
                .filter(id.eq_any(notification_ids)),
        )
        .set((is_read.eq(true), read_at.eq(Some(Utc::now().naive_utc()))))
        .execute(&mut conn)
        .map_err(|e| format!("Update failed: {e}"))?;

        Ok(count)
    }

    /// Mark all notifications as read for a user
    pub async fn mark_all_read(&self, user_uuid_val: &Uuid) -> Result<usize, String> {
        use crate::schema::notifications::dsl::*;

        let mut conn = self
            .pool
            .get()
            .map_err(|e| format!("Database error: {e}"))?;

        let count = diesel::update(
            notifications
                .filter(user_uuid.eq(user_uuid_val))
                .filter(is_read.eq(false)),
        )
        .set((is_read.eq(true), read_at.eq(Some(Utc::now().naive_utc()))))
        .execute(&mut conn)
        .map_err(|e| format!("Update failed: {e}"))?;

        Ok(count)
    }

    /// Delete a notification for a user
    #[allow(dead_code)]
    pub async fn delete_notification(
        &self,
        user_uuid_val: &Uuid,
        notification_id: i32,
    ) -> Result<bool, String> {
        use crate::schema::notifications::dsl::*;

        let mut conn = self
            .pool
            .get()
            .map_err(|e| format!("Database error: {e}"))?;

        let count = diesel::delete(
            notifications
                .filter(user_uuid.eq(user_uuid_val))
                .filter(id.eq(notification_id)),
        )
        .execute(&mut conn)
        .map_err(|e| format!("Delete failed: {e}"))?;

        Ok(count > 0)
    }

    /// Delete multiple notifications for a user
    pub async fn delete_notifications(
        &self,
        user_uuid_val: &Uuid,
        notification_ids: &[i32],
    ) -> Result<usize, String> {
        use crate::schema::notifications::dsl::*;

        let mut conn = self
            .pool
            .get()
            .map_err(|e| format!("Database error: {e}"))?;

        let count = diesel::delete(
            notifications
                .filter(user_uuid.eq(user_uuid_val))
                .filter(id.eq_any(notification_ids)),
        )
        .execute(&mut conn)
        .map_err(|e| format!("Delete failed: {e}"))?;

        Ok(count)
    }
}
