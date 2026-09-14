//! Per-channel delivery state and the retry worker.
//!
//! `notifications.channels_delivered` records what succeeded and nothing else.
//! A push the relay refused or an email the queue rejected left no trace and
//! was never retried, so at-least-once stopped at the notification row. One
//! `notification_deliveries` row per notification × channel closes that: the
//! channel loop records each outcome, and the worker below re-attempts
//! `pending` rows whose `next_attempt_at` has passed, with backoff, until they
//! deliver or exhaust [`MAX_ATTEMPTS`] and become `failed`.
//!
//! The JSONB list stays as the read model the API serves; both are written
//! together, and the escalation ladder (deferred) will read this table.

use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use tracing::{debug, info, warn};

use super::channels::{ChannelError, NotificationDeliveryChannel};
use super::service::NotificationService;
use super::types::{DeliverableNotification, NotificationChannel, NotificationPayload};
use crate::db::{DbConnection, Pool};
use crate::schema::notification_deliveries as nd;

pub const STATUS_PENDING: &str = "pending";
pub const STATUS_DELIVERED: &str = "delivered";
pub const STATUS_FAILED: &str = "failed";

pub const MAX_ATTEMPTS: i16 = 5;
const RETRY_POLL_SECS: u64 = 30;
const RETRY_BATCH: i64 = 100;

/// Backoff after the `attempts`-th failed attempt.
pub fn backoff_secs(attempts: i16) -> i64 {
    match attempts {
        ..=1 => 30,
        2 => 120,
        3 => 600,
        _ => 3600,
    }
}

#[derive(Insertable)]
#[diesel(table_name = nd)]
struct NewDelivery<'a> {
    notification_id: i32,
    channel: &'a str,
    status: &'a str,
    delivered_at: Option<DateTime<Utc>>,
    payload: serde_json::Value,
    workspace_id: i32,
}

/// Record the channels a notification will go to. `delivered_now` are the
/// ones the persist itself satisfied (in-app is the sync emit). Idempotent:
/// a retry of the same notification inserts nothing.
pub fn insert_for(
    conn: &mut DbConnection,
    notification_id: i32,
    workspace_id: i32,
    payload: &NotificationPayload,
    channels: &[NotificationChannel],
    delivered_now: &[NotificationChannel],
) -> QueryResult<()> {
    if channels.is_empty() {
        return Ok(());
    }
    let payload = serde_json::to_value(payload).unwrap_or(serde_json::Value::Null);
    let now = Utc::now();
    let rows: Vec<NewDelivery<'_>> = channels
        .iter()
        .map(|c| {
            let done = delivered_now.contains(c);
            NewDelivery {
                notification_id,
                channel: c.as_str(),
                status: if done {
                    STATUS_DELIVERED
                } else {
                    STATUS_PENDING
                },
                delivered_at: done.then_some(now),
                payload: payload.clone(),
                workspace_id,
            }
        })
        .collect();
    diesel::insert_into(nd::table)
        .values(&rows)
        .on_conflict_do_nothing()
        .execute(conn)?;
    Ok(())
}

/// Channels still owed for a notification, so a redelivery skips what is done.
pub fn pending_channels(
    conn: &mut DbConnection,
    notification_id: i32,
) -> QueryResult<Vec<NotificationChannel>> {
    let names: Vec<String> = nd::table
        .filter(nd::notification_id.eq(notification_id))
        .filter(nd::status.eq(STATUS_PENDING))
        .select(nd::channel)
        .load(conn)?;
    Ok(names
        .iter()
        .filter_map(|n| NotificationChannel::from_str(n))
        .collect())
}

pub fn mark_delivered(
    conn: &mut DbConnection,
    notification_id: i32,
    channel: NotificationChannel,
) -> QueryResult<usize> {
    diesel::update(
        nd::table
            .filter(nd::notification_id.eq(notification_id))
            .filter(nd::channel.eq(channel.as_str())),
    )
    .set((
        nd::status.eq(STATUS_DELIVERED),
        nd::delivered_at.eq(Some(Utc::now())),
        nd::next_attempt_at.eq(None::<DateTime<Utc>>),
    ))
    .execute(conn)
}

/// Record a failed attempt: reschedule with backoff, or mark `failed` once
/// the attempts are spent. `error_kind` is a kind, never a body.
pub fn mark_attempt_failed(
    conn: &mut DbConnection,
    notification_id: i32,
    channel: NotificationChannel,
    error_kind: &str,
) -> QueryResult<usize> {
    let attempts: i16 = nd::table
        .filter(nd::notification_id.eq(notification_id))
        .filter(nd::channel.eq(channel.as_str()))
        .select(nd::attempts)
        .first(conn)?;
    let attempts = attempts + 1;
    let exhausted = attempts >= MAX_ATTEMPTS;
    diesel::update(
        nd::table
            .filter(nd::notification_id.eq(notification_id))
            .filter(nd::channel.eq(channel.as_str())),
    )
    .set((
        nd::attempts.eq(attempts),
        nd::status.eq(if exhausted {
            STATUS_FAILED
        } else {
            STATUS_PENDING
        }),
        nd::next_attempt_at.eq(if exhausted {
            None
        } else {
            Some(Utc::now() + chrono::Duration::seconds(backoff_secs(attempts)))
        }),
        nd::last_error.eq(Some(error_kind.to_string())),
    ))
    .execute(conn)
}

/// The kind a channel error is recorded as.
pub fn error_kind(e: &ChannelError) -> String {
    match e {
        ChannelError::RateLimited => "rate_limited".into(),
        ChannelError::ChannelDisabled => "channel_disabled".into(),
        ChannelError::InvalidRecipient(_) => "invalid_recipient".into(),
        ChannelError::DeliveryFailed(kind) => kind.clone(),
        other => format!("{other:?}")
            .split('(')
            .next()
            .unwrap_or("error")
            .to_ascii_lowercase(),
    }
}

#[derive(Debug)]
struct DueRow {
    notification_id: i32,
    channel: String,
    payload: serde_json::Value,
    workspace_id: i32,
}

/// Claim due rows. Stamps `next_attempt_at` forward so a second worker (or
/// the next tick) does not pick them up while this one is delivering.
fn claim_due(conn: &mut DbConnection) -> QueryResult<Vec<DueRow>> {
    // One transaction: the row locks from SKIP LOCKED have to outlive the
    // select, or two workers claim the same rows.
    conn.transaction(claim_due_txn)
}

fn claim_due_txn(conn: &mut DbConnection) -> QueryResult<Vec<DueRow>> {
    let now = Utc::now();
    let due: Vec<(i32, String)> = nd::table
        .filter(nd::status.eq(STATUS_PENDING))
        .filter(nd::next_attempt_at.le(now))
        .order(nd::next_attempt_at.asc())
        .limit(RETRY_BATCH)
        .for_update()
        .skip_locked()
        .select((nd::notification_id, nd::channel))
        .load(conn)?;
    let mut rows = Vec::with_capacity(due.len());
    for (id, ch) in due {
        let updated: Vec<(i32, String, serde_json::Value, i32)> = diesel::update(
            nd::table
                .filter(nd::notification_id.eq(id))
                .filter(nd::channel.eq(&ch)),
        )
        .set(nd::next_attempt_at.eq(now + chrono::Duration::seconds(300)))
        .returning((
            nd::notification_id,
            nd::channel,
            nd::payload,
            nd::workspace_id,
        ))
        .get_results(conn)?;
        rows.extend(updated.into_iter().map(
            |(notification_id, channel, payload, workspace_id)| DueRow {
                notification_id,
                channel,
                payload,
                workspace_id,
            },
        ));
    }
    Ok(rows)
}

/// Re-attempt due deliveries on a fixed cadence. Runs everywhere the
/// notification service does; on more than one machine `SKIP LOCKED` keeps
/// them from colliding.
pub fn spawn_retry_worker(
    pool: Pool,
    service: Arc<NotificationService>,
    shutdown: tokio_util::sync::CancellationToken,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        info!("notification delivery retry worker started");
        let mut interval = tokio::time::interval(Duration::from_secs(RETRY_POLL_SECS));
        loop {
            tokio::select! {
                _ = shutdown.cancelled() => {
                    info!("notification delivery retry worker: shutting down");
                    return;
                }
                _ = interval.tick() => {}
            }
            if let Err(e) = retry_once(&pool, &service).await {
                warn!(error = %e, "notification delivery retry failed");
            }
        }
    })
}

async fn retry_once(pool: &Pool, service: &NotificationService) -> Result<(), String> {
    // cross-tenant: the retry queue spans every workspace; each row is
    // delivered and recorded under its own workspace pin below.
    let due = crate::sync::session::background_run(
        pool,
        "background:notification_retry_claim",
        claim_due,
    )
    .map_err(|e| e.to_string())?;
    if due.is_empty() {
        return Ok(());
    }
    debug!(count = due.len(), "notification delivery retry: claimed");

    for row in due {
        let Some(channel) = NotificationChannel::from_str(&row.channel) else {
            continue;
        };
        let Ok(payload) = serde_json::from_value::<NotificationPayload>(row.payload) else {
            let _ = crate::sync::session::run_in_workspace(
                pool,
                "background:notification_retry_record",
                row.workspace_id,
                |conn| mark_attempt_failed(conn, row.notification_id, channel, "bad_payload"),
            );
            continue;
        };
        let deliverable = DeliverableNotification {
            id: Some(row.notification_id),
            uuid: uuid::Uuid::now_v7(),
            payload,
            channels: vec![channel],
        };
        let Some(handler): Option<Arc<dyn NotificationDeliveryChannel>> = service.channel(channel)
        else {
            continue;
        };
        let outcome = handler.deliver(&deliverable).await;
        service
            .record_delivery_outcome(
                row.notification_id,
                row.workspace_id,
                channel,
                outcome.as_ref().err(),
            )
            .await;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_grows_and_caps() {
        assert_eq!(backoff_secs(1), 30);
        assert_eq!(backoff_secs(2), 120);
        assert_eq!(backoff_secs(3), 600);
        assert_eq!(backoff_secs(4), 3600);
        assert_eq!(backoff_secs(9), 3600);
    }

    #[test]
    fn error_kinds_are_short_and_bodiless() {
        assert_eq!(error_kind(&ChannelError::RateLimited), "rate_limited");
        assert_eq!(
            error_kind(&ChannelError::DeliveryFailed("dpa_required".into())),
            "dpa_required"
        );
        assert_eq!(
            error_kind(&ChannelError::InvalidRecipient("alex@example.test".into())),
            "invalid_recipient"
        );
    }
}
