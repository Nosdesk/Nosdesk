//! Notification preferences service
//!
//! Manages user notification preferences with caching for performance.

use chrono::Utc;
use diesel::prelude::*;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::db::Pool;
use crate::models::{NotificationPreferenceResponse, NotificationType as NotificationTypeModel};

use super::types::{NotificationChannel, NotificationTypeCode};

/// Manages user notification preferences with caching
pub struct PreferenceService {
    pool: Pool,
    /// Cache: user_uuid -> (notification_type_code -> Vec<enabled_channels>)
    cache: Arc<RwLock<HashMap<Uuid, HashMap<String, Vec<NotificationChannel>>>>>,
}

impl PreferenceService {
    pub fn new(pool: Pool) -> Self {
        Self {
            pool,
            cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Get enabled channels for a user and notification type
    pub async fn get_enabled_channels(
        &self,
        user_uuid: &Uuid,
        notification_type: &NotificationTypeCode,
    ) -> Result<Vec<NotificationChannel>, String> {
        let type_code = notification_type.as_str().to_string();

        // Check cache first
        {
            let cache = self.cache.read().await;
            if let Some(user_prefs) = cache.get(user_uuid) {
                if let Some(channels) = user_prefs.get(&type_code) {
                    return Ok(channels.clone());
                }
            }
        }

        // Query database
        let channels = self
            .load_preferences_from_db(user_uuid, &type_code)
            .await?;

        // Update cache
        {
            let mut cache = self.cache.write().await;
            cache
                .entry(*user_uuid)
                .or_insert_with(HashMap::new)
                .insert(type_code, channels.clone());
        }

        Ok(channels)
    }

    /// Load preferences from database, falling back to defaults
    async fn load_preferences_from_db(
        &self,
        user_uuid_val: &Uuid,
        type_code: &str,
    ) -> Result<Vec<NotificationChannel>, String> {
        use crate::schema::notification_preferences::dsl::*;
        use crate::schema::notification_types;

        let mut conn = self.pool.get().map_err(|e| format!("Database error: {e}"))?;

        // Get notification type ID and defaults
        let type_info: (i32, serde_json::Value) = notification_types::table
            .filter(notification_types::code.eq(type_code))
            .select((notification_types::id, notification_types::default_channels))
            .first(&mut conn)
            .map_err(|e| format!("Notification type not found: {e}"))?;

        let (type_id, default_channels) = type_info;

        // Get user preferences for this type
        let prefs: Vec<(String, bool)> = notification_preferences
            .filter(user_uuid.eq(user_uuid_val))
            .filter(notification_type_id.eq(type_id))
            .select((channel, enabled))
            .load(&mut conn)
            .unwrap_or_default();

        // If no preferences, use defaults from notification_types table
        if prefs.is_empty() {
            return Ok(self.parse_default_channels(&default_channels));
        }

        // Return enabled channels
        Ok(prefs
            .into_iter()
            .filter(|(_, is_enabled)| *is_enabled)
            .filter_map(|(chan, _)| NotificationChannel::from_str(&chan))
            .collect())
    }

    /// Parse default channels from JSONB value
    fn parse_default_channels(&self, defaults: &serde_json::Value) -> Vec<NotificationChannel> {
        defaults
            .as_array()
            .unwrap_or(&vec![])
            .iter()
            .filter_map(|v| v.as_str())
            .filter_map(NotificationChannel::from_str)
            .collect()
    }

    /// Update user preference (and invalidate cache)
    pub async fn set_preference(
        &self,
        user_uuid_val: &Uuid,
        notification_type: &NotificationTypeCode,
        channel_val: NotificationChannel,
        enabled_val: bool,
    ) -> Result<(), String> {
        use crate::schema::notification_preferences::dsl::*;
        use crate::schema::notification_types;

        let mut conn = self.pool.get().map_err(|e| format!("Database error: {e}"))?;

        // Get notification type ID
        let type_id: i32 = notification_types::table
            .filter(notification_types::code.eq(notification_type.as_str()))
            .select(notification_types::id)
            .first(&mut conn)
            .map_err(|e| format!("Notification type not found: {e}"))?;

        // Upsert preference
        diesel::insert_into(notification_preferences)
            .values((
                user_uuid.eq(user_uuid_val),
                notification_type_id.eq(type_id),
                channel.eq(channel_val.as_str()),
                enabled.eq(enabled_val),
                created_at.eq(Utc::now().naive_utc()),
                updated_at.eq(Utc::now().naive_utc()),
            ))
            .on_conflict((user_uuid, notification_type_id, channel))
            .do_update()
            .set((
                enabled.eq(enabled_val),
                updated_at.eq(Utc::now().naive_utc()),
            ))
            .execute(&mut conn)
            .map_err(|e| format!("Failed to update preference: {e}"))?;

        // Invalidate cache for this user
        {
            let mut cache = self.cache.write().await;
            cache.remove(user_uuid_val);
        }

        Ok(())
    }

    /// Get all preferences for a user (for settings UI)
    pub async fn get_all_preferences(
        &self,
        user_uuid_val: &Uuid,
    ) -> Result<Vec<NotificationPreferenceResponse>, String> {
        use crate::schema::notification_preferences::dsl::*;
        use crate::schema::notification_types;

        let mut conn = self.pool.get().map_err(|e| format!("Database error: {e}"))?;

        // Get all notification types
        let types: Vec<NotificationTypeModel> = notification_types::table
            .order(notification_types::id)
            .load(&mut conn)
            .map_err(|e| format!("Failed to load notification types: {e}"))?;

        // Get all user preferences
        let user_prefs: Vec<(i32, String, bool)> = notification_preferences
            .filter(user_uuid.eq(user_uuid_val))
            .select((notification_type_id, channel, enabled))
            .load(&mut conn)
            .unwrap_or_default();

        // Build response grouped by notification type
        let mut responses = Vec::new();
        for notif_type in types {
            let mut channels_map: HashMap<String, bool> = HashMap::new();

            // Parse default channels
            let defaults = self.parse_default_channels(&notif_type.default_channels);

            // Set defaults
            for chan in &[NotificationChannel::InApp, NotificationChannel::Email] {
                channels_map.insert(chan.as_str().to_string(), defaults.contains(chan));
            }

            // Override with user preferences
            for (type_id, chan, is_enabled) in &user_prefs {
                if *type_id == notif_type.id {
                    channels_map.insert(chan.clone(), *is_enabled);
                }
            }

            responses.push(NotificationPreferenceResponse {
                notification_type: notif_type.code,
                notification_name: notif_type.name,
                description: notif_type.description,
                category: notif_type.category,
                channels: channels_map,
            });
        }

        Ok(responses)
    }

    /// Invalidate entire cache (useful for testing or admin operations)
    #[allow(dead_code)]
    pub async fn invalidate_cache(&self) {
        let mut cache = self.cache.write().await;
        cache.clear();
    }
}
