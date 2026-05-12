//! Inbound event pipeline.
//!
//! Given a normalized [`InboundEvent`] emitted by some adapter, this
//! module runs it through the fixed stages:
//!
//!   1. **Dispatch on variant.** Only `MessageReceived` is handled in
//!      phase 1. Edits/deletes log and skip — task #15 / #18 will wire
//!      them in once the lifecycle model is settled.
//!   2. **Loop filter.** `LoopMarkers::any()` aborts before any DB work,
//!      so our auto-reply never ping-pongs with an out-of-office.
//!   3. **Dedup.** `channel_messages.(channel_id, external_id, direction)`
//!      is unique; if we've already ingested this message we stop.
//!   4. **Thread resolve.** Adapter-supplied cascade (defaults to
//!      [`super::threading::default_explicit_threading`]) returns
//!      `Some(ticket_id)` for a reply and `None` for a new ticket.
//!   5. **Identity resolve.** Re-uses [`find_or_create_guest_user`] so
//!      drive-by reporters land in the same auto-provisioned state as
//!      public guest submissions.
//!   6. **Persist.** Open ticket (if new) → insert comment → record the
//!      `channel_messages` row with `ticket_id`/`comment_id` links.
//!   7. **Side effects.** Materialize attachments, broadcast SSE, index
//!      for search. Each is optional (via `PipelineContext`) so unit
//!      tests can exercise the pure DB path without live services.
//!
//! The pipeline is deliberately ignorant of *which* channel raised the
//! event — it only reads `channel.provider` and `InboundMessage` fields
//! defined in [`super`]. Adapters that need custom threading override
//! `resolve_thread`; adapters that need custom attachment handling
//! override [`materialize_attachment`]'s External branch by populating
//! Inline before handing to the pipeline.

use std::sync::Arc;

use actix_web::web;
use diesel::Connection;
use serde_json::json;
use tracing::{debug, info, warn};

use crate::db::DbConnection;
use crate::models::{
    Channel, Comment, NewAttachment, NewChannelMessage, NewComment, NewTicket, Ticket,
    CHANNEL_DIRECTION_INBOUND,
};
use crate::repository::{channels as channels_repo, comments as comments_repo, tickets as tickets_repo};
use crate::repository::user_helpers::{find_or_create_guest_user, GuestUserResult};
use crate::sync::actor::ActorContext;
use crate::sync::session;
use crate::services::channels::{
    ChannelAdapter, InboundAttachment, InboundEvent, InboundMessage,
};
use crate::handlers::sse::{SseEvent, SseState};
use crate::utils::storage::Storage;

/// Outcome of processing a single inbound event. Returned for logging /
/// metrics / test assertions — the caller doesn't need to branch on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PipelineOutcome {
    /// A new ticket was opened from this message.
    TicketOpened { ticket_id: i32, comment_id: i32 },
    /// An existing ticket got a new reply.
    ReplyAppended { ticket_id: i32, comment_id: i32 },
    /// Message matched an existing `channel_messages` row — already
    /// processed in a previous run. No-op.
    SkippedDuplicate,
    /// `LoopMarkers` flagged this as an auto-reply / out-of-office.
    SkippedLoop,
    /// Inbound message was a delivery-status notification (DSN /
    /// bounce). Short-circuited here so it doesn't open a new
    /// ticket or trigger an auto-reply; future passes will link it
    /// to the original outbound row.
    SkippedBounce,
    /// Phase-1 only handles `MessageReceived`. Other variants are
    /// logged and skipped.
    SkippedUnsupportedVariant,
    /// Sender had no `known_email` — we can't auto-provision without
    /// one. Phase-1 email always has one; this is a safety net for
    /// future chat adapters where some providers hide emails.
    SkippedNoIdentity,
    /// Email resolved to a verified/privileged account. Refuse to
    /// attach the message — the real owner needs to sign in properly.
    SkippedEmailClaimed,
}

/// Errors the pipeline can emit. Adapters can retry on [`Self::Db`] for
/// transient storage errors but [`Self::IdentityClaimed`] is a policy
/// decision, not a failure — it's represented as an `Outcome` variant
/// above rather than an error.
#[derive(Debug)]
pub enum PipelineError {
    Db(diesel::result::Error),
    Attachment(String),
}

impl std::fmt::Display for PipelineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Db(e) => write!(f, "db error: {e}"),
            Self::Attachment(m) => write!(f, "attachment error: {m}"),
        }
    }
}
impl std::error::Error for PipelineError {}

impl From<diesel::result::Error> for PipelineError {
    fn from(e: diesel::result::Error) -> Self {
        Self::Db(e)
    }
}

/// Optional runtime handles. All fields are `Option` so unit tests can
/// pass [`PipelineContext::bare`] and exercise the DB path alone.
#[derive(Clone, Default)]
pub struct PipelineContext {
    pub storage: Option<Arc<dyn Storage>>,
    pub sse: Option<web::Data<SseState>>,
    pub search: Option<Arc<crate::services::search::SearchService>>,
    pub http: Option<reqwest::Client>,
    /// SMTP handle used to send the auto-acknowledgement on newly
    /// opened tickets. `None` disables the auto-ack branch — unit
    /// tests leave this unset to avoid touching SMTP.
    pub email: Option<Arc<crate::utils::email::EmailService>>,
    /// Pool handle for the auto-ack spawn (needs to be cloneable into
    /// a `'static` task). Paired with `email`: both or neither should
    /// be set.
    pub pool: Option<crate::db::Pool>,
}

impl PipelineContext {
    /// Context with no side-effect handles. The DB writes still happen;
    /// storage / SSE / search / HTTP fetch are skipped with a debug log.
    pub fn bare() -> Self {
        Self::default()
    }
}

// ---------- Entry point ----------

/// Run a single inbound event through the pipeline.
pub async fn process_event(
    adapter: &dyn ChannelAdapter,
    channel: &Channel,
    event: InboundEvent,
    conn: &mut DbConnection,
    ctx: &PipelineContext,
) -> Result<PipelineOutcome, PipelineError> {
    let msg = match event {
        InboundEvent::MessageReceived(m) => m,
        InboundEvent::MessageEdited { external_id, .. } => {
            debug!(channel_id = channel.id, %external_id, "skip: edit events not yet handled");
            return Ok(PipelineOutcome::SkippedUnsupportedVariant);
        }
        InboundEvent::MessageDeleted { external_id, .. } => {
            debug!(channel_id = channel.id, %external_id, "skip: delete events not yet handled");
            return Ok(PipelineOutcome::SkippedUnsupportedVariant);
        }
    };

    if msg.is_bounce {
        // Idempotency: same DSN arriving twice (forwarder loop,
        // manual IMAP replay, multi-channel re-delivery) must not
        // re-process. The check happens up here rather than at the
        // general dedup point further down so a duplicate bounce
        // doesn't double-stamp the outbound row and double-count
        // the suppression's bounce_count.
        if channels_repo::find_by_external_id(conn, channel.id, &msg.external_id)?.is_some() {
            debug!(
                channel_id = channel.id,
                external_id = %msg.external_id,
                "skip: duplicate bounce DSN already processed"
            );
            return Ok(PipelineOutcome::SkippedDuplicate);
        }

        // Best-effort linkage to the originating outbound row. If
        // the DSN was malformed or the embedded original message
        // didn't carry our deterministic Message-ID, we still
        // short-circuit (so no ticket / auto-reply) but log the
        // miss so admins know coverage isn't complete.
        if let Some(report) = msg.bounce_report.as_ref() {
            match crate::repository::outbound_emails::mark_bounced(
                conn,
                &report.original_message_id,
                report.recipient.as_deref(),
                report.diagnostic.as_deref(),
            ) {
                Ok(0) => {
                    debug!(
                        channel_id = channel.id,
                        message_id = %report.original_message_id,
                        "bounce: no matching outbound row"
                    );
                }
                Ok(n) => {
                    debug!(
                        channel_id = channel.id,
                        message_id = %report.original_message_id,
                        rows = n,
                        recipient = ?report.recipient,
                        "bounce: linked to outbound row"
                    );
                }
                Err(e) => {
                    warn!(
                        channel_id = channel.id,
                        error = %e,
                        message_id = %report.original_message_id,
                        "bounce: failed to update outbound row"
                    );
                }
            }

            // Auto-suppress on hard bounces only. Soft bounces
            // (4xx — transient) shouldn't permanently block a
            // recipient; they retry naturally on the next send.
            if let (Some(recipient), Some(diagnostic)) =
                (report.recipient.as_deref(), report.diagnostic.as_deref())
            {
                if is_hard_bounce(diagnostic) {
                    let new = crate::models::NewEmailSuppression {
                        email: recipient.to_string(),
                        reason: crate::models::email_suppression_reason::HARD_BOUNCE.to_string(),
                        bounce_diagnostic: Some(diagnostic.to_string()),
                    };
                    if let Err(e) = crate::repository::email_suppressions::upsert(conn, new) {
                        warn!(
                            channel_id = channel.id,
                            error = %e,
                            recipient = %recipient,
                            "bounce: failed to add suppression"
                        );
                    } else {
                        debug!(
                            channel_id = channel.id,
                            recipient = %recipient,
                            "bounce: recipient auto-suppressed"
                        );
                    }
                }
            }
        } else {
            debug!(
                channel_id = channel.id,
                external_id = %msg.external_id,
                "bounce: detected but DSN was unparseable; no linkage"
            );
        }

        // Record the DSN itself so a re-arrival (forwarder loop,
        // replay, multi-channel redelivery) is caught by the
        // duplicate check at the top of this branch on the next
        // pass. ticket_id / comment_id are None because a bounce
        // doesn't open a ticket; from_address preserves the
        // postmaster-style sender for audit.
        if let Err(e) = channels_repo::record_message(
            conn,
            crate::models::NewChannelMessage {
                channel_id: channel.id,
                external_id: msg.external_id.clone(),
                direction: crate::models::CHANNEL_DIRECTION_INBOUND.to_string(),
                ticket_id: None,
                comment_id: None,
                in_reply_to: None,
                from_address: msg.from.known_email.clone(),
                author_user_uuid: None,
                raw_metadata: None,
            },
        ) {
            // Failing to record the dedup marker just means the
            // next arrival of this DSN will re-process — annoying
            // (one duplicate bounce_count bump) but not corrupting.
            // Don't let it fail the outer pipeline.
            warn!(
                channel_id = channel.id,
                error = %e,
                external_id = %msg.external_id,
                "bounce: failed to record dedup marker"
            );
        }
        return Ok(PipelineOutcome::SkippedBounce);
    }

    if msg.loop_markers.any() {
        debug!(
            channel_id = channel.id,
            external_id = %msg.external_id,
            "skip: loop/auto-reply markers present"
        );
        return Ok(PipelineOutcome::SkippedLoop);
    }

    // Dedup: if the same inbound external_id is already recorded, stop.
    if channels_repo::find_by_external_id(conn, channel.id, &msg.external_id)?.is_some() {
        debug!(
            channel_id = channel.id,
            external_id = %msg.external_id,
            "skip: message already ingested"
        );
        return Ok(PipelineOutcome::SkippedDuplicate);
    }

    // Identity: we require an email for now. Chat adapters that hide
    // emails will need a separate resolver in a later task.
    let Some(sender_email) = msg.from.known_email.clone() else {
        warn!(
            channel_id = channel.id,
            provider = %msg.from.provider,
            external_id = %msg.from.external_id,
            "skip: sender has no known email; cannot auto-provision"
        );
        return Ok(PipelineOutcome::SkippedNoIdentity);
    };

    // Thread resolution — `None` means "start a new ticket". Stays
    // outside the transaction below because it's a read-only lookup and
    // because the trait's `async fn` contract can't be invoked inside
    // Diesel's synchronous transaction closure. A concurrent ingest of
    // the same message would be caught by the UNIQUE constraint on
    // `channel_messages` at transaction commit, so the read-then-write
    // isn't a correctness hazard here.
    let existing_ticket_id = adapter.resolve_thread(&msg, channel.id, conn).await;

    // Atomically: identity resolve (which may create a guest user row)
    // → get-or-create ticket → insert comment → write the
    // `channel_messages` dedup row. Everything that writes during
    // ingestion happens inside this transaction, so a failure on any
    // step rolls the lot back. Before this was a transaction, a
    // partial success would orphan a guest user (if identity resolve
    // created one), a comment (if the dedup row write failed), and
    // let the next poll re-ingest into a second comment on the same
    // ticket.
    let ingest = conn.transaction::<_, PipelineError, _>(|conn| {
        // Attribute every emit in this transaction to the inbound
        // channel pipeline so sync_actions records the system actor
        // rather than NULL. The outer call has no HTTP request, so
        // we synthesise a system actor here.
        let actor = ActorContext::system("channels:inbound");
        session::set_actor(conn, &actor)?;

        // Identity resolution. Routes through the forward-aware
        // branch when the envelope sender is a verified tech;
        // otherwise falls into the normal guest-user path. See
        // `resolve_identity` for the full decision tree.
        let (sender, forwarded_by_uuid) = match resolve_identity(channel, &msg, &sender_email, conn, ctx)? {
            Resolved::Identified { user, forwarded_by } => (user, forwarded_by),
            // Skip paths write nothing; the transaction commits as a
            // no-op and the outer code turns `Ingest::Skip` into the
            // matching `PipelineOutcome`.
            Resolved::Skip(outcome) => return Ok(Ingest::Skip(outcome)),
        };
        let sender_uuid = sender.uuid;

        let (ticket, comment, is_new_ticket) = match existing_ticket_id {
            Some(ticket_id) => {
                let ticket = tickets_repo::get_ticket_by_id(conn, ticket_id)?;
                let comment =
                    insert_inbound_comment(conn, ticket.id, sender_uuid, &msg, forwarded_by_uuid, ctx)?;
                (ticket, comment, false)
            }
            None => {
                let ticket = open_ticket_from_message(conn, channel, &msg, sender_uuid)?;
                let comment =
                    insert_inbound_comment(conn, ticket.id, sender_uuid, &msg, forwarded_by_uuid, ctx)?;
                (ticket, comment, true)
            }
        };

        let in_reply_to = msg.references.first().cloned();
        channels_repo::record_message(
            conn,
            NewChannelMessage {
                channel_id: channel.id,
                external_id: msg.external_id.clone(),
                direction: CHANNEL_DIRECTION_INBOUND.to_string(),
                ticket_id: Some(ticket.id),
                comment_id: Some(comment.id),
                in_reply_to,
                from_address: Some(sender_email.clone()),
                author_user_uuid: Some(sender_uuid),
                raw_metadata: Some(msg.raw_metadata.clone()),
            },
        )?;

        Ok(Ingest::Done {
            ticket,
            comment,
            is_new_ticket,
            sender_uuid,
        })
    })?;

    let (ticket, comment, is_new_ticket, sender_uuid) = match ingest {
        Ingest::Skip(outcome) => return Ok(outcome),
        Ingest::Done {
            ticket,
            comment,
            is_new_ticket,
            sender_uuid,
        } => (ticket, comment, is_new_ticket, sender_uuid),
    };

    // Attachments — best effort. Each failure is logged and skipped so
    // one malformed file doesn't lose the whole message. Runs outside
    // the transaction because it may do network fetches (External
    // attachments) and storage writes, which shouldn't hold a DB row
    // lock for seconds at a time.
    persist_attachments(conn, &ctx, comment.id, sender_uuid, &msg.attachments).await;

    // Side effects (optional).
    if let Some(sse) = &ctx.sse {
        if is_new_ticket {
            sse.broadcast_event(SseEvent::TicketCreated {
                ticket_id: ticket.id,
                ticket: serde_json::to_value(&ticket).unwrap_or_default(),
                timestamp: chrono::Utc::now(),
            })
            .await;
        } else {
            sse.broadcast_event(SseEvent::CommentAdded {
                ticket_id: ticket.id,
                comment: serde_json::to_value(&comment).unwrap_or_default(),
                timestamp: chrono::Utc::now(),
            })
            .await;
        }
    }
    if let Some(search) = &ctx.search {
        if is_new_ticket {
            crate::services::search::indexing_tasks::spawn_index_ticket(
                search.clone(),
                ticket.clone(),
                None,
            );
        } else {
            crate::services::search::indexing_tasks::spawn_index_comment(
                search.clone(),
                comment.clone(),
                ticket.title.clone(),
            );
        }
    }

    info!(
        channel_id = channel.id,
        ticket_id = ticket.id,
        comment_id = comment.id,
        is_new_ticket,
        "ingested inbound channel message"
    );

    // Fire the system auto-acknowledgement when this message just
    // opened a fresh ticket. Gated on having both pool + email in
    // context — tests typically leave those unset. The spawn is
    // fire-and-forget so an SMTP hiccup never blocks ingestion.
    if is_new_ticket {
        if let (Some(pool), Some(email)) = (ctx.pool.clone(), ctx.email.clone()) {
            super::auto_ack::spawn_auto_ack(
                pool,
                email,
                channel.clone(),
                ticket.clone(),
                msg.external_id.clone(),
            );
        }
    }

    Ok(if is_new_ticket {
        PipelineOutcome::TicketOpened {
            ticket_id: ticket.id,
            comment_id: comment.id,
        }
    } else {
        PipelineOutcome::ReplyAppended {
            ticket_id: ticket.id,
            comment_id: comment.id,
        }
    })
}

/// Classify a DSN diagnostic as a hard or soft bounce.
///
/// "Hard" = permanent recipient-side failure (mailbox doesn't
/// exist, user disabled, etc.). These auto-populate the suppression
/// list so we stop wasting deliveries.
///
/// "Soft" = transient (4xx codes; mailbox full, server down). Not
/// suppressed because the next send will likely succeed.
///
/// Carve-outs from the 5xx universe per Mailgun / SES / Postmark
/// production patterns:
///   - `5.0.0` is "other / undefined" per RFC 3463 — too vague to
///     act on without risking a real customer.
///   - `5.7.x` is policy / security (SPF, DKIM, DMARC, content
///     filtering, greylisting). These are usually *sender*-side
///     problems; suppressing the recipient just because we failed
///     DKIM loses them forever for a configuration issue we own.
///
/// Default-deny: anything we can't classify counts as soft. Better
/// to retry a real address than to block a real customer over a
/// parsing miss.
fn is_hard_bounce(diagnostic: &str) -> bool {
    // Scan first, decide after. A single diagnostic often carries
    // both the basic SMTP code (550) and the enhanced status (5.7.1);
    // a token-by-token short-circuit would return on the basic code
    // before reaching the more-specific enhanced status and apply
    // the wrong verdict.
    let mut saw_hard_enhanced = false;
    let mut saw_carve_out_enhanced = false;
    let mut saw_basic_5xx = false;

    for word in diagnostic.split(|c: char| !c.is_ascii_alphanumeric() && c != '.') {
        // RFC 3463 enhanced status: `5.x.y`
        if word.starts_with("5.") && word.matches('.').count() == 2 {
            if word == "5.0.0" || word.starts_with("5.7.") {
                saw_carve_out_enhanced = true;
            } else {
                saw_hard_enhanced = true;
            }
            continue;
        }
        // SMTP basic 5xx code (bare 3-digit number starting with 5).
        if word.len() == 3 && word.starts_with('5') && word.chars().all(|c| c.is_ascii_digit()) {
            saw_basic_5xx = true;
        }
    }

    // Enhanced status is more specific than the basic SMTP code, so
    // when present it takes precedence. A carve-out enhanced status
    // (5.0.0 / 5.7.x) wins even if a basic 5xx is also in the line.
    if saw_carve_out_enhanced && !saw_hard_enhanced {
        return false;
    }
    saw_hard_enhanced || saw_basic_5xx
}

// ---------- DB helpers ----------

/// Outcome carried out of the ingest transaction. `Skip` signals that
/// identity resolution said stop (EmailClaimed, forwarded-tech-to-tech,
/// etc.) and the transaction committed a no-op; `Done` carries the
/// rows the transaction created so the caller can drive post-commit
/// side effects (attachments, SSE, search index, auto-ack).
enum Ingest {
    Skip(PipelineOutcome),
    Done {
        ticket: Ticket,
        comment: Comment,
        is_new_ticket: bool,
        sender_uuid: uuid::Uuid,
    },
}

/// Outcome of identity resolution. Either we've got a `User` to
/// attribute the ticket to (plus, optionally, the tech who forwarded
/// the message in), or we're bailing with a specific skip reason.
enum Resolved {
    Identified {
        user: crate::models::User,
        forwarded_by: Option<uuid::Uuid>,
    },
    Skip(PipelineOutcome),
}

/// Decide who the inbound message should be attributed to.
///
/// - If the envelope sender is a verified Nosdesk user AND the body
///   parses as a forward, the original customer becomes the requester
///   (auto-provisioned) and the tech is recorded on the comment for
///   audit. This is the tech-forward workflow mainstream helpdesks
///   (Zendesk, Freshdesk, Help Scout, Zammad) all implement.
/// - If the sender is verified but no forward markers exist, we
///   refuse to silently attribute the ticket to the tech and return
///   `SkippedEmailClaimed` — the tech will notice the missing ticket
///   and can re-send as a proper forward.
/// - Otherwise (unknown sender) we run the standard guest-user
///   auto-provision path.
fn resolve_identity(
    channel: &Channel,
    msg: &InboundMessage,
    sender_email: &str,
    conn: &mut DbConnection,
    ctx: &PipelineContext,
) -> Result<Resolved, PipelineError> {
    use crate::repository::user_helpers::find_verified_user_by_email;
    let observer = ctx
        .search
        .as_ref()
        .map(|s| s as &dyn crate::repository::user_helpers::UserCreatedObserver);

    let verified_sender = find_verified_user_by_email(sender_email, conn)?;
    if let Some(tech) = verified_sender {
        let Some(fwd) = super::forward_parser::extract(msg) else {
            info!(
                channel_id = channel.id,
                external_id = %msg.external_id,
                "skip: sender email is claimed by a verified/privileged account"
            );
            return Ok(Resolved::Skip(PipelineOutcome::SkippedEmailClaimed));
        };

        info!(
            channel_id = channel.id,
            external_id = %msg.external_id,
            forwarded_by = %tech.uuid,
            original = %fwd.email,
            "tech forward detected; attributing ticket to original sender"
        );
        let display = fwd.display_name.clone().unwrap_or_else(|| fwd.email.clone());
        match find_or_create_guest_user(&fwd.email, &display, conn, observer)? {
            GuestUserResult::Created(u) | GuestUserResult::Existing(u) => Ok(Resolved::Identified {
                user: u,
                forwarded_by: Some(tech.uuid),
            }),
            // Tech A forwarding tech B's message — refuse. Either
            // direction of attribution is surprising; we'd rather the
            // tech handle this manually.
            GuestUserResult::EmailClaimed => {
                info!(
                    channel_id = channel.id,
                    external_id = %msg.external_id,
                    "skip: forwarded sender is also a verified account"
                );
                Ok(Resolved::Skip(PipelineOutcome::SkippedEmailClaimed))
            }
        }
    } else {
        // Unknown sender — standard guest provisioning. `EmailClaimed`
        // shouldn't trigger here now that the verified lookup above
        // covers it, but the branch exists as a safety net against
        // races between this call and
        // `find_or_create_guest_user`'s own check.
        match find_or_create_guest_user(sender_email, &msg.from.display_name, conn, observer)? {
            GuestUserResult::Created(u) | GuestUserResult::Existing(u) => Ok(Resolved::Identified {
                user: u,
                forwarded_by: None,
            }),
            GuestUserResult::EmailClaimed => Ok(Resolved::Skip(PipelineOutcome::SkippedEmailClaimed)),
        }
    }
}

fn open_ticket_from_message(
    conn: &mut DbConnection,
    channel: &Channel,
    msg: &InboundMessage,
    requester_uuid: uuid::Uuid,
) -> Result<Ticket, diesel::result::Error> {
    let title = msg
        .subject
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("(no subject)")
        .to_string();

    // Inbound channel tickets land in the workspace default workflow
    // state (currently "Backlog"). Tech triages from there.
    let default_state = crate::repository::workflow_states::default_state(conn)?;
    let new_ticket = NewTicket {
        title,
        workflow_state_id: default_state.id,
        requester_uuid: Some(requester_uuid),
        submitted_via: Some(channel.provider.clone()),
        origin_channel_id: Some(channel.id),
        triage_state: Some("untriaged".into()),
        ..Default::default()
    };

    tickets_repo::create_ticket(conn, new_ticket)
}

fn insert_inbound_comment(
    conn: &mut DbConnection,
    ticket_id: i32,
    user_uuid: uuid::Uuid,
    msg: &InboundMessage,
    forwarded_by_user_uuid: Option<uuid::Uuid>,
    ctx: &PipelineContext,
) -> Result<Comment, diesel::result::Error> {
    let mut metadata = json!({
        "provider": msg.from.provider,
        "external_id": msg.external_id,
        "references": msg.references,
        "recipients": msg.recipients,
    });
    // When a tech forwarded the message into the helpdesk, stamp the
    // tech's uuid so the ticket view can render a "Forwarded by X"
    // note without having to re-parse the raw body. The sender's
    // address itself isn't duplicated here — it lives on the joined
    // `channel_messages.from_address` row and the comment-list
    // endpoint surfaces it as a top-level `from_address` DTO field.
    if let Some(by) = forwarded_by_user_uuid {
        metadata["forwarded_by_user_uuid"] = json!(by.to_string());
    }

    // Prefer the rich HTML body when the message has one — it carries
    // the formatting the customer's mail client emitted (lists, links,
    // signatures, embedded images via `cid:` references) and the
    // ticket view renders it in a sandboxed iframe so the structure is
    // preserved. Fall back to the flat `body_text` for plaintext-only
    // emails (mailing lists, bots, console-mail clients).
    let (content, content_format) = match msg.body_html.as_ref() {
        Some(html) if !html.trim().is_empty() => {
            (html.clone(), crate::models::ContentFormat::Html)
        }
        _ => (msg.body_text.clone(), crate::models::ContentFormat::Plaintext),
    };
    let new_comment = NewComment {
        content,
        ticket_id,
        user_uuid,
        channel_metadata: Some(metadata),
        is_internal: false,
        content_format,
    };

    let observer = ctx
        .search
        .as_ref()
        .map(|s| s as &dyn crate::repository::comments::CommentCreatedObserver);
    comments_repo::create_comment(conn, new_comment, observer)
}

async fn persist_attachments(
    conn: &mut DbConnection,
    ctx: &PipelineContext,
    comment_id: i32,
    uploader: uuid::Uuid,
    attachments: &[InboundAttachment],
) {
    let Some(storage) = ctx.storage.as_ref() else {
        if !attachments.is_empty() {
            debug!(
                count = attachments.len(),
                "skipping attachment persistence (no storage in pipeline context)"
            );
        }
        return;
    };

    for att in attachments {
        let materialized = match materialize_attachment(att, ctx.http.as_ref()).await {
            Ok(m) => m,
            Err(e) => {
                warn!(error = %e, "failed to materialize inbound attachment; skipping");
                continue;
            }
        };

        let stored = match storage
            .store_file(
                &materialized.bytes,
                &materialized.filename,
                &materialized.mime_type,
                "tickets",
            )
            .await
        {
            Ok(s) => s,
            Err(e) => {
                warn!(error = ?e, "failed to store inbound attachment; skipping");
                continue;
            }
        };

        let new_att = NewAttachment {
            url: stored.url,
            name: materialized.filename,
            file_size: Some(stored.size as i64),
            mime_type: Some(materialized.mime_type),
            checksum: None,
            comment_id: Some(comment_id),
            uploaded_by: Some(uploader),
            transcription: None,
        };
        if let Err(e) = comments_repo::create_attachment(conn, new_att) {
            warn!(error = %e, "failed to insert attachment row; file is stored but orphaned");
        }
    }
}

struct MaterializedAttachment {
    filename: String,
    mime_type: String,
    bytes: Vec<u8>,
}

/// Hard cap on individual inbound attachment size. 25 MiB is what Gmail
/// / Google Workspace caps outgoing at and what most corporate MTAs
/// accept inbound; legitimate tickets rarely exceed it. Enforced for
/// both [`InboundAttachment::Inline`] (already-in-memory bytes from
/// IMAP fetch) and [`InboundAttachment::External`] (URL-fetched blobs
/// from future chat adapters). Oversized attachments are dropped with
/// a warn log so the ticket still opens without them.
const MAX_ATTACHMENT_BYTES: usize = 25 * 1024 * 1024;

async fn materialize_attachment(
    att: &InboundAttachment,
    http: Option<&reqwest::Client>,
) -> Result<MaterializedAttachment, PipelineError> {
    match att {
        InboundAttachment::Inline {
            filename,
            mime_type,
            bytes,
        } => {
            if bytes.len() > MAX_ATTACHMENT_BYTES {
                return Err(PipelineError::Attachment(format!(
                    "inline attachment exceeds {} MiB cap ({} bytes)",
                    MAX_ATTACHMENT_BYTES / 1024 / 1024,
                    bytes.len()
                )));
            }
            Ok(MaterializedAttachment {
                filename: filename.clone(),
                mime_type: mime_type.clone(),
                bytes: bytes.clone(),
            })
        }
        InboundAttachment::External {
            filename,
            mime_type,
            url,
            auth_header,
            size_bytes,
        } => {
            // SSRF guard: scheme + host vetting before we hand the URL
            // to reqwest. The External branch is dormant today (email
            // attachments are all Inline); this protects the surface
            // when Slack / Teams adapters land and start producing
            // External references.
            let parsed = url::Url::parse(url)
                .map_err(|e| PipelineError::Attachment(format!("invalid url: {e}")))?;
            if !matches!(parsed.scheme(), "http" | "https") {
                return Err(PipelineError::Attachment(format!(
                    "rejected url scheme: {}",
                    parsed.scheme()
                )));
            }
            if let Some(host) = parsed.host_str() {
                if host_looks_internal(host) {
                    return Err(PipelineError::Attachment(format!(
                        "rejected internal/loopback host: {host}"
                    )));
                }
            } else {
                return Err(PipelineError::Attachment("url has no host".into()));
            }
            // Pre-flight size check when the adapter told us how big.
            // The stream-level cap below is the authoritative gate;
            // this just saves a round-trip on obvious offenders.
            if let Some(claimed) = size_bytes {
                if *claimed as usize > MAX_ATTACHMENT_BYTES {
                    return Err(PipelineError::Attachment(format!(
                        "external attachment claims {} bytes; over cap",
                        claimed
                    )));
                }
            }

            let client = http.ok_or_else(|| {
                PipelineError::Attachment(
                    "external attachment requires http client in pipeline context".into(),
                )
            })?;
            let mut req = client.get(url);
            if let Some((name, value)) = auth_header {
                req = req.header(name, value);
            }
            let resp = req
                .send()
                .await
                .map_err(|e| PipelineError::Attachment(format!("fetch failed: {e}")))?
                .error_for_status()
                .map_err(|e| PipelineError::Attachment(format!("upstream: {e}")))?;
            let bytes = resp
                .bytes()
                .await
                .map_err(|e| PipelineError::Attachment(format!("body: {e}")))?
                .to_vec();
            if bytes.len() > MAX_ATTACHMENT_BYTES {
                return Err(PipelineError::Attachment(format!(
                    "external attachment exceeds {} MiB cap ({} bytes)",
                    MAX_ATTACHMENT_BYTES / 1024 / 1024,
                    bytes.len()
                )));
            }
            Ok(MaterializedAttachment {
                filename: filename.clone(),
                mime_type: mime_type.clone(),
                bytes,
            })
        }
    }
}

/// Reject URLs pointing at loopback / link-local / RFC1918 ranges,
/// plus hostnames a resolver might turn into those. String-level
/// check is deliberately conservative — any refusal is recoverable
/// (the ticket opens without the attachment); a miss would be an
/// SSRF vulnerability.
fn host_looks_internal(host: &str) -> bool {
    let lower = host.to_ascii_lowercase();
    // Obvious DNS names.
    if matches!(
        lower.as_str(),
        "localhost" | "localhost.localdomain" | "metadata" | "metadata.google.internal"
    ) {
        return true;
    }
    // *.local (mDNS) and *.internal domains.
    if lower.ends_with(".local") || lower.ends_with(".internal") {
        return true;
    }
    // IPv4 literal? Block loopback, private, link-local, and the
    // AWS/GCP metadata address.
    if let Ok(ip) = lower.parse::<std::net::Ipv4Addr>() {
        return ip.is_loopback()
            || ip.is_private()
            || ip.is_link_local()
            || ip.is_broadcast()
            || ip.is_documentation()
            || ip.is_unspecified()
            || ip.octets() == [169, 254, 169, 254];
    }
    // IPv6 literal (with or without brackets).
    let bare = lower.trim_start_matches('[').trim_end_matches(']');
    if let Ok(ip) = bare.parse::<std::net::Ipv6Addr>() {
        return ip.is_loopback() || ip.is_unspecified() || ip.is_multicast();
    }
    false
}

// ---------- Tests ----------

#[cfg(test)]
mod tests {
    //! The pipeline tests exercise the DB-write path using `PipelineContext::bare()`
    //! and a stub adapter. They cover:
    //!
    //! - Loop short-circuit
    //! - Dedup on repeated external_id
    //! - New-ticket path (opens ticket, inserts comment, records message)
    //! - Reply path (appends to resolved ticket)
    //! - Edit/Delete variants skipped
    //! - Missing email → SkippedNoIdentity
    //! - Claimed email → SkippedEmailClaimed (via verified existing user)
    //!
    //! SSE / search / storage side effects are covered by the E2E task (#23).

    use super::*;
    use crate::models::{NewChannelMessage, UserRole, CHANNEL_DIRECTION_OUTBOUND};
    use crate::repository::user_helpers::create_user_with_email;
    use crate::services::channels::{
        threading::default_explicit_threading, ChannelError, ExternalIdentity, LoopMarkers,
        OutboundContent, OutboundMessage, ThreadContext,
    };
    use crate::test_helpers::{setup_test_connection, TestFixtures};
    use async_trait::async_trait;
    use chrono::Utc;

    struct StubAdapter;

    #[async_trait]
    impl ChannelAdapter for StubAdapter {
        fn id(&self) -> &str {
            "stub:0"
        }
        fn provider(&self) -> &'static str {
            "email_imap"
        }
        async fn send_reply(
            &self,
            _thread: &ThreadContext,
            _content: &OutboundContent,
        ) -> Result<OutboundMessage, ChannelError> {
            unreachable!("outbound is out of scope for pipeline tests")
        }
        async fn resolve_thread(
            &self,
            event: &InboundMessage,
            channel_id: i32,
            conn: &mut DbConnection,
        ) -> Option<i32> {
            default_explicit_threading(event, channel_id, conn).await
        }
    }

    fn sample_message(external_id: &str, references: Vec<String>, subject: Option<&str>) -> InboundMessage {
        InboundMessage {
            external_id: external_id.into(),
            from: ExternalIdentity {
                provider: "email_imap".into(),
                external_id: "alice@example.com".into(),
                display_name: "Alice".into(),
                known_email: Some("alice@example.com".into()),
            },
            subject: subject.map(str::to_string),
            body_text: "hello from alice".into(),
            body_html: None,
            attachments: vec![],
            references,
            received_at: Utc::now(),
            loop_markers: LoopMarkers::default(),
            raw_metadata: json!({"k": "v"}),
            recipients: vec!["support@yourco.com".into()],
            is_bounce: false,
            bounce_report: None,
        }
    }

    #[tokio::test]
    async fn opens_new_ticket_on_unthreaded_message() {
        let mut conn = setup_test_connection();
        let ch = TestFixtures::create_channel(&mut conn, "email_imap");
        let event =
            InboundEvent::MessageReceived(sample_message("<m1@ex>", vec![], Some("Printer fire")));

        let outcome = process_event(&StubAdapter, &ch, event, &mut conn, &PipelineContext::bare())
            .await
            .unwrap();

        let (ticket_id, comment_id) = match outcome {
            PipelineOutcome::TicketOpened { ticket_id, comment_id } => (ticket_id, comment_id),
            other => panic!("expected TicketOpened, got {other:?}"),
        };

        // Ticket linked back to channel.
        let ticket = tickets_repo::get_ticket_by_id(&mut conn, ticket_id).unwrap();
        assert_eq!(ticket.title, "Printer fire");
        assert_eq!(ticket.origin_channel_id, Some(ch.id));
        assert_eq!(ticket.submitted_via.as_deref(), Some("email_imap"));

        // channel_messages row persisted with linkage.
        let recorded = channels_repo::find_by_external_id(&mut conn, ch.id, "<m1@ex>")
            .unwrap()
            .expect("message should be recorded");
        assert_eq!(recorded.ticket_id, Some(ticket_id));
        assert_eq!(recorded.comment_id, Some(comment_id));
        assert_eq!(recorded.direction, CHANNEL_DIRECTION_INBOUND);
    }

    #[tokio::test]
    async fn falls_back_to_placeholder_title_for_empty_subject() {
        let mut conn = setup_test_connection();
        let ch = TestFixtures::create_channel(&mut conn, "email_imap");
        let event = InboundEvent::MessageReceived(sample_message("<m2@ex>", vec![], Some("   ")));

        let outcome = process_event(&StubAdapter, &ch, event, &mut conn, &PipelineContext::bare())
            .await
            .unwrap();
        let ticket_id = match outcome {
            PipelineOutcome::TicketOpened { ticket_id, .. } => ticket_id,
            other => panic!("{other:?}"),
        };
        let ticket = tickets_repo::get_ticket_by_id(&mut conn, ticket_id).unwrap();
        assert_eq!(ticket.title, "(no subject)");
    }

    #[tokio::test]
    async fn appends_reply_when_references_match_prior_outbound() {
        let mut conn = setup_test_connection();
        let ch = TestFixtures::create_channel(&mut conn, "email_imap");
        let user = TestFixtures::create_user(&mut conn, "tech", UserRole::Technician);
        let ticket = TestFixtures::create_ticket(&mut conn, "parent", Some(user.uuid), None);

        // Simulate an outbound we emitted for this ticket.
        channels_repo::record_message(
            &mut conn,
            NewChannelMessage {
                channel_id: ch.id,
                external_id: "<out-1@host>".into(),
                direction: CHANNEL_DIRECTION_OUTBOUND.into(),
                ticket_id: Some(ticket.id),
                comment_id: None,
                in_reply_to: None,
                from_address: None,
                author_user_uuid: None,
                raw_metadata: None,
            },
        )
        .unwrap();

        let event = InboundEvent::MessageReceived(sample_message(
            "<reply-1@ex>",
            vec!["<out-1@host>".into()],
            Some("Re: parent"),
        ));

        let outcome = process_event(&StubAdapter, &ch, event, &mut conn, &PipelineContext::bare())
            .await
            .unwrap();
        let (attached_ticket, comment_id) = match outcome {
            PipelineOutcome::ReplyAppended { ticket_id, comment_id } => (ticket_id, comment_id),
            other => panic!("expected ReplyAppended, got {other:?}"),
        };
        assert_eq!(attached_ticket, ticket.id);

        let recorded = channels_repo::find_by_external_id(&mut conn, ch.id, "<reply-1@ex>")
            .unwrap()
            .unwrap();
        assert_eq!(recorded.ticket_id, Some(ticket.id));
        assert_eq!(recorded.comment_id, Some(comment_id));
        assert_eq!(recorded.in_reply_to.as_deref(), Some("<out-1@host>"));
    }

    #[tokio::test]
    async fn skips_loop_markers() {
        let mut conn = setup_test_connection();
        let ch = TestFixtures::create_channel(&mut conn, "email_imap");
        let mut msg = sample_message("<loop@ex>", vec![], Some("Out of office"));
        msg.loop_markers.is_auto_reply = true;

        let outcome = process_event(
            &StubAdapter,
            &ch,
            InboundEvent::MessageReceived(msg),
            &mut conn,
            &PipelineContext::bare(),
        )
        .await
        .unwrap();
        assert_eq!(outcome, PipelineOutcome::SkippedLoop);

        // No channel_messages row should have been recorded.
        assert!(channels_repo::find_by_external_id(&mut conn, ch.id, "<loop@ex>")
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn skips_bounces() {
        let mut conn = setup_test_connection();
        let ch = TestFixtures::create_channel(&mut conn, "email_imap");
        let mut msg = sample_message("<dsn@ex>", vec![], Some("Delivery Status Notification"));
        msg.is_bounce = true;

        let outcome = process_event(
            &StubAdapter,
            &ch,
            InboundEvent::MessageReceived(msg),
            &mut conn,
            &PipelineContext::bare(),
        )
        .await
        .unwrap();
        assert_eq!(outcome, PipelineOutcome::SkippedBounce);

        // A bounce must not produce a ticket nor leak into the
        // channel_messages ledger; the suppression-list pass will
        // pick it up from the raw DSN content instead.
        assert!(channels_repo::find_by_external_id(&mut conn, ch.id, "<dsn@ex>")
            .unwrap()
            .is_none());
    }

    /// Helper: build a NewOutboundEmail for use in bounce-flow tests.
    /// All fields are filled with realistic-but-uninteresting values
    /// so each test only has to specify what it actually cares about
    /// (Message-ID for linkage, recipient for suppression).
    fn outbound_row(channel_id: i32, message_id: &str, recipient: &str) -> crate::models::NewOutboundEmail {
        crate::models::NewOutboundEmail {
            channel_id,
            ticket_id: None,
            comment_id: None,
            recipient: recipient.to_string(),
            subject: "Re: ticket".to_string(),
            body_text: "body".to_string(),
            body_html: None,
            message_id: message_id.to_string(),
            in_reply_to: None,
            references_list: vec![],
            headers_json: serde_json::json!({}),
            correlation_id: None,
        }
    }

    /// End-to-end: a real Postfix DSN (parsed from the corpus fixture)
    /// flows through the pipeline. We expect the matching outbound
    /// row to be stamped bounced AND the recipient to land on the
    /// suppression list, AND a subsequent enqueue for the same
    /// recipient to short-circuit to `suppressed`.
    #[tokio::test]
    async fn hard_bounce_marks_row_and_suppresses_recipient_end_to_end() {
        let mut conn = setup_test_connection();
        let ch = TestFixtures::create_channel(&mut conn, "email_imap");

        // Enqueue an outbound row whose Message-ID matches the fixture's
        // embedded original message. The pipeline will look for this
        // row by message_id when it processes the DSN.
        let original = crate::repository::outbound_emails::enqueue(
            &mut conn,
            outbound_row(ch.id, "out-42-canonical@yourco.com", "bob@example.org"),
        )
        .unwrap();
        assert!(original.bounced_at.is_none(), "fresh row has no bounce stamp");

        // Parse the canonical Postfix DSN fixture through the real
        // email_imap entry point so detect_bounce + parse_bounce both
        // fire and bounce_report gets populated.
        let raw = include_bytes!("../../../tests/fixtures/dsn/postfix-canonical.eml");
        let msg = super::super::email_imap::parse_rfc822_into_inbound_message(raw, None)
            .expect("fixture should parse");
        assert!(msg.is_bounce, "detect_bounce should flag the fixture");
        assert!(msg.bounce_report.is_some(), "parse_bounce should yield a report");

        let outcome = process_event(
            &StubAdapter,
            &ch,
            InboundEvent::MessageReceived(msg),
            &mut conn,
            &PipelineContext::bare(),
        )
        .await
        .unwrap();
        assert_eq!(outcome, PipelineOutcome::SkippedBounce);

        // Outbound row should now carry the bounce stamp.
        let refreshed = crate::repository::outbound_emails::get(&mut conn, original.id).unwrap();
        assert!(refreshed.bounced_at.is_some(), "outbound row should be marked bounced");
        assert_eq!(refreshed.bounce_recipient.as_deref(), Some("bob@example.org"));
        assert!(
            refreshed
                .bounce_diagnostic
                .as_deref()
                .map(|d| d.contains("User unknown"))
                .unwrap_or(false),
            "diagnostic should carry the upstream reason, got {:?}",
            refreshed.bounce_diagnostic,
        );

        // Recipient should be on the suppression list (5.1.1 is hard).
        assert!(
            crate::repository::email_suppressions::is_suppressed(&mut conn, "bob@example.org")
                .unwrap(),
            "recipient should be auto-suppressed",
        );

        // The next enqueue for the same recipient should short-circuit
        // to suppressed without ever entering the worker's claim set.
        let blocked = crate::repository::outbound_emails::enqueue_or_suppress(
            &mut conn,
            outbound_row(ch.id, "out-followup@yourco.com", "bob@example.org"),
        )
        .unwrap();
        assert_eq!(
            blocked.status,
            crate::models::outbound_email_status::SUPPRESSED
        );
    }

    /// 4xx soft bounce: outbound row gets the bounce stamp (so admins
    /// can see the failure) but the recipient is NOT added to the
    /// suppression list. Tomorrow's send should still go out.
    #[tokio::test]
    async fn soft_bounce_marks_row_but_does_not_suppress() {
        let mut conn = setup_test_connection();
        let ch = TestFixtures::create_channel(&mut conn, "email_imap");

        let _original = crate::repository::outbound_emails::enqueue(
            &mut conn,
            outbound_row(ch.id, "out-soft@yourco.com", "backed-up@example.org"),
        )
        .unwrap();

        let raw = include_bytes!("../../../tests/fixtures/dsn/soft-bounce-4xx.eml");
        let msg = super::super::email_imap::parse_rfc822_into_inbound_message(raw, None).unwrap();

        let outcome = process_event(
            &StubAdapter,
            &ch,
            InboundEvent::MessageReceived(msg),
            &mut conn,
            &PipelineContext::bare(),
        )
        .await
        .unwrap();
        assert_eq!(outcome, PipelineOutcome::SkippedBounce);

        assert!(
            !crate::repository::email_suppressions::is_suppressed(
                &mut conn,
                "backed-up@example.org",
            )
            .unwrap(),
            "soft bounce (4.x.x) must NOT auto-suppress",
        );
    }

    /// Duplicate DSN ingest must not double-count the suppression's
    /// `bounce_count`. The second arrival of the same external_id
    /// short-circuits as `SkippedDuplicate`, leaving the count at 1.
    #[tokio::test]
    async fn duplicate_dsn_does_not_double_count_bounce_count() {
        let mut conn = setup_test_connection();
        let ch = TestFixtures::create_channel(&mut conn, "email_imap");

        let _original = crate::repository::outbound_emails::enqueue(
            &mut conn,
            outbound_row(ch.id, "out-42-canonical@yourco.com", "bob@example.org"),
        )
        .unwrap();

        let raw = include_bytes!("../../../tests/fixtures/dsn/postfix-canonical.eml");

        // First arrival: full bounce processing.
        let msg1 = super::super::email_imap::parse_rfc822_into_inbound_message(raw, None).unwrap();
        let outcome1 = process_event(
            &StubAdapter,
            &ch,
            InboundEvent::MessageReceived(msg1),
            &mut conn,
            &PipelineContext::bare(),
        )
        .await
        .unwrap();
        assert_eq!(outcome1, PipelineOutcome::SkippedBounce);

        // Read the suppression row's bounce_count after the first
        // arrival; sanity-check it's 1.
        let after_first = crate::repository::email_suppressions::list(&mut conn, 10, None)
            .unwrap()
            .into_iter()
            .find(|s| s.email == "bob@example.org")
            .expect("recipient should be suppressed after first DSN");
        assert_eq!(
            after_first.bounce_count, 1,
            "first arrival should leave bounce_count at 1"
        );

        // Second arrival of the same DSN. Same external_id, same
        // payload — the pipeline must short-circuit on the dedup
        // check rather than re-processing.
        let msg2 = super::super::email_imap::parse_rfc822_into_inbound_message(raw, None).unwrap();
        let outcome2 = process_event(
            &StubAdapter,
            &ch,
            InboundEvent::MessageReceived(msg2),
            &mut conn,
            &PipelineContext::bare(),
        )
        .await
        .unwrap();
        assert_eq!(outcome2, PipelineOutcome::SkippedDuplicate);

        // bounce_count must remain at 1 — re-processing would have
        // bumped it via the upsert's ON CONFLICT DO UPDATE branch.
        let after_second = crate::repository::email_suppressions::list(&mut conn, 10, None)
            .unwrap()
            .into_iter()
            .find(|s| s.email == "bob@example.org")
            .expect("recipient should still be suppressed");
        assert_eq!(
            after_second.bounce_count, 1,
            "duplicate DSN must not increment bounce_count, got {}",
            after_second.bounce_count
        );
    }

    /// 5.7.x policy rejection: outbound row gets marked bounced, but
    /// the recipient is NOT suppressed (the failure is almost always
    /// sender-side — SPF / DKIM / content filtering).
    #[tokio::test]
    async fn policy_5_7_x_bounce_marks_row_but_does_not_suppress() {
        let mut conn = setup_test_connection();
        let ch = TestFixtures::create_channel(&mut conn, "email_imap");

        let _original = crate::repository::outbound_emails::enqueue(
            &mut conn,
            outbound_row(ch.id, "out-policy@yourco.com", "valid@example.org"),
        )
        .unwrap();

        let raw = include_bytes!("../../../tests/fixtures/dsn/policy-5_7_1.eml");
        let msg = super::super::email_imap::parse_rfc822_into_inbound_message(raw, None).unwrap();

        let outcome = process_event(
            &StubAdapter,
            &ch,
            InboundEvent::MessageReceived(msg),
            &mut conn,
            &PipelineContext::bare(),
        )
        .await
        .unwrap();
        assert_eq!(outcome, PipelineOutcome::SkippedBounce);

        assert!(
            !crate::repository::email_suppressions::is_suppressed(&mut conn, "valid@example.org")
                .unwrap(),
            "5.7.x policy rejection must NOT auto-suppress (sender-side failure)",
        );
    }

    #[test]
    fn is_hard_bounce_detects_5xx_smtp_codes() {
        // Combined basic + enhanced status (the common DSN shape).
        assert!(is_hard_bounce("smtp; 550 5.1.1 User unknown"));
        assert!(is_hard_bounce("550 5.1.1 User unknown"));
        // Bare enhanced status outside the carve-out set.
        assert!(is_hard_bounce("5.1.1 Bad destination mailbox address"));
        // Bare SMTP code with no enhanced status at all.
        assert!(is_hard_bounce("552 mailbox full"));
    }

    #[test]
    fn is_hard_bounce_rejects_4xx_soft_failures() {
        assert!(!is_hard_bounce("smtp; 421 4.7.0 Try again later"));
        assert!(!is_hard_bounce("4.4.1 Connection refused"));
        assert!(!is_hard_bounce("452 Insufficient storage"));
    }

    #[test]
    fn is_hard_bounce_rejects_unparseable_diagnostic() {
        // Conservative default: nothing 5xx-shaped, no suppression.
        assert!(!is_hard_bounce(""));
        assert!(!is_hard_bounce("Delivery failed for unspecified reason"));
        assert!(!is_hard_bounce("smtp; queue too long"));
    }

    #[test]
    fn is_hard_bounce_excludes_5_7_x_policy_failures() {
        // 5.7.x = policy / security. SPF, DKIM, DMARC, content
        // filtering, greylisting. Almost always a sender-side fix;
        // suppressing the recipient would lose them forever for our
        // own misconfiguration.
        assert!(!is_hard_bounce("5.7.1 Message rejected by policy"));
        assert!(!is_hard_bounce("5.7.0 Authentication required"));
        assert!(!is_hard_bounce("5.7.26 DMARC alignment failed"));
        assert!(!is_hard_bounce("smtp; 550 5.7.1 greylisted"));
    }

    #[test]
    fn is_hard_bounce_excludes_5_0_0_undefined() {
        // 5.0.0 = "other / undefined" per RFC 3463 — too vague to
        // auto-suppress.
        assert!(!is_hard_bounce("5.0.0 Failure"));
        assert!(!is_hard_bounce("smtp; 550 5.0.0 unspecified"));
    }

    #[tokio::test]
    async fn dedupes_on_repeated_external_id() {
        let mut conn = setup_test_connection();
        let ch = TestFixtures::create_channel(&mut conn, "email_imap");
        let event_a =
            InboundEvent::MessageReceived(sample_message("<dup@ex>", vec![], Some("First pass")));
        let event_b =
            InboundEvent::MessageReceived(sample_message("<dup@ex>", vec![], Some("Second pass")));

        let first = process_event(
            &StubAdapter,
            &ch,
            event_a,
            &mut conn,
            &PipelineContext::bare(),
        )
        .await
        .unwrap();
        assert!(matches!(first, PipelineOutcome::TicketOpened { .. }));

        let second = process_event(
            &StubAdapter,
            &ch,
            event_b,
            &mut conn,
            &PipelineContext::bare(),
        )
        .await
        .unwrap();
        assert_eq!(second, PipelineOutcome::SkippedDuplicate);
    }

    #[tokio::test]
    async fn skips_edit_and_delete_events() {
        let mut conn = setup_test_connection();
        let ch = TestFixtures::create_channel(&mut conn, "email_imap");
        let edit = InboundEvent::MessageEdited {
            external_id: "<e@ex>".into(),
            new_body_text: "body".into(),
            new_body_html: None,
            edited_at: Utc::now(),
        };
        let del = InboundEvent::MessageDeleted {
            external_id: "<d@ex>".into(),
            deleted_at: Utc::now(),
        };

        assert_eq!(
            process_event(&StubAdapter, &ch, edit, &mut conn, &PipelineContext::bare())
                .await
                .unwrap(),
            PipelineOutcome::SkippedUnsupportedVariant
        );
        assert_eq!(
            process_event(&StubAdapter, &ch, del, &mut conn, &PipelineContext::bare())
                .await
                .unwrap(),
            PipelineOutcome::SkippedUnsupportedVariant
        );
    }

    #[tokio::test]
    async fn skips_when_sender_has_no_email() {
        let mut conn = setup_test_connection();
        let ch = TestFixtures::create_channel(&mut conn, "email_imap");
        let mut msg = sample_message("<no-email@ex>", vec![], Some("hi"));
        msg.from.known_email = None;

        let outcome = process_event(
            &StubAdapter,
            &ch,
            InboundEvent::MessageReceived(msg),
            &mut conn,
            &PipelineContext::bare(),
        )
        .await
        .unwrap();
        assert_eq!(outcome, PipelineOutcome::SkippedNoIdentity);
    }

    #[tokio::test]
    async fn skips_when_sender_email_is_claimed_by_real_account() {
        let mut conn = setup_test_connection();

        // Seed: real registered admin with a verified email.
        let admin = crate::models::NewUser {
            uuid: uuid::Uuid::now_v7(),
            name: "Admin".into(),
            role: UserRole::Admin,
            pronouns: None,
            avatar_url: None,
            banner_url: None,
            avatar_thumb: None,
            theme: None,
            microsoft_uuid: None,
            mfa_secret: None,
            mfa_enabled: false,
            mfa_backup_codes: None,
            signature: None,
            dashboard_layout: None,
        };
        create_user_with_email(admin, "claimed@example.com".into(), true, None, &mut conn, None)
            .expect("seed admin");

        let ch = TestFixtures::create_channel(&mut conn, "email_imap");
        let mut msg = sample_message("<claimed@ex>", vec![], Some("hi"));
        msg.from.known_email = Some("claimed@example.com".into());
        msg.from.external_id = "claimed@example.com".into();

        let outcome = process_event(
            &StubAdapter,
            &ch,
            InboundEvent::MessageReceived(msg),
            &mut conn,
            &PipelineContext::bare(),
        )
        .await
        .unwrap();
        assert_eq!(outcome, PipelineOutcome::SkippedEmailClaimed);
    }

    #[tokio::test]
    async fn tech_forward_attributes_ticket_to_original_sender() {
        // A technician forwards a customer's email into the helpdesk.
        // The pipeline should parse the forwarded From: out of the
        // body, use the customer as requester, and tag the comment
        // with `forwarded_by_user_uuid` so the UI can render the
        // audit trail.
        let mut conn = setup_test_connection();

        let tech_user = crate::models::NewUser {
            uuid: uuid::Uuid::now_v7(),
            name: "Tech".into(),
            role: UserRole::Technician,
            pronouns: None,
            avatar_url: None,
            banner_url: None,
            avatar_thumb: None,
            theme: None,
            microsoft_uuid: None,
            mfa_secret: None,
            mfa_enabled: false,
            mfa_backup_codes: None,
            signature: None,
            dashboard_layout: None,
        };
        let (tech, _) =
            create_user_with_email(tech_user, "tech@yourco.com".into(), true, None, &mut conn, None)
                .expect("seed tech");

        let ch = TestFixtures::create_channel(&mut conn, "email_imap");
        let mut msg = sample_message("<fwd-1@yourco>", vec![], Some("Fwd: Printer fire"));
        msg.from.known_email = Some("tech@yourco.com".into());
        msg.from.external_id = "tech@yourco.com".into();
        msg.from.display_name = "Tech".into();
        msg.body_text = "\
Please handle.

---------- Forwarded message ---------
From: Alice Customer <alice@customer.example>
Subject: Printer fire

My printer is literally on fire.
"
        .into();

        let outcome = process_event(
            &StubAdapter,
            &ch,
            InboundEvent::MessageReceived(msg),
            &mut conn,
            &PipelineContext::bare(),
        )
        .await
        .unwrap();

        let (ticket_id, comment_id) = match outcome {
            PipelineOutcome::TicketOpened { ticket_id, comment_id } => (ticket_id, comment_id),
            other => panic!("expected TicketOpened, got {other:?}"),
        };

        // Ticket's requester is the original customer, NOT the tech.
        let ticket = tickets_repo::get_ticket_by_id(&mut conn, ticket_id).unwrap();
        let requester_uuid = ticket.requester_uuid.expect("ticket has requester");
        assert_ne!(requester_uuid, tech.uuid, "tech must not be the requester");

        // Comment carries the audit marker.
        use crate::schema::comments::dsl as c;
        use diesel::prelude::*;
        let metadata: Option<serde_json::Value> = c::comments
            .filter(c::id.eq(comment_id))
            .select(c::channel_metadata)
            .first(&mut conn)
            .unwrap();
        let metadata = metadata.expect("channel_metadata present");
        assert_eq!(
            metadata["forwarded_by_user_uuid"].as_str(),
            Some(tech.uuid.to_string().as_str()),
            "forwarded_by_user_uuid should point at the forwarding tech"
        );
    }

    #[tokio::test]
    async fn verified_sender_without_forward_markers_is_still_rejected() {
        // Safety net: a verified tech who sends a plain (non-
        // forwarded) email should NOT auto-open a ticket with
        // themselves as requester. Fall through to SkippedEmailClaimed
        // so the tech gets visible feedback that they need to forward
        // properly.
        let mut conn = setup_test_connection();
        let tech = crate::models::NewUser {
            uuid: uuid::Uuid::now_v7(),
            name: "Tech2".into(),
            role: UserRole::Admin,
            pronouns: None,
            avatar_url: None,
            banner_url: None,
            avatar_thumb: None,
            theme: None,
            microsoft_uuid: None,
            mfa_secret: None,
            mfa_enabled: false,
            mfa_backup_codes: None,
            signature: None,
            dashboard_layout: None,
        };
        create_user_with_email(tech, "admin2@yourco.com".into(), true, None, &mut conn, None)
            .expect("seed admin");

        let ch = TestFixtures::create_channel(&mut conn, "email_imap");
        let mut msg = sample_message("<plain@yourco>", vec![], Some("just a note"));
        msg.from.known_email = Some("admin2@yourco.com".into());
        msg.body_text = "Hey team, no forward here, just a check-in.".into();

        let outcome = process_event(
            &StubAdapter,
            &ch,
            InboundEvent::MessageReceived(msg),
            &mut conn,
            &PipelineContext::bare(),
        )
        .await
        .unwrap();
        assert_eq!(outcome, PipelineOutcome::SkippedEmailClaimed);
    }

    // ---------- SSRF / attachment-size guard tests ----------
    //
    // The External-attachment branch is dormant today (email sends
    // Inline only) but the guard is load-bearing for the Slack /
    // Teams adapters that will use it. Test the gating here so a
    // future regression in `host_looks_internal` or the size caps
    // trips a red test, not a production incident.

    #[test]
    fn host_looks_internal_blocks_loopback_and_private_ranges() {
        for bad in [
            "localhost",
            "127.0.0.1",
            "127.1.2.3",
            "10.0.0.5",
            "192.168.1.100",
            "172.16.5.5",
            "169.254.169.254", // AWS/GCP metadata
            "::1",
            "metadata.google.internal",
            "foo.local",
            "bar.internal",
        ] {
            assert!(
                host_looks_internal(bad),
                "expected {bad} to be flagged internal"
            );
        }
    }

    #[test]
    fn host_looks_internal_allows_public_hosts() {
        for ok in [
            "example.com",
            "api.github.com",
            "8.8.8.8",
            "1.1.1.1",
            "storage.googleapis.com",
        ] {
            assert!(
                !host_looks_internal(ok),
                "expected {ok} to be accepted"
            );
        }
    }

    #[tokio::test]
    async fn external_url_loopback_is_rejected() {
        let att = InboundAttachment::External {
            filename: "x.bin".into(),
            mime_type: "application/octet-stream".into(),
            url: "http://127.0.0.1:6379/".into(),
            auth_header: None,
            size_bytes: None,
        };
        let client = reqwest::Client::new();
        let err = match materialize_attachment(&att, Some(&client)).await {
            Ok(_) => panic!("expected materialize_attachment to fail"),
            Err(e) => e,
        };
        assert!(
            format!("{err}").contains("loopback"),
            "expected loopback rejection, got: {err}"
        );
    }

    #[tokio::test]
    async fn external_url_with_oversized_claim_is_rejected() {
        let att = InboundAttachment::External {
            filename: "big.bin".into(),
            mime_type: "application/octet-stream".into(),
            url: "https://example.com/big.bin".into(),
            auth_header: None,
            size_bytes: Some((MAX_ATTACHMENT_BYTES as u64) + 1),
        };
        let client = reqwest::Client::new();
        let err = match materialize_attachment(&att, Some(&client)).await {
            Ok(_) => panic!("expected materialize_attachment to fail"),
            Err(e) => e,
        };
        assert!(
            format!("{err}").contains("over cap"),
            "expected over-cap rejection, got: {err}"
        );
    }

    #[tokio::test]
    async fn oversized_inline_attachment_is_rejected() {
        let att = InboundAttachment::Inline {
            filename: "big.bin".into(),
            mime_type: "application/octet-stream".into(),
            bytes: vec![0u8; MAX_ATTACHMENT_BYTES + 1],
        };
        let err = match materialize_attachment(&att, None).await {
            Ok(_) => panic!("expected materialize_attachment to fail"),
            Err(e) => e,
        };
        assert!(
            format!("{err}").contains("cap"),
            "expected cap rejection, got: {err}"
        );
    }

    #[tokio::test]
    async fn external_url_with_bad_scheme_is_rejected() {
        let att = InboundAttachment::External {
            filename: "x.bin".into(),
            mime_type: "application/octet-stream".into(),
            url: "file:///etc/passwd".into(),
            auth_header: None,
            size_bytes: None,
        };
        let client = reqwest::Client::new();
        let err = match materialize_attachment(&att, Some(&client)).await {
            Ok(_) => panic!("expected materialize_attachment to fail"),
            Err(e) => e,
        };
        assert!(
            format!("{err}").contains("scheme"),
            "expected scheme rejection, got: {err}"
        );
    }
}
