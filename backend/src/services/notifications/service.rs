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
use crate::models::{NewNotification, Notification, NotificationResponse, SyncAggregate, SyncOp};
use crate::sync::emit::{self, SyncEmit};

use super::channels::{ChannelError, NotificationDeliveryChannel};
use super::preferences::PreferenceService;
use super::types::{
    DeliverableNotification, NotificationChannel, NotificationEntity, NotificationEvent,
    NotificationPayload,
};

/// Interrupt burst-cap window (seconds). A recipient who already received
/// `INTERRUPT_BURST_LIMIT` interrupting notifications within this window has
/// further interrupts downgraded to quiet (bell only) until it drains.
const INTERRUPT_BURST_WINDOW_SECS: i64 = 60;
/// Max interrupting notifications per recipient per window before downgrade.
const INTERRUPT_BURST_LIMIT: i64 = 5;

/// Earned-interrupt window (days): the lookback over which a recipient's
/// engagement with a notification kind is measured.
const EARNED_INTERRUPT_WINDOW_DAYS: i64 = 30;
/// Minimum SEEN notifications of a kind before its interrupt right can be
/// revoked — below this there isn't enough signal to auto-tune.
const EARNED_INTERRUPT_MIN_SEEN: i64 = 12;
/// A kind keeps its interrupt right while the recipient reads at least this
/// fraction of the ones they saw; below it, the kind has stopped earning it.
const EARNED_INTERRUPT_MIN_READ_RATE: f64 = 0.15;

/// Whether a kind's interrupt right is revoked purely from the seen/read
/// signal (before the explicit-preference veto). Extracted for unit testing.
fn earned_interrupt_ignored(seen: i64, read: i64) -> bool {
    seen >= EARNED_INTERRUPT_MIN_SEEN
        && (read as f64) < (seen as f64) * EARNED_INTERRUPT_MIN_READ_RATE
}

/// Central notification service that orchestrates notification creation and delivery
pub struct NotificationService {
    pool: Pool,
    /// Channels use std::sync::RwLock since operations are quick and don't need async
    channels: RwLock<HashMap<NotificationChannel, Arc<dyn NotificationDeliveryChannel>>>,
    preference_service: Arc<PreferenceService>,
    /// Cache: notification_type_code -> notification_type_id (uses tokio RwLock for async access)
    type_id_cache: Arc<TokioRwLock<HashMap<String, i32>>>,
}

impl NotificationService {
    pub fn new(pool: Pool, type_id_cache: Arc<TokioRwLock<HashMap<String, i32>>>) -> Self {
        let preference_service = Arc::new(PreferenceService::new(pool.clone()));
        Self {
            pool,
            channels: RwLock::new(HashMap::new()),
            preference_service,
            type_id_cache,
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

        // 1. Which channels deliver this type immediately for the recipient, in
        //    this notification's workspace (resolves user + workspace-admin +
        //    system inheritance).
        let enabled_channels = self
            .preference_service
            .get_enabled_channels(
                &payload.recipient_uuid,
                payload.workspace_id,
                &payload.notification_type,
            )
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

        // 3. Persist notification to database. The in-app payload carries whether
        //    the client should interrupt (resolved in-app frequency == instant)
        //    vs land quietly in the bell.
        let mut interrupts = self
            .preference_service
            .in_app_interrupts(
                &payload.recipient_uuid,
                payload.workspace_id,
                &payload.notification_type,
            )
            .await?;

        // Origin filter: a user can opt to be interrupted only by human-
        // originated events, so a non-human actor (scheduled jobs, rule
        // automations) lands quietly in the bell. Only checked when the
        // notification would otherwise interrupt AND the actor is non-human,
        // keeping it off the hot path for ordinary human notifications.
        if interrupts
            && payload.actor.kind != crate::sync::ActorKind::User
            && self
                .preference_service
                .interrupt_human_only(&payload.recipient_uuid)
                .await?
        {
            interrupts = false;
        }

        // Burst cap: if this would interrupt but the recipient has already been
        // interrupted `INTERRUPT_BURST_LIMIT` times in the last window, downgrade
        // to quiet — it still lands in the bell, it just doesn't add another
        // toast to a storm. Counts only interrupting rows, so a flood of quiet
        // notifications never suppresses a genuine one.
        if interrupts
            && self
                .interrupt_burst_exceeded(&payload.recipient_uuid, payload.workspace_id)
                .await?
        {
            interrupts = false;
        }

        // Earned interrupts: a kind keeps its right to interrupt only while the
        // recipient engages with it. If they consistently see-but-don't-read
        // this kind, downgrade to quiet (still in the bell). Explicit prefs win;
        // toast-only users are never counted (see `earned_interrupt_ok`).
        if interrupts
            && !self
                .earned_interrupt_ok(
                    &payload.recipient_uuid,
                    payload.workspace_id,
                    payload.notification_type.as_str(),
                )
                .await?
        {
            interrupts = false;
        }
        let notification_id = self
            .persist_notification(&payload, &deliverable_channels, interrupts)
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
                    channels
                        .get(channel_type)
                        .map(|channel| (*channel_type, channel.clone()))
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
                        .mark_channel_delivered(
                            notification_id,
                            deliverable.payload.workspace_id,
                            channel_type,
                        )
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

    /// Whether the recipient has already been interrupted at least
    /// `INTERRUPT_BURST_LIMIT` times within the last `INTERRUPT_BURST_WINDOW_SECS`
    /// in this workspace. Counts persisted `interrupts = true` rows (the bell
    /// stays complete regardless). Pinned to the entity's workspace so it runs
    /// under RLS like the persist path.
    async fn interrupt_burst_exceeded(
        &self,
        recipient_uuid: &Uuid,
        workspace_id: i32,
    ) -> Result<bool, String> {
        use crate::schema::notifications;

        let recipient = *recipient_uuid;
        let cutoff =
            Utc::now().naive_utc() - chrono::Duration::seconds(INTERRUPT_BURST_WINDOW_SECS);
        let count = crate::sync::session::run_in_workspace(
            &self.pool,
            "background:interrupt_burst_count",
            workspace_id,
            move |conn| {
                notifications::table
                    .filter(notifications::user_uuid.eq(recipient))
                    .filter(notifications::interrupts.eq(true))
                    .filter(notifications::created_at.gt(cutoff))
                    .count()
                    .get_result::<i64>(conn)
            },
        )
        .map_err(|e| format!("Failed to count recent interrupts: {e}"))?;

        Ok(count >= INTERRUPT_BURST_LIMIT)
    }

    /// Whether this notification's interrupt right still stands under earned
    /// interrupts. A kind keeps interrupting only while the recipient engages
    /// with it: if, over the recent window, they SAW at least
    /// `EARNED_INTERRUPT_MIN_SEEN` notifications of the kind in this workspace
    /// but READ fewer than `EARNED_INTERRUPT_MIN_READ_RATE` of them, the kind
    /// has stopped earning its interrupt and this one lands quietly in the bell.
    ///
    /// Safeguards:
    /// - The denominator is SEEN notifications (`seen_at` is stamped only when
    ///   the recipient opens the bell), so a user who lives off toasts and never
    ///   opens the bell is never counted and never muted.
    /// - An explicit in-app preference for the kind always wins — auto-tuning
    ///   only touches recipients who are on the default.
    /// - It only ever downgrades to quiet (the bell keeps the row) and recovers
    ///   on its own once the recipient starts engaging again.
    ///
    /// Runs on the interrupting minority path only; the read-rate scan is
    /// bounded by the window + the `idx_notifications_user_created` index. If it
    /// ever gets hot, materialise the per-(user, kind) verdict in a daily sweep
    /// instead of scanning per send.
    async fn earned_interrupt_ok(
        &self,
        recipient_uuid: &Uuid,
        workspace_id: i32,
        type_code: &str,
    ) -> Result<bool, String> {
        let type_id = self.get_notification_type_id(type_code).await?;
        let recipient = *recipient_uuid;
        let cutoff = Utc::now().naive_utc() - chrono::Duration::days(EARNED_INTERRUPT_WINDOW_DAYS);

        // Per-workspace engagement, RLS-scoped like the persist path.
        let (seen, read) = crate::sync::session::run_in_workspace(
            &self.pool,
            "background:earned_interrupt_engagement",
            workspace_id,
            move |conn| {
                use crate::schema::notifications as n;
                let seen: i64 = n::table
                    .filter(n::user_uuid.eq(recipient))
                    .filter(n::notification_type_id.eq(type_id))
                    .filter(n::seen_at.is_not_null())
                    .filter(n::created_at.gt(cutoff))
                    .count()
                    .get_result(conn)?;
                let read: i64 = n::table
                    .filter(n::user_uuid.eq(recipient))
                    .filter(n::notification_type_id.eq(type_id))
                    .filter(n::seen_at.is_not_null())
                    .filter(n::created_at.gt(cutoff))
                    .filter(n::is_read.eq(true))
                    .count()
                    .get_result(conn)?;
                Ok((seen, read))
            },
        )
        .map_err(|e| format!("Failed to measure notification engagement: {e}"))?;

        // Not enough signal, or a healthy read share → the interrupt stands.
        if !earned_interrupt_ignored(seen, read) {
            return Ok(true);
        }

        // Strong ignore signal: revoke the interrupt UNLESS the recipient set an
        // explicit in-app preference for this kind (respect their choice).
        // cross-tenant: notification_preferences is global per user (unique key excludes workspace_id).
        let has_explicit = crate::sync::session::background_run(
            &self.pool,
            "background:earned_interrupt_explicit_pref",
            move |conn| {
                use crate::schema::notification_preferences as np;
                diesel::select(diesel::dsl::exists(
                    np::table
                        .filter(np::user_uuid.eq(recipient))
                        .filter(np::notification_type_id.eq(type_id))
                        .filter(np::channel.eq("in_app")),
                ))
                .get_result::<bool>(conn)
            },
        )
        .map_err(|e| format!("Failed to check explicit notification preference: {e}"))?;

        Ok(has_explicit)
    }

    /// Persist notification to database
    async fn persist_notification(
        &self,
        payload: &NotificationPayload,
        channels: &[NotificationChannel],
        interrupts: bool,
    ) -> Result<i32, String> {
        use crate::schema::notifications;

        let type_id = self
            .get_notification_type_id(payload.notification_type.as_str())
            .await?;

        // Merge entity-specific metadata for navigation purposes
        let mut metadata = payload.metadata.clone();
        if let serde_json::Value::Object(ref mut map) = metadata {
            match &payload.entity {
                NotificationEntity::DocumentationPage { id, slug, .. } => {
                    map.insert("page_id".to_string(), serde_json::json!(id));
                    map.insert("slug".to_string(), serde_json::json!(slug));
                }
                NotificationEntity::Asset { id, name } => {
                    map.insert("asset_id".to_string(), serde_json::json!(id));
                    map.insert("asset_name".to_string(), serde_json::json!(name));
                }
                _ => {
                    map.insert(
                        "ticket_id".to_string(),
                        serde_json::json!(payload.entity.ticket_id()),
                    );
                }
            }
        } else {
            metadata = match &payload.entity {
                NotificationEntity::DocumentationPage { id, slug, .. } => {
                    serde_json::json!({
                        "page_id": id,
                        "slug": slug
                    })
                }
                NotificationEntity::Asset { id, name } => {
                    serde_json::json!({
                        "asset_id": id,
                        "asset_name": name,
                    })
                }
                _ => {
                    serde_json::json!({
                        "ticket_id": payload.entity.ticket_id()
                    })
                }
            };
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
            interrupts,
        };

        // The notification sync emit IS the in-app delivery, so gate it
        // on the in-app channel being deliverable, matching the old
        // InAppChannel.deliver. A user with in-app disabled (but e.g.
        // email on) gets the persisted row but no live toast / bell
        // update, exactly as before.
        let emit_in_app = channels
            .iter()
            .any(|c| matches!(c, NotificationChannel::InApp));

        // notifications is RLS-enabled and notifications.workspace_id
        // defaults from app.workspace_id; this service is a background
        // dispatcher with no request-bound pin. Pin the entity's workspace
        // (carried on the payload from the call site) so the insert + sync
        // emit write the right workspace_id instead of NULL. Entity-derived,
        // not recipient-derived: a recipient can belong to several
        // workspaces, and the notification belongs to the one its entity
        // lives in.
        let notification: Notification = crate::sync::session::run_in_workspace(
            &self.pool,
            "background:notification_persist",
            payload.workspace_id,
            |conn| {
                let notification: Notification = diesel::insert_into(notifications::table)
                    .values(&new_notification)
                    .get_result(conn)?;

                if emit_in_app {
                    // Emit a sync_actions row in the same transaction so the
                    // notification reaches the recipient's clients on every
                    // backend machine (cross-machine via Postgres NOTIFY),
                    // not just the one that created it. Scoped to the
                    // recipient's private `user:<uuid>` group so no other
                    // user can see it. workspace_id resolves from the pinned
                    // app.workspace_id (the recipient's workspace), same as
                    // the insert above.
                    //
                    // The data is the full NotificationEvent so the client
                    // can render the toast straight from the sync row.
                    // Mirrors `NotificationEvent::from(&DeliverableNotification)`.
                    let event = NotificationEvent {
                        id: notification.uuid,
                        notification_type: payload.notification_type.as_str().to_string(),
                        title: payload.title.clone(),
                        body: payload.body.clone(),
                        entity_type: payload.entity.entity_type().to_string(),
                        entity_id: payload.entity.entity_id(),
                        ticket_id: payload.entity.ticket_id(),
                        actor: payload.actor.clone(),
                        metadata: payload.metadata.clone(),
                        timestamp: payload.created_at,
                        interrupts,
                    };
                    emit::record(
                        conn,
                        SyncEmit {
                            aggregate: SyncAggregate::Notification,
                            aggregate_id: notification.id.to_string(),
                            op: SyncOp::Insert,
                            event_type: "notification.created",
                            data: serde_json::to_value(&event).unwrap_or_default(),
                            groups: vec![format!("user:{}", payload.recipient_uuid)],
                            causation_id: None,
                        },
                    )?;
                }

                Ok(notification)
            },
        )
        .map_err(|e| format!("Failed to persist notification: {e}"))?;

        Ok(notification.id)
    }

    /// Mark a channel as having delivered the notification
    async fn mark_channel_delivered(
        &self,
        notification_id: i32,
        workspace_id_val: i32,
        channel: NotificationChannel,
    ) -> Result<(), String> {
        use crate::schema::notifications::dsl::*;

        // Scoped to the notification's workspace (RLS on notifications), matching
        // the create and read paths.
        crate::sync::session::run_in_workspace(
            &self.pool,
            "background:notification_channel_delivered",
            workspace_id_val,
            |conn| {
                // Get current channels_delivered
                let current: Notification = notifications.find(notification_id).first(conn)?;

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

                diesel::update(notifications.find(notification_id))
                    .set(channels_delivered.eq(serde_json::json!(delivered)))
                    .execute(conn)?;
                Ok::<_, diesel::result::Error>(())
            },
        )
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
        use crate::schema::notification_types::dsl::{code, id as id_col, notification_types};

        // notification_types is non-tenant (system catalog) so a
        // straight pool.get + query would work without bypass —
        // but routing through background_run keeps every method
        // here uniform and the role baseline reset that set_actor
        // performs is harmless for non-tenant reads.
        // cross-tenant: notification_types is a global system catalog (no workspace).
        let type_id: i32 = crate::sync::session::background_run(
            &self.pool,
            "background:notification_type_lookup",
            |conn| {
                notification_types
                    .filter(code.eq(type_code))
                    .select(id_col)
                    .first(conn)
            },
        )
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
        workspace_id_val: i32,
        limit: i64,
    ) -> Result<Vec<NotificationResponse>, String> {
        use crate::schema::notification_types;
        use crate::schema::notifications::dsl::*;

        let results: Vec<(Notification, String)> = crate::sync::session::run_in_workspace(
            &self.pool,
            "background:notification_get_unread",
            workspace_id_val,
            |conn| {
                notifications
                    .inner_join(notification_types::table)
                    .filter(user_uuid.eq(user_uuid_val))
                    .filter(is_read.eq(false))
                    // Archived items drop out of the active inbox.
                    .filter(archived_at.is_null())
                    // Snoozed items stay hidden until their time passes
                    // (auto-unsnooze by the read filter, no extra write).
                    .filter(
                        snoozed_until
                            .is_null()
                            .or(snoozed_until.le(Utc::now().naive_utc())),
                    )
                    .order(created_at.desc())
                    .limit(limit)
                    .select((
                        crate::schema::notifications::all_columns,
                        notification_types::code,
                    ))
                    .load(conn)
            },
        )
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
                seen_at: n.seen_at,
                archived_at: n.archived_at,
                snoozed_until: n.snoozed_until,
                created_at: n.created_at,
            })
            .collect())
    }

    /// Get all notifications for a user (with pagination)
    pub async fn get_all(
        &self,
        user_uuid_val: &Uuid,
        workspace_id_val: i32,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<NotificationResponse>, String> {
        use crate::schema::notification_types;
        use crate::schema::notifications::dsl::*;

        let results: Vec<(Notification, String)> = crate::sync::session::run_in_workspace(
            &self.pool,
            "background:notification_get_all",
            workspace_id_val,
            |conn| {
                notifications
                    .inner_join(notification_types::table)
                    .filter(user_uuid.eq(user_uuid_val))
                    // Archived items drop out of the active inbox (they
                    // remain retrievable once an Archived view exists).
                    .filter(archived_at.is_null())
                    // Snoozed items stay hidden until their time passes.
                    .filter(
                        snoozed_until
                            .is_null()
                            .or(snoozed_until.le(Utc::now().naive_utc())),
                    )
                    .order(created_at.desc())
                    .limit(limit)
                    .offset(offset)
                    .select((
                        crate::schema::notifications::all_columns,
                        notification_types::code,
                    ))
                    .load(conn)
            },
        )
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
                seen_at: n.seen_at,
                archived_at: n.archived_at,
                snoozed_until: n.snoozed_until,
                created_at: n.created_at,
            })
            .collect())
    }

    /// Get unread notification count for a user
    pub async fn get_unread_count(
        &self,
        user_uuid_val: &Uuid,
        workspace_id_val: i32,
    ) -> Result<i64, String> {
        use crate::schema::notifications::dsl::*;

        crate::sync::session::run_in_workspace(
            &self.pool,
            "background:notification_unread_count",
            workspace_id_val,
            |conn| {
                notifications
                    .filter(user_uuid.eq(user_uuid_val))
                    .filter(is_read.eq(false))
                    // Archived items leave the active unread set too.
                    .filter(archived_at.is_null())
                    .count()
                    .get_result(conn)
            },
        )
        .map_err(|e| format!("Query failed: {e}"))
    }

    /// Mark notifications as read
    pub async fn mark_read(
        &self,
        user_uuid_val: &Uuid,
        workspace_id_val: i32,
        notification_ids: &[i32],
    ) -> Result<usize, String> {
        use crate::schema::notifications::dsl::*;

        crate::sync::session::run_in_workspace(
            &self.pool,
            "background:notification_mark_read",
            workspace_id_val,
            |conn| {
                diesel::update(
                    notifications
                        .filter(user_uuid.eq(user_uuid_val))
                        .filter(id.eq_any(notification_ids)),
                )
                .set((is_read.eq(true), read_at.eq(Some(Utc::now().naive_utc()))))
                .execute(conn)
            },
        )
        .map_err(|e| format!("Update failed: {e}"))
    }

    /// Mark all notifications as read for a user
    pub async fn mark_all_read(
        &self,
        user_uuid_val: &Uuid,
        workspace_id_val: i32,
    ) -> Result<usize, String> {
        use crate::schema::notifications::dsl::*;

        crate::sync::session::run_in_workspace(
            &self.pool,
            "background:notification_mark_all_read",
            workspace_id_val,
            |conn| {
                diesel::update(
                    notifications
                        .filter(user_uuid.eq(user_uuid_val))
                        .filter(is_read.eq(false)),
                )
                .set((is_read.eq(true), read_at.eq(Some(Utc::now().naive_utc()))))
                .execute(conn)
            },
        )
        .map_err(|e| format!("Update failed: {e}"))
    }

    /// Count unseen, active (not archived) notifications for a user.
    /// Drives the bell badge: opening the panel marks items seen and
    /// clears the badge without marking each one read (seen != read).
    pub async fn get_unseen_count(
        &self,
        user_uuid_val: &Uuid,
        workspace_id_val: i32,
    ) -> Result<i64, String> {
        use crate::schema::notifications::dsl::*;

        crate::sync::session::run_in_workspace(
            &self.pool,
            "background:notification_unseen_count",
            workspace_id_val,
            |conn| {
                notifications
                    .filter(user_uuid.eq(user_uuid_val))
                    .filter(seen_at.is_null())
                    .filter(archived_at.is_null())
                    // Snoozed items don't ping the badge until they surface.
                    .filter(
                        snoozed_until
                            .is_null()
                            .or(snoozed_until.le(Utc::now().naive_utc())),
                    )
                    .count()
                    .get_result(conn)
            },
        )
        .map_err(|e| format!("Query failed: {e}"))
    }

    /// Mark all of a user's unseen notifications as seen (badge clear on
    /// panel/inbox open). Distinct from mark-all-read: seeing clears the
    /// badge; reading is a separate, explicit per-item action.
    pub async fn mark_all_seen(
        &self,
        user_uuid_val: &Uuid,
        workspace_id_val: i32,
    ) -> Result<usize, String> {
        use crate::schema::notifications::dsl::*;

        crate::sync::session::run_in_workspace(
            &self.pool,
            "background:notification_mark_all_seen",
            workspace_id_val,
            |conn| {
                diesel::update(
                    notifications
                        .filter(user_uuid.eq(user_uuid_val))
                        .filter(seen_at.is_null()),
                )
                .set(seen_at.eq(Some(Utc::now().naive_utc())))
                .execute(conn)
            },
        )
        .map_err(|e| format!("Update failed: {e}"))
    }

    /// Mark notifications unread (inverse of mark_read): clears is_read
    /// and read_at so an item returns to the unread set.
    pub async fn mark_unread(
        &self,
        user_uuid_val: &Uuid,
        workspace_id_val: i32,
        notification_ids: &[i32],
    ) -> Result<usize, String> {
        use crate::schema::notifications::dsl::*;

        crate::sync::session::run_in_workspace(
            &self.pool,
            "background:notification_mark_unread",
            workspace_id_val,
            |conn| {
                diesel::update(
                    notifications
                        .filter(user_uuid.eq(user_uuid_val))
                        .filter(id.eq_any(notification_ids)),
                )
                .set((is_read.eq(false), read_at.eq(None::<chrono::NaiveDateTime>)))
                .execute(conn)
            },
        )
        .map_err(|e| format!("Update failed: {e}"))
    }

    /// Archive (or unarchive) notifications: reversible triage that hides
    /// items from the active inbox without deleting them, replacing the
    /// destructive dismiss. `archived = false` restores them.
    pub async fn set_archived(
        &self,
        user_uuid_val: &Uuid,
        workspace_id_val: i32,
        notification_ids: &[i32],
        archived: bool,
    ) -> Result<usize, String> {
        use crate::schema::notifications::dsl::*;

        crate::sync::session::run_in_workspace(
            &self.pool,
            "background:notification_set_archived",
            workspace_id_val,
            |conn| {
                diesel::update(
                    notifications
                        .filter(user_uuid.eq(user_uuid_val))
                        .filter(id.eq_any(notification_ids)),
                )
                .set(archived_at.eq(if archived {
                    Some(Utc::now().naive_utc())
                } else {
                    None::<chrono::NaiveDateTime>
                }))
                .execute(conn)
            },
        )
        .map_err(|e| format!("Update failed: {e}"))
    }

    /// Snooze notifications until a given time: hides them from the
    /// active inbox until `until`, after which the read-side filter
    /// auto-unsnoozes them (no follow-up write needed). Passing a past
    /// timestamp effectively unsnoozes immediately.
    pub async fn snooze(
        &self,
        user_uuid_val: &Uuid,
        workspace_id_val: i32,
        notification_ids: &[i32],
        until: chrono::NaiveDateTime,
    ) -> Result<usize, String> {
        use crate::schema::notifications::dsl::*;

        crate::sync::session::run_in_workspace(
            &self.pool,
            "background:notification_snooze",
            workspace_id_val,
            |conn| {
                diesel::update(
                    notifications
                        .filter(user_uuid.eq(user_uuid_val))
                        .filter(id.eq_any(notification_ids)),
                )
                .set(snoozed_until.eq(Some(until)))
                .execute(conn)
            },
        )
        .map_err(|e| format!("Update failed: {e}"))
    }

    /// Delete multiple notifications for a user
    pub async fn delete_notifications(
        &self,
        user_uuid_val: &Uuid,
        workspace_id_val: i32,
        notification_ids: &[i32],
    ) -> Result<usize, String> {
        use crate::schema::notifications::dsl::*;

        crate::sync::session::run_in_workspace(
            &self.pool,
            "background:notification_delete",
            workspace_id_val,
            |conn| {
                diesel::delete(
                    notifications
                        .filter(user_uuid.eq(user_uuid_val))
                        .filter(id.eq_any(notification_ids)),
                )
                .execute(conn)
            },
        )
        .map_err(|e| format!("Delete failed: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::earned_interrupt_ignored;

    #[test]
    fn insufficient_sample_never_revokes() {
        // Below the minimum seen count there isn't enough signal, even at a 0%
        // read rate.
        assert!(!earned_interrupt_ignored(0, 0));
        assert!(!earned_interrupt_ignored(11, 0));
    }

    #[test]
    fn healthy_read_share_keeps_the_interrupt() {
        // 12 seen, 3 read = 25% ≥ 15% → interrupt stands.
        assert!(!earned_interrupt_ignored(12, 3));
        // All read.
        assert!(!earned_interrupt_ignored(40, 40));
    }

    #[test]
    fn consistent_ignore_revokes() {
        // 20 seen, 1 read = 5% < 15% → revoked (subject to the explicit-pref
        // veto, applied by the caller).
        assert!(earned_interrupt_ignored(20, 1));
        // 12 seen, 0 read.
        assert!(earned_interrupt_ignored(12, 0));
    }

    #[test]
    fn boundary_read_rate_is_inclusive_keep() {
        // Exactly 15% of 20 is 3 reads → kept (>= threshold).
        assert!(!earned_interrupt_ignored(20, 3));
        // 2 of 20 = 10% → revoked.
        assert!(earned_interrupt_ignored(20, 2));
    }
}
