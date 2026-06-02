//! Worker loop: claim → dispatch → terminate.
//!
//! This module exposes one entry point — [`run_one_drain`] — that the
//! listener / scheduler invokes when there's work to do. It claims a
//! batch via the repository's CTE-with-UPDATE-and-SKIP-LOCKED pattern,
//! dispatches each row through the channel adapter (currently
//! email_imap only; the trait refactor in Pass 3 broadens this), and
//! marks each row's terminal state.
//!
//! Crash safety: if the worker dies between claim and terminate, the
//! row stays `sending` with a lease. The lease sweeper
//! ([`crate::repository::outbound_emails::sweep_expired_leases`])
//! recovers it after the lease expires (5 min default). Combined with
//! the deterministic Message-ID, this is at-least-once delivery —
//! receiving MTAs and customer MUAs deduplicate on Message-ID.

use crate::db::Pool;
use crate::models::{NewChannelMessage, OutboundEmail};
use crate::repository::channels as channels_repo;
use crate::repository::outbound_emails as repo;
use crate::services::email_queue::circuit::{BreakerState, CircuitBreaker};
use crate::services::email_queue::retry::{classify, next_attempt_at, RetryDecision, MAX_ATTEMPTS};
use crate::utils::email::{EmailService, OutboundEmailMessage};
use anyhow::Result;
use std::sync::Arc;
use tracing::{debug, info, info_span, warn, Instrument};

/// channel_messages.direction value for outbound rows. Mirrors the
/// constant in models.rs (CHANNEL_DIRECTION_OUTBOUND); inlined here to
/// avoid an import cycle if the constant ever moves.
const CHANNEL_DIRECTION_OUTBOUND: &str = "outbound";

/// Lease length for in-flight rows. A worker that crashes mid-send
/// orphans the row; the lease sweeper recovers it after this expires.
/// 5 min is comfortably above any reasonable SMTP RTT, comfortably
/// below "operator notices the queue is stuck."
const LEASE_SECONDS: i64 = 300;

/// Snapshot of one drain pass. Used for periodic-job status reporting.
#[derive(Debug, Default, Clone, Copy)]
pub struct WorkerStats {
    pub claimed: usize,
    pub sent: usize,
    pub failed: usize,
    pub dead: usize,
    pub suppressed: usize,
    pub circuit_skipped: usize,
}

/// Drive one drain cycle: claim a batch, dispatch each row, terminate
/// it. Returns stats. Designed to be called in a loop by the listener
/// (on `pg_notify`) and by the periodic safety-net tick.
///
/// `email` is the shared EmailService used by the existing fire-and-
/// forget path; passing it in lets us share the SMTP transport and
/// the breaker state across listener-triggered and periodic invocations.
pub async fn run_one_drain(
    pool: Pool,
    email: Arc<EmailService>,
    breaker: Arc<CircuitBreaker>,
) -> Result<WorkerStats> {
    let mut stats = WorkerStats::default();

    // Don't even claim if the breaker is open — there's no point
    // moving rows to `sending` only to release them.
    if !breaker.allow().await {
        debug!("email_queue: circuit open, skipping drain");
        stats.circuit_skipped = 1;
        return Ok(stats);
    }

    // Claim batch under bypass — outbound_emails is RLS-enabled
    // (Phase 3c.2) and the worker is a platform-level scheduler
    // that drains across whatever rows are ready; no request-bound
    // workspace pin exists here.
    let claimed =
        crate::sync::session::background_run(&pool, "background:email_queue_claim", |conn| {
            repo::claim_batch(conn, repo::DEFAULT_BATCH_SIZE, LEASE_SECONDS)
        })
        .map_err(|e| anyhow::anyhow!("claim_batch failed: {e}"))?;
    stats.claimed = claimed.len();
    if claimed.is_empty() {
        return Ok(stats);
    }

    for row in claimed {
        let span = info_span!(
            "email_queue.send",
            queue_id = row.id,
            channel_id = row.channel_id,
            ticket_id = row.ticket_id,
            comment_id = row.comment_id,
            attempt = row.attempts,
            message_id = %row.message_id,
            recipient_domain = recipient_domain(&row.recipient).unwrap_or("?"),
            correlation_id = row.correlation_id.map(|id| id.to_string()).unwrap_or_default(),
        );
        let pool = pool.clone();
        let email = email.clone();
        let breaker = breaker.clone();
        async move {
            let outcome = dispatch(&row, email, breaker.clone()).await;
            // Terminate (update outbound_emails row status) under
            // bypass for the same RLS reason.
            let term_result = crate::sync::session::background_run(
                &pool,
                "background:email_queue_terminate",
                |conn| {
                    terminate_row(conn, &row, outcome, &mut stats);
                    Ok::<_, diesel::result::Error>(())
                },
            );
            if let Err(e) = term_result {
                warn!(error = %e, "could not terminate row");
            }
        }
        .instrument(span)
        .await;
    }

    Ok(stats)
}

/// Dispatch one row to SMTP. Returns the outcome the terminate step
/// uses to update the row's status. Records breaker success/failure.
async fn dispatch(
    row: &OutboundEmail,
    email: Arc<EmailService>,
    breaker: Arc<CircuitBreaker>,
) -> DispatchOutcome {
    // Re-check the breaker right before send — a different worker
    // task may have tripped it while we were processing the batch.
    if breaker.state().await == BreakerState::Open {
        return DispatchOutcome::CircuitSkip;
    }

    let references: Vec<String> = row
        .references_list
        .iter()
        .filter_map(|r| r.as_deref().map(str::to_owned))
        .collect();
    // Headers_json may carry extra MIME headers (Auto-Submitted,
    // X-Auto-Response-Suppress) the producer wanted on the wire.
    // For Pass 1 we honour `auto_submitted` only — the full custom-
    // header surface lands later if needed.
    let auto_submitted = row
        .headers_json
        .get("Auto-Submitted")
        .and_then(|v| v.as_str())
        .map(|s| s.eq_ignore_ascii_case("auto-replied"))
        .unwrap_or(false);

    let message = OutboundEmailMessage {
        to: &row.recipient,
        subject: &row.subject,
        body_text: &row.body_text,
        body_html: row.body_html.as_deref(),
        message_id: &row.message_id,
        in_reply_to: row.in_reply_to.as_deref(),
        references: &references,
        auto_submitted,
    };

    match email.send_ticket_reply(message).await {
        Ok(()) => {
            breaker.record_success().await;
            DispatchOutcome::Sent
        }
        Err(err) => {
            // EmailService::send_ticket_reply returns a String today.
            // We can't recover an SMTP code from it without surgery on
            // lettre's error type; do a coarse classification by
            // looking for a numeric code at the start of the message
            // (lettre format is `Failed to send ticket reply: …code…`).
            // Pass 1 ships this conservative form; Pass 2's adapter
            // refactor can return a structured SmtpError.
            let code = parse_smtp_code(&err);
            breaker.record_failure().await;
            DispatchOutcome::Failed { error: err, code }
        }
    }
}

#[derive(Debug)]
enum DispatchOutcome {
    Sent,
    Failed { error: String, code: Option<u16> },
    CircuitSkip,
}

fn terminate_row(
    conn: &mut crate::db::DbConnection,
    row: &OutboundEmail,
    outcome: DispatchOutcome,
    stats: &mut WorkerStats,
) {
    match outcome {
        DispatchOutcome::Sent => {
            // Two writes on success:
            //   1. Flip the queue row to `sent` (terminal).
            //   2. Record an outbound `channel_messages` row so later
            //      inbound replies thread back via the existing
            //      external_id lookup. The Message-ID we stamped at
            //      enqueue is the external_id consumers will see.
            if let Err(e) = repo::mark_sent(conn, row.id) {
                warn!(error = %e, queue_id = row.id, "mark_sent failed");
                return;
            }
            // Channel-reply rows record an outbound `channel_messages`
            // row so later inbound replies thread back via the
            // existing external_id lookup. Transactional rows
            // (channel_id = NULL) don't have a thread to anchor and
            // skip the bookkeeping; they're send-and-done.
            let Some(channel_id) = row.channel_id else {
                return;
            };
            let new_msg = NewChannelMessage {
                channel_id,
                external_id: format!("<{}>", row.message_id),
                direction: CHANNEL_DIRECTION_OUTBOUND.into(),
                ticket_id: row.ticket_id,
                comment_id: row.comment_id,
                in_reply_to: row.in_reply_to.clone(),
                from_address: None,
                author_user_uuid: None,
                raw_metadata: None,
            };
            if let Err(e) = channels_repo::record_message(conn, new_msg) {
                // Non-fatal: the email was sent; we just lost the
                // ledger row. Future inbound thread-resolution may
                // miss this anchor but the customer still got the
                // reply.
                warn!(
                    error = %e,
                    queue_id = row.id,
                    "channel_messages record_message failed after sent"
                );
            }
            stats.sent += 1;
            info!(queue_id = row.id, "email sent");
        }
        DispatchOutcome::Failed { error, code } => {
            // attempts has already been incremented by claim_batch.
            let decision = classify(code, row.attempts);
            match decision {
                RetryDecision::Retry => {
                    let next = next_attempt_at(chrono::Utc::now(), row.attempts);
                    if let Err(e) =
                        repo::mark_failed(conn, row.id, &error, code.map(i32::from), next)
                    {
                        warn!(error = %e, queue_id = row.id, "mark_failed failed");
                    } else {
                        stats.failed += 1;
                        info!(
                            queue_id = row.id,
                            attempts = row.attempts,
                            smtp_code = ?code,
                            error = %error,
                            next_attempt_at = %next,
                            "email send failed; will retry"
                        );
                    }
                }
                RetryDecision::Dead => {
                    if let Err(e) = repo::mark_dead(conn, row.id, &error, code.map(i32::from)) {
                        warn!(error = %e, queue_id = row.id, "mark_dead failed");
                    } else {
                        stats.dead += 1;
                        warn!(
                            queue_id = row.id,
                            attempts = row.attempts,
                            smtp_code = ?code,
                            error = %error,
                            "email send permanently failed"
                        );
                    }
                }
                RetryDecision::Suppress => {
                    if let Err(e) = repo::mark_dead(conn, row.id, &error, code.map(i32::from)) {
                        warn!(error = %e, queue_id = row.id, "mark_dead (suppress) failed");
                    } else {
                        stats.dead += 1;
                        // Pass 2 wires the suppression list update here.
                        // For Pass 1 we mark dead and warn; the suppression
                        // table doesn't exist yet.
                        warn!(
                            queue_id = row.id,
                            recipient = %row.recipient,
                            smtp_code = ?code,
                            "email send rejected as bad recipient (suppression list pending Pass 2)"
                        );
                    }
                }
            }
        }
        DispatchOutcome::CircuitSkip => {
            // Don't burn an attempt — release the claim so the row goes
            // back to `pending` with attempts decremented to its prior
            // value. Next drain after the breaker recovers picks it up.
            if let Err(e) = repo::release_claim(conn, row.id) {
                warn!(error = %e, queue_id = row.id, "release_claim failed");
            } else {
                stats.circuit_skipped += 1;
                debug!(queue_id = row.id, "circuit open, releasing claim");
            }
        }
    }
    let _ = MAX_ATTEMPTS; // satisfies the unused-import lint when retry isn't directly referenced
}

fn recipient_domain(recipient: &str) -> Option<&str> {
    recipient.split_once('@').map(|(_, domain)| domain)
}

/// Lift a 3-digit SMTP reply code out of an error string. Best-effort:
/// lettre's `Send` error rendered to string typically contains the
/// reply text; if not, the classifier degrades to "no code" which
/// retries until MAX_ATTEMPTS.
fn parse_smtp_code(error_text: &str) -> Option<u16> {
    // Look for a 3-digit token, checking it's a valid SMTP class.
    for word in error_text.split(|c: char| !c.is_ascii_digit()) {
        if word.len() == 3 {
            if let Ok(code) = word.parse::<u16>() {
                if (200..=599).contains(&code) {
                    return Some(code);
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_smtp_code_finds_3xx_in_lettre_text() {
        assert_eq!(
            parse_smtp_code("Failed to send: 550 No such user"),
            Some(550)
        );
        assert_eq!(
            parse_smtp_code("transient: 421 service not available"),
            Some(421)
        );
    }

    #[test]
    fn parse_smtp_code_returns_none_when_absent() {
        assert_eq!(parse_smtp_code("connection reset"), None);
        assert_eq!(parse_smtp_code(""), None);
    }

    #[test]
    fn parse_smtp_code_ignores_random_3digit_numbers() {
        // 999 isn't a valid SMTP class so we don't pick it up.
        assert_eq!(parse_smtp_code("error 999 ms timeout"), None);
    }

    #[test]
    fn recipient_domain_extracts_the_domain() {
        assert_eq!(recipient_domain("alice@example.com"), Some("example.com"));
        assert_eq!(recipient_domain("no-at-sign"), None);
    }
}
