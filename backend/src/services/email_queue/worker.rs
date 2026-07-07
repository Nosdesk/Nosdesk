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
use crate::models::{outbound_email_sender_identity, NewChannelMessage, OutboundEmail};
use crate::repository::channels as channels_repo;
use crate::repository::outbound_emails as repo;
use crate::services::email_queue::circuit::{BreakerState, CircuitBreaker, CircuitBreakerRegistry};
use crate::services::email_queue::retry::{classify, next_attempt_at, RetryDecision, MAX_ATTEMPTS};
use crate::services::outbound_email::OutboundEmailResolver;
use crate::utils::email::{EmailService, OutboundEmailMessage};
use anyhow::Result;
use std::collections::HashSet;
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
    /// Rows whose sending identity isn't configured yet (no workspace
    /// identity and no env fallback). Released without burning an attempt,
    /// so they send once an admin configures one.
    pub unconfigured: usize,
}

/// Drive one drain cycle: claim a batch, dispatch each row, terminate
/// it. Returns stats. Designed to be called in a loop by the listener
/// (on `pg_notify`) and by the periodic safety-net tick.
///
/// `resolver` maps each row to the `EmailService` to send it with: the
/// row's workspace identity (or the env fallback) for conversation /
/// notification mail, the instance identity for auth mail. The breaker
/// state is shared across listener-triggered and periodic invocations.
pub async fn run_one_drain(
    pool: Pool,
    resolver: Arc<OutboundEmailResolver>,
    registry: Arc<CircuitBreakerRegistry>,
) -> Result<WorkerStats> {
    let mut stats = WorkerStats::default();

    // The instance relay carries platform/auth mail and is the default for
    // workspace mail, so if its breaker is open there's little point claiming a
    // batch only to release it. Skip the whole drain then, preserving the old
    // single-breaker behaviour for the common case. Per-host breakers below
    // still isolate an individual tenant relay within a drain, so one broken
    // tenant relay never pauses the instance relay's mail.
    let platform = resolver.platform();
    if let Some(p) = &platform {
        let host = p.config().smtp_host.clone();
        if !registry.for_host(&host).await.allow().await {
            debug!("email_queue: instance relay circuit open, skipping drain");
            stats.circuit_skipped = 1;
            return Ok(stats);
        }
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

    // Resolve a sending identity for the drain before the send loop.
    // Workspace-identity rows resolve their workspace's own SMTP (or the env
    // fallback); platform rows use the instance identity. Read every
    // workspace identity for this batch in one checkout rather than one per
    // row. A resolve failure defers the workspace mail (empty map → the
    // rows take the `Unconfigured` branch) instead of erroring the drain.
    let workspace_ids: Vec<i32> = claimed
        .iter()
        .filter(|r| r.sender_identity != outbound_email_sender_identity::PLATFORM)
        .map(|r| r.workspace_id)
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    let ws_services = if workspace_ids.is_empty() {
        std::collections::HashMap::new()
    } else {
        crate::sync::session::background_run(&pool, "background:email_queue_resolve", |conn| {
            resolver.resolve_batch(conn, &workspace_ids)
        })
        .unwrap_or_else(|e| {
            warn!(error = %e, "email_queue: resolving workspace identities failed; deferring this drain's workspace mail");
            std::collections::HashMap::new()
        })
    };
    // `platform` was resolved above for the pre-claim breaker check; reuse it.

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
        let registry = registry.clone();
        // The identity to send this row with: the instance identity for
        // auth mail, otherwise the workspace's resolved service. `None`
        // means nothing is configured to send it yet.
        let service = if row.sender_identity == outbound_email_sender_identity::PLATFORM {
            platform.clone()
        } else {
            ws_services.get(&row.workspace_id).cloned()
        };
        async move {
            // Pre-send: skip recipients already on the suppression list
            // (prior hard bounce or complaint) — but ONLY for opt-out-able
            // NOTIFICATION mail. Transactional mail (password reset, invitation,
            // MFA, the agent's own reply, auto-ack) is must-deliver: a hard
            // bounce or a *forged* DSN must never be able to add a victim's
            // address to the list and thereby lock them out of account recovery.
            // Reputation protection for conversation mail is enforced earlier, at
            // enqueue time (`enqueue_or_suppress`) and on the direct channel-send
            // paths, where the block is surfaced rather than silently dropping a
            // human's reply. The list is global (no workspace_id), so a bypass
            // read is fine; a failed lookup falls through to attempting the send.
            let suppressed = if row.mail_class
                == crate::models::outbound_email_mail_class::NOTIFICATION
            {
                crate::sync::session::background_run(
                    &pool,
                    "background:email_queue_suppress_check",
                    |conn| {
                        crate::repository::email_suppressions::is_suppressed(conn, &row.recipient)
                    },
                )
                .unwrap_or(false)
            } else {
                false
            };
            let outcome = if suppressed {
                DispatchOutcome::Suppressed
            } else {
                match service {
                    // Use the breaker for this row's relay host, so a broken
                    // tenant relay trips only its own transport.
                    Some(svc) => {
                        let breaker = registry.for_host(&svc.config().smtp_host).await;
                        dispatch(&row, svc, breaker).await
                    }
                    None => DispatchOutcome::Unconfigured,
                }
            };
            // Terminate (update outbound_emails status, and for channel
            // replies record the outbound channel_messages row) pinned to
            // the row's workspace. Bypass alone isn't enough: the
            // channel_messages.workspace_id column default reads
            // app.workspace_id, so without the pin the insert writes NULL
            // and trips the NOT NULL constraint.
            let term_result = crate::sync::session::background_run_in_workspace(
                &pool,
                "background:email_queue_terminate",
                row.workspace_id,
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
    // headers_json carries the producer's Auto-Submitted value
    // ("auto-generated" for transactional/notification mail,
    // "auto-replied" for a direct reply). Pass it through verbatim so the
    // RFC 3834 + Exchange loop-prevention headers actually ship; any value
    // other than "no" marks the message as system-authored. (Previously
    // only "auto-replied" was honoured, so transactional mail silently
    // shipped without the headers.)
    let auto_submitted = row
        .headers_json
        .get("Auto-Submitted")
        .and_then(|v| v.as_str())
        .filter(|s| !s.eq_ignore_ascii_case("no"));

    // B3: producer-supplied Reply-To (channel inbound mailbox), same bag as
    // Auto-Submitted. Absent / blank means no header.
    let reply_to = row
        .headers_json
        .get("Reply-To")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());

    // B1: VERP Return-Path so a bounce links back to THIS row by token, even
    // when the remote MTA doesn't echo our Message-ID. The base is the polled
    // mailbox (= the Reply-To target), so the bounce lands where replies already
    // do. Off unless SMTP_VERP_SECRET is set, so existing deployments are
    // unchanged until an operator enables and deliverability-tests it.
    let envelope_from: Option<String> = crate::utils::verp::configured_secret()
        .zip(reply_to)
        .and_then(|(secret, base)| crate::utils::verp::tagged_return_path(base, row.id, &secret));

    // B2: producer-supplied one-click unsubscribe URL, set only on notification
    // mail. Same headers_json bag as Reply-To.
    let list_unsubscribe = row
        .headers_json
        .get("List-Unsubscribe")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());

    let message = OutboundEmailMessage {
        to: &row.recipient,
        subject: &row.subject,
        body_text: &row.body_text,
        body_html: row.body_html.as_deref(),
        message_id: &row.message_id,
        in_reply_to: row.in_reply_to.as_deref(),
        references: &references,
        auto_submitted,
        mail_class: &row.mail_class,
        reply_to,
        envelope_from: envelope_from.as_deref(),
        list_unsubscribe,
    };

    match email.send_outbound(&message).await {
        Ok(outcome) => {
            breaker.record_success().await;
            DispatchOutcome::Sent {
                provider_message_id: outcome.provider_message_id,
            }
        }
        Err(err) => {
            // `err` is a structured SmtpError: `smtp_code()` is lettre's
            // authoritative status from the server reply (not scraped from
            // text), so the retry / suppression decisions below key off a real
            // code. The error's Display text is persisted as the row's last
            // error for operators.
            let code = err.smtp_code();
            breaker.record_failure().await;
            DispatchOutcome::Failed {
                error: err.to_string(),
                code,
            }
        }
    }
}

#[derive(Debug)]
enum DispatchOutcome {
    Sent {
        provider_message_id: Option<String>,
    },
    Failed {
        error: String,
        code: Option<u16>,
    },
    CircuitSkip,
    /// Recipient is on the suppression list; no SMTP attempt was made.
    Suppressed,
    /// No sending identity is configured for this row yet (no workspace
    /// identity and no env fallback). No SMTP attempt was made.
    Unconfigured,
}

fn terminate_row(
    conn: &mut crate::db::DbConnection,
    row: &OutboundEmail,
    outcome: DispatchOutcome,
    stats: &mut WorkerStats,
) {
    match outcome {
        DispatchOutcome::Sent {
            provider_message_id,
        } => {
            // Two writes on success:
            //   1. Flip the queue row to `sent` (terminal), persisting any
            //      provider message id (None for SMTP).
            //   2. Record an outbound `channel_messages` row so later
            //      inbound replies thread back via the existing
            //      external_id lookup. The Message-ID we stamped at
            //      enqueue is the external_id consumers will see.
            if let Err(e) = repo::mark_sent(conn, row.id, provider_message_id.as_deref()) {
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
                    // An authoritative 550/551/553 recipient reject. The code now
                    // comes from lettre's structured status (SmtpError::smtp_code),
                    // not a scrape of the error text, so it's safe to act on: a
                    // transient message that merely contained "550" can no longer
                    // masquerade as a hard reject. Dead-letter the row AND add the
                    // recipient to the global suppression list so future sends
                    // short-circuit before the relay. Mirrors the inbound DSN path
                    // (`bounce_parser` -> `email_suppressions::upsert`).
                    if let Err(e) = repo::mark_dead(conn, row.id, &error, code.map(i32::from)) {
                        warn!(error = %e, queue_id = row.id, "mark_dead (suppress) failed");
                    } else {
                        stats.dead += 1;
                        let suppression = crate::models::NewEmailSuppression {
                            email: row.recipient.clone(),
                            reason: crate::models::email_suppression_reason::HARD_BOUNCE
                                .to_string(),
                            bounce_diagnostic: Some(error.clone()),
                        };
                        match crate::repository::email_suppressions::upsert(conn, suppression) {
                            Ok(_) => {
                                stats.suppressed += 1;
                                warn!(
                                    queue_id = row.id,
                                    recipient = %row.recipient,
                                    smtp_code = ?code,
                                    "email rejected as bad recipient; dead-lettered and recipient suppressed"
                                );
                            }
                            Err(e) => warn!(
                                error = %e,
                                recipient = %row.recipient,
                                "email_suppressions upsert failed; row dead-lettered but recipient not suppressed"
                            ),
                        }
                    }
                }
            }
        }
        DispatchOutcome::Suppressed => {
            // No SMTP attempt was made: the recipient is already suppressed.
            // Mark dead so it won't retry.
            if let Err(e) = repo::mark_dead(conn, row.id, "recipient on suppression list", None) {
                warn!(error = %e, queue_id = row.id, "mark_dead (pre-send suppressed) failed");
            } else {
                stats.suppressed += 1;
                info!(
                    queue_id = row.id,
                    recipient = %row.recipient,
                    "email skipped: recipient on suppression list"
                );
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
        DispatchOutcome::Unconfigured => {
            // No identity to send with yet. Release the claim without
            // burning an attempt, like CircuitSkip, so the row sends once
            // an admin configures a workspace identity (or env SMTP).
            if let Err(e) = repo::release_claim(conn, row.id) {
                warn!(error = %e, queue_id = row.id, "release_claim failed");
            } else {
                stats.unconfigured += 1;
                debug!(
                    queue_id = row.id,
                    sender_identity = %row.sender_identity,
                    "no sending identity configured, releasing claim"
                );
            }
        }
    }
    let _ = MAX_ATTEMPTS; // satisfies the unused-import lint when retry isn't directly referenced
}

fn recipient_domain(recipient: &str) -> Option<&str> {
    recipient.split_once('@').map(|(_, domain)| domain)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recipient_domain_extracts_the_domain() {
        assert_eq!(recipient_domain("alice@example.com"), Some("example.com"));
        assert_eq!(recipient_domain("no-at-sign"), None);
    }
}
