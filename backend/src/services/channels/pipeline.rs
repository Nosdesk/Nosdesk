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
use diesel::OptionalExtension;
use serde_json::json;
use tracing::{debug, info, warn};

use crate::db::DbConnection;
use crate::handlers::sse::SseState;
use crate::models::{
    Channel, Comment, NewAttachment, NewChannelMessage, NewComment, NewTicket, Ticket,
    CHANNEL_DIRECTION_INBOUND,
};
use crate::repository::user_helpers::{find_or_create_guest_user, GuestUserResult};
use crate::repository::{
    channels as channels_repo, comments as comments_repo, tickets as tickets_repo,
};
use crate::services::channels::{ChannelAdapter, InboundAttachment, InboundEvent, InboundMessage};
use crate::sync::actor::ActorContext;
use crate::sync::session;
use crate::utils::storage::{Storage, WorkspaceScopedStorage};

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
    /// The `From` named a verified or privileged (staff) account but DMARC
    /// explicitly failed — a spoofing attempt. Refused rather than ingested
    /// as that identity. Only fires on an explicit DMARC `fail` (a domain that
    /// publishes DMARC), so unauthenticated self-host mail is unaffected.
    SkippedSpoofedSender,
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
    /// Outbound resolver used to send the auto-acknowledgement on newly
    /// opened tickets, from the ticket's workspace identity (or the env
    /// fallback). `None` disables the auto-ack branch — unit tests leave
    /// this unset to avoid touching SMTP.
    pub resolver: Option<Arc<crate::services::outbound_email::OutboundEmailResolver>>,
    /// Pool handle for the auto-ack spawn (needs to be cloneable into
    /// a `'static` task). Paired with `resolver`: both or neither should
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
        //
        // RFC 3464 §2.1 allows multiple per-recipient blocks in one
        // DSN. We process each report independently so a multi-
        // recipient bounce suppresses every failed address — not
        // just the first one.
        if msg.bounce_reports.is_empty() {
            debug!(
                channel_id = channel.id,
                external_id = %msg.external_id,
                "bounce: detected but DSN was unparseable; no linkage"
            );
        }

        // B1b: prefer VERP linkage. If the DSN was addressed to one of our
        // VERP Return-Paths, the embedded token names the originating row
        // directly, so we don't depend on the remote MTA echoing the original
        // Message-ID (many don't). Only for single-report DSNs: our outbound
        // rows are per-recipient, so a bounce of our mail carries one report,
        // and one token must not be applied across several reports.
        let verp_row_id = if msg.bounce_reports.len() == 1 {
            crate::utils::verp::configured_secret().and_then(|secret| {
                msg.recipients
                    .iter()
                    .find_map(|addr| crate::utils::verp::row_id_from_address(addr, &secret))
            })
        } else {
            None
        };

        for report in &msg.bounce_reports {
            let linkage = match verp_row_id {
                Some(id) => crate::repository::outbound_emails::mark_bounced_by_id(
                    conn,
                    id,
                    report.recipient.as_deref(),
                    report.diagnostic.as_deref(),
                ),
                None => crate::repository::outbound_emails::mark_bounced(
                    conn,
                    &report.original_message_id,
                    report.recipient.as_deref(),
                    report.diagnostic.as_deref(),
                ),
            };
            // How the row was located, for the log line below.
            let via = verp_row_id.map_or("message-id", |_| "verp");
            match linkage {
                Ok(0) => {
                    debug!(
                        channel_id = channel.id,
                        message_id = %report.original_message_id,
                        via,
                        "bounce: no matching outbound row"
                    );
                }
                Ok(n) => {
                    debug!(
                        channel_id = channel.id,
                        message_id = %report.original_message_id,
                        via,
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
                        via,
                        "bounce: failed to update outbound row"
                    );
                }
            }

            // Auto-suppress on hard bounces only. Soft bounces
            // (4xx — transient) shouldn't permanently block a
            // recipient; they retry naturally on the next send.
            // `BounceReport::is_hard` prefers the structured Status
            // code (RFC 3464's canonical signal) and falls back to
            // scanning the diagnostic when Status is absent.
            if let Some(recipient) = report.recipient.as_deref() {
                if report.is_hard() {
                    let new = crate::models::NewEmailSuppression {
                        email: recipient.to_string(),
                        reason: crate::models::email_suppression_reason::HARD_BOUNCE.to_string(),
                        bounce_diagnostic: report.diagnostic.clone(),
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

    // Impersonation guard (B3): a `From` that DMARC explicitly *failed* — the
    // domain publishes a policy and the message did not align — must not be
    // ingested as whatever account it names. Only reject when it names a
    // verified or privileged (staff) account: that's the identity worth
    // spoofing (staff trust + tech-forward attribution). An explicit fail on an
    // unknown/unverified address is just spam and still opens a guest ticket
    // below, and `SenderAuth::Unknown` (no DMARC policy, or a self-host MTA that
    // stamps no `Authentication-Results`) is trusted exactly as before, so
    // ordinary and self-host mail are unaffected. Identity tables are global,
    // so this read needs no workspace pin.
    if msg.sender_auth == crate::services::channels::SenderAuth::Fail {
        if let Some(claimed) =
            crate::repository::user_helpers::find_verified_user_by_email(&sender_email, conn)?
        {
            warn!(
                channel_id = channel.id,
                external_id = %msg.external_id,
                claimed_account = %claimed.uuid,
                "skip: DMARC-failed inbound From names a verified/privileged account \
                 (spoofing); refusing to ingest as that identity"
            );
            return Ok(PipelineOutcome::SkippedSpoofedSender);
        }
    }

    // Thread resolution — `None` means "start a new ticket". Stays
    // outside the transaction below because it's a read-only lookup and
    // because the trait's `async fn` contract can't be invoked inside
    // Diesel's synchronous transaction closure. A concurrent ingest of
    // the same message would be caught by the UNIQUE constraint on
    // `channel_messages` at transaction commit, so the read-then-write
    // isn't a correctness hazard here.
    let existing_ticket_id = adapter.resolve_thread(&msg, channel.id, conn).await;

    // Raw RFC-822 archive. Done before the transaction because
    // storage uploads are async and shouldn't hold a DB connection;
    // a failure here is non-fatal (we log and proceed with a NULL
    // `raw_source_uri`). The .eml is the source of truth for
    // re-running quote extraction on policy change and powers the
    // "Show original message" affordance, so it's worth a best
    // effort but not worth aborting the comment for.
    let raw_source_uri = store_raw_eml(ctx, channel.workspace_id, &msg).await;

    // Atomically: identity resolve (which may create a guest user row)
    // → get-or-create ticket → insert comment → write the
    // `channel_messages` dedup row. Everything that writes during
    // ingestion happens inside this transaction, so a failure on any
    // step rolls the lot back. Before this was a transaction, a
    // partial success would orphan a guest user (if identity resolve
    // created one), a comment (if the dedup row write failed), and
    // let the next poll re-ingest into a second comment on the same
    // ticket.
    let (ticket, comment, is_new_ticket, sender_uuid) =
        conn.transaction::<_, PipelineError, _>(|conn| {
            // Attribute every emit in this transaction to the inbound
            // channel pipeline so sync_actions records the system actor
            // rather than NULL. The outer call has no HTTP request, so
            // we synthesise a system actor here.
            //
            // `.with_workspace` is load-bearing: ingestion writes tickets
            // and comments, both workspace-scoped (workspace_id NOT NULL,
            // RLS) and audit-triggered (audit_log.workspace_id NOT NULL,
            // defaulted from the app.workspace_id GUC). Without the channel's
            // workspace on the actor, set_actor leaves the GUC unset and the
            // first insert aborts the whole ingest transaction. The channel
            // owns the workspace the inbound message belongs to.
            let actor =
                ActorContext::system("channels:inbound").with_workspace(channel.workspace_id);
            session::set_actor(conn, &actor)?;

            // Identity resolution. The sender becomes the requester (their
            // own account if they have one, else a guest); a staff member
            // forwarding a customer's mail redirects attribution to that
            // customer. See `resolve_identity` for the full decision tree.
            let (sender, forwarded_by_uuid) =
                resolve_identity(channel, &msg, &sender_email, conn, ctx)?;
            let sender_uuid = sender.uuid;

            // existing_ticket_id may point at a ticket that no longer exists, was
            // resolved by a loose subject "#N" match, or lives in another workspace
            // (RLS-hidden under the channel's pin). Treat a missing or invisible
            // ticket as "start a new one" rather than erroring, which would drop
            // the inbound message after the IMAP cursor already advanced.
            let resolved_existing = match existing_ticket_id {
                Some(ticket_id) => tickets_repo::get_ticket_by_id(conn, ticket_id).optional()?,
                None => None,
            };

            let (ticket, comment, is_new_ticket) = match resolved_existing {
                Some(ticket) => {
                    let comment = insert_inbound_comment(
                        conn,
                        ticket.id,
                        sender_uuid,
                        &msg,
                        forwarded_by_uuid,
                        raw_source_uri.clone(),
                        ctx,
                    )?;
                    (ticket, comment, false)
                }
                None => {
                    let ticket = open_ticket_from_message(conn, channel, &msg, sender_uuid)?;
                    let comment = insert_inbound_comment(
                        conn,
                        ticket.id,
                        sender_uuid,
                        &msg,
                        forwarded_by_uuid,
                        raw_source_uri.clone(),
                        ctx,
                    )?;
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

            Ok((ticket, comment, is_new_ticket, sender_uuid))
        })?;

    // Attachments — best effort. Each failure is logged and skipped so
    // one malformed file doesn't lose the whole message. Runs outside
    // the transaction because it may do network fetches (External
    // attachments) and storage writes, which shouldn't hold a DB row
    // lock for seconds at a time.
    persist_attachments(
        conn,
        ctx,
        channel.workspace_id,
        comment.id,
        sender_uuid,
        &msg.attachments,
    )
    .await;

    // New tickets + comments both reach clients through the sync pool
    // (the repository writes emit `ticket.created` / `comment.created`);
    // no discrete SSE side effects here.
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
        if let (Some(pool), Some(resolver)) = (ctx.pool.clone(), ctx.resolver.clone()) {
            super::auto_ack::spawn_auto_ack(
                pool,
                resolver,
                channel.clone(),
                ticket.clone(),
                msg.external_id.clone(),
                msg.content_language.clone(),
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

// ---------- DB helpers ----------

/// Decide who the inbound message should be attributed to, returning the
/// requester plus the forwarding staff member (if any).
///
/// Inbound mail always opens (or threads onto) a ticket; nobody is rejected
/// for having an account. The requester is the sender, resolved to their
/// existing account if one exists and an auto-provisioned guest otherwise.
///
/// The one special case is the **tech-forward workflow**: when a staff member
/// (one who can handle tickets) forwards a *customer's* email, attribution is
/// redirected to that customer and the staff member is recorded as the
/// forwarder for the audit trail. This is the pattern mainstream helpdesks
/// (Zendesk, Freshdesk, Help Scout, Zammad) implement. A staff member emailing
/// in directly (no forward markers) is simply the requester, same as anyone
/// else — an employee on an internal IT desk can raise a ticket by emailing in.
///
/// Loops, auto-replies, and bounces are filtered earlier (`loop_markers` /
/// `is_bounce`), so they never reach here.
fn resolve_identity(
    channel: &Channel,
    msg: &InboundMessage,
    sender_email: &str,
    conn: &mut DbConnection,
    ctx: &PipelineContext,
) -> Result<(crate::models::User, Option<uuid::Uuid>), PipelineError> {
    use crate::repository::user_helpers::{find_verified_user_by_email, user_can_handle_tickets};
    let observer = ctx
        .search
        .as_ref()
        .map(|s| s as &dyn crate::repository::user_helpers::UserCreatedObserver);

    if let Some(sender) = find_verified_user_by_email(sender_email, conn)? {
        // Tech-forward: only staff may forward on a customer's behalf, and only
        // when the body actually carries forward markers. Then the original
        // customer is the requester and the staff member is the forwarder.
        if user_can_handle_tickets(conn, &sender) {
            if let Some(fwd) = super::forward_parser::extract(msg) {
                let display = fwd
                    .display_name
                    .clone()
                    .unwrap_or_else(|| fwd.email.clone());
                let customer = resolve_requester(&fwd.email, &display, conn, observer)?;
                info!(
                    channel_id = channel.id,
                    external_id = %msg.external_id,
                    forwarded_by = %sender.uuid,
                    original = %fwd.email,
                    "tech forward detected; attributing ticket to original sender"
                );
                return Ok((customer, Some(sender.uuid)));
            }
        }
        // Member, or staff emailing in directly: the sender is the requester.
        return Ok((sender, None));
    }

    // Unknown sender: auto-provision (or reuse) a guest account.
    let user = resolve_requester(sender_email, &msg.from.display_name, conn, observer)?;
    Ok((user, None))
}

/// Resolve an email address to the user a ticket should be attributed to:
/// their existing account if one exists, otherwise an auto-provisioned guest.
fn resolve_requester(
    email: &str,
    display_name: &str,
    conn: &mut DbConnection,
    observer: Option<&dyn crate::repository::user_helpers::UserCreatedObserver>,
) -> Result<crate::models::User, PipelineError> {
    use crate::repository::user_helpers::find_verified_user_by_email;
    if let Some(u) = find_verified_user_by_email(email, conn)? {
        return Ok(u);
    }
    match find_or_create_guest_user(email, display_name, conn, observer)? {
        GuestUserResult::Created(u) | GuestUserResult::Existing(u) => Ok(u),
        // Unreachable in a single transaction (the lookup above already caught
        // any verified/privileged account), but if the email turned out to be
        // claimed, attribute to that real account rather than dropping the mail.
        GuestUserResult::EmailClaimed => find_verified_user_by_email(email, conn)?
            .ok_or(PipelineError::Db(diesel::result::Error::NotFound)),
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
        // Spam-flagged mail still opens a ticket (never dropped), but badged
        // and de-prioritised so it triages out of the way without being lost.
        spam_suspected: msg.spam_suspected,
        priority: if msg.spam_suspected {
            crate::models::TicketPriority::Low
        } else {
            crate::models::TicketPriority::default()
        },
        ..Default::default()
    };

    // Surface channel context on the `ticket.created` activity row
    // so the agent sees "Created from email by Alice <alice@…>"
    // instead of a bare "System created this ticket". The display
    // name is filtered for emptiness to avoid rendering `<unknown>`-
    // style artifacts when the From header had no quoted-name part.
    fn non_empty(s: &str) -> Option<String> {
        let trimmed = s.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    }
    let annotation = tickets_repo::TicketCreationAnnotation {
        source: Some(format!("channel:{}", channel.provider)),
        from_email: msg.from.known_email.clone(),
        from_name: non_empty(&msg.from.display_name),
        subject: msg.subject.as_deref().and_then(non_empty),
    };

    tickets_repo::create_ticket_with_annotation(conn, new_ticket, annotation, None)
}

/// Fold a stripped signature and the quoted thread into the single collapsed
/// region, signature first (its original position, before the quote).
fn combine_collapsed(
    signature: Option<String>,
    quoted: Option<String>,
    sep: &str,
) -> Option<String> {
    match (signature, quoted) {
        (Some(s), Some(q)) => Some(format!("{s}{sep}{q}")),
        (Some(s), None) => Some(s),
        (None, q) => q,
    }
}

fn insert_inbound_comment(
    conn: &mut DbConnection,
    ticket_id: i32,
    user_uuid: uuid::Uuid,
    msg: &InboundMessage,
    forwarded_by_user_uuid: Option<uuid::Uuid>,
    raw_source_uri: Option<String>,
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
    // Mail the provider flagged as spam still opens a ticket (never silently
    // drop a customer's request), but we stamp the verdict so the ticket view
    // can badge it and agents can triage.
    if msg.spam_suspected {
        metadata["spam_suspected"] = json!(true);
    }

    // Prefer the rich HTML body when the message has one — it carries
    // the formatting the customer's mail client emitted (lists, links,
    // signatures, embedded images via `cid:` references) and the
    // ticket view renders it in a sandboxed iframe so the structure is
    // preserved. Fall back to the flat `body_text` for plaintext-only
    // emails (mailing lists, bots, console-mail clients).
    let (content, content_format) = match msg.body_html.as_ref() {
        Some(html) if !html.trim().is_empty() => (html.clone(), crate::models::ContentFormat::Html),
        _ => (
            msg.body_text.clone(),
            crate::models::ContentFormat::Plaintext,
        ),
    };

    // Order matters: for HTML bodies we sanitise FIRST, then split,
    // so the `new_content` and `quoted_content` columns carry
    // render-safe HTML the frontend can inject directly. Splitting
    // before sanitising would leak unsanitised markup into those
    // columns, forcing the renderer to re-sanitise per-render or
    // risk XSS. For plaintext there's nothing to sanitise; we split
    // the raw text directly.
    // Split the reply from the quoted thread (B5: and the sender's signature),
    // then sanitise each stored part. The HTML split MUST run on RAW HTML: the
    // quote/signature markers (gmail_quote, blockquote cite, gmail_signature) are
    // `class`/`id` attributes the sanitiser strips, so a sanitise-first order
    // would never find them. Sanitising `new_content` + `quoted_content` AFTER
    // the split keeps the stored columns render-safe (the invariant the old
    // sanitise-first order protected). Plaintext needs no sanitising. The removed
    // signature folds into the collapsed quoted region (its original position,
    // before the quote) so nothing is lost in-app; the raw body stays the source
    // of truth for recovery.
    let split = match content_format {
        crate::models::ContentFormat::Html => {
            let quote = super::email_quote::split_html(&content);
            let sig = super::email_signature::strip_html(&quote.new_content);
            let quoted = combine_collapsed(sig.signature, quote.quoted_content, "\n");
            super::email_quote::QuoteSplit {
                new_content: super::email_sanitise::sanitise(&sig.content).html,
                quoted_content: quoted.map(|q| super::email_sanitise::sanitise(&q).html),
            }
        }
        _ => {
            let quote = super::email_quote::split_plaintext(&content);
            let sig = super::email_signature::strip_plaintext(&quote.new_content);
            super::email_quote::QuoteSplit {
                new_content: sig.content,
                quoted_content: combine_collapsed(sig.signature, quote.quoted_content, "\n\n"),
            }
        }
    };

    // Native-first render tiering: classify the (already-sanitised)
    // body into text / simple / rich so the frontend renders the common
    // case inline and reserves the iframe for genuinely rich mail. For
    // `simple` this reduces new/quoted content to a semantic-HTML
    // subset; for `text`/`rich` it passes them through.
    let classified = super::email_render_kind::classify(
        content_format,
        &split.new_content,
        split.quoted_content.as_deref(),
    );

    let new_comment = NewComment {
        content,
        ticket_id,
        user_uuid,
        channel_metadata: Some(metadata),
        is_internal: false,
        content_format,
        render_kind: Some(classified.kind.as_str().to_string()),
        // Persist both raw body parts so a future re-sanitise or
        // re-extract can run without re-fetching from upstream.
        // Empty plaintext is coerced to NULL so the DB carries a
        // clean tri-state: NULL = no part of this MIME type, Some
        // = a real body. `body_html` is already `Option<String>`
        // and the parser only sets it when an `text/html` part
        // existed, so the same invariant holds without coercion.
        body_text: (!msg.body_text.trim().is_empty()).then(|| msg.body_text.clone()),
        body_html: msg.body_html.clone(),
        new_content: Some(classified.new_content),
        quoted_content: classified.quoted_content,
        raw_source_uri,
    };

    let observer = ctx
        .search
        .as_ref()
        .map(|s| s as &dyn crate::repository::comments::CommentCreatedObserver);

    // Carry the channel context onto the `comment.created` activity
    // row so inbound replies render as
    // "alice@example.com replied via email" instead of the generic
    // "System commented on this ticket". Same shape and source tags
    // as the `ticket.created` annotation upstream.
    fn non_empty(s: &str) -> Option<String> {
        let t = s.trim();
        if t.is_empty() {
            None
        } else {
            Some(t.to_string())
        }
    }
    let annotation = crate::repository::comments::CommentCreationAnnotation {
        source: Some(format!("channel:{}", msg.from.provider)),
        from_email: msg.from.known_email.clone(),
        from_name: non_empty(&msg.from.display_name),
    };

    comments_repo::create_comment_with_annotation(conn, new_comment, annotation, observer)
}

/// Persist the raw RFC-822 source to the storage backend so the
/// `.eml` can be re-fetched later (Show original, re-parse on
/// policy change). Returns the storage path on success; logs and
/// returns `None` on any failure — the comment still ingests with
/// `raw_source_uri = NULL`, which the consumer treats as "raw not
/// available for this comment."
///
/// Files land in the `email_raw` folder. Filename includes the
/// channel external_id (sanitised) so an operator browsing the
/// backing storage can map a file to a comment without a DB
/// lookup; the storage abstraction also prepends a uuid so
/// collisions across messages with the same Message-ID are
/// impossible.
async fn store_raw_eml(
    ctx: &PipelineContext,
    workspace_id: i32,
    msg: &InboundMessage,
) -> Option<String> {
    // Scope to the channel's workspace so the archived .eml lands under
    // ws/{id}/email_raw/... The returned (logical) path is what gets
    // stored in comments.raw_source_uri and read back via the same scope.
    let storage = WorkspaceScopedStorage::arc(ctx.storage.as_ref()?.clone(), workspace_id);
    let bytes = msg.raw_bytes.as_ref()?;

    let safe_id: String = msg
        .external_id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .take(80)
        .collect();
    let filename = format!("{}.eml", safe_id);

    match storage
        .store_file(bytes, &filename, "message/rfc822", "email_raw")
        .await
    {
        Ok(stored) => Some(stored.path),
        Err(e) => {
            warn!(
                error = ?e,
                external_id = %msg.external_id,
                "failed to archive raw .eml; comment will record NULL raw_source_uri"
            );
            None
        }
    }
}

async fn persist_attachments(
    conn: &mut DbConnection,
    ctx: &PipelineContext,
    workspace_id: i32,
    comment_id: i32,
    uploader: uuid::Uuid,
    attachments: &[InboundAttachment],
) {
    let Some(base_storage) = ctx.storage.as_ref() else {
        if !attachments.is_empty() {
            debug!(
                count = attachments.len(),
                "skipping attachment persistence (no storage in pipeline context)"
            );
        }
        return;
    };
    // Scope to the channel's workspace so inbound attachments land under
    // ws/{id}/tickets/... like every other tenant object.
    let storage = WorkspaceScopedStorage::arc(base_storage.clone(), workspace_id);

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
    //! - Sender with an existing account (member or staff) → ticket attributed
    //!   to that account; tech-forward redirects to the original customer
    //!
    //! SSE / search / storage side effects are covered by the E2E task (#23).

    use super::*;
    use crate::models::{NewChannelMessage, CHANNEL_DIRECTION_OUTBOUND};
    use crate::repository::user_helpers::create_user_with_email;
    use crate::services::channels::{
        threading::default_explicit_threading, ChannelError, ExternalIdentity, LoopMarkers,
        OutboundContent, OutboundMessage, SenderAuth, ThreadContext,
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

    fn sample_message(
        external_id: &str,
        references: Vec<String>,
        subject: Option<&str>,
    ) -> InboundMessage {
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
            bounce_reports: Vec::new(),
            raw_bytes: None,
            content_language: None,
            source_ref: None,
            spam_suspected: false,
            sender_auth: SenderAuth::Unknown,
        }
    }

    /// `sample_message` with the body overridden. `body_html = Some`
    /// drives the HTML path; `None` keeps it plaintext.
    fn message_with_body(
        external_id: &str,
        subject: &str,
        body_text: &str,
        body_html: Option<&str>,
    ) -> InboundMessage {
        let mut m = sample_message(external_id, vec![], Some(subject));
        m.body_text = body_text.into();
        m.body_html = body_html.map(str::to_string);
        m
    }

    /// Regression guard for the split-before-sanitise order (B5): a Gmail-shaped
    /// HTML reply (new text, then a `gmail_signature` block, then a `gmail_quote`
    /// block) must land with the signature AND the quote trimmed out of
    /// `new_content`. This only works because the split runs on RAW HTML — the
    /// markers are `class` attributes the sanitiser strips, so a sanitise-first
    /// order would leave both inline. Folds the trimmed parts into the collapsed
    /// region.
    #[tokio::test]
    async fn html_quote_and_signature_stripped_through_sanitiser() {
        use crate::schema::comments;
        use diesel::prelude::*;
        let mut conn = setup_test_connection();
        let ch = TestFixtures::create_channel(&mut conn, "email_imap");

        let html = concat!(
            r#"<div dir="ltr"><div>Yes, ship it.</div>"#,
            r#"<div class="gmail_signature" data-smartmail="gmail_signature">"#,
            r#"<div>Jane Doe</div><div>Acme</div></div>"#,
            r#"<div class="gmail_quote"><blockquote type="cite">"#,
            r#"the original question</blockquote></div></div>"#,
        );

        let out = process_event(
            &StubAdapter,
            &ch,
            InboundEvent::MessageReceived(message_with_body(
                "<sig-quote@ex>",
                "Re: x",
                "",
                Some(html),
            )),
            &mut conn,
            &PipelineContext::bare(),
        )
        .await
        .expect("process_event");
        let comment_id = match out {
            PipelineOutcome::TicketOpened { comment_id, .. } => comment_id,
            other => panic!("expected TicketOpened, got {other:?}"),
        };
        let (new_content, quoted): (Option<String>, Option<String>) = comments::table
            .filter(comments::id.eq(comment_id))
            .select((comments::new_content, comments::quoted_content))
            .first(&mut conn)
            .expect("ingested comment");

        let nc = new_content.unwrap_or_default();
        assert!(
            nc.contains("Yes, ship it"),
            "new_content keeps the reply: {nc}"
        );
        assert!(
            !nc.contains("Jane Doe"),
            "signature stripped from new_content: {nc}"
        );
        assert!(
            !nc.contains("original question"),
            "quote stripped from new_content: {nc}"
        );
        let q = quoted.unwrap_or_default();
        assert!(
            q.contains("Jane Doe") && q.contains("original question"),
            "collapsed region carries both the signature and the quote: {q}"
        );
    }

    /// End-to-end render-tier classification (Item J native-first): a
    /// real message flows through sanitise → quote-split → classify →
    /// persist, and the stored comment carries the right `render_kind`.
    /// Also exercises the inbound-pipeline workspace fix: ticket +
    /// comment creation must succeed under the channel's workspace.
    #[tokio::test]
    async fn ingest_classifies_render_kind() {
        let mut conn = setup_test_connection();
        let ch = TestFixtures::create_channel(&mut conn, "email_imap");

        async fn ingest(
            conn: &mut DbConnection,
            ch: &Channel,
            msg: InboundMessage,
        ) -> (Option<String>, Option<String>) {
            use crate::schema::comments;
            use diesel::prelude::*;
            let out = process_event(
                &StubAdapter,
                ch,
                InboundEvent::MessageReceived(msg),
                conn,
                &PipelineContext::bare(),
            )
            .await
            .expect("process_event");
            let comment_id = match out {
                PipelineOutcome::TicketOpened { comment_id, .. } => comment_id,
                other => panic!("expected TicketOpened, got {other:?}"),
            };
            comments::table
                .filter(comments::id.eq(comment_id))
                .select((comments::render_kind, comments::new_content))
                .first::<(Option<String>, Option<String>)>(conn)
                .expect("ingested comment")
        }

        // Plaintext reply -> text bubble.
        let (rk, _) = ingest(
            &mut conn,
            &ch,
            message_with_body(
                "<rk-text@ex>",
                "Plain",
                "Thanks, that fixed it!\nCheers",
                None,
            ),
        )
        .await;
        assert_eq!(rk.as_deref(), Some("text"));

        // Human HTML reply (inline marks + link, no layout) -> simple,
        // reduced to a semantic subset.
        let (rk, nc) = ingest(
            &mut conn,
            &ch,
            message_with_body(
                "<rk-simple@ex>",
                "HTML",
                "",
                Some(
                    r#"<div dir="ltr"><p>Thanks, that <b>fixed</b> it!</p><p>Add a <a href="https://x.test/seat">2nd seat</a>?</p></div>"#,
                ),
            ),
        )
        .await;
        assert_eq!(rk.as_deref(), Some("simple"));
        let nc = nc.unwrap_or_default();
        assert!(nc.contains("2nd seat"), "simple keeps the link text: {nc}");
        assert!(!nc.to_lowercase().contains("<table"), "simple has no table");

        // Newsletter-style layout -> rich (kept whole for the iframe).
        let (rk, _) = ingest(
            &mut conn,
            &ch,
            message_with_body(
                "<rk-rich@ex>",
                "News",
                "",
                Some(
                    r#"<table width="600"><tr><td><h1>Deals</h1><p>Buy now</p></td></tr></table>"#,
                ),
            ),
        )
        .await;
        assert_eq!(rk.as_deref(), Some("rich"));
    }

    #[tokio::test]
    async fn opens_new_ticket_on_unthreaded_message() {
        let mut conn = setup_test_connection();
        let ch = TestFixtures::create_channel(&mut conn, "email_imap");
        let event =
            InboundEvent::MessageReceived(sample_message("<m1@ex>", vec![], Some("Printer fire")));

        let outcome = process_event(
            &StubAdapter,
            &ch,
            event,
            &mut conn,
            &PipelineContext::bare(),
        )
        .await
        .unwrap();

        let (ticket_id, comment_id) = match outcome {
            PipelineOutcome::TicketOpened {
                ticket_id,
                comment_id,
            } => (ticket_id, comment_id),
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

        let outcome = process_event(
            &StubAdapter,
            &ch,
            event,
            &mut conn,
            &PipelineContext::bare(),
        )
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
        let user = TestFixtures::create_user(&mut conn, "tech", "technician");
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

        let outcome = process_event(
            &StubAdapter,
            &ch,
            event,
            &mut conn,
            &PipelineContext::bare(),
        )
        .await
        .unwrap();
        let (attached_ticket, comment_id) = match outcome {
            PipelineOutcome::ReplyAppended {
                ticket_id,
                comment_id,
            } => (ticket_id, comment_id),
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
        assert!(
            channels_repo::find_by_external_id(&mut conn, ch.id, "<loop@ex>")
                .unwrap()
                .is_none()
        );
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

        // The DSN row is recorded with `ticket_id = None` so a
        // duplicate arrival is caught by the dedup short-circuit at
        // the top of the bounce branch. The presence of the marker
        // is what makes the second pass return `SkippedDuplicate`
        // instead of double-processing.
        let recorded = channels_repo::find_by_external_id(&mut conn, ch.id, "<dsn@ex>")
            .unwrap()
            .expect("bounce dedup marker should be recorded");
        assert!(recorded.ticket_id.is_none(), "bounces don't open a ticket");
    }

    /// Helper: build a NewOutboundEmail for use in bounce-flow tests.
    /// All fields are filled with realistic-but-uninteresting values
    /// so each test only has to specify what it actually cares about
    /// (Message-ID for linkage, recipient for suppression).
    fn outbound_row(
        channel_id: i32,
        message_id: &str,
        recipient: &str,
    ) -> crate::models::NewOutboundEmail {
        crate::models::NewOutboundEmail {
            channel_id: Some(channel_id),
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
            idempotency_key: None,
            sender_identity: crate::models::outbound_email_sender_identity::WORKSPACE.to_string(),
            mail_class: crate::models::outbound_email_mail_class::TRANSACTIONAL.to_string(),
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
        assert!(
            original.bounced_at.is_none(),
            "fresh row has no bounce stamp"
        );

        // Parse the canonical Postfix DSN fixture through the real
        // email_imap entry point so detect_bounce + parse_bounce both
        // fire and bounce_report gets populated.
        let raw = include_bytes!("../../../tests/fixtures/dsn/postfix-canonical.eml");
        let msg = super::super::email_imap::parse_rfc822_into_inbound_message(raw, None)
            .expect("fixture should parse");
        assert!(msg.is_bounce, "detect_bounce should flag the fixture");
        assert!(
            !msg.bounce_reports.is_empty(),
            "parse_bounce should yield at least one report"
        );

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
        assert!(
            refreshed.bounced_at.is_some(),
            "outbound row should be marked bounced"
        );
        assert_eq!(
            refreshed.bounce_recipient.as_deref(),
            Some("bob@example.org")
        );
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

    /// RFC 3464 §2.1 multi-block DSN: when a single outbound (e.g.
    /// to a distribution list) bounces for several downstream
    /// recipients, the pipeline must suppress *each* failed address
    /// independently rather than just the first per-recipient block.
    #[tokio::test]
    async fn multi_recipient_dsn_suppresses_every_failed_recipient() {
        let mut conn = setup_test_connection();
        let ch = TestFixtures::create_channel(&mut conn, "email_imap");

        // One outbound row, sent to a list address. The DSN names
        // two downstream members (alice, carol) that the list
        // server failed to deliver to.
        let _original = crate::repository::outbound_emails::enqueue(
            &mut conn,
            outbound_row(ch.id, "out-multi@yourco.com", "mailing-list@example.org"),
        )
        .unwrap();

        let raw = include_bytes!("../../../tests/fixtures/dsn/multi-recipient.eml");
        let msg = super::super::email_imap::parse_rfc822_into_inbound_message(raw, None).unwrap();
        assert_eq!(
            msg.bounce_reports.len(),
            2,
            "multi-recipient fixture should yield 2 reports",
        );

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

        // Both failed downstream addresses must be on the
        // suppression list, even though only one outbound row
        // existed (we sent to the list, not to the members).
        assert!(
            crate::repository::email_suppressions::is_suppressed(&mut conn, "alice@example.org")
                .unwrap(),
            "alice should be auto-suppressed",
        );
        assert!(
            crate::repository::email_suppressions::is_suppressed(&mut conn, "carol@example.org")
                .unwrap(),
            "carol should be auto-suppressed",
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

    // Classifier unit tests live in `bounce_parser::tests` alongside
    // the `is_hard_bounce` / `BounceReport::is_hard` definitions; the
    // pipeline tests above exercise the integration path end-to-end.

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
    async fn verified_staff_emailing_directly_opens_ticket_for_themselves() {
        // A staff member (admin) emails the channel directly, no forward
        // markers. They become the requester and a ticket opens — emailing in
        // is never rejected for who the sender is.
        let mut conn = setup_test_connection();

        let admin = crate::models::NewUser {
            uuid: uuid::Uuid::now_v7(),
            name: "Admin".into(),
            pronouns: None,
            avatar_url: None,
            banner_url: None,
            avatar_thumb: None,
            microsoft_uuid: None,
            mfa_secret: None,
            mfa_secret_kek_id: None,
            mfa_enabled: false,
            platform_role: Some("platform_admin".to_string()),
        };
        let (admin, _) = create_user_with_email(
            admin,
            crate::models::WorkspaceRole::Admin,
            "claimed@example.com".into(),
            true,
            None,
            &mut conn,
            None,
            crate::repository::workspaces::SeatWriteAuthority::ControlPlane,
        )
        .and_then(|o| o.into_created())
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

        let ticket_id = match outcome {
            PipelineOutcome::TicketOpened { ticket_id, .. } => ticket_id,
            other => panic!("expected TicketOpened, got {other:?}"),
        };
        let ticket = tickets_repo::get_ticket_by_id(&mut conn, ticket_id).unwrap();
        assert_eq!(
            ticket.requester_uuid,
            Some(admin.uuid),
            "the staff sender should be the requester"
        );
    }

    #[tokio::test]
    async fn dmarc_failed_spoof_of_staff_is_refused() {
        // B3: a From that DMARC explicitly failed and names a privileged
        // (staff) account is a spoofing attempt. It must NOT be ingested as
        // that account — no ticket, no comment authored as the agent.
        let mut conn = setup_test_connection();

        let admin = crate::models::NewUser {
            uuid: uuid::Uuid::now_v7(),
            name: "Admin".into(),
            pronouns: None,
            avatar_url: None,
            banner_url: None,
            avatar_thumb: None,
            microsoft_uuid: None,
            mfa_secret: None,
            mfa_secret_kek_id: None,
            mfa_enabled: false,
            platform_role: Some("platform_admin".to_string()),
        };
        let (_admin, _) = create_user_with_email(
            admin,
            crate::models::WorkspaceRole::Admin,
            "claimed@example.com".into(),
            true,
            None,
            &mut conn,
            None,
            crate::repository::workspaces::SeatWriteAuthority::ControlPlane,
        )
        .and_then(|o| o.into_created())
        .expect("seed admin");

        let ch = TestFixtures::create_channel(&mut conn, "email_imap");
        let mut msg = sample_message("<spoof@ex>", vec![], Some("please reset the CEO password"));
        msg.from.known_email = Some("claimed@example.com".into());
        msg.from.external_id = "claimed@example.com".into();
        msg.sender_auth = SenderAuth::Fail;

        let outcome = process_event(
            &StubAdapter,
            &ch,
            InboundEvent::MessageReceived(msg),
            &mut conn,
            &PipelineContext::bare(),
        )
        .await
        .unwrap();

        assert_eq!(
            outcome,
            PipelineOutcome::SkippedSpoofedSender,
            "a DMARC-failed spoof of a staff account must be refused"
        );
    }

    #[tokio::test]
    async fn dmarc_failed_unknown_sender_still_opens_guest_ticket() {
        // The guard is narrow: an explicit DMARC fail from an *unknown* address
        // (no matching account) is just spam and still opens a guest ticket —
        // we never drop a customer's request over a failed alignment.
        let mut conn = setup_test_connection();
        let ch = TestFixtures::create_channel(&mut conn, "email_imap");
        let mut msg = sample_message("<stranger@ex>", vec![], Some("help"));
        // sample_message's sender (alice@example.com) is not a seeded account.
        msg.sender_auth = SenderAuth::Fail;

        let outcome = process_event(
            &StubAdapter,
            &ch,
            InboundEvent::MessageReceived(msg),
            &mut conn,
            &PipelineContext::bare(),
        )
        .await
        .unwrap();

        assert!(
            matches!(outcome, PipelineOutcome::TicketOpened { .. }),
            "unknown DMARC-failed sender should still open a guest ticket, got {outcome:?}"
        );
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
            pronouns: None,
            avatar_url: None,
            banner_url: None,
            avatar_thumb: None,
            microsoft_uuid: None,
            mfa_secret: None,
            mfa_secret_kek_id: None,
            mfa_enabled: false,
            platform_role: None,
        };
        let (tech, _) = create_user_with_email(
            tech_user,
            crate::models::WorkspaceRole::Agent,
            "tech@yourco.com".into(),
            true,
            None,
            &mut conn,
            None,
            crate::repository::workspaces::SeatWriteAuthority::ControlPlane,
        )
        .and_then(|o| o.into_created())
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
            PipelineOutcome::TicketOpened {
                ticket_id,
                comment_id,
            } => (ticket_id, comment_id),
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
    async fn spam_suspected_message_opens_a_flagged_ticket() {
        // A provider-flagged-as-spam message still opens a ticket (we never
        // silently drop a customer request) and the comment carries the flag
        // so agents can triage.
        let mut conn = setup_test_connection();
        let ch = TestFixtures::create_channel(&mut conn, "email_imap");
        let mut msg = sample_message("<spam@ex>", vec![], Some("cheap deals"));
        msg.from.known_email = Some("sender@elsewhere.com".into());
        msg.spam_suspected = true;

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
            PipelineOutcome::TicketOpened {
                ticket_id,
                comment_id,
            } => (ticket_id, comment_id),
            other => panic!("expected TicketOpened, got {other:?}"),
        };
        // The ticket opens (never dropped), flagged and de-prioritised.
        let ticket = tickets_repo::get_ticket_by_id(&mut conn, ticket_id).unwrap();
        assert!(
            ticket.spam_suspected,
            "spam mail should open a flagged ticket"
        );
        assert_eq!(ticket.priority, crate::models::TicketPriority::Low);
        // The inbound comment also carries the per-message flag.
        use crate::schema::comments::dsl as c;
        use diesel::prelude::*;
        let metadata: Option<serde_json::Value> = c::comments
            .filter(c::id.eq(comment_id))
            .select(c::channel_metadata)
            .first(&mut conn)
            .unwrap();
        assert_eq!(
            metadata.expect("channel_metadata present")["spam_suspected"],
            serde_json::json!(true),
        );
    }

    #[tokio::test]
    async fn verified_member_emailing_in_opens_ticket_for_themselves() {
        // An end-user member (e.g. an employee on an internal IT desk) emails
        // the channel. They're not staff, so they become the requester and a
        // ticket opens, attributed to their real account — this is the gap the
        // old EmailClaimed skip created.
        let mut conn = setup_test_connection();
        let member = crate::models::NewUser {
            uuid: uuid::Uuid::now_v7(),
            name: "Employee".into(),
            pronouns: None,
            avatar_url: None,
            banner_url: None,
            avatar_thumb: None,
            microsoft_uuid: None,
            mfa_secret: None,
            mfa_secret_kek_id: None,
            mfa_enabled: false,
            platform_role: None,
        };
        let (member, _) = create_user_with_email(
            member,
            crate::models::WorkspaceRole::Member,
            "employee@yourco.com".into(),
            true,
            None,
            &mut conn,
            None,
            crate::repository::workspaces::SeatWriteAuthority::ControlPlane,
        )
        .and_then(|o| o.into_created())
        .expect("seed member");

        let ch = TestFixtures::create_channel(&mut conn, "email_imap");
        let mut msg = sample_message("<plain@yourco>", vec![], Some("printer down"));
        msg.from.known_email = Some("employee@yourco.com".into());
        msg.body_text = "My printer won't turn on.".into();

        let outcome = process_event(
            &StubAdapter,
            &ch,
            InboundEvent::MessageReceived(msg),
            &mut conn,
            &PipelineContext::bare(),
        )
        .await
        .unwrap();

        let ticket_id = match outcome {
            PipelineOutcome::TicketOpened { ticket_id, .. } => ticket_id,
            other => panic!("expected TicketOpened, got {other:?}"),
        };
        let ticket = tickets_repo::get_ticket_by_id(&mut conn, ticket_id).unwrap();
        assert_eq!(
            ticket.requester_uuid,
            Some(member.uuid),
            "the member sender should be the requester, attributed to their real account"
        );
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
            assert!(!host_looks_internal(ok), "expected {ok} to be accepted");
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
