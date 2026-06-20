//! Inbound-email webhook: the HTTPS endpoint AWS SNS POSTs to when SES
//! receives mail for a forwarding address.
//!
//! Flow: verify the SNS signature, then dispatch by message type. A
//! `SubscriptionConfirmation` is confirmed by fetching its `SubscribeURL`. A
//! `Notification` carries an SES "Received" event: gate on the spam/virus
//! verdicts, route the envelope recipient's `<token>` to a workspace +
//! channel, fetch the raw MIME from S3, and feed it into the existing channels
//! parse pipeline. Mail to an unknown token that passed the scans is recorded
//! in the platform dead-letter log rather than dropped silently.
//!
//! Response codes are chosen for SNS's retry behaviour: 2xx for "handled,
//! don't retry" (including deliberate drops), 5xx only for transient failures
//! worth retrying (S3 read, DB), 403 for a failed signature (never retry).

use std::sync::Arc;

use actix_web::{web, HttpResponse};
use once_cell::sync::Lazy;
use tracing::{error, info, warn};

use crate::db::Pool;
use crate::handlers::sse::SseState;
use crate::models::{NewInboundDeadLetter, INBOUND_DEAD_LETTER_REASON_UNKNOWN_TOKEN};
use crate::repository::channels as channels_repo;
use crate::repository::{inbound_addresses, inbound_dead_letters};
use crate::services::channels::email_forward::EmailForwardAdapter;
use crate::services::channels::email_imap::parse_rfc822_into_inbound_message;
use crate::services::channels::pipeline::{self, PipelineContext, PipelineOutcome};
use crate::services::channels::InboundEvent;
use crate::services::inbound_email::s3_fetch::InboundS3;
use crate::services::inbound_email::{ses, sns};
use crate::services::search::SearchService;
use crate::sync::actor::ActorContext;
use crate::sync::session::{elevate_session_role, reset_session_role};
use crate::utils::storage::Storage;

/// Shared HTTP client for the SNS certificate fetch + subscription
/// confirmation. Reused so each request doesn't spin up a new connection pool.
static HTTP: Lazy<reqwest::Client> = Lazy::new(reqwest::Client::new);

/// The domain forwarding addresses live under (`<token>@<inbound_domain>`).
fn inbound_domain() -> String {
    std::env::var("NOSDESK_INBOUND_DOMAIN").unwrap_or_default()
}

/// `POST /api/inbound/email` — the SNS subscription target. Unauthenticated;
/// the SNS signature is the authentication (see [`sns`]).
pub async fn receive(
    body: web::Bytes,
    pool: web::Data<Pool>,
    storage: web::Data<Arc<dyn Storage>>,
    sse_state: web::Data<SseState>,
    search_service: web::Data<Arc<SearchService>>,
    inbound_s3: web::Data<Option<InboundS3>>,
) -> HttpResponse {
    let message = match sns::SnsMessage::parse(&body) {
        Ok(m) => m,
        Err(e) => {
            warn!(error = %e, "inbound: unparseable SNS body");
            return HttpResponse::BadRequest().finish();
        }
    };

    if let Err(e) = sns::verify_message(&HTTP, &message).await {
        warn!(error = %e, "inbound: SNS signature verification failed");
        return HttpResponse::Forbidden().finish();
    }

    if message.is_subscription_confirmation() {
        return confirm_subscription(&message).await;
    }
    if !message.is_notification() {
        // UnsubscribeConfirmation or any other type: nothing to do.
        info!(kind = %message.type_, "inbound: ignoring non-notification SNS message");
        return HttpResponse::Ok().finish();
    }

    let notification = match ses::SesNotification::parse(&message.message) {
        Ok(n) => n,
        Err(e) => {
            warn!(error = %e, "inbound: SNS notification was not a parseable SES event");
            return HttpResponse::Ok().finish();
        }
    };

    // Spam/virus gate. Drops are silent (200, no retry, no row).
    match ses::gate(&notification.receipt) {
        ses::GateOutcome::DropVirus => {
            warn!("inbound: dropped message failing virus scan");
            return HttpResponse::Ok().finish();
        }
        ses::GateOutcome::DropSpam => {
            info!("inbound: dropped message failing spam scan");
            return HttpResponse::Ok().finish();
        }
        ses::GateOutcome::Pass => {}
    }

    let Some(s3) = inbound_s3.get_ref() else {
        // Hosted always configures this; an unconfigured server can't fetch the
        // body, and retrying won't fix config, so don't ask SNS to retry.
        error!(
            "inbound: received SES notification but NOSDESK_INBOUND_S3_BUCKET is not configured"
        );
        return HttpResponse::Ok().finish();
    };
    let Some(object_key) = notification.s3_object_key().map(|s| s.to_string()) else {
        warn!("inbound: SES notification has no S3 object key; nothing to fetch");
        return HttpResponse::Ok().finish();
    };

    let domain = inbound_domain();
    let token = ses::first_token(&notification.receipt.recipients, &domain)
        .or_else(|| ses::first_token(&notification.mail.destination, &domain));

    // Resolve the token to a workspace + channel. The lookup is cross-tenant
    // (we don't know the workspace until the token resolves), so it runs on a
    // system/bypass connection.
    let resolved = match &token {
        Some(tok) => {
            let tok = tok.clone();
            match crate::sync::session::background_run(
                &pool,
                "inbound:resolve_token",
                move |conn| inbound_addresses::find_active_by_token(conn, &tok),
            ) {
                Ok(r) => r,
                Err(e) => {
                    error!(error = %e, "inbound: token resolution failed");
                    return HttpResponse::ServiceUnavailable().finish();
                }
            }
        }
        None => None,
    };

    let Some(address) = resolved else {
        // Unknown (or absent) token + scans passed: dead-letter so a
        // misconfigured forward is diagnosable instead of vanishing.
        return record_dead_letter(&pool, &notification, &object_key);
    };

    // Known token: fetch the raw MIME and run it through the pipeline.
    let raw = match s3.get(&object_key).await {
        Ok(bytes) => bytes,
        Err(e) => {
            error!(error = %e, key = %object_key, "inbound: S3 fetch failed");
            return HttpResponse::ServiceUnavailable().finish();
        }
    };

    let mut msg = match parse_rfc822_into_inbound_message(&raw, None) {
        Ok(m) => m,
        Err(e) => {
            warn!(error = %e, "inbound: raw MIME failed to parse; dropping");
            return HttpResponse::Ok().finish();
        }
    };
    // Preserve the original for "show original" / re-parsing on policy change.
    if msg.raw_bytes.is_none() {
        msg.raw_bytes = Some(raw);
    }

    match ingest(
        &pool,
        storage.get_ref().clone(),
        sse_state.clone(),
        search_service.get_ref().clone(),
        address.workspace_id,
        address.channel_id,
        msg,
    )
    .await
    {
        Ok(outcome) => {
            info!(
                channel_id = address.channel_id,
                workspace_id = address.workspace_id,
                ?outcome,
                "inbound: processed forwarded message"
            );
            HttpResponse::Ok().finish()
        }
        Err(e) => {
            error!(error = %e, "inbound: pipeline failed; asking SNS to retry");
            HttpResponse::ServiceUnavailable().finish()
        }
    }
}

/// Run a resolved message through the channels parse pipeline, pinned to the
/// owning workspace. Mirrors the IMAP poll loop's session handling: the
/// pipeline holds the connection across `.await` points while writing RLS
/// tenant tables, so we elevate the session for the workspace and always reset
/// before the connection returns to the pool.
async fn ingest(
    pool: &Pool,
    storage: Arc<dyn Storage>,
    sse: web::Data<SseState>,
    search: Arc<SearchService>,
    workspace_id: i32,
    channel_id: i32,
    msg: crate::services::channels::InboundMessage,
) -> Result<PipelineOutcome, String> {
    let mut conn = pool.get().map_err(|e| format!("pool acquire: {e}"))?;
    let actor = ActorContext::system("inbound:ingest").with_workspace(workspace_id);
    elevate_session_role(&mut conn, &actor).map_err(|e| format!("elevate session: {e}"))?;

    let channel = match channels_repo::find(&mut conn, channel_id) {
        Ok(c) => c,
        Err(e) => {
            reset_session_role(&mut conn);
            return Err(format!("load channel {channel_id}: {e}"));
        }
    };

    let adapter = EmailForwardAdapter::new(channel_id);
    // Auto-ack (and outbound threadback) for forwarding channels are wired with
    // the outbound dispatch path, so resolver/pool are left unset here; the
    // attachment/SSE/search side effects still run.
    let ctx = PipelineContext {
        storage: Some(storage),
        sse: Some(sse),
        search: Some(search),
        http: Some(HTTP.clone()),
        resolver: None,
        pool: None,
    };

    let result = pipeline::process_event(
        &adapter,
        &channel,
        InboundEvent::MessageReceived(msg),
        &mut conn,
        &ctx,
    )
    .await;

    reset_session_role(&mut conn);
    result.map_err(|e| e.to_string())
}

/// Record clean inbound mail that resolved to no active token. The table is
/// untenanted, so this runs on a system connection.
fn record_dead_letter(
    pool: &Pool,
    notification: &ses::SesNotification,
    object_key: &str,
) -> HttpResponse {
    let recipient = notification
        .first_recipient()
        .unwrap_or_default()
        .to_string();
    let row = NewInboundDeadLetter {
        envelope_recipient: recipient,
        from_address: notification.sender(),
        subject: notification.subject(),
        s3_key: object_key.to_string(),
        reason: INBOUND_DEAD_LETTER_REASON_UNKNOWN_TOKEN.to_string(),
    };
    match crate::sync::session::background_run(pool, "inbound:dead_letter", move |conn| {
        inbound_dead_letters::record(conn, row)
    }) {
        Ok(rec) => {
            info!(
                id = rec.id,
                recipient = %rec.envelope_recipient,
                "inbound: recorded unrouted mail in dead-letter log"
            );
            HttpResponse::Ok().finish()
        }
        Err(e) => {
            error!(error = %e, "inbound: failed to record dead-letter");
            HttpResponse::ServiceUnavailable().finish()
        }
    }
}

/// Confirm an SNS HTTPS subscription by fetching its `SubscribeURL`. The URL is
/// constrained to an AWS SNS host (the same allowlist as the signing cert)
/// before we fetch it.
async fn confirm_subscription(message: &sns::SnsMessage) -> HttpResponse {
    let Some(subscribe_url) = &message.subscribe_url else {
        warn!("inbound: SubscriptionConfirmation without a SubscribeURL");
        return HttpResponse::BadRequest().finish();
    };
    let url = match sns::validate_cert_url(subscribe_url) {
        Ok(u) => u,
        Err(e) => {
            warn!(error = %e, "inbound: SubscribeURL is not an AWS SNS host");
            return HttpResponse::BadRequest().finish();
        }
    };
    match HTTP
        .get(url)
        .send()
        .await
        .and_then(|r| r.error_for_status())
    {
        Ok(_) => {
            info!(topic = %message.topic_arn, "inbound: confirmed SNS subscription");
            HttpResponse::Ok().finish()
        }
        Err(e) => {
            error!(error = %e, "inbound: SNS subscription confirmation fetch failed");
            HttpResponse::ServiceUnavailable().finish()
        }
    }
}
