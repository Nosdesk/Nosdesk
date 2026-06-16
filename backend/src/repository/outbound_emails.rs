//! Outbound email queue repository.
//!
//! The queue is the durable replacement for the fire-and-forget
//! `tokio::spawn` send path. Producers (the comment-creation handler,
//! the auto-ack flow) call [`enqueue`]; the worker in
//! `services/email_queue/worker.rs` calls [`claim_batch`] and one of
//! [`mark_sent`] / [`mark_failed`] / [`mark_dead`] / [`mark_suppressed`]
//! per row. The lease sweeper calls [`sweep_expired_leases`].
//!
//! Claim semantics use `SELECT FOR UPDATE SKIP LOCKED` inside a CTE
//! that flips status to `sending` and bumps the attempt counter in a
//! single round trip — the canonical "implicit ACK" pattern shared by
//! pgmq, pg-boss, Solid Queue, etc. Workers never double-claim a row.
//!
//! Crash recovery rests on `lease_token` + `lease_expires_at`: a worker
//! that holds a row in `sending` and dies orphans the row. The lease
//! sweeper finds rows whose lease expired and bumps them back to
//! `pending` (or `failed` for backoff). Combined with the deterministic
//! `Message-ID` persisted at enqueue, this is at-least-once delivery
//! with consumer-side dedupe (the receiving MTA / customer MUA).

use crate::db::DbConnection;
use crate::models::{outbound_email_status, NewOutboundEmail, OutboundEmail};
use crate::schema::outbound_emails;
use diesel::prelude::*;
use diesel::result::Error as DieselError;
use diesel::sql_types::{BigInt, Integer, Nullable, Text, Timestamptz};

/// Batch size for the worker's claim loop. Email is high-latency I/O
/// (200ms-2s per send); 10 sends = 10-20s of work, well under the
/// 5-minute lease.
pub const DEFAULT_BATCH_SIZE: i64 = 10;

// sync-audit-only: internal worker queue; delivery state surfaced via admin queue view
/// Insert a new row in the queue. The trigger fires `pg_notify` so the
/// listener wakes immediately.
pub fn enqueue(
    conn: &mut DbConnection,
    new_row: NewOutboundEmail,
) -> Result<OutboundEmail, DieselError> {
    diesel::insert_into(outbound_emails::table)
        .values(&new_row)
        .get_result(conn)
}

// sync-audit-only: internal worker queue; delivery state surfaced via admin queue view
/// Enqueue a row keyed by an idempotency token. Two enqueues that
/// share the same `idempotency_key` collapse to a single queue row:
/// the first wins, the second returns the existing row without
/// firing a fresh send. Powers the "at-least-once → effectively-once"
/// semantics transactional callers (password reset, invitation,
/// notification) rely on — a network blip between the handler and
/// the DB can safely retry without delivering two copies.
///
/// The key MUST be set on `new_row.idempotency_key`. Callers that
/// don't need idempotency should use the bare `enqueue` instead;
/// the partial unique index doesn't index NULL keys, so a NULL key
/// here would degrade silently to a non-idempotent insert.
///
/// Returns the row in whichever state it ended up:
///   * Fresh insert → newly-pending row, ready for the worker.
///   * Conflict     → the previously-enqueued row, unchanged.
pub fn enqueue_idempotent(
    conn: &mut DbConnection,
    new_row: NewOutboundEmail,
) -> Result<OutboundEmail, DieselError> {
    debug_assert!(
        new_row.idempotency_key.is_some(),
        "enqueue_idempotent requires a non-None idempotency_key; \
         use `enqueue` for non-idempotent inserts",
    );
    let key = match new_row.idempotency_key.clone() {
        Some(k) => k,
        None => return Err(DieselError::NotFound),
    };

    let inserted = diesel::insert_into(outbound_emails::table)
        .values(&new_row)
        .on_conflict_do_nothing()
        .get_result::<OutboundEmail>(conn)
        .optional()?;

    match inserted {
        Some(row) => Ok(row),
        None => {
            // Key already used by a prior enqueue. Return that row so
            // the caller can log the queue id without distinguishing
            // first-insert from retry. Operators inspecting the admin
            // queue UI see only one row per logical send regardless.
            outbound_emails::table
                .filter(outbound_emails::idempotency_key.eq(&key))
                .first(conn)
        }
    }
}

// sync-audit-only: internal worker queue; delivery state surfaced via admin queue view
/// Enqueue a row, but if the recipient is on the suppression list,
/// short-circuit to `suppressed` status without ever entering the
/// worker's claim set. Wrapped in a transaction so the INSERT and
/// the subsequent `mark_suppressed` either both land or neither
/// does — the LISTEN/NOTIFY trigger only fires at commit time, so
/// the worker never observes the intermediate pending state.
///
/// Returns the row in its final state (either pending or
/// suppressed) so the caller can log + report without a refetch.
pub fn enqueue_or_suppress(
    conn: &mut DbConnection,
    new_row: NewOutboundEmail,
) -> Result<OutboundEmail, DieselError> {
    use diesel::Connection;
    conn.transaction::<OutboundEmail, DieselError, _>(|conn| {
        let suppressed =
            crate::repository::email_suppressions::is_suppressed(conn, &new_row.recipient)?;
        if suppressed {
            let row: OutboundEmail = diesel::insert_into(outbound_emails::table)
                .values(&new_row)
                .get_result(conn)?;
            mark_suppressed(conn, row.id, "recipient on suppression list")?;
            get(conn, row.id)
        } else {
            diesel::insert_into(outbound_emails::table)
                .values(&new_row)
                .get_result(conn)
        }
    })
}

/// Fetch a single row by id. Used by the admin handler.
pub fn get(conn: &mut DbConnection, id: i64) -> Result<OutboundEmail, DieselError> {
    outbound_emails::table.find(id).first(conn)
}

// sync-audit-only: worker lease acquisition on the outbound queue
/// Atomically claim up to `limit` due rows. Returns the claimed rows
/// in `sending` state with leases set; the worker dispatches each then
/// calls one of the `mark_*` functions to terminate.
///
/// Single CTE: SELECT...FOR UPDATE SKIP LOCKED inside an UPDATE that
/// flips status, increments attempts, sets the lease. One round trip,
/// no double-claim possible. Lease defaults to 5 minutes — long enough
/// to absorb SMTP slowness, bounded enough that crash recovery is
/// deterministic.
pub fn claim_batch(
    conn: &mut DbConnection,
    limit: i64,
    lease_seconds: i64,
) -> Result<Vec<OutboundEmail>, DieselError> {
    // `clock_timestamp()` rather than `now()` for the lease so that
    // crash recovery works across transactions. `now()` returns the
    // calling transaction's start time and is stable within it; the
    // sweeper running in a separate transaction would compare the
    // lease against its own (different) `now()` and the relative
    // ordering becomes brittle. `clock_timestamp()` gives wall-clock
    // semantics, which is what a lease actually needs.
    diesel::sql_query(
        r#"
        WITH claimed AS (
            SELECT id
            FROM outbound_emails
            WHERE status IN ('pending', 'failed')
              AND next_attempt_at <= now()
            ORDER BY next_attempt_at
            LIMIT $1
            FOR UPDATE SKIP LOCKED
        )
        UPDATE outbound_emails o
        SET status = 'sending',
            attempts = attempts + 1,
            lease_token = gen_random_uuid(),
            lease_expires_at = clock_timestamp() + ($2 || ' seconds')::interval
        FROM claimed
        WHERE o.id = claimed.id
        RETURNING o.*
        "#,
    )
    .bind::<BigInt, _>(limit)
    .bind::<Text, _>(lease_seconds.to_string())
    .load::<OutboundEmail>(conn)
}

// sync-audit-only: worker terminal state transition on the outbound queue
/// Mark a successful send. Terminal state.
pub fn mark_sent(
    conn: &mut DbConnection,
    id: i64,
    provider_message_id: Option<&str>,
) -> Result<usize, DieselError> {
    diesel::sql_query(
        r#"
        UPDATE outbound_emails
        SET status = 'sent',
            sent_at = now(),
            lease_token = NULL,
            lease_expires_at = NULL,
            last_error = NULL,
            last_smtp_code = NULL,
            provider_message_id = COALESCE($2, provider_message_id)
        WHERE id = $1
        "#,
    )
    .bind::<BigInt, _>(id)
    .bind::<diesel::sql_types::Nullable<diesel::sql_types::Text>, _>(provider_message_id)
    .execute(conn)
}

// sync-audit-only: worker terminal state transition on the outbound queue
/// Mark a transient failure. Schedules the next retry. Caller computes
/// `next_attempt_at` per the retry policy (backoff + jitter).
pub fn mark_failed(
    conn: &mut DbConnection,
    id: i64,
    error_message: &str,
    smtp_code: Option<i32>,
    next_attempt_at: chrono::DateTime<chrono::Utc>,
) -> Result<usize, DieselError> {
    diesel::sql_query(
        r#"
        UPDATE outbound_emails
        SET status = 'failed',
            last_error = $2,
            last_smtp_code = $3,
            next_attempt_at = $4,
            lease_token = NULL,
            lease_expires_at = NULL
        WHERE id = $1
        "#,
    )
    .bind::<BigInt, _>(id)
    .bind::<Text, _>(error_message)
    .bind::<Nullable<Integer>, _>(smtp_code)
    .bind::<Timestamptz, _>(next_attempt_at)
    .execute(conn)
}

// sync-audit-only: worker terminal state transition on the outbound queue
/// Mark a permanent failure. Terminal state — no further retries.
pub fn mark_dead(
    conn: &mut DbConnection,
    id: i64,
    error_message: &str,
    smtp_code: Option<i32>,
) -> Result<usize, DieselError> {
    diesel::sql_query(
        r#"
        UPDATE outbound_emails
        SET status = 'dead',
            last_error = $2,
            last_smtp_code = $3,
            failed_at = now(),
            lease_token = NULL,
            lease_expires_at = NULL
        WHERE id = $1
        "#,
    )
    .bind::<BigInt, _>(id)
    .bind::<Text, _>(error_message)
    .bind::<Nullable<Integer>, _>(smtp_code)
    .execute(conn)
}

// sync-audit-only: worker terminal state transition on the outbound queue
/// Mark a row as suppressed (recipient on suppression list). Worker
/// sets this at claim time before any SMTP traffic. Terminal state.
pub fn mark_suppressed(
    conn: &mut DbConnection,
    id: i64,
    reason: &str,
) -> Result<usize, DieselError> {
    diesel::sql_query(
        r#"
        UPDATE outbound_emails
        SET status = 'suppressed',
            last_error = $2,
            failed_at = now(),
            lease_token = NULL,
            lease_expires_at = NULL
        WHERE id = $1
        "#,
    )
    .bind::<BigInt, _>(id)
    .bind::<Text, _>(reason)
    .execute(conn)
}

// sync-audit-only: worker terminal state transition on the outbound queue
/// Stamp bounce metadata onto an outbound row matched by its
/// deterministic Message-ID. Does NOT change `status` — a bounce
/// is delivery-result detail recorded alongside the SMTP outcome
/// rather than a fresh state. Most bounced rows sit in `sent`
/// status because the SMTP relay accepted the handoff and the
/// remote MTA only rejected later via DSN.
///
/// Returns the number of rows updated. Zero means we couldn't
/// match the DSN's original-Message-ID to any outbound row — the
/// caller logs and moves on (the inbound is still treated as a
/// bounce skip per J Pass 2.1, just unlinked).
pub fn mark_bounced(
    conn: &mut DbConnection,
    message_id: &str,
    recipient: Option<&str>,
    diagnostic: Option<&str>,
) -> Result<usize, DieselError> {
    diesel::sql_query(
        r#"
        UPDATE outbound_emails
        SET bounced_at = now(),
            bounce_recipient = $2,
            bounce_diagnostic = $3
        WHERE message_id = $1
        "#,
    )
    .bind::<Text, _>(message_id)
    .bind::<Nullable<Text>, _>(recipient)
    .bind::<Nullable<Text>, _>(diagnostic)
    .execute(conn)
}

/// Resolve the workspace + recipient for a row by its provider message id
/// (Resend `email_id`). Used by the Resend webhook to scope its audited
/// writes (delivery/bounce + suppression) to the right workspace. Returns
/// `None` when no row carries that provider id (e.g. an event for a send
/// from before provider tracking, or a non-tracked provider).
pub fn workspace_and_recipient_by_provider(
    conn: &mut DbConnection,
    provider_message_id: &str,
) -> Result<Option<(i32, String)>, DieselError> {
    use crate::schema::outbound_emails::dsl as oe;
    oe::outbound_emails
        .filter(oe::provider_message_id.eq(provider_message_id))
        .select((oe::workspace_id, oe::recipient))
        .first::<(i32, String)>(conn)
        .optional()
}

// sync-audit-only: delivery confirmation from the provider webhook; not a tier-1 aggregate, covered by the outbound_emails audit trigger.
/// Stamp the provider's confirmed-delivery time on a row, matched by its
/// provider message id. Idempotent: a redelivered `email.delivered`
/// webhook keeps the first timestamp.
pub fn mark_delivered_by_provider(
    conn: &mut DbConnection,
    provider_message_id: &str,
) -> Result<usize, DieselError> {
    diesel::sql_query(
        r#"
        UPDATE outbound_emails
        SET delivered_at = COALESCE(delivered_at, now())
        WHERE provider_message_id = $1
        "#,
    )
    .bind::<Text, _>(provider_message_id)
    .execute(conn)
}

// sync-audit-only: bounce metadata from the provider webhook; mirrors mark_bounced but keyed on the provider message id.
/// Stamp bounce metadata on a row matched by its provider message id
/// (the Resend webhook path; `mark_bounced` is the RFC-Message-ID/DSN
/// path). Does NOT change `status`, mirroring `mark_bounced`.
pub fn mark_bounced_by_provider(
    conn: &mut DbConnection,
    provider_message_id: &str,
    recipient: Option<&str>,
    diagnostic: Option<&str>,
) -> Result<usize, DieselError> {
    diesel::sql_query(
        r#"
        UPDATE outbound_emails
        SET bounced_at = now(),
            bounce_recipient = $2,
            bounce_diagnostic = $3
        WHERE provider_message_id = $1
        "#,
    )
    .bind::<Text, _>(provider_message_id)
    .bind::<Nullable<Text>, _>(recipient)
    .bind::<Nullable<Text>, _>(diagnostic)
    .execute(conn)
}

// sync-audit-only: worker lease release on the outbound queue
/// Release a claim without recording a failure — used by the circuit
/// breaker when SMTP is down and the worker shouldn't burn an attempt.
/// Sets status back to `pending` and clears the lease so another worker
/// pass can pick it up after the breaker recovers.
pub fn release_claim(conn: &mut DbConnection, id: i64) -> Result<usize, DieselError> {
    diesel::sql_query(
        r#"
        UPDATE outbound_emails
        SET status = 'pending',
            attempts = attempts - 1,
            lease_token = NULL,
            lease_expires_at = NULL
        WHERE id = $1 AND status = 'sending'
        "#,
    )
    .bind::<BigInt, _>(id)
    .execute(conn)
}

// sync-audit-only: worker recovery sweep on stalled leases
/// Periodic sweeper: rows whose lease expired (worker crashed mid-send)
/// move back to `failed` so the next claim cycle picks them up. Returns
/// the count of swept rows for the scheduler to log.
///
/// Uses `clock_timestamp()` to compare against `lease_expires_at` —
/// `now()` is the transaction-start time and would be stable inside
/// long-running maintenance transactions, defeating the sweep.
pub fn sweep_expired_leases(conn: &mut DbConnection) -> Result<usize, DieselError> {
    diesel::sql_query(
        r#"
        UPDATE outbound_emails
        SET status = 'failed',
            last_error = COALESCE(last_error, 'lease expired (worker crash)'),
            next_attempt_at = clock_timestamp(),
            lease_token = NULL,
            lease_expires_at = NULL
        WHERE status = 'sending' AND lease_expires_at < clock_timestamp()
        "#,
    )
    .execute(conn)
}

// sync-audit-only: admin-triggered retry on the outbound queue
/// Operator action: bump `next_attempt_at` to now and reset attempts on
/// dead rows so the worker re-tries immediately.
pub fn retry_now(conn: &mut DbConnection, id: i64) -> Result<usize, DieselError> {
    diesel::sql_query(
        r#"
        UPDATE outbound_emails
        SET status = 'pending',
            next_attempt_at = now(),
            attempts = CASE WHEN status = 'dead' THEN 0 ELSE attempts END,
            failed_at = NULL,
            last_error = NULL,
            last_smtp_code = NULL
        WHERE id = $1 AND status IN ('failed', 'dead', 'suppressed')
        "#,
    )
    .bind::<BigInt, _>(id)
    .execute(conn)
}

/// Operator action: cancel a pending or failed row. Marks as suppressed
/// with a "cancelled by admin" reason for audit clarity.
pub fn cancel(conn: &mut DbConnection, id: i64) -> Result<usize, DieselError> {
    mark_suppressed(conn, id, "cancelled by admin")
}

/// Filter set for [`list`]. All fields optional; the admin UI passes
/// the user's filter selection.
#[derive(Debug, Clone, Default)]
pub struct ListFilter {
    pub status: Option<Vec<String>>,
    pub ticket_id: Option<i32>,
    pub recipient_domain: Option<String>,
    pub since: Option<chrono::DateTime<chrono::Utc>>,
    pub until: Option<chrono::DateTime<chrono::Utc>>,
}

/// Cursor for keyset pagination on `(created_at DESC, id DESC)`.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct Cursor {
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub id: i64,
}

#[derive(Debug)]
pub struct Page {
    pub rows: Vec<OutboundEmail>,
    pub next_cursor: Option<Cursor>,
}

/// Admin list endpoint. Keyset pagination on `(created_at DESC, id DESC)`.
/// `limit` is clamped to `[1, 200]`.
pub fn list(
    conn: &mut DbConnection,
    filter: &ListFilter,
    cursor: Option<Cursor>,
    limit: i64,
) -> Result<Page, DieselError> {
    let limit = limit.clamp(1, 200);
    let mut q = outbound_emails::table.into_boxed();

    if let Some(statuses) = &filter.status {
        q = q.filter(outbound_emails::status.eq_any(statuses));
    }
    if let Some(t) = filter.ticket_id {
        q = q.filter(outbound_emails::ticket_id.eq(t));
    }
    if let Some(domain) = &filter.recipient_domain {
        // Simple suffix match. Domains live in lowercase by convention.
        let pattern = format!("%@{}", domain.to_lowercase());
        q = q.filter(outbound_emails::recipient.ilike(pattern));
    }
    if let Some(since) = filter.since {
        q = q.filter(outbound_emails::created_at.ge(since));
    }
    if let Some(until) = filter.until {
        q = q.filter(outbound_emails::created_at.lt(until));
    }
    if let Some(c) = cursor {
        // Tuple comparison spelled out so Diesel can build a single
        // index seek. Same shape as audit_log::list — keyset cursor
        // on (timestamp DESC, id DESC) avoids OFFSET on a write-heavy
        // table.
        q = q.filter(
            outbound_emails::created_at
                .lt(c.created_at)
                .or(outbound_emails::created_at
                    .eq(c.created_at)
                    .and(outbound_emails::id.lt(c.id))),
        );
    }

    let rows: Vec<OutboundEmail> = q
        .order((
            outbound_emails::created_at.desc(),
            outbound_emails::id.desc(),
        ))
        .limit(limit + 1)
        .load(conn)?;

    Ok(paginate(rows, limit))
}

/// `list` over-fetches by one row to detect "more after this page."
/// When the over-fetch row is present, drop it and emit a cursor
/// pointing at the new last row.
fn paginate(mut rows: Vec<OutboundEmail>, limit: i64) -> Page {
    if rows.len() as i64 > limit {
        rows.pop();
        let cursor = rows.last().map(|r| Cursor {
            created_at: r.created_at,
            id: r.id,
        });
        Page {
            rows,
            next_cursor: cursor,
        }
    } else {
        Page {
            rows,
            next_cursor: None,
        }
    }
}

/// Aggregate counts per status — drives the admin dashboard's top stats
/// bar. Cheap because of `outbound_emails_due_idx` + the small set of
/// distinct status values.
pub fn count_by_status(conn: &mut DbConnection) -> Result<Vec<(String, i64)>, DieselError> {
    use diesel::dsl::count_star;
    outbound_emails::table
        .group_by(outbound_emails::status)
        .select((outbound_emails::status, count_star()))
        .load::<(String, i64)>(conn)
}

// sync-audit-only: read-side health probe; counts pending rows on the outbound queue
/// Pending-row gauge: count + age of the oldest. Drives the SLA alert
/// "outbound queue is backed up." Returns `(count, oldest_age_seconds)`.
pub fn pending_health(conn: &mut DbConnection) -> Result<(i64, Option<i64>), DieselError> {
    #[derive(diesel::QueryableByName)]
    struct Row {
        #[diesel(sql_type = BigInt)]
        count: i64,
        #[diesel(sql_type = Nullable<BigInt>)]
        oldest_age_seconds: Option<i64>,
    }
    let row: Row = diesel::sql_query(
        r#"
        SELECT
            COUNT(*) AS count,
            EXTRACT(EPOCH FROM (now() - MIN(created_at)))::bigint AS oldest_age_seconds
        FROM outbound_emails
        WHERE status IN ('pending', 'failed')
        "#,
    )
    .get_result(conn)?;
    let _ = outbound_email_status::PENDING; // re-export sanity
    Ok((row.count, row.oldest_age_seconds))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::NewOutboundEmail;
    use crate::test_helpers::{setup_test_connection, TestFixtures};
    use chrono::Utc;

    fn seed_channel(conn: &mut DbConnection) -> i32 {
        TestFixtures::create_channel(conn, "email_imap").id
    }

    fn fresh_row(channel_id: i32, suffix: &str) -> NewOutboundEmail {
        NewOutboundEmail {
            channel_id: Some(channel_id),
            ticket_id: None,
            comment_id: None,
            recipient: format!("test-{suffix}@example.com"),
            subject: format!("test {suffix}"),
            body_text: format!("body {suffix}"),
            body_html: None,
            message_id: format!("test-{suffix}-{}@example.com", uuid::Uuid::now_v7()),
            in_reply_to: None,
            references_list: vec![],
            headers_json: serde_json::json!({}),
            correlation_id: None,
            idempotency_key: None,
            sender_identity: crate::models::outbound_email_sender_identity::WORKSPACE.to_string(),
        }
    }

    #[test]
    fn enqueue_and_get_round_trip() {
        let mut conn = setup_test_connection();
        let ch = seed_channel(&mut conn);
        let row = enqueue(&mut conn, fresh_row(ch, "rt")).expect("enqueue");
        let fetched = get(&mut conn, row.id).expect("get");
        assert_eq!(fetched.recipient, row.recipient);
        assert_eq!(fetched.status, outbound_email_status::PENDING);
        assert_eq!(fetched.attempts, 0);
    }

    #[test]
    fn claim_batch_picks_pending_rows_and_sets_lease() {
        let mut conn = setup_test_connection();
        let ch = seed_channel(&mut conn);
        let r1 = enqueue(&mut conn, fresh_row(ch, "c1")).unwrap();
        let r2 = enqueue(&mut conn, fresh_row(ch, "c2")).unwrap();

        let claimed = claim_batch(&mut conn, 5, 300).expect("claim");
        let claimed_ids: Vec<i64> = claimed.iter().map(|r| r.id).collect();
        assert!(claimed_ids.contains(&r1.id));
        assert!(claimed_ids.contains(&r2.id));
        for row in &claimed {
            assert_eq!(row.status, outbound_email_status::SENDING);
            assert_eq!(row.attempts, 1);
            assert!(row.lease_token.is_some());
            assert!(row.lease_expires_at.is_some());
        }
    }

    #[test]
    fn claim_batch_skips_future_next_attempt() {
        let mut conn = setup_test_connection();
        let ch = seed_channel(&mut conn);
        // Enqueue then push next_attempt_at into the future via mark_failed.
        let r = enqueue(&mut conn, fresh_row(ch, "future")).unwrap();
        mark_failed(
            &mut conn,
            r.id,
            "transient",
            Some(421),
            Utc::now() + chrono::Duration::hours(1),
        )
        .unwrap();

        let claimed = claim_batch(&mut conn, 5, 300).expect("claim");
        let claimed_ids: Vec<i64> = claimed.iter().map(|r| r.id).collect();
        assert!(
            !claimed_ids.contains(&r.id),
            "row scheduled in the future should not be claimed"
        );
    }

    #[test]
    fn mark_sent_clears_lease_and_sets_terminal_state() {
        let mut conn = setup_test_connection();
        let ch = seed_channel(&mut conn);
        let r = enqueue(&mut conn, fresh_row(ch, "sent")).unwrap();
        let _ = claim_batch(&mut conn, 5, 300).unwrap();
        mark_sent(&mut conn, r.id, None).expect("mark_sent");
        let fetched = get(&mut conn, r.id).unwrap();
        assert_eq!(fetched.status, outbound_email_status::SENT);
        assert!(fetched.sent_at.is_some());
        assert!(fetched.lease_token.is_none());
        assert!(fetched.lease_expires_at.is_none());
    }

    #[test]
    fn mark_failed_reschedules_and_clears_lease() {
        let mut conn = setup_test_connection();
        let ch = seed_channel(&mut conn);
        let r = enqueue(&mut conn, fresh_row(ch, "f")).unwrap();
        let _ = claim_batch(&mut conn, 5, 300).unwrap();
        let next = Utc::now() + chrono::Duration::minutes(2);
        mark_failed(&mut conn, r.id, "boom", Some(450), next).expect("mark_failed");
        let fetched = get(&mut conn, r.id).unwrap();
        assert_eq!(fetched.status, outbound_email_status::FAILED);
        assert_eq!(fetched.last_error.as_deref(), Some("boom"));
        assert_eq!(fetched.last_smtp_code, Some(450));
        assert!(fetched.lease_token.is_none());
    }

    #[test]
    fn mark_dead_is_terminal() {
        let mut conn = setup_test_connection();
        let ch = seed_channel(&mut conn);
        let r = enqueue(&mut conn, fresh_row(ch, "d")).unwrap();
        let _ = claim_batch(&mut conn, 5, 300).unwrap();
        mark_dead(&mut conn, r.id, "permanent", Some(550)).expect("mark_dead");
        let fetched = get(&mut conn, r.id).unwrap();
        assert_eq!(fetched.status, outbound_email_status::DEAD);
        assert!(fetched.failed_at.is_some());

        // A subsequent claim cycle MUST NOT pick this up — the partial
        // index excludes 'dead' specifically.
        let claimed = claim_batch(&mut conn, 5, 300).expect("claim");
        let ids: Vec<i64> = claimed.iter().map(|r| r.id).collect();
        assert!(!ids.contains(&r.id));
    }

    #[test]
    fn release_claim_returns_to_pending_and_decrements_attempts() {
        let mut conn = setup_test_connection();
        let ch = seed_channel(&mut conn);
        let r = enqueue(&mut conn, fresh_row(ch, "rel")).unwrap();
        let _ = claim_batch(&mut conn, 5, 300).unwrap();
        // After claim: status=sending, attempts=1.
        release_claim(&mut conn, r.id).expect("release");
        let fetched = get(&mut conn, r.id).unwrap();
        assert_eq!(fetched.status, outbound_email_status::PENDING);
        assert_eq!(
            fetched.attempts, 0,
            "release_claim decrements the attempt the worker speculatively bumped"
        );
        assert!(fetched.lease_token.is_none());
    }

    #[test]
    fn sweep_expired_leases_recovers_orphaned_rows() {
        let mut conn = setup_test_connection();
        let ch = seed_channel(&mut conn);
        let r = enqueue(&mut conn, fresh_row(ch, "orphan")).unwrap();
        // Claim with a 0-second lease so it's instantly expired.
        let _ = claim_batch(&mut conn, 5, 0).unwrap();
        // Wait one tick so now() definitely exceeds lease_expires_at.
        std::thread::sleep(std::time::Duration::from_millis(50));
        let swept = sweep_expired_leases(&mut conn).expect("sweep");
        assert!(swept >= 1, "should have swept at least our orphan");
        let fetched = get(&mut conn, r.id).unwrap();
        assert_eq!(fetched.status, outbound_email_status::FAILED);
        assert!(fetched.lease_token.is_none());
    }

    #[test]
    fn retry_now_resets_dead_row() {
        let mut conn = setup_test_connection();
        let ch = seed_channel(&mut conn);
        let r = enqueue(&mut conn, fresh_row(ch, "retry")).unwrap();
        let _ = claim_batch(&mut conn, 5, 300).unwrap();
        mark_dead(&mut conn, r.id, "test", None).unwrap();

        retry_now(&mut conn, r.id).expect("retry_now");
        let fetched = get(&mut conn, r.id).unwrap();
        assert_eq!(fetched.status, outbound_email_status::PENDING);
        assert_eq!(fetched.attempts, 0);
        assert!(fetched.failed_at.is_none());
    }

    #[test]
    fn list_with_status_filter() {
        let mut conn = setup_test_connection();
        let ch = seed_channel(&mut conn);
        let pending = enqueue(&mut conn, fresh_row(ch, "p1")).unwrap();
        let _ = enqueue(&mut conn, fresh_row(ch, "p2")).unwrap();
        let r3 = enqueue(&mut conn, fresh_row(ch, "p3")).unwrap();
        let _ = claim_batch(&mut conn, 5, 300).unwrap();
        mark_sent(&mut conn, r3.id, None).unwrap();

        let page = list(
            &mut conn,
            &ListFilter {
                status: Some(vec![outbound_email_status::SENT.to_string()]),
                ..Default::default()
            },
            None,
            10,
        )
        .expect("list");
        let ids: Vec<i64> = page.rows.iter().map(|r| r.id).collect();
        assert!(ids.contains(&r3.id), "sent row should be in filter");
        assert!(!ids.contains(&pending.id), "pending row should be excluded");
    }
}
