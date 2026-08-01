//! Notification preferences service
//!
//! Resolves effective delivery per (user, workspace, type, channel) across the
//! three-layer inheritance and manages both user overrides and admin
//! (workspace) defaults, with an in-memory cache for the hot delivery path.
//!
//! **Inheritance (highest wins):**
//!   1. system default — `notification_types.default_channels` (a listed
//!      channel means `instant`, else `off`)
//!   2. workspace default — `workspace_notification_defaults` (admin-set)
//!   3. user override — `notification_preferences`
//!
//! A `locked` workspace-default cell cannot be overridden by the user
//! (mandatory notifications). See `docs/notification-preferences-and-push-design`.

use chrono::Utc;
use diesel::prelude::*;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::db::Pool;
use crate::models::{
    NotificationPreferenceResponse, NotificationType as NotificationTypeModel,
    WorkspaceNotificationDefaultResponse,
};

use super::types::{NotificationChannel, NotificationFrequency, NotificationTypeCode};

/// The channels the resolver considers (and the settings matrix exposes). Push
/// defaults to `off` — it's opt-in after the user registers a device + grants
/// OS permission — so it's never in a type's `default_channels`.
const RESOLVED_CHANNELS: [NotificationChannel; 3] = [
    NotificationChannel::InApp,
    NotificationChannel::Email,
    NotificationChannel::Push,
];

/// Manages notification preferences + workspace defaults with caching.
pub struct PreferenceService {
    pool: Pool,
    /// Cache keyed by (user, workspace) → type code → the channels that resolve
    /// to `instant` (i.e. deliver now). Workspace is part of the key because a
    /// user's effective delivery depends on the workspace's admin defaults.
    cache: Arc<
        RwLock<
            HashMap<
                (Uuid, i32),
                HashMap<String, Vec<(NotificationChannel, NotificationFrequency)>>,
            >,
        >,
    >,
}

impl PreferenceService {
    pub fn new(pool: Pool) -> Self {
        Self {
            pool,
            cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Channels to deliver *immediately* (`instant`) for this user, in this
    /// workspace, for this type. `digest` rows are collected later by the digest
    /// batcher; `off` never delivers.
    /// The resolved deliver-now `(channel, frequency)` pairs for this (user,
    /// workspace, type): every channel to deliver immediately. For in-app that is
    /// `instant` OR `quiet` (both land in the bell; they differ only in whether
    /// the client interrupts); for email/push only `instant`. `digest` (collected
    /// later by the batcher) and `off` never appear here. Cached.
    async fn resolve_delivery(
        &self,
        user_uuid: &Uuid,
        workspace_id: i32,
        notification_type: &NotificationTypeCode,
    ) -> Result<Vec<(NotificationChannel, NotificationFrequency)>, String> {
        let type_code = notification_type.as_str().to_string();
        let key = (*user_uuid, workspace_id);

        {
            let cache = self.cache.read().await;
            if let Some(user_prefs) = cache.get(&key) {
                if let Some(pairs) = user_prefs.get(&type_code) {
                    return Ok(pairs.clone());
                }
            }
        }

        let pairs = self
            .load_preferences_from_db(user_uuid, workspace_id, &type_code)
            .await?;

        {
            let mut cache = self.cache.write().await;
            cache
                .entry(key)
                .or_default()
                .insert(type_code, pairs.clone());
        }

        Ok(pairs)
    }

    /// Channels to deliver *now* for this (user, workspace, type). Thin wrapper
    /// over `resolve_delivery` (drops the resolved frequency).
    pub async fn get_enabled_channels(
        &self,
        user_uuid: &Uuid,
        workspace_id: i32,
        notification_type: &NotificationTypeCode,
    ) -> Result<Vec<NotificationChannel>, String> {
        Ok(self
            .resolve_delivery(user_uuid, workspace_id, notification_type)
            .await?
            .into_iter()
            .map(|(channel, _)| channel)
            .collect())
    }

    /// Whether the recipient's resolved IN-APP frequency is `instant` (the client
    /// should interrupt with a toast + desktop notification) vs `quiet` (land in
    /// the bell only). `false` when in-app doesn't deliver at all.
    pub async fn in_app_interrupts(
        &self,
        user_uuid: &Uuid,
        workspace_id: i32,
        notification_type: &NotificationTypeCode,
    ) -> Result<bool, String> {
        Ok(self
            .resolve_delivery(user_uuid, workspace_id, notification_type)
            .await?
            .into_iter()
            .any(|(channel, freq)| {
                channel == NotificationChannel::InApp && freq == NotificationFrequency::Instant
            }))
    }

    /// Resolve the `instant` channels for one (user, workspace, type) across the
    /// full inheritance.
    async fn load_preferences_from_db(
        &self,
        user_uuid_val: &Uuid,
        workspace_id_val: i32,
        type_code: &str,
    ) -> Result<Vec<(NotificationChannel, NotificationFrequency)>, String> {
        use crate::schema::{
            notification_preferences as np, notification_types as nt,
            workspace_notification_defaults as wnd,
        };

        // notification_preferences is GLOBAL per user: its unique key is
        // (user_uuid, notification_type_id, channel) with workspace_id excluded,
        // so a user has ONE override row (stamped under their primary workspace)
        // that applies across all their workspaces. Reading it under an RLS pin
        // to some other active workspace would filter that row out (the table
        // FORCEs a workspace_id = app.workspace_id policy) and silently drop the
        // user's prefs. So read under bypass; the per-workspace
        // workspace_notification_defaults is scoped by the explicit workspace_id
        // filter in the query below.
        let (default_channels, type_interrupts, ws_defaults, user_prefs) =
            // cross-tenant: notification_preferences is global per user (unique key excludes workspace_id); wnd is filtered by workspace_id in the query.
            crate::sync::session::background_run(
                &self.pool,
                "background:notification_pref_load",
                |conn| {
                    let (type_id, default_channels, type_interrupts): (
                        i32,
                        serde_json::Value,
                        bool,
                    ) = nt::table
                        .filter(nt::code.eq(type_code))
                        .select((nt::id, nt::default_channels, nt::interrupts))
                        .first(conn)?;
                    let ws_defaults: Vec<(String, String, bool)> = wnd::table
                        .filter(wnd::workspace_id.eq(workspace_id_val))
                        .filter(wnd::notification_type_id.eq(type_id))
                        .select((wnd::channel, wnd::frequency, wnd::locked))
                        .load(conn)
                        .unwrap_or_default();
                    let user_prefs: Vec<(String, bool, Option<String>)> = np::table
                        .filter(np::user_uuid.eq(user_uuid_val))
                        .filter(np::notification_type_id.eq(type_id))
                        .select((np::channel, np::enabled, np::frequency))
                        .load(conn)
                        .unwrap_or_default();
                    Ok::<_, diesel::result::Error>((
                        default_channels,
                        type_interrupts,
                        ws_defaults,
                        user_prefs,
                    ))
                },
            )
            .map_err(|e| format!("Failed to load notification preferences: {e}"))?;

        let system_defaults = self.parse_default_channels(&default_channels);
        let ws_map = Self::ws_default_map(ws_defaults);
        let user_map = Self::user_pref_map(user_prefs);

        Ok(RESOLVED_CHANNELS
            .into_iter()
            .filter_map(|ch| {
                let freq = Self::resolve_channel(
                    &ch,
                    type_interrupts,
                    &system_defaults,
                    &ws_map,
                    &user_map,
                )
                .0;
                // A channel that can't be quiet (email/push) coerces quiet ->
                // instant, so a mis-set quiet still delivers rather than vanishing.
                let freq = if freq == NotificationFrequency::Quiet && !ch.supports_quiet() {
                    NotificationFrequency::Instant
                } else {
                    freq
                };
                // Deliver-now = instant on any channel, or quiet on in-app (bell
                // without an interrupt). digest/off are not delivered here.
                let delivers_now = freq == NotificationFrequency::Instant
                    || (freq == NotificationFrequency::Quiet && ch.supports_quiet());
                delivers_now.then_some((ch, freq))
            })
            .collect())
    }

    /// Effective `(frequency, locked)` for one channel across the inheritance.
    fn resolve_channel(
        channel: &NotificationChannel,
        type_interrupts: bool,
        system_defaults: &[NotificationChannel],
        ws_map: &HashMap<String, (NotificationFrequency, bool)>,
        user_map: &HashMap<String, NotificationFrequency>,
    ) -> (NotificationFrequency, bool) {
        let key = channel.as_str();
        // Base = workspace default if set, else the system default. A listed
        // channel defaults to `instant`, EXCEPT in-app on an informational
        // (non-interrupting) type, which defaults to `quiet` (bell only). A
        // channel not in the default list is `off`.
        let (base, locked) = match ws_map.get(key) {
            Some((freq, locked)) => (*freq, *locked),
            None => {
                let f = if system_defaults.contains(channel) {
                    if channel.supports_quiet() && !type_interrupts {
                        NotificationFrequency::Quiet
                    } else {
                        NotificationFrequency::Instant
                    }
                } else {
                    NotificationFrequency::Off
                };
                (f, false)
            }
        };
        // Locked cells ignore the user override; otherwise the user's row wins.
        let effective = if locked {
            base
        } else {
            user_map.get(key).copied().unwrap_or(base)
        };
        (effective, locked)
    }

    fn ws_default_map(
        rows: Vec<(String, String, bool)>,
    ) -> HashMap<String, (NotificationFrequency, bool)> {
        rows.into_iter()
            .filter_map(|(c, f, l)| NotificationFrequency::from_str(&f).map(|freq| (c, (freq, l))))
            .collect()
    }

    fn user_pref_map(
        rows: Vec<(String, bool, Option<String>)>,
    ) -> HashMap<String, NotificationFrequency> {
        rows.into_iter()
            .map(|(c, en, freq)| (c, NotificationFrequency::from_row(freq.as_deref(), en)))
            .collect()
    }

    /// Parse default channels from the type's JSONB array.
    fn parse_default_channels(&self, defaults: &serde_json::Value) -> Vec<NotificationChannel> {
        defaults
            .as_array()
            .unwrap_or(&vec![])
            .iter()
            .filter_map(|v| v.as_str())
            .filter_map(NotificationChannel::from_str)
            .collect()
    }

    async fn clear_cache(&self) {
        self.cache.write().await.clear();
    }

    /// Update a user preference cell. Dual-writes the new `frequency` and the
    /// legacy `enabled` bool. `digest` coerces to `instant` on channels that
    /// don't batch (in_app/push).
    pub async fn set_preference(
        &self,
        user_uuid_val: &Uuid,
        notification_type: &NotificationTypeCode,
        channel_val: NotificationChannel,
        frequency_val: NotificationFrequency,
    ) -> Result<(), String> {
        use crate::schema::notification_preferences::dsl::*;
        use crate::schema::notification_types;

        // Coerce a frequency the channel can't honour to `instant` (rather than
        // silently dropping delivery): `digest` is email-only, `quiet` is
        // in-app-only.
        let frequency_val = match frequency_val {
            NotificationFrequency::Digest if !channel_val.supports_digest() => {
                NotificationFrequency::Instant
            }
            NotificationFrequency::Quiet if !channel_val.supports_quiet() => {
                NotificationFrequency::Instant
            }
            other => other,
        };
        let enabled_val = frequency_val.to_enabled();

        // notification_preferences carries an audit trigger but has no
        // workspace_id of its own; resolve the user's primary workspace to pin
        // the actor so the trigger's audit_log insert has a workspace_id.
        // cross-tenant: pre-write workspace resolution: only the user is known; the upsert below runs pinned.
        let resolved_workspace = crate::sync::session::background_run(
            &self.pool,
            "background:notification_pref_set",
            |conn| crate::repository::workspaces::primary_workspace_for_user(conn, *user_uuid_val),
        )
        .map_err(|e| format!("Failed to resolve workspace for preference: {e}"))?;

        let mut conn = self
            .pool
            .get()
            .map_err(|e| format!("Failed to acquire connection: {e}"))?;
        let actor = crate::sync::actor::ActorContext::system("background:notification_pref_set")
            .with_workspace(resolved_workspace);
        crate::sync::session::with_actor_bypass_context(&mut conn, &actor, |conn| {
            let type_id: i32 = notification_types::table
                .filter(notification_types::code.eq(notification_type.as_str()))
                .select(notification_types::id)
                .first(conn)?;
            diesel::insert_into(notification_preferences)
                .values((
                    user_uuid.eq(user_uuid_val),
                    notification_type_id.eq(type_id),
                    channel.eq(channel_val.as_str()),
                    enabled.eq(enabled_val),
                    frequency.eq(frequency_val.as_str()),
                    created_at.eq(Utc::now().naive_utc()),
                    updated_at.eq(Utc::now().naive_utc()),
                ))
                .on_conflict((user_uuid, notification_type_id, channel))
                .do_update()
                .set((
                    enabled.eq(enabled_val),
                    frequency.eq(frequency_val.as_str()),
                    updated_at.eq(Utc::now().naive_utc()),
                ))
                .execute(conn)?;
            Ok::<_, diesel::result::Error>(())
        })
        .map_err(|e| format!("Failed to update preference: {e}"))?;

        self.clear_cache().await;
        Ok(())
    }

    /// One-click unsubscribe (RFC 8058): set the email channel to `off` for every
    /// type for this user, so notification mail stops while transactional mail is
    /// unaffected. Preferences are global per user; the resolved workspace only
    /// pins the audit trigger.
    pub async fn disable_all_email(&self, user_uuid_val: &Uuid) -> Result<(), String> {
        // cross-tenant: pre-write workspace resolution: only the user is known; the write below runs pinned.
        let resolved_workspace = crate::sync::session::background_run(
            &self.pool,
            "background:notification_unsubscribe",
            |conn| crate::repository::workspaces::primary_workspace_for_user(conn, *user_uuid_val),
        )
        .map_err(|e| format!("Failed to resolve workspace for unsubscribe: {e}"))?;

        let mut conn = self
            .pool
            .get()
            .map_err(|e| format!("Failed to acquire connection: {e}"))?;
        let actor = crate::sync::actor::ActorContext::system("background:notification_unsubscribe")
            .with_workspace(resolved_workspace);
        crate::sync::session::with_actor_bypass_context(&mut conn, &actor, |conn| {
            diesel::sql_query(
                "INSERT INTO notification_preferences \
                   (user_uuid, notification_type_id, channel, enabled, frequency, created_at, updated_at) \
                 SELECT $1, nt.id, 'email', false, 'off', now(), now() FROM notification_types nt \
                 ON CONFLICT (user_uuid, notification_type_id, channel) \
                 DO UPDATE SET enabled = false, frequency = 'off', updated_at = now()",
            )
            .bind::<diesel::sql_types::Uuid, _>(*user_uuid_val)
            .execute(conn)
        })
        .map_err(|e| format!("Failed to disable email notifications: {e}"))?;

        self.clear_cache().await;
        Ok(())
    }

    /// Full per-type matrix for the USER settings UI — the *effective* frequency
    /// per channel (workspace default as base, user override on top, locked
    /// forced), plus which cells are admin-locked. Resolves the user's primary
    /// workspace so the inherited defaults are correct.
    pub async fn get_all_preferences(
        &self,
        user_uuid_val: &Uuid,
    ) -> Result<Vec<NotificationPreferenceResponse>, String> {
        use crate::schema::{
            notification_preferences as np, notification_types as nt,
            workspace_notification_defaults as wnd,
        };

        // cross-tenant: resolves the user's primary workspace (only the user is known at this call).
        let (types, ws_defaults, user_prefs) = crate::sync::session::background_run(
            &self.pool,
            "background:notification_pref_get_all",
            |conn| {
                let workspace_id_val = crate::repository::workspaces::primary_workspace_for_user(
                    conn,
                    *user_uuid_val,
                )?;
                let types: Vec<NotificationTypeModel> = nt::table.order(nt::id).load(conn)?;
                let ws_defaults: Vec<(i32, String, String, bool)> = wnd::table
                    .filter(wnd::workspace_id.eq(workspace_id_val))
                    .select((
                        wnd::notification_type_id,
                        wnd::channel,
                        wnd::frequency,
                        wnd::locked,
                    ))
                    .load(conn)
                    .unwrap_or_default();
                let user_prefs: Vec<(i32, String, bool, Option<String>)> = np::table
                    .filter(np::user_uuid.eq(user_uuid_val))
                    .select((
                        np::notification_type_id,
                        np::channel,
                        np::enabled,
                        np::frequency,
                    ))
                    .load(conn)
                    .unwrap_or_default();
                Ok::<_, diesel::result::Error>((types, ws_defaults, user_prefs))
            },
        )
        .map_err(|e| format!("Failed to load notification preferences: {e}"))?;

        let mut responses = Vec::new();
        for notif_type in types {
            let system_defaults = self.parse_default_channels(&notif_type.default_channels);
            let ws_map = Self::ws_default_map(
                ws_defaults
                    .iter()
                    .filter(|(tid, _, _, _)| *tid == notif_type.id)
                    .map(|(_, c, f, l)| (c.clone(), f.clone(), *l))
                    .collect(),
            );
            let user_map = Self::user_pref_map(
                user_prefs
                    .iter()
                    .filter(|(tid, _, _, _)| *tid == notif_type.id)
                    .map(|(_, c, en, freq)| (c.clone(), *en, freq.clone()))
                    .collect(),
            );

            let mut channels = HashMap::new();
            let mut frequencies = HashMap::new();
            let mut locked = HashMap::new();
            for ch in RESOLVED_CHANNELS {
                let (freq, is_locked) = Self::resolve_channel(
                    &ch,
                    notif_type.interrupts,
                    &system_defaults,
                    &ws_map,
                    &user_map,
                );
                channels.insert(ch.as_str().to_string(), freq.to_enabled());
                frequencies.insert(ch.as_str().to_string(), freq.as_str().to_string());
                locked.insert(ch.as_str().to_string(), is_locked);
            }

            responses.push(NotificationPreferenceResponse {
                notification_type: notif_type.code,
                notification_name: notif_type.name,
                description: notif_type.description,
                category: notif_type.category,
                channels,
                frequencies,
                locked,
            });
        }

        Ok(responses)
    }

    /// Full per-type matrix for the ADMIN defaults UI — the workspace default
    /// per channel (falling back to the system default) + `locked`.
    pub async fn get_workspace_defaults(
        &self,
        workspace_id_val: i32,
    ) -> Result<Vec<WorkspaceNotificationDefaultResponse>, String> {
        use crate::schema::{notification_types as nt, workspace_notification_defaults as wnd};

        // Pin to the workspace: RLS scopes the workspace-defaults read to it,
        // making the isolation structural rather than resting on the manual
        // workspace_id filter. notification_types is a global catalog (no RLS).
        let (types, ws_defaults) = crate::sync::session::run_in_workspace(
            &self.pool,
            "background:notification_ws_defaults_get",
            workspace_id_val,
            |conn| {
                let types: Vec<NotificationTypeModel> = nt::table.order(nt::id).load(conn)?;
                let ws_defaults: Vec<(i32, String, String, bool)> = wnd::table
                    .filter(wnd::workspace_id.eq(workspace_id_val))
                    .select((
                        wnd::notification_type_id,
                        wnd::channel,
                        wnd::frequency,
                        wnd::locked,
                    ))
                    .load(conn)
                    .unwrap_or_default();
                Ok::<_, diesel::result::Error>((types, ws_defaults))
            },
        )
        .map_err(|e| format!("Failed to load workspace notification defaults: {e}"))?;

        let mut responses = Vec::new();
        for notif_type in types {
            let system_defaults = self.parse_default_channels(&notif_type.default_channels);
            let ws_map = Self::ws_default_map(
                ws_defaults
                    .iter()
                    .filter(|(tid, _, _, _)| *tid == notif_type.id)
                    .map(|(_, c, f, l)| (c.clone(), f.clone(), *l))
                    .collect(),
            );

            let mut frequencies = HashMap::new();
            let mut locked = HashMap::new();
            for ch in RESOLVED_CHANNELS {
                // No user layer here — the workspace default IS the value, else
                // the system default.
                let (freq, is_locked) = match ws_map.get(ch.as_str()) {
                    Some((f, l)) => (*f, *l),
                    None => {
                        let f = if system_defaults.contains(&ch) {
                            NotificationFrequency::Instant
                        } else {
                            NotificationFrequency::Off
                        };
                        (f, false)
                    }
                };
                frequencies.insert(ch.as_str().to_string(), freq.as_str().to_string());
                locked.insert(ch.as_str().to_string(), is_locked);
            }

            responses.push(WorkspaceNotificationDefaultResponse {
                notification_type: notif_type.code,
                notification_name: notif_type.name,
                description: notif_type.description,
                category: notif_type.category,
                frequencies,
                locked,
            });
        }

        Ok(responses)
    }

    /// Set (admin) a workspace default cell. `digest` coerces to `instant` on
    /// non-batching channels. Clears the whole cache (a default change affects
    /// every user in the workspace).
    pub async fn set_workspace_default(
        &self,
        workspace_id_val: i32,
        notification_type: &NotificationTypeCode,
        channel_val: NotificationChannel,
        frequency_val: NotificationFrequency,
        locked_val: bool,
    ) -> Result<(), String> {
        use crate::schema::notification_types;
        use crate::schema::workspace_notification_defaults::dsl::*;

        // Coerce a frequency the channel can't honour to `instant` (rather than
        // silently dropping delivery): `digest` is email-only, `quiet` is
        // in-app-only.
        let frequency_val = match frequency_val {
            NotificationFrequency::Digest if !channel_val.supports_digest() => {
                NotificationFrequency::Instant
            }
            NotificationFrequency::Quiet if !channel_val.supports_quiet() => {
                NotificationFrequency::Instant
            }
            other => other,
        };

        let mut conn = self
            .pool
            .get()
            .map_err(|e| format!("Failed to acquire connection: {e}"))?;
        let actor =
            crate::sync::actor::ActorContext::system("background:notification_ws_default_set")
                .with_workspace(workspace_id_val);
        crate::sync::session::with_actor_bypass_context(&mut conn, &actor, |conn| {
            let type_id: i32 = notification_types::table
                .filter(notification_types::code.eq(notification_type.as_str()))
                .select(notification_types::id)
                .first(conn)?;
            diesel::insert_into(workspace_notification_defaults)
                .values((
                    workspace_id.eq(workspace_id_val),
                    notification_type_id.eq(type_id),
                    channel.eq(channel_val.as_str()),
                    frequency.eq(frequency_val.as_str()),
                    locked.eq(locked_val),
                    created_at.eq(Utc::now().naive_utc()),
                    updated_at.eq(Utc::now().naive_utc()),
                ))
                .on_conflict((workspace_id, notification_type_id, channel))
                .do_update()
                .set((
                    frequency.eq(frequency_val.as_str()),
                    locked.eq(locked_val),
                    updated_at.eq(Utc::now().naive_utc()),
                ))
                .execute(conn)?;
            Ok::<_, diesel::result::Error>(())
        })
        .map_err(|e| format!("Failed to set workspace notification default: {e}"))?;

        self.clear_cache().await;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ch(s: &str) -> NotificationChannel {
        NotificationChannel::from_str(s).unwrap()
    }

    #[test]
    fn resolves_to_system_default_when_no_workspace_or_user() {
        // in_app is a system default (instant); email is not (off).
        let sys = vec![NotificationChannel::InApp];
        let ws = HashMap::new();
        let user = HashMap::new();
        assert_eq!(
            PreferenceService::resolve_channel(&ch("in_app"), true, &sys, &ws, &user),
            (NotificationFrequency::Instant, false)
        );
        assert_eq!(
            PreferenceService::resolve_channel(&ch("email"), true, &sys, &ws, &user),
            (NotificationFrequency::Off, false)
        );
    }

    #[test]
    fn workspace_default_overrides_system() {
        let sys = vec![]; // email off by system
        let mut ws = HashMap::new();
        ws.insert("email".to_string(), (NotificationFrequency::Digest, false));
        let user = HashMap::new();
        assert_eq!(
            PreferenceService::resolve_channel(&ch("email"), true, &sys, &ws, &user),
            (NotificationFrequency::Digest, false)
        );
    }

    #[test]
    fn user_override_wins_unless_locked() {
        let sys = vec![];
        let mut ws = HashMap::new();
        let mut user = HashMap::new();
        user.insert("email".to_string(), NotificationFrequency::Off);

        // Unlocked workspace default → the user's override wins.
        ws.insert("email".to_string(), (NotificationFrequency::Instant, false));
        assert_eq!(
            PreferenceService::resolve_channel(&ch("email"), true, &sys, &ws, &user).0,
            NotificationFrequency::Off
        );

        // Locked workspace default → the user cannot override it.
        ws.insert("email".to_string(), (NotificationFrequency::Instant, true));
        assert_eq!(
            PreferenceService::resolve_channel(&ch("email"), true, &sys, &ws, &user),
            (NotificationFrequency::Instant, true)
        );
    }

    #[test]
    fn in_app_default_follows_type_interrupt_classification() {
        // in_app is a system default. An interrupting type defaults it to
        // instant (toast); an informational type defaults it to quiet (bell
        // only). Email ignores the quiet notion regardless.
        let sys = vec![NotificationChannel::InApp, NotificationChannel::Email];
        let ws = HashMap::new();
        let user = HashMap::new();

        assert_eq!(
            PreferenceService::resolve_channel(&ch("in_app"), true, &sys, &ws, &user).0,
            NotificationFrequency::Instant,
            "interrupting type → in-app instant"
        );
        assert_eq!(
            PreferenceService::resolve_channel(&ch("in_app"), false, &sys, &ws, &user).0,
            NotificationFrequency::Quiet,
            "informational type → in-app quiet"
        );
        // Email has no quiet tier: it stays instant even for an informational
        // (non-interrupting) type.
        assert_eq!(
            PreferenceService::resolve_channel(&ch("email"), false, &sys, &ws, &user).0,
            NotificationFrequency::Instant,
            "email default is instant regardless of interrupt classification"
        );
    }
}
