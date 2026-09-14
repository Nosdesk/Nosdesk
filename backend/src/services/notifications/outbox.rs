//! The `notification_outbox` dispatcher.
//!
//! `tr_sync_actions_notification_outbox` enqueues one row per `sync_actions`
//! row the deriver can act on. This loop claims those rows, one instance at a
//! time (`FOR UPDATE SKIP LOCKED`), derives and resolves the notifications and
//! hands them to `NotificationService::notify`, which is unchanged: the
//! preference resolution, interrupt gates, persistence and channel delivery all
//! stay where they were. Only the source of the payload moved.
//!
//! Delivery is at least once. A claim stamps `claimed_at` and bumps
//! `attempts`; the row is deleted only after every payload was handed off
//! without error. A failure reschedules it with backoff, and after
//! [`MAX_ATTEMPTS`] it is kept with `dead_at` set rather than dropped. A claim
//! that never reported back (the process died) becomes claimable again after
//! [`STALE_CLAIM_SECS`]. The retry is safe because a derived notification is
//! unique per (event, recipient, type): `persist_notification` inserts nothing
//! the second time.
//!
//! Wakes on the trigger's `pg_notify('notification_outbox_new')` when a
//! `DATABASE_URL` is available for a LISTEN connection, and polls every
//! [`POLL_SECS`] regardless, so a missed wake costs latency, never a
//! notification.
//!
//! This is the only path for assignment, status and comment notifications;
//! there is no switch, on a single instance or many.

use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel::sql_types::{BigInt, Integer, Jsonb, Nullable, SmallInt, Text, Timestamptz};
use tokio::sync::mpsc;
use tokio_postgres::{AsyncMessage, NoTls};
use tracing::{debug, info, warn};
use uuid::Uuid;

use super::deriver::{derive, resolve, SyncActionRow};
use super::service::NotificationService;
use crate::db::Pool;

const POLL_SECS: u64 = 2;
const BATCH: i64 = 100;
pub const MAX_ATTEMPTS: i16 = 8;
const STALE_CLAIM_SECS: i64 = 300;
/// Rows older than this at claim time are acknowledged without deriving.
/// `seed-demo` emits real, backdated events; without this a seeded workspace
/// notifies every assignee and mention in the fixture.
const MAX_AGE_SECS: i64 = 600;
/// Concurrent `notify()` calls per batch. Each does its own pool work and,
/// for push, a relay round-trip.
const DELIVERY_CONCURRENCY: usize = 8;

/// Backoff after the `attempts`-th failure.
fn backoff_secs(attempts: i16) -> i64 {
    match attempts {
        ..=1 => 1,
        2 => 10,
        3 => 60,
        4 => 600,
        _ => 3600,
    }
}

#[derive(QueryableByName, Debug)]
pub struct Claimed {
    #[diesel(sql_type = BigInt)]
    pub sync_id: i64,
    #[diesel(sql_type = SmallInt)]
    pub attempts: i16,
    #[diesel(sql_type = Nullable<Integer>)]
    pub workspace_id: Option<i32>,
    #[diesel(sql_type = Nullable<Text>)]
    pub event_type: Option<String>,
    #[diesel(sql_type = Nullable<Jsonb>)]
    pub data: Option<serde_json::Value>,
    #[diesel(sql_type = Nullable<diesel::sql_types::Uuid>)]
    pub actor_uuid: Option<Uuid>,
    #[diesel(sql_type = Nullable<Text>)]
    pub actor_kind: Option<String>,
    #[diesel(sql_type = Nullable<Timestamptz>)]
    pub occurred_at: Option<DateTime<Utc>>,
}

/// Claim due rows and load their source events in one short transaction.
/// A row whose `sync_actions` source is gone (a partition dropped under it)
/// comes back with null columns and is acknowledged below.
pub fn claim_batch(conn: &mut crate::db::DbConnection) -> QueryResult<Vec<Claimed>> {
    diesel::sql_query(
        r#"
        WITH due AS (
            SELECT sync_id FROM notification_outbox
            WHERE dead_at IS NULL
              AND next_attempt_at <= now()
              AND (claimed_at IS NULL OR claimed_at < now() - make_interval(secs => $1))
            ORDER BY sync_id
            LIMIT $2
            FOR UPDATE SKIP LOCKED
        ),
        claimed AS (
            UPDATE notification_outbox o
               SET claimed_at = now(), attempts = o.attempts + 1
              FROM due
             WHERE o.sync_id = due.sync_id
         RETURNING o.sync_id, o.attempts
        )
        SELECT c.sync_id, c.attempts,
               s.workspace_id, s.event_type, s.data, s.actor_uuid, s.actor_kind, s.occurred_at
          FROM claimed c
          LEFT JOIN sync_actions s ON s.sync_id = c.sync_id
         ORDER BY c.sync_id
        "#,
    )
    .bind::<diesel::sql_types::Double, _>(STALE_CLAIM_SECS as f64)
    .bind::<BigInt, _>(BATCH)
    .load(conn)
}

/// Delete delivered rows.
pub fn ack(conn: &mut crate::db::DbConnection, sync_ids: &[i64]) -> QueryResult<usize> {
    use crate::schema::notification_outbox::dsl::*;
    if sync_ids.is_empty() {
        return Ok(0);
    }
    diesel::delete(notification_outbox.filter(sync_id.eq_any(sync_ids))).execute(conn)
}

/// Reschedule a row after its `attempt`-th failure, or dead-letter it.
pub fn fail(
    conn: &mut crate::db::DbConnection,
    id: i64,
    attempt: i16,
    error_kind: &str,
) -> QueryResult<usize> {
    use crate::schema::notification_outbox::dsl::*;
    let dead = attempt >= MAX_ATTEMPTS;
    diesel::update(notification_outbox.filter(sync_id.eq(id)))
        .set((
            claimed_at.eq(None::<DateTime<Utc>>),
            next_attempt_at.eq(Utc::now() + chrono::Duration::seconds(backoff_secs(attempt))),
            last_error.eq(Some(error_kind.to_string())),
            dead_at.eq(if dead { Some(Utc::now()) } else { None }),
        ))
        .execute(conn)
}

/// A claimed row that did not complete, for the reschedule pass.
struct Failed {
    sync_id: i64,
    attempts: i16,
    kind: &'static str,
}

pub struct Dispatcher {
    pool: Pool,
    service: Arc<NotificationService>,
}

impl Dispatcher {
    pub fn spawn(
        pool: Pool,
        service: Arc<NotificationService>,
        database_url: Option<String>,
        shutdown: tokio_util::sync::CancellationToken,
    ) -> tokio::task::JoinHandle<()> {
        let this = Dispatcher { pool, service };
        tokio::spawn(async move {
            info!("notification outbox dispatcher started");
            tokio::select! {
                _ = shutdown.cancelled() => info!("notification outbox dispatcher: shutting down"),
                _ = this.run(database_url) => {}
            }
        })
    }

    async fn run(&self, database_url: Option<String>) {
        let (wake_tx, mut wake_rx) = mpsc::channel::<()>(8);
        if let Some(url) = database_url {
            tokio::spawn(listen(url, wake_tx));
        }
        let mut interval = tokio::time::interval(Duration::from_secs(POLL_SECS));
        loop {
            tokio::select! {
                _ = interval.tick() => {}
                w = wake_rx.recv() => { if w.is_none() { interval.tick().await; } }
            }
            // Drain until a batch comes back short.
            loop {
                match self.dispatch_batch().await {
                    Ok(n) if n as i64 >= BATCH => continue,
                    Ok(_) => break,
                    Err(e) => {
                        warn!(error = %e, "notification outbox: batch failed");
                        break;
                    }
                }
            }
        }
    }

    /// Claim, derive, deliver, acknowledge. Returns how many rows were
    /// claimed so the caller knows whether to go again.
    async fn dispatch_batch(&self) -> Result<usize, String> {
        // cross-tenant: the outbox is a queue across every workspace; each
        // row is resolved under its own workspace pin below.
        let claimed = crate::sync::session::background_run(
            &self.pool,
            "background:notification_outbox_claim",
            claim_batch,
        )
        .map_err(|e| e.to_string())?;
        if claimed.is_empty() {
            return Ok(0);
        }
        let count = claimed.len();

        let now = Utc::now();
        let mut done: Vec<i64> = Vec::new();
        let mut failed: Vec<Failed> = Vec::new();
        let mut deliveries: Vec<(i64, i16, super::types::NotificationPayload)> = Vec::new();

        for c in claimed {
            let (
                Some(workspace_id),
                Some(event_type),
                Some(data),
                Some(actor_kind),
                Some(occurred_at),
            ) = (
                c.workspace_id,
                c.event_type,
                c.data,
                c.actor_kind,
                c.occurred_at,
            )
            else {
                // Source row gone; nothing to derive from.
                done.push(c.sync_id);
                continue;
            };
            if (now - occurred_at).num_seconds() > MAX_AGE_SECS {
                debug!(
                    sync_id = c.sync_id,
                    "notification outbox: row too old; acknowledged without deriving"
                );
                done.push(c.sync_id);
                continue;
            }
            let row = SyncActionRow {
                sync_id: c.sync_id,
                workspace_id,
                event_type,
                data,
                actor_uuid: c.actor_uuid,
                actor_kind,
                occurred_at,
            };
            let intents = derive(&row);
            if intents.is_empty() {
                done.push(c.sync_id);
                continue;
            }

            let row_for_resolve = row.clone();
            let intents_for_resolve = intents.clone();
            let resolved = crate::sync::session::run_in_workspace(
                &self.pool,
                "background:notification_outbox_resolve",
                workspace_id,
                move |conn| {
                    let mut all = Vec::new();
                    for intent in intents_for_resolve {
                        all.extend(resolve(conn, &row_for_resolve, intent)?);
                    }
                    Ok(all)
                },
            );
            let payloads = match resolved {
                Ok(p) => p,
                Err(e) => {
                    warn!(sync_id = row.sync_id, error = %e, "notification outbox: resolve failed");
                    failed.push(Failed {
                        sync_id: row.sync_id,
                        attempts: c.attempts,
                        kind: "resolve",
                    });
                    continue;
                }
            };

            if payloads.is_empty() {
                // Nothing to send (a status move within one category, a
                // comment whose only participant is its author).
                done.push(row.sync_id);
                continue;
            }
            debug!(
                sync_id = row.sync_id,
                event_type = %row.event_type,
                intents = intents.len(),
                recipients = payloads.len(),
                "notification outbox: delivering"
            );
            for p in payloads {
                deliveries.push((row.sync_id, c.attempts, p));
            }
        }

        // Deliver, bounded, then fold outcomes per source row: a row is done
        // only when every one of its payloads was accepted.
        if !deliveries.is_empty() {
            use futures::stream::{self, StreamExt};
            let service = self.service.clone();
            let results: Vec<(i64, i16, Result<(), String>)> = stream::iter(deliveries)
                .map(|(sync_id, attempts, payload)| {
                    let service = service.clone();
                    async move { (sync_id, attempts, service.notify(payload).await) }
                })
                .buffer_unordered(DELIVERY_CONCURRENCY)
                .collect()
                .await;
            let mut failed_ids: Vec<(i64, i16)> = Vec::new();
            let mut ok_ids: Vec<i64> = Vec::new();
            for (sync_id, attempts, r) in results {
                match r {
                    Ok(()) => ok_ids.push(sync_id),
                    Err(e) => {
                        warn!(sync_id, error = %e, "notification outbox: notify failed");
                        failed_ids.push((sync_id, attempts));
                    }
                }
            }
            for id in ok_ids {
                if !failed_ids.iter().any(|(f, _)| *f == id) && !done.contains(&id) {
                    done.push(id);
                }
            }
            failed_ids.sort_unstable();
            failed_ids.dedup();
            for (sync_id, attempts) in failed_ids {
                failed.push(Failed {
                    sync_id,
                    attempts,
                    kind: "notify",
                });
            }
        }

        // cross-tenant: acknowledgements and reschedules are on the queue
        // table, which has no workspace.
        crate::sync::session::background_run(
            &self.pool,
            "background:notification_outbox_ack",
            move |conn| {
                ack(conn, &done)?;
                for f in &failed {
                    fail(conn, f.sync_id, f.attempts, f.kind)?;
                }
                Ok(())
            },
        )
        .map_err(|e| e.to_string())?;

        Ok(count)
    }
}

/// Hold a LISTEN connection on `notification_outbox_new` and nudge the
/// dispatcher on every notification. Reconnects with backoff; the poll covers
/// the gaps.
async fn listen(database_url: String, wake: mpsc::Sender<()>) {
    let mut backoff = Duration::from_secs(1);
    loop {
        match listen_once(&database_url, &wake).await {
            Ok(()) => backoff = Duration::from_secs(1),
            Err(e) => {
                debug!(error = %e, "notification outbox listener disconnected; reconnecting");
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(Duration::from_secs(30));
            }
        }
    }
}

async fn listen_once(database_url: &str, wake: &mpsc::Sender<()>) -> Result<(), anyhow::Error> {
    let (client, connection) = tokio_postgres::connect(database_url, NoTls).await?;
    let wake = wake.clone();
    let driver = tokio::spawn(async move {
        use futures::StreamExt;
        let mut connection = Box::pin(connection);
        let mut stream = futures::stream::poll_fn(move |cx| connection.as_mut().poll_message(cx));
        while let Some(msg) = stream.next().await {
            match msg {
                Ok(AsyncMessage::Notification(_)) => {
                    // A full channel means a wake is already pending.
                    let _ = wake.try_send(());
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
    });
    client
        .batch_execute("LISTEN notification_outbox_new")
        .await?;
    debug!("notification outbox listening on `notification_outbox_new`");
    let _ = driver.await;
    Err(anyhow::anyhow!("notification stream closed"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_grows_and_caps() {
        assert_eq!(backoff_secs(1), 1);
        assert_eq!(backoff_secs(2), 10);
        assert_eq!(backoff_secs(3), 60);
        assert_eq!(backoff_secs(4), 600);
        assert_eq!(backoff_secs(5), 3600);
        assert_eq!(backoff_secs(50), 3600);
    }
}
