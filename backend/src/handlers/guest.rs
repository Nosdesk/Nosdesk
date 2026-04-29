//! Public (unauthenticated) guest-facing endpoints.
//!
//! All handlers in this module intentionally run outside the cookie auth
//! middleware. Each handler re-checks the relevant toggle in `site_settings`
//! before doing any work, so disabling a feature in the admin UI immediately
//! closes its public surface.

use actix_multipart::Multipart;
use actix_web::{web, HttpRequest, HttpResponse, Responder};
use chrono::Utc;
use diesel::prelude::*;
use futures::{StreamExt, TryStreamExt};
use ipnetwork::IpNetwork;
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use crate::db::{DbConnection, Pool};
use crate::handlers::helpers;
use crate::models::{
    NewAttachment, NewTicket, PublicSiteSettings, SiteSettings, TicketPriority, TicketStatus,
};
use crate::repository::user_helpers::GuestUserResult;
use crate::repository::{self, site_settings, user_helpers};
use crate::services::search::indexing_tasks;
use crate::services::search::SearchService;
use crate::utils::file_validation::{
    FileValidator, GUEST_ATTACHMENT_TTL_MINUTES, GUEST_MAX_FILES_PER_TICKET,
    GUEST_MAX_FILE_SIZE_MB,
};
use crate::utils::rate_limit::{self, RateLimiter};
use crate::utils::storage::Storage;

// ---------- Constants ----------

/// Upper bound on the submitter's name. 120 chars is generous for personal
/// names (the CLDR 99th percentile is ~60) while still tight enough that
/// a bot can't dump prose into the field.
const GUEST_MAX_NAME_LENGTH: usize = 120;

/// Upper bound on the free-text description of a ticket. Matches the
/// `maxlength` attribute on the frontend textarea.
const GUEST_MAX_DESCRIPTION_LENGTH: usize = 10_000;

/// Upper bound on a public documentation search query — prevents a
/// pathological ILIKE pattern from dominating Postgres CPU.
const GUEST_DOC_SEARCH_MAX_QUERY_LENGTH: usize = 200;

/// Max rows returned by the public documentation search endpoint.
const GUEST_DOC_SEARCH_RESULT_LIMIT: i64 = 25;

/// Per-IP cap on guest uploads per hour. See the comment next to the
/// corresponding rate-limiter call — tightening this is the lever for
/// disk-fill abuse.
const GUEST_UPLOADS_PER_HOUR: u32 = 20;

// ---------- Request payloads ----------

/// Body accepted by `POST /api/public/tickets`. Submission is rejected if
/// the honeypot [`website`][Self::website] field is non-empty.
#[derive(Debug, Deserialize)]
pub struct SubmitGuestTicketRequest {
    pub name: String,
    pub email: String,
    pub title: String,
    pub description: String,
    pub priority: Option<String>,
    /// Client-supplied list of temp attachment IDs from
    /// `POST /api/public/files/temp`. The submit handler validates that
    /// each ID is unclaimed and recent before binding it to the ticket.
    #[serde(default)]
    pub attachment_ids: Vec<i32>,
    /// Honeypot field — a decoy input that's hidden via CSS/`sr-only` on
    /// the real form. Humans never fill it; naive spam bots auto-fill any
    /// input they find. Non-empty value → silently reject.
    ///
    /// The name ("website") is deliberate bait — it's one of the most
    /// common fields bots are trained to populate.
    #[serde(default)]
    pub website: Option<String>,
}

// ---------- Helpers ----------

/// Parse a priority string into the enum. Returns `None` for unknown input
/// so the caller can reject it with 400 rather than silently coercing.
/// A missing (`None`) input defaults to Medium.
fn parse_priority(s: Option<&str>) -> Option<TicketPriority> {
    match s {
        None => Some(TicketPriority::Medium),
        Some(v) => match v.to_ascii_lowercase().as_str() {
            "low" => Some(TicketPriority::Low),
            "medium" => Some(TicketPriority::Medium),
            "high" => Some(TicketPriority::High),
            _ => None,
        },
    }
}

/// Return true if the domain portion of the email has any MX record, OR
/// any A/AAAA record as an RFC 5321 §5.1 fallback. Used as a cheap
/// pre-flight filter on guest submissions.
///
/// Returns `true` on resolver errors (fail-open) so upstream DNS flakiness
/// doesn't reject legitimate submissions; the verification email is the
/// authoritative "can we reach this address" check.
async fn email_domain_has_mx(email: &str) -> bool {
    use hickory_resolver::config::{ResolverConfig, ResolverOpts};
    use hickory_resolver::TokioAsyncResolver;

    let Some(domain) = email.rsplit('@').next() else {
        return false;
    };
    if domain.is_empty() {
        return false;
    }

    let mut opts = ResolverOpts::default();
    // Short timeout: we don't want the submit path to stall if the
    // resolver is slow. ~2s is a reasonable ceiling.
    opts.timeout = std::time::Duration::from_secs(2);
    opts.attempts = 1;

    let resolver = TokioAsyncResolver::tokio(ResolverConfig::default(), opts);

    // MX record presence is the strong signal.
    match resolver.mx_lookup(domain).await {
        Ok(mx) if mx.iter().next().is_some() => return true,
        // Explicit NXDOMAIN means the domain itself doesn't exist.
        Err(e) => {
            let kind = e.kind();
            if matches!(
                kind,
                hickory_resolver::error::ResolveErrorKind::NoRecordsFound { .. }
            ) {
                // Fall through to A/AAAA check — some tiny domains serve
                // mail via A record per RFC 5321 §5.1.
            } else {
                // Transient resolver errors → fail-open.
                warn!(error = %e, %domain, "MX lookup errored; allowing submission");
                return true;
            }
        }
        Ok(_) => { /* MX query succeeded but returned no records; check A */ }
    }

    // A/AAAA fallback. If THIS also returns NXDOMAIN, the domain is dead.
    match resolver.lookup_ip(domain).await {
        Ok(ips) => ips.iter().next().is_some(),
        Err(e) => {
            let kind = e.kind();
            if matches!(
                kind,
                hickory_resolver::error::ResolveErrorKind::NoRecordsFound { .. }
            ) {
                false
            } else {
                warn!(error = %e, %domain, "A lookup errored; allowing submission");
                true
            }
        }
    }
}

/// Escape `%`, `_`, and `\` for safe use in an `ILIKE` pattern.
/// Diesel parameterizes the value, but LIKE wildcards still apply on the
/// Postgres side, so un-escaped metacharacters would change match semantics.
fn escape_like(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

fn valid_email(e: &str) -> bool {
    // Cheap shape check; full validation is deferred to downstream
    // systems (SMTP, auth). We just want to reject obvious garbage.
    let trimmed = e.trim();
    let at = trimmed.find('@');
    match at {
        Some(i) if i > 0 && i < trimmed.len() - 1 => {
            trimmed[i + 1..].contains('.') && trimmed.len() <= 254
        }
        _ => false,
    }
}

fn get_settings(conn: &mut DbConnection) -> Option<SiteSettings> {
    site_settings::get_site_settings(conn).ok()
}

fn client_ip(req: &HttpRequest) -> Option<IpNetwork> {
    req.peer_addr()
        .and_then(|addr| addr.ip().to_string().parse().ok())
}

/// Best-effort audit log; failures are swallowed so they can't break a
/// legitimate submission.
fn log_guest_event(
    conn: &mut DbConnection,
    user_uuid: Uuid,
    event_type: &str,
    req: &HttpRequest,
    details: serde_json::Value,
) {
    use crate::utils::security_events::{record_security_event, SecurityEventInput};

    if let Err(e) = record_security_event(
        conn,
        SecurityEventInput {
            user_uuid,
            event_type,
            severity: "info",
            details: Some(details),
            request: Some(req),
            session_id: None,
        },
    ) {
        warn!(error = %e, "Failed to record guest security event");
    }
}

// ---------- Public settings ----------

/// GET /api/public/settings — branding + which guest features are enabled.
pub async fn get_public_settings(pool: web::Data<Pool>) -> impl Responder {
    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    match site_settings::get_site_settings(&mut conn) {
        Ok(s) => HttpResponse::Ok().json(PublicSiteSettings::from(&s)),
        Err(e) => {
            warn!(error = ?e, "Failed to load site_settings for public endpoint");
            HttpResponse::Ok().json(json!({
                "app_name": "Nosdesk",
                "logo_url": null,
                "logo_light_url": null,
                "favicon_url": null,
                "primary_color": null,
                "guest_tickets_enabled": false,
                "guest_public_docs_enabled": false,
                "guest_kb_search_enabled": false,
                "guest_ticket_lookup_enabled": false,
                "guest_help_page_enabled": false,
            }))
        }
    }
}

// ---------- Guest ticket submission ----------

/// POST /api/public/tickets
pub async fn submit_guest_ticket(
    pool: web::Data<Pool>,
    sse_state: web::Data<crate::handlers::sse::SseState>,
    search_service: web::Data<Arc<SearchService>>,
    storage: web::Data<Arc<dyn Storage>>,
    req: HttpRequest,
    body: web::Json<SubmitGuestTicketRequest>,
) -> impl Responder {
    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let settings = match get_settings(&mut conn) {
        Some(s) => s,
        None => {
            return HttpResponse::ServiceUnavailable()
                .json(json!({"error": "Settings unavailable"}));
        }
    };

    if !settings.guest_tickets_enabled {
        return HttpResponse::Forbidden().json(json!({
            "error": "Guest ticket submission is disabled"
        }));
    }

    // Honeypot: a non-empty `website` field means a bot filled a hidden
    // input. Return a plausible-looking 400 rather than surfacing that the
    // trap worked — keeps the bot from learning to avoid it.
    if body.website.as_deref().map(str::trim).is_some_and(|s| !s.is_empty()) {
        debug!(ip = ?client_ip(&req), "Guest ticket submission tripped honeypot");
        return HttpResponse::BadRequest().json(json!({"error": "Invalid submission"}));
    }

    // Basic input validation
    let name = body.name.trim();
    let email = body.email.trim();
    let title = body.title.trim();
    let description = body.description.trim();

    if name.is_empty() || name.len() > GUEST_MAX_NAME_LENGTH {
        return HttpResponse::BadRequest().json(json!({"error": "Invalid name"}));
    }
    if !valid_email(email) {
        return HttpResponse::BadRequest().json(json!({"error": "Invalid email"}));
    }
    // MX pre-check: reject addresses whose domain has no mail servers. This
    // is a cheap filter that catches `random@madeupdomain.tld` garbage
    // before we commit to creating a user + sending a confirmation email
    // that will just hard-bounce. The check fails open on resolver errors
    // so flaky upstream DNS doesn't break legitimate submissions.
    if !email_domain_has_mx(email).await {
        return HttpResponse::BadRequest()
            .json(json!({"error": "We can't deliver mail to that address."}));
    }
    if title.is_empty() || title.len() > 255 {
        return HttpResponse::BadRequest().json(json!({"error": "Invalid title"}));
    }
    if description.is_empty() || description.len() > GUEST_MAX_DESCRIPTION_LENGTH {
        return HttpResponse::BadRequest().json(json!({"error": "Invalid description"}));
    }

    // Resolve priority early so we can 400 on bad client input before touching
    // the DB. If the admin forced a default we use that; otherwise we honor
    // the submitter's choice (defaulting to Medium if omitted).
    let priority = {
        let resolved = settings
            .guest_ticket_default_priority
            .as_deref()
            .map(|p| parse_priority(Some(p)))
            .unwrap_or_else(|| parse_priority(body.priority.as_deref()));
        match resolved {
            Some(p) => p,
            None => {
                return HttpResponse::BadRequest()
                    .json(json!({"error": "Invalid priority"}));
            }
        }
    };

    // Per-IP rate limit, sourced from site_settings. Uses the shared Redis
    // sliding-window helper (matches auth / MFA rate limiting). Fail-open on
    // Redis errors so a misconfigured cache doesn't take down submissions —
    // this mirrors the existing helper's behavior and the app's other
    // fail-open limiters. The baseline actix-limitation middleware still
    // enforces the coarse per-minute cap underneath.
    if let Some(ip) = client_ip(&req).map(|n| n.ip()) {
        let key = format!("guest_tickets:{ip}");
        let max = u32::try_from(settings.guest_ticket_rate_limit_per_hour.max(1))
            .unwrap_or(u32::MAX);
        let redis_url = rate_limit::get_redis_url();
        match RateLimiter::check_rate_limit(&redis_url, &key, max, 3600).await {
            Ok(true) => { /* allowed */ }
            Ok(false) => {
                return HttpResponse::TooManyRequests().json(json!({
                    "error": "Too many submissions from your network. Please try again later."
                }));
            }
            Err(e) => {
                warn!(error = %e, "Guest-ticket rate limiter unavailable; allowing request");
            }
        }
    }

    // Find or provision the requester user. If the email is already attached
    // to a real account we refuse the submission rather than silently
    // attaching the ticket to someone else's identity.
    let (user, is_new_guest) = match user_helpers::find_or_create_guest_user(email, name, &mut conn, Some(search_service.get_ref())) {
        Ok(GuestUserResult::Created(u)) => (u, true),
        Ok(GuestUserResult::Existing(u)) => (u, false),
        Ok(GuestUserResult::EmailClaimed) => {
            return HttpResponse::Conflict().json(json!({
                "error": "Please sign in to submit a ticket with this email address."
            }));
        }
        Err(e) => {
            error!(error = %e, "Failed to provision guest user");
            return HttpResponse::InternalServerError()
                .json(json!({"error": "Failed to create ticket"}));
        }
    };

    // When email verification is enabled, tickets start in a `pending` state
    // invisible to techs (see ticket_query::apply_filters). The verification
    // link in the invitation email flips them to `verified` atomically in
    // accept_invitation::verify_pending_tickets_for_user.
    let verification_required = settings.guest_ticket_email_verification;
    let lookup_token = Uuid::new_v4();
    let new_ticket = NewTicket {
        title: title.to_string(),
        status: TicketStatus::Open,
        priority,
        requester_uuid: Some(user.uuid),
        assignee_uuid: None,
        category_id: None,
        submitted_via: Some("guest".to_string()),
        guest_lookup_token: Some(lookup_token),
        verification_state: if verification_required {
            Some("pending".to_string())
        } else {
            None
        },
        origin_channel_id: None,
    };

    let ticket = match repository::tickets::create_ticket(&mut conn, new_ticket) {
        Ok(t) => t,
        Err(e) => {
            error!(error = %e, "Failed to create guest ticket");
            return HttpResponse::InternalServerError()
                .json(json!({"error": "Failed to create ticket"}));
        }
    };

    // Persist the description as the initial comment from the guest user so
    // techs see the reported issue in the normal ticket timeline.
    let new_comment = crate::models::NewComment {
        content: description.to_string(),
        user_uuid: user.uuid,
        ticket_id: ticket.id,
        channel_metadata: None,
        is_internal: false,
        // Guest portal currently posts plaintext into the description.
        // (When the guest form moves to a rich editor, change this to
        // match what the editor produces.)
        content_format: crate::models::ContentFormat::Plaintext,
    };
    let first_comment_id = match repository::comments::create_comment(&mut conn, new_comment, Some(search_service.get_ref())) {
        Ok(c) => Some(c.id),
        Err(e) => {
            warn!(
                error = %e,
                ticket_id = ticket.id,
                "Failed to persist guest-ticket description as comment"
            );
            None
        }
    };

    // Claim any referenced attachments. Cap at GUEST_MAX_FILES_PER_TICKET —
    // silently truncate extras rather than rejecting the submission.
    if let Some(comment_id) = first_comment_id {
        if !body.attachment_ids.is_empty() && settings.guest_ticket_attachments_enabled {
            let ids: Vec<i32> = body
                .attachment_ids
                .iter()
                .copied()
                .take(GUEST_MAX_FILES_PER_TICKET)
                .collect();
            claim_guest_attachments(
                &mut conn,
                storage.get_ref(),
                &ids,
                ticket.id,
                comment_id,
                user.uuid,
            )
            .await;
        }
    }

    log_guest_event(
        &mut conn,
        user.uuid,
        "guest_ticket_submitted",
        &req,
        json!({
            "ticket_id": ticket.id,
            "email_domain": email.rsplit('@').next().unwrap_or(""),
            "new_account": is_new_guest,
        }),
    );

    // Email dispatch:
    //   - verification on  → send confirmation every time (the link is also
    //     the "release my pending ticket" trigger, so even Existing guests
    //     with multiple pending tickets need a fresh, working link).
    //   - verification off → only send on new accounts (no ticket-gating
    //     concern, so re-sending to returning submitters would just be noise).
    //
    // The email itself is framed as "confirm your ticket submission" rather
    // than the generic "welcome / invitation" copy — see
    // EmailService::send_guest_ticket_confirmation_email.
    let send_email = verification_required || is_new_guest;
    let email_sent = if send_email {
        match crate::handlers::users::send_guest_ticket_confirmation(
            &mut conn,
            &req,
            user.uuid,
            email,
            name,
        )
        .await
        {
            crate::handlers::users::SendInvitationResult::Success => true,
            other => {
                warn!(
                    user_uuid = %user.uuid,
                    ticket_id = ticket.id,
                    result = %other,
                    "Failed to send guest ticket confirmation email"
                );
                false
            }
        }
    } else {
        false
    };

    // Only surface the ticket to techs once it's actually verified. Pending
    // tickets skip SSE + search indexing entirely — accept_invitation picks
    // them up at verification time and fires these broadcasts then.
    if !verification_required {
        indexing_tasks::spawn_index_ticket(
            search_service.get_ref().clone(),
            ticket.clone(),
            None,
        );
        sse_state
            .broadcast_event(crate::handlers::sse::SseEvent::TicketCreated {
                ticket_id: ticket.id,
                ticket: serde_json::to_value(&ticket).unwrap_or_default(),
                timestamp: chrono::Utc::now(),
            })
            .await;
    }

    info!(
        ticket_id = ticket.id,
        email_sent,
        verification_required,
        "Guest ticket submitted"
    );

    if verification_required {
        // Don't disclose the ticket id or lookup token yet — the Zendesk-
        // style UX treats a pending ticket as "not yet submitted" from the
        // requester's perspective. They get both once they verify.
        HttpResponse::Accepted().json(json!({
            "verification_required": true,
            "email_sent": email_sent,
        }))
    } else {
        HttpResponse::Created().json(json!({
            "verification_required": false,
            "ticket_id": ticket.id,
            "lookup_token": lookup_token.to_string(),
            "status_url": format!("/ticket-status/{}", lookup_token),
            "email_sent": email_sent,
        }))
    }
}

// ---------- Guest ticket status lookup ----------

/// GET /api/public/tickets/{token}
pub async fn get_guest_ticket_status(
    pool: web::Data<Pool>,
    path: web::Path<String>,
) -> impl Responder {
    let token_str = path.into_inner();
    let token = match Uuid::parse_str(&token_str) {
        Ok(t) => t,
        Err(_) => return HttpResponse::NotFound().finish(),
    };

    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let settings = match get_settings(&mut conn) {
        Some(s) => s,
        None => return HttpResponse::ServiceUnavailable().finish(),
    };

    if !settings.guest_ticket_lookup_enabled {
        return HttpResponse::Forbidden().json(json!({
            "error": "Guest ticket status lookup is disabled"
        }));
    }

    match repository::tickets::find_by_lookup_token(&mut conn, token) {
        Ok(t) => HttpResponse::Ok().json(json!({
            "ticket_id": t.id,
            "title": t.title,
            "status": t.status,
            "priority": t.priority,
            "created_at": t.created_at,
            "updated_at": t.updated_at,
            "closed_at": t.closed_at,
        })),
        Err(diesel::result::Error::NotFound) => HttpResponse::NotFound().finish(),
        Err(e) => {
            error!(error = %e, "Error looking up guest ticket");
            HttpResponse::InternalServerError().finish()
        }
    }
}

// ---------- Public documentation ----------

/// GET /api/public/docs
pub async fn list_public_docs(pool: web::Data<Pool>) -> impl Responder {
    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let settings = match get_settings(&mut conn) {
        Some(s) => s,
        None => return HttpResponse::ServiceUnavailable().finish(),
    };
    if !settings.guest_public_docs_enabled {
        return HttpResponse::Forbidden()
            .json(json!({"error": "Public documentation is disabled"}));
    }

    use crate::schema::documentation_pages::dsl::*;
    let rows = documentation_pages
        .filter(is_public.eq(true))
        .filter(deleted_at.is_null())
        .select((id, uuid, title, slug, icon, updated_at))
        .load::<(i32, Uuid, String, String, Option<String>, chrono::NaiveDateTime)>(&mut conn);

    match rows {
        Ok(list) => {
            let items: Vec<_> = list
                .into_iter()
                .map(|(pid, puuid, ptitle, pslug, picon, pupdated)| {
                    json!({
                        "id": pid,
                        "uuid": puuid,
                        "title": ptitle,
                        "slug": pslug,
                        "icon": picon,
                        "updated_at": pupdated,
                    })
                })
                .collect();
            HttpResponse::Ok().json(items)
        }
        Err(e) => {
            error!(error = %e, "Failed to list public docs");
            HttpResponse::InternalServerError().finish()
        }
    }
}

/// GET /api/public/docs/{slug}
pub async fn get_public_doc(
    pool: web::Data<Pool>,
    path: web::Path<String>,
) -> impl Responder {
    let slug_param = path.into_inner();
    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let settings = match get_settings(&mut conn) {
        Some(s) => s,
        None => return HttpResponse::ServiceUnavailable().finish(),
    };
    if !settings.guest_public_docs_enabled {
        return HttpResponse::Forbidden()
            .json(json!({"error": "Public documentation is disabled"}));
    }

    use crate::schema::documentation_pages::dsl::*;
    let page: Option<(i32, Uuid, String, String, Option<String>, Option<Vec<u8>>, chrono::NaiveDateTime)> =
        documentation_pages
            .filter(is_public.eq(true))
            .filter(deleted_at.is_null())
            .filter(slug.eq(&slug_param))
            .select((id, uuid, title, slug, icon, yjs_document, updated_at))
            .first(&mut conn)
            .optional()
            .unwrap_or(None);

    match page {
        Some((pid, puuid, ptitle, pslug, picon, pdoc, pupdated)) => HttpResponse::Ok().json(json!({
            "id": pid,
            "uuid": puuid,
            "title": ptitle,
            "slug": pslug,
            "icon": picon,
            "yjs_document": pdoc,
            "updated_at": pupdated,
        })),
        None => HttpResponse::NotFound().finish(),
    }
}

/// Query params for `GET /api/public/docs/search`.
#[derive(Debug, Deserialize)]
pub struct PublicDocsSearchQuery {
    pub q: String,
}

/// GET /api/public/docs/search?q=...
pub async fn search_public_docs(
    pool: web::Data<Pool>,
    query: web::Query<PublicDocsSearchQuery>,
) -> impl Responder {
    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };
    let settings = match get_settings(&mut conn) {
        Some(s) => s,
        None => return HttpResponse::ServiceUnavailable().finish(),
    };
    if !settings.guest_kb_search_enabled || !settings.guest_public_docs_enabled {
        return HttpResponse::Forbidden()
            .json(json!({"error": "Public documentation search is disabled"}));
    }

    let q = query.q.trim();
    if q.is_empty() || q.len() > GUEST_DOC_SEARCH_MAX_QUERY_LENGTH {
        return HttpResponse::BadRequest().json(json!({"error": "Invalid query"}));
    }

    use crate::schema::documentation_pages::dsl::*;
    let pattern = format!("%{}%", escape_like(q));
    let rows = documentation_pages
        .filter(is_public.eq(true))
        .filter(deleted_at.is_null())
        .filter(title.ilike(&pattern))
        .select((id, uuid, title, slug, icon, updated_at))
        .limit(GUEST_DOC_SEARCH_RESULT_LIMIT)
        .load::<(i32, Uuid, String, String, Option<String>, chrono::NaiveDateTime)>(&mut conn);

    match rows {
        Ok(list) => {
            let items: Vec<_> = list
                .into_iter()
                .map(|(pid, puuid, ptitle, pslug, picon, pupdated)| {
                    json!({
                        "id": pid,
                        "uuid": puuid,
                        "title": ptitle,
                        "slug": pslug,
                        "icon": picon,
                        "updated_at": pupdated,
                    })
                })
                .collect();
            debug!(count = items.len(), q = %q, "Public doc search");
            HttpResponse::Ok().json(items)
        }
        Err(e) => {
            error!(error = %e, "Public doc search failed");
            HttpResponse::InternalServerError().finish()
        }
    }
}

// ---------- Guest attachment upload ----------

/// POST /api/public/files/temp
///
/// Unauthenticated single-file upload for guest ticket submissions. Stores
/// the file in the shared `temp/` bucket and returns an id the client must
/// include in its submit payload. Abuse controls:
///
/// * Feature must be on (`guest_tickets_enabled` AND
///   `guest_ticket_attachments_enabled`).
/// * Per-IP rate limit, tighter than the ticket-submit limit — stops drive-by
///   storage-fill attacks even when no actual tickets are being created.
/// * Strict MIME allowlist + extension match + `infer` magic-byte check
///   ([`FileValidator::validate_guest_upload`]).
/// * Per-file size capped via [`GUEST_MAX_FILE_SIZE_MB`] (chunk-validated so
///   we bail before loading the whole payload into memory).
/// * Sanitized filename (no path traversal, no control chars).
/// * Resulting temp files are opaque — not served publicly. The only way
///   to retrieve them is to submit a ticket that references their id;
///   after that they move into the ticket's folder and inherit regular
///   ticket-file ACLs.
pub async fn upload_guest_attachment(
    pool: web::Data<Pool>,
    storage: web::Data<Arc<dyn Storage>>,
    req: HttpRequest,
    mut payload: Multipart,
) -> impl Responder {
    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let settings = match get_settings(&mut conn) {
        Some(s) => s,
        None => {
            return HttpResponse::ServiceUnavailable()
                .json(json!({"error": "Settings unavailable"}));
        }
    };

    if !settings.guest_tickets_enabled || !settings.guest_ticket_attachments_enabled {
        return HttpResponse::Forbidden()
            .json(json!({"error": "Attachments are not accepted"}));
    }

    // Dedicated per-IP rate limiter for uploads. Keeping it distinct from
    // the submit rate limiter matters: an attacker can upload-and-abandon
    // to fill disk without ever completing a submission.
    if let Some(ip) = client_ip(&req).map(|n| n.ip()) {
        let key = format!("guest_upload:{ip}");
        let redis_url = rate_limit::get_redis_url();
        match RateLimiter::check_rate_limit(&redis_url, &key, GUEST_UPLOADS_PER_HOUR, 3600).await {
            Ok(true) => { /* allowed */ }
            Ok(false) => {
                return HttpResponse::TooManyRequests().json(json!({
                    "error": "Too many uploads from your network. Please try again later."
                }));
            }
            Err(e) => {
                warn!(error = %e, "Guest upload rate limiter unavailable; allowing request");
            }
        }
    }

    // Read exactly one file field. Guests can only attach one file per
    // request — the client loops if they pick multiple. This keeps the
    // per-request surface small and maps 1:1 with the rate limiter.
    let mut field = match payload.try_next().await {
        Ok(Some(f)) => f,
        Ok(None) => {
            return HttpResponse::BadRequest()
                .json(json!({"error": "No file in request"}));
        }
        Err(e) => {
            debug!(error = %e, "Multipart parse error");
            return HttpResponse::BadRequest()
                .json(json!({"error": "Could not read upload"}));
        }
    };

    if field.name() != "file" {
        return HttpResponse::BadRequest()
            .json(json!({"error": "Expected field 'file'"}));
    }

    let original_filename = match field
        .content_disposition()
        .get_filename()
        .map(|s| s.to_string())
    {
        Some(n) if !n.is_empty() => n,
        _ => {
            return HttpResponse::BadRequest()
                .json(json!({"error": "Filename is required"}));
        }
    };

    let sanitized_filename = match FileValidator::sanitize_filename(&original_filename) {
        Ok(n) => n,
        Err(e) => {
            return HttpResponse::BadRequest()
                .json(json!({"error": e.to_string()}));
        }
    };

    // Read with incremental size check against the tighter guest cap.
    let max_bytes = GUEST_MAX_FILE_SIZE_MB * 1024 * 1024;
    let mut file_data = Vec::new();
    while let Some(chunk) = field.next().await {
        let data = match chunk {
            Ok(d) => d,
            Err(e) => {
                debug!(error = %e, "Read chunk error");
                return HttpResponse::BadRequest()
                    .json(json!({"error": "Upload interrupted"}));
            }
        };
        if file_data.len() + data.len() > max_bytes {
            return HttpResponse::PayloadTooLarge().json(json!({
                "error": format!("File exceeds {GUEST_MAX_FILE_SIZE_MB}MB limit")
            }));
        }
        file_data.extend_from_slice(&data);
    }

    let detected_mime = match FileValidator::validate_guest_upload(&file_data, &sanitized_filename)
    {
        Ok(m) => m,
        Err(e) => {
            debug!(error = ?e, filename = %sanitized_filename, "Guest upload rejected");
            return HttpResponse::BadRequest()
                .json(json!({"error": e.to_string()}));
        }
    };

    // SHA-256 checksum for integrity (mirrors the authenticated upload path).
    use ring::digest;
    let checksum_bytes = digest::digest(&digest::SHA256, &file_data);
    let checksum: String = checksum_bytes
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();

    let total_size = file_data.len();
    let stored_file = match storage
        .store_file(&file_data, &sanitized_filename, &detected_mime, "temp")
        .await
    {
        Ok(sf) => sf,
        Err(e) => {
            error!(error = ?e, "Failed to store guest upload");
            return HttpResponse::InternalServerError()
                .json(json!({"error": "Failed to store file"}));
        }
    };

    let new_attachment = NewAttachment {
        url: stored_file.url.clone(),
        name: sanitized_filename.clone(),
        file_size: Some(total_size as i64),
        mime_type: Some(detected_mime.clone()),
        checksum: Some(checksum),
        // Claimed by submit_guest_ticket when the submission arrives.
        comment_id: None,
        uploaded_by: None,
        transcription: None,
    };

    match repository::create_attachment(&mut conn, new_attachment) {
        Ok(att) => {
            info!(
                attachment_id = att.id,
                size = total_size,
                mime = %detected_mime,
                "Guest upload stored"
            );
            HttpResponse::Created().json(json!({
                "id": att.id,
                "name": sanitized_filename,
                "size": total_size,
                "mime_type": detected_mime,
            }))
        }
        Err(e) => {
            // Best-effort cleanup of the orphaned file. If this fails,
            // the daily temp-cleanup job will pick it up.
            let _ = storage.delete_file(&stored_file.path).await;
            error!(error = ?e, "Failed to record guest upload");
            HttpResponse::InternalServerError()
                .json(json!({"error": "Failed to save attachment"}))
        }
    }
}

/// Claim a set of guest-uploaded attachment ids, moving the files into the
/// ticket's folder and binding each row to the first comment.
///
/// Runs inside [`submit_guest_ticket`] after the ticket + first comment
/// have been created. Validates each id as:
///   - exists
///   - unclaimed (comment_id IS NULL, uploaded_by IS NULL)
///   - within [`GUEST_ATTACHMENT_TTL_MINUTES`] of creation
/// Anything that fails validation is skipped with a warning — a stuck or
/// missing attachment must never fail ticket creation.
async fn claim_guest_attachments(
    conn: &mut DbConnection,
    storage: &Arc<dyn Storage>,
    attachment_ids: &[i32],
    ticket_id: i32,
    comment_id: i32,
    requester_uuid: Uuid,
) {
    use crate::schema::attachments;

    let cutoff = Utc::now().naive_utc() - chrono::Duration::minutes(GUEST_ATTACHMENT_TTL_MINUTES);

    // Load + filter in one query: only rows that are still eligible to be
    // claimed. The IN list is already capped by the caller at
    // GUEST_MAX_FILES_PER_TICKET, so this scales fine.
    let candidates: Vec<crate::models::Attachment> = match attachments::table
        .filter(attachments::id.eq_any(attachment_ids))
        .filter(attachments::comment_id.is_null())
        .filter(attachments::uploaded_by.is_null())
        .filter(attachments::created_at.ge(cutoff))
        .load(conn)
    {
        Ok(rows) => rows,
        Err(e) => {
            warn!(error = %e, ticket_id, "Failed to load guest attachments for claim");
            return;
        }
    };

    for mut att in candidates {
        // URL looks like "/uploads/temp/<uuid>_<name>" — the storage path
        // the trait wants is relative to uploads/.
        let temp_storage_path = att.url.trim_start_matches("/uploads/").to_string();
        let file_only = temp_storage_path
            .trim_start_matches("temp/")
            .to_string();
        let new_storage_path = format!("tickets/{ticket_id}/{file_only}");

        match storage.move_file(&temp_storage_path, &new_storage_path).await {
            Ok(_) => {
                att.url = format!("/uploads/{new_storage_path}");
                att.comment_id = Some(comment_id);
                att.uploaded_by = Some(requester_uuid);

                if let Err(e) = diesel::update(attachments::table.find(att.id))
                    .set((
                        attachments::url.eq(&att.url),
                        attachments::comment_id.eq(Some(comment_id)),
                        attachments::uploaded_by.eq(Some(requester_uuid)),
                    ))
                    .execute(conn)
                {
                    warn!(error = %e, attachment_id = att.id, "Failed to update claimed attachment");
                }
            }
            Err(e) => {
                warn!(error = ?e, attachment_id = att.id, "Failed to move guest attachment");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_priority_accepts_canonical_names_case_insensitive() {
        assert!(matches!(parse_priority(Some("low")), Some(TicketPriority::Low)));
        assert!(matches!(parse_priority(Some("Low")), Some(TicketPriority::Low)));
        assert!(matches!(parse_priority(Some("LOW")), Some(TicketPriority::Low)));
        assert!(matches!(parse_priority(Some("medium")), Some(TicketPriority::Medium)));
        assert!(matches!(parse_priority(Some("high")), Some(TicketPriority::High)));
    }

    #[test]
    fn parse_priority_none_defaults_to_medium() {
        assert!(matches!(parse_priority(None), Some(TicketPriority::Medium)));
    }

    #[test]
    fn parse_priority_rejects_unknown() {
        // Caller is expected to 400 on `None` — silent coercion to Medium
        // would hide client bugs.
        assert!(parse_priority(Some("critical")).is_none());
        assert!(parse_priority(Some("")).is_none());
        assert!(parse_priority(Some("urgent!")).is_none());
    }

    #[test]
    fn valid_email_accepts_shape_check() {
        assert!(valid_email("alice@example.com"));
        assert!(valid_email("a@b.c"));
        assert!(valid_email("user+tag@sub.example.co.uk"));
    }

    #[test]
    fn valid_email_rejects_garbage() {
        assert!(!valid_email(""));
        assert!(!valid_email("plain"));
        assert!(!valid_email("@example.com"));
        assert!(!valid_email("user@"));
        assert!(!valid_email("user@host")); // no dot in domain
        assert!(!valid_email(&format!("user@{}.com", "x".repeat(300)))); // > 254
    }

    #[test]
    fn escape_like_escapes_all_three_metacharacters() {
        assert_eq!(escape_like("50%"), r"50\%");
        assert_eq!(escape_like("some_name"), r"some\_name");
        // Backslash must be escaped first; otherwise we'd double-escape % / _.
        assert_eq!(escape_like(r"C:\path"), r"C:\\path");
        assert_eq!(escape_like(r"a\%b_c"), r"a\\\%b\_c");
    }

    #[test]
    fn escape_like_passes_through_safe_input() {
        assert_eq!(escape_like("hello world"), "hello world");
        assert_eq!(escape_like(""), "");
    }
}

