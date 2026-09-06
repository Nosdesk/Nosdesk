//! Push delivery channel (mobile/web, APNs/FCM).
//!
//! Provider-agnostic: the concrete sender is a [`PushSender`] wired in a later
//! step. Until one is configured the channel is registered but `is_available`
//! is false, so push preferences are visible in settings yet inert (nothing is
//! silently dropped — `notify()` filters out unavailable channels).
//!
//! **Privacy:** the payload carries CONTEXT, never message content. In the
//! workspace's `detailed` mode (admin default): the ticket subject + a "who did
//! what" line (`push_body`) — the same information already in the notification
//! email. In `private` mode: only the generic type label ("tap to view"). A
//! comment's text, etc. is never sent in either mode. Lock-screen exposure is
//! handled by the OS (iOS "Show Previews: When Unlocked"; Android `visibility:
//! PRIVATE`). Set via `workspaces.settings.notification_push_detail`.

use async_trait::async_trait;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tracing::{debug, info};

use super::super::types::{DeliverableNotification, NotificationChannel};
use super::{ChannelError, ChannelResult, NotificationDeliveryChannel};
use crate::db::Pool;

/// A push payload. Content is context-only (never the message body itself):
/// in the workspace's `detailed` mode `title` is the ticket subject and `body`
/// is a "who did what" line; in `private` mode `title` is the generic type
/// label and `body` is `None` ("tap to view"). Either way it carries no comment
/// text — the same exposure as the notification email.
/// `Serialize` is for the cloud relay wire format only (see
/// `relay_client`), which forwards this struct verbatim. It carries alert text,
/// so do not serialize it into a log line.
#[derive(Debug, Clone, serde::Serialize)]
pub struct PushPayload {
    pub title: String,
    /// Context line ("Alice mentioned you"); `None` in the workspace's private mode.
    pub body: Option<String>,
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

    /// Last relay interaction, when this sender forwards through the cloud
    /// relay. `None` for the native and no-op senders, which have no remote to
    /// report on.
    ///
    /// Surfaced on `GET /api/admin/edition`: it is the only signal a
    /// self-hoster gets for why relayed push is failing, and it separates
    /// "cannot reach the relay" from "the relay refused this licence" from
    /// "the DPA has not been accepted".
    fn relay_status(&self) -> Option<super::relay_client::RelayStatus> {
        None
    }

    /// Short static label for logs ("relay", "native", "none"). Lets the boot
    /// line name the live sender without the caller matching on a concrete type.
    fn name(&self) -> &'static str;
}

/// Placeholder sender: not configured, sends nothing. Replaced by APNs/FCM.
pub struct NoopPushSender;

#[async_trait]
impl PushSender for NoopPushSender {
    fn is_configured(&self) -> bool {
        false
    }
    fn name(&self) -> &'static str {
        "none"
    }
    async fn send(&self, _targets: &[PushTarget], _payload: &PushPayload) -> Vec<String> {
        Vec::new()
    }
}

/// The push channel.
pub struct PushChannel {
    pool: Pool,
    sender: Arc<dyn PushSender>,
    /// Whether the "no registered devices" case has been reported yet.
    ///
    /// That exit is the single most common reason push looks broken on a fresh
    /// install, and it is silent: `deliver` returns `Ok(())` and nothing
    /// distinguishes it from a delivered push. One line per process makes it
    /// visible without logging on every notification for a workspace where most
    /// people have not installed the app.
    no_devices_reported: AtomicBool,
}

impl PushChannel {
    pub fn new(pool: Pool, sender: Arc<dyn PushSender>) -> Self {
        Self {
            pool,
            sender,
            no_devices_reported: AtomicBool::new(false),
        }
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
        // cross-tenant: a user's push devices span workspaces; targets load across all of them.
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
            // Info once, debug thereafter: enough to diagnose a fresh install,
            // quiet enough for steady state.
            if !self.no_devices_reported.swap(true, Ordering::Relaxed) {
                info!(
                    recipient_uuid = %recipient,
                    "Push skipped: recipient has no registered devices. Devices register                      on sign-in from the mobile app; this is logged once per process."
                );
            } else {
                debug!(recipient_uuid = %recipient, "Push skipped: no registered devices");
            }
            return Ok(());
        }

        // Workspace admin content level: `detailed` (default) enriches the push
        // with context — the ticket subject + a "who did what" line; `private`
        // sends only the generic type label ("tap to view"). Never the message
        // body itself, in either mode.
        let detailed = crate::sync::session::run_in_workspace(
            &self.pool,
            "background:push_ws_content_level",
            notification.payload.workspace_id,
            |conn| {
                crate::repository::workspaces::get_notification_push_detail(
                    conn,
                    notification.payload.workspace_id,
                )
            },
        )
        .unwrap_or(true);

        let ntype = &notification.payload.notification_type;
        let (title, body) = if detailed {
            let ctx = notification.payload.entity.context_title();
            let title = if ctx.trim().is_empty() {
                ntype.title().to_string()
            } else {
                ctx
            };
            (title, Some(ntype.push_body(&notification.payload.actor)))
        } else {
            (ntype.title().to_string(), None)
        };

        let payload = PushPayload {
            title,
            body,
            notification_type: ntype.as_str().to_string(),
            entity_type: notification.payload.entity.entity_type().to_string(),
            entity_id: notification.payload.entity.entity_id(),
            ticket_id: notification.payload.entity.ticket_id(),
        };

        let invalid = self.sender.send(&targets, &payload).await;
        if !invalid.is_empty() {
            // cross-tenant: device-token cleanup spans the user's devices across workspaces.
            let _ = crate::sync::session::background_run(
                &self.pool,
                "background:push_prune_tokens",
                |conn| crate::repository::push_devices::revoke_tokens(conn, &invalid),
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pins the once-per-process contract the no-devices notice relies on: the
    /// first `swap` reports, every later one falls through to debug. A
    /// workspace where most members have not installed the app would otherwise
    /// log on nearly every notification it sends.
    ///
    /// This exercises the flag, not `deliver` — driving the real path needs a
    /// pool and a seeded recipient, which is disproportionate for a log line.
    #[test]
    fn no_devices_notice_fires_once_per_process() {
        let flag = AtomicBool::new(false);
        assert!(
            !flag.swap(true, Ordering::Relaxed),
            "first hit reports at info"
        );
        for _ in 0..5 {
            assert!(
                flag.swap(true, Ordering::Relaxed),
                "subsequent hits fall through to debug"
            );
        }
    }

    /// Every sender names itself, so the boot line can never print an empty
    /// label for whichever one was selected.
    #[test]
    fn senders_report_a_static_name() {
        assert_eq!(NoopPushSender.name(), "none");
        assert!(!NoopPushSender.is_configured());
    }
}
