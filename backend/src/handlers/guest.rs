//! Public (unauthenticated) guest-facing endpoints.
//!
//! All handlers in this module intentionally run outside the cookie auth
//! middleware. Each handler re-checks the relevant toggle in `site_settings`
//! before doing any work, so disabling a feature in the admin UI immediately
//! closes its public surface.

use actix_multipart::Multipart;
use actix_web::{web, HttpRequest, HttpResponse};
use chrono::Utc;
use diesel::prelude::*;
use futures::TryStreamExt;
use ipnetwork::IpNetwork;
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use crate::db::{DbConnection, Pool};
use crate::errors::{self, ApiError};
use crate::extractors::WorkspaceContext;
use crate::handlers::helpers;
use crate::models::{NewAttachment, NewTicket, PublicSiteSettings, SiteSettings, TicketPriority};
use crate::repository::user_helpers::GuestUserResult;
use crate::repository::{self, site_settings, user_helpers};
use crate::services::search::indexing_tasks;
use crate::services::search::SearchService;
use crate::services::upload_intake::{self as intake, IntakeError, Intaken, Refusal, UploadPolicy};
use crate::sync::actor::ActorContext;
use crate::sync::session;
use crate::utils::client_ip::RequestOrigin;
use crate::utils::file_validation::{
    FileValidationError, GUEST_ATTACHMENT_TTL_MINUTES, GUEST_MAX_FILES_PER_TICKET,
    GUEST_MAX_FILE_SIZE_MB,
};
use crate::utils::rate_limit::{self, RateLimiter};
use crate::utils::storage::{Storage, WorkspaceScopedStorage};

/// Public guest routes, mounted inside the rate-limited `/api/public` scope in
/// main.rs (paths are scope-relative; the scope keeps its JSON cap + limiter).
pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.route("/settings", web::get().to(get_public_settings))
        .route("/tickets", web::post().to(submit_guest_ticket))
        .route("/form-challenge", web::get().to(form_challenge))
        .route("/files/temp", web::post().to(upload_guest_attachment))
        .route("/docs", web::get().to(list_public_docs))
        .route("/docs/search", web::get().to(search_public_docs))
        .route("/docs/{slug}", web::get().to(get_public_doc))
        .route(
            "/unsubscribe",
            web::post().to(crate::handlers::unsubscribe::one_click),
        )
        .route(
            "/unsubscribe",
            web::get().to(crate::handlers::unsubscribe::landing),
        );
}

/// Whether the request form requires its proof-of-work challenge. On unless
/// an operator sets `NOSDESK_FORM_CHALLENGE=off` (for example to post to the
/// form from their own server-side integration).
fn challenge_required() -> bool {
    !matches!(
        std::env::var("NOSDESK_FORM_CHALLENGE")
            .as_deref()
            .map(str::trim),
        Ok("off") | Ok("false") | Ok("0")
    )
}

/// `GET /api/public/form-challenge`: a fresh challenge for the request form.
pub async fn form_challenge(ws: WorkspaceContext) -> HttpResponse {
    match crate::utils::form_challenge::issue(ws.workspace_id) {
        Some(c) => HttpResponse::Ok()
            .insert_header(("Cache-Control", "no-store"))
            .json(c),
        None => errors::internal("Challenge unavailable"),
    }
}

/// Verify a submission's solved challenge and record it as used. Fails open
/// only if the replay store is unreachable (the signature and timing checks
/// still ran).
async fn check_challenge(
    workspace_id: i32,
    solution: Option<&crate::utils::form_challenge::Solution>,
) -> Result<(), &'static str> {
    if !challenge_required() {
        return Ok(());
    }
    let solution = solution.ok_or("missing")?;
    let verified = crate::utils::form_challenge::verify(workspace_id, solution)?;
    let ttl = (verified.expires - chrono::Utc::now().timestamp()).max(1) as u64;
    match RateLimiter::claim_once(&rate_limit::get_redis_url(), &verified.replay_key, ttl).await {
        Ok(true) => Ok(()),
        Ok(false) => Err("reused"),
        Err(e) => {
            warn!(error = %e, "form challenge replay store unavailable; allowing");
            Ok(())
        }
    }
}

/// Build a workspace-pinned system actor for guest paths. The
/// `WorkspaceContextMiddleware` runs ahead of auth and attaches
/// `WorkspaceContext` to every request (apex / subdomain
/// resolution), so the guest paths can pin a real workspace
/// without needing a logged-in user. Guests are scoped to a
/// tenant — same as authenticated requests — they just don't
/// have a User actor attribution.
fn guest_actor(ws: &WorkspaceContext, reference: &'static str) -> ActorContext {
    ActorContext::system(reference).with_workspace(ws.workspace_id)
}

// ---------- Constants ----------

/// Upper bound on the submitter's name. 120 chars is generous for personal
/// names (the CLDR 99th percentile is ~60) while still tight enough that
/// a bot can't dump prose into the field.
const GUEST_MAX_NAME_LENGTH: usize = 120;

/// Upper bound on the free-text description of a ticket. Matches the
/// `maxlength` attribute on the frontend textarea.
const GUEST_MIN_TITLE_CHARS: usize = 3;
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
    /// One of the settings' `request_types`.
    #[serde(default)]
    pub category_id: Option<i32>,
    /// Claim tokens returned by `POST /api/public/files/temp`, one per
    /// pending upload.
    ///
    /// These used to be the raw `attachments` row ids, which are sequential
    /// and therefore guessable, and the claim checked only that a row was
    /// unclaimed and recent. Naming a neighbouring id reparented somebody
    /// else's pending upload into your own ticket. A guest has no session to
    /// bind an upload to, so the capability itself is signed instead; see
    /// `utils::guest_attachment_token`.
    #[serde(default)]
    pub attachment_tokens: Vec<String>,
    /// The solved proof-of-work challenge from `/form-challenge`.
    #[serde(default)]
    pub challenge: Option<crate::utils::form_challenge::Solution>,
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
        Some(v) => TicketPriority::parse(v),
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
    use hickory_resolver::net::{runtime::TokioRuntimeProvider, DnsError, NetError};
    use hickory_resolver::TokioResolver;

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

    // hickory 0.26 replaced `TokioAsyncResolver::tokio(config, opts)`
    // with a builder pattern that takes the runtime provider
    // explicitly and chains options before `.build()`. The build can
    // fail (e.g. invalid TLS config), but the default
    // `TokioRuntimeProvider` + `ResolverConfig` combination doesn't
    // hit any of those paths — fail-open if it ever does.
    let resolver = match TokioResolver::builder_with_config(
        ResolverConfig::default(),
        TokioRuntimeProvider::default(),
    )
    .with_options(opts)
    .build()
    {
        Ok(r) => r,
        Err(e) => {
            warn!(error = %e, "DNS resolver build failed; allowing submission");
            return true;
        }
    };

    // hickory 0.26 also reshaped error kinds: the NXDOMAIN
    // discriminant moved to `NetError::Dns(DnsError::NoRecordsFound(_))`,
    // a tuple-struct variant rather than the prior brace-style. Same
    // semantics, different match shape.

    // MX record presence is the strong signal. `Lookup::answers()`
    // is the new accessor in 0.26 (the prior `iter()` shorthand
    // moved to `LookupIp` only).
    match resolver.mx_lookup(domain).await {
        Ok(mx) if !mx.answers().is_empty() => return true,
        // Explicit NXDOMAIN means the domain itself doesn't exist.
        Err(e) => {
            if matches!(&e, NetError::Dns(DnsError::NoRecordsFound(_))) {
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
            if matches!(&e, NetError::Dns(DnsError::NoRecordsFound(_))) {
                false
            } else {
                warn!(error = %e, %domain, "A lookup errored; allowing submission");
                true
            }
        }
    }
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
    // Guest submissions are public; per-IP rate limiting only
    // works if we see the real client. Route through the central
    // helper so TRUSTED_PROXIES gates X-Forwarded-For consistently.
    crate::utils::client_ip::from_http_request(req).and_then(|ip| ip.to_string().parse().ok())
}

/// Best-effort audit log; failures are swallowed so they can't break a
/// legitimate submission.
fn log_guest_event(
    conn: &mut DbConnection,
    user_uuid: Uuid,
    event_type: &str,
    origin: &RequestOrigin,
    details: serde_json::Value,
) {
    use crate::utils::security_events::{record_security_event_from, SecurityEventInput};

    if let Err(e) = record_security_event_from(
        conn,
        SecurityEventInput {
            user_uuid: Some(user_uuid),
            event_type,
            severity: "info",
            details: Some(details),
            request: None,
        },
        origin,
    ) {
        warn!(error = %e, "Failed to record guest security event");
    }
}

// ---------- Public settings ----------

/// GET /api/public/settings — branding + which guest features are enabled.
pub async fn get_public_settings(
    pool: web::Data<Pool>,
    ws: WorkspaceContext,
) -> Result<HttpResponse, ApiError> {
    let mut conn = helpers::db_conn(&pool)?;

    let actor = guest_actor(&ws, "guest:public_settings");
    let loaded = session::with_actor_context(&mut conn, &actor, |conn| {
        let settings = site_settings::get_site_settings(conn)?;
        // The request types the submit form offers, when it's on.
        let types = if settings.guest_tickets_enabled {
            repository::categories::requester_request_types(conn)?
        } else {
            Vec::new()
        };
        let notice = crate::handlers::notices::public_notice(conn)?;
        Ok::<_, diesel::result::Error>((settings, types, notice))
    });
    match loaded {
        Ok((s, types, notice)) => {
            let mut body =
                serde_json::to_value(PublicSiteSettings::from(&s)).unwrap_or_else(|_| json!({}));
            body["request_types"] = json!(types
                .into_iter()
                .map(crate::handlers::portal::RequestType::from)
                .collect::<Vec<_>>());
            // A known issue the team has posted, shown on every public page.
            body["notice"] = json!(notice);
            Ok(HttpResponse::Ok().json(body))
        }
        Err(e) => {
            warn!(error = ?e, "Failed to load site_settings for public endpoint");
            Ok(HttpResponse::Ok().json(json!({
                "app_name": "Nosdesk",
                "logo_url": null,
                "logo_light_url": null,
                "favicon_url": null,
                "primary_color": null,
                "guest_tickets_enabled": false,
                "guest_public_docs_enabled": false,
                "guest_kb_search_enabled": false,
                "guest_help_page_enabled": false,
            })))
        }
    }
}

// ---------- Guest ticket submission ----------

/// POST /api/public/tickets
/// Anonymous requests one workspace accepts in a day.
const GUEST_TICKETS_PER_WORKSPACE_PER_DAY: u32 = 1000;

/// Unconfirmed requests one address may have waiting at once.
const MAX_PENDING_PER_ADDRESS: i64 = 3;

/// The answer to every submission that got past validation and the limits:
/// the same status and body whatever became of it (a new account, a reused
/// one, an address with an account that must sign in instead, one with too
/// many requests waiting), so it says nothing about the address. Nothing
/// ticket-derived is in it, not even with confirmation off, because a
/// submission for an address with an account files no ticket to describe.
/// It varies only with the workspace's confirmation setting.
fn submission_answer(req: &HttpRequest, verification_required: bool) -> HttpResponse {
    let locale = crate::utils::locale::request_locale(req);
    let key = if verification_required {
        "guest-submit-answer-verify"
    } else {
        "guest-submit-received-message"
    };
    let message = format!(
        "{} {}",
        crate::utils::i18n::tr(&locale, key),
        crate::utils::i18n::tr(&locale, "guest-submit-have-account-hint")
    );
    HttpResponse::Accepted().json(json!({
        "verification_required": verification_required,
        "message": message,
    }))
}

/// A request-form submission, validated, as [`create_guest_submission`] writes
/// it.
pub struct GuestSubmission<'a> {
    pub email: &'a str,
    pub name: &'a str,
    pub title: &'a str,
    pub description: &'a str,
    pub priority: TicketPriority,
    pub category_id: Option<i32>,
    pub verification_required: bool,
}

/// Why [`create_guest_submission`] wrote nothing.
pub enum CreateError {
    EmailClaimed,
    /// The address already has [`MAX_PENDING_PER_ADDRESS`] unconfirmed
    /// requests; more wait until one is confirmed.
    TooManyPending,
    /// A held request's confirmation couldn't be queued, so it was not
    /// filed either.
    ConfirmationNotQueued,
    /// A held request's confirmation was held (the address's daily cap, its
    /// stop link, the workspace's ceiling on new addresses) and no earlier
    /// link to the address is outstanding, so it was not filed either.
    ConfirmationHeld,
    Internal,
}

impl From<diesel::result::Error> for CreateError {
    fn from(_: diesel::result::Error) -> Self {
        Self::Internal
    }
}

/// What [`create_guest_submission`] wrote.
pub struct CreatedSubmission {
    pub user: crate::models::User,
    /// A guest account was created for the address (not reused).
    pub is_new_guest: bool,
    /// The request is held until the address is confirmed: always with
    /// confirmation on, and with it off for an account with a proven or
    /// vouched address that holds no seat here, which the confirmation seats.
    pub held: bool,
    pub ticket: crate::models::Ticket,
    pub first_comment_id: Option<i32>,
}

/// Provision-or-find the requester, resolve the default workflow state, create
/// the ticket and persist the initial comment. Run it in one transaction
/// pinned to `workspace_id`, so the workspace_id column default fires for
/// every INSERT and a partial failure rolls back cleanly.
pub fn create_guest_submission(
    conn: &mut DbConnection,
    workspace_id: i32,
    search: &Arc<SearchService>,
    submission: &GuestSubmission<'_>,
) -> Result<CreatedSubmission, CreateError> {
    let GuestSubmission {
        email,
        name,
        title,
        description,
        priority,
        category_id: requested_type,
        verification_required,
    } = *submission;
    // One submission per address at a time, so the pending cap below holds.
    repository::tickets::lock_guest_submissions_for(conn, email)?;
    let (user, is_new_guest, held) =
        match user_helpers::find_or_create_guest_user(email, name, conn, Some(search)) {
            Ok(GuestUserResult::Created(u)) => (u, true, verification_required),
            Ok(GuestUserResult::Existing(u)) => {
                // A reused account may hold seats only elsewhere, and without one
                // here its View request link and portal sign-in refuse it. A held
                // request seats it when the address is confirmed
                // (`invitation::complete_verification`); one that needs no
                // confirmation is live at once, so it is seated now, as a
                // mailed-in request's sender is.
                if !verification_required {
                    crate::repository::workspaces::join_as_requester(conn, workspace_id, u.uuid)?;
                }
                (u, false, verification_required)
            }
            // An account with a proven or vouched address, seated only in
            // other workspaces and on no team anywhere, is held and confirmed
            // as a reused guest is, whatever the confirmation setting: the
            // typed address proves nothing until its mailbox answers, and the
            // confirmation seats it here. Anyone else with an account signs in
            // instead.
            Ok(GuestUserResult::EmailClaimed) => {
                match user_helpers::account_to_confirm_and_seat(conn, workspace_id, email)? {
                    Some(u) => (u, false, true),
                    None => return Err(CreateError::EmailClaimed),
                }
            }
            Err(e) => {
                error!(error = %e, "Failed to provision guest user");
                return Err(CreateError::Internal);
            }
        };

    // Unconfirmed requests are capped per address, so confirming one
    // email can only ever release a handful.
    if held
        && repository::tickets::count_pending_for_requester(conn, user.uuid)?
            >= MAX_PENDING_PER_ADDRESS
    {
        return Err(CreateError::TooManyPending);
    }

    let default_state = repository::workflow_states::default_state(conn).map_err(|e| {
        error!(error = %e, "Failed to resolve default workflow state");
        CreateError::Internal
    })?;

    let category_id = repository::categories::offered_request_type(conn, requested_type)?;
    let new_ticket = NewTicket {
        title: title.to_string(),
        workflow_state_id: default_state.id,
        priority,
        requester_uuid: Some(user.uuid),
        category_id,
        submitted_via: Some("guest".to_string()),
        verification_state: if held {
            Some("pending".to_string())
        } else {
            None
        },
        ..Default::default()
    };

    // Tag the activity row so the renderer can phrase the entry as
    // "Created via public portal by <name> <<email>>" instead of the
    // generic "System created this ticket". Same shape as inbound
    // email tickets; the renderer switches on `source`.
    let creation_annotation = repository::tickets::TicketCreationAnnotation {
        source: Some("guest_portal".to_string()),
        from_email: Some(email.to_string()),
        from_name: if name.is_empty() {
            None
        } else {
            Some(name.to_string())
        },
        subject: Some(title.to_string()),
    };

    let ticket = repository::tickets::create_ticket_with_annotation(
        conn,
        new_ticket,
        creation_annotation,
        None,
    )
    .map_err(|e| {
        error!(error = %e, "Failed to create guest ticket");
        CreateError::Internal
    })?;

    // Persist the description as the initial comment from the
    // guest user so techs see the reported issue in the normal
    // ticket timeline. A comment-write failure is non-fatal —
    // the ticket itself is the primary artefact — so we log
    // and continue with comment_id = None.
    let new_comment = crate::models::NewComment {
        content: description.to_string(),
        user_uuid: user.uuid,
        ticket_id: ticket.id,
        content_format: crate::models::ContentFormat::Plaintext,
        ..Default::default()
    };
    let comment_annotation = repository::comments::CommentCreationAnnotation {
        source: Some("guest_portal".to_string()),
        from_email: Some(email.to_string()),
        from_name: if name.is_empty() {
            None
        } else {
            Some(name.to_string())
        },
        imported: false,
    };
    let first_comment_id = match repository::comments::create_comment_with_annotation(
        conn,
        new_comment,
        comment_annotation,
        Some(search),
    ) {
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

    Ok(CreatedSubmission {
        user,
        is_new_guest,
        held,
        ticket,
        first_comment_id,
    })
}

pub async fn submit_guest_ticket(
    pool: web::Data<Pool>,
    search_service: web::Data<Arc<SearchService>>,
    storage: web::Data<Arc<dyn Storage>>,
    req: HttpRequest,
    ws: WorkspaceContext,
    body: web::Json<SubmitGuestTicketRequest>,
) -> Result<HttpResponse, ApiError> {
    let mut conn = helpers::db_conn(&pool)?;

    // Workspace-pinned actor: every subsequent DB call goes through
    // `with_actor_context` so RLS-protected tenant tables
    // (site_settings, workflow_states, tickets, comments,
    // attachments) read the workspace GUC and the workspace_id
    // column default fires for every INSERT.
    let actor = guest_actor(&ws, "guest:ticket_create");

    let settings = match session::with_actor_context(&mut conn, &actor, |c| {
        Ok::<_, diesel::result::Error>(get_settings(c))
    }) {
        Ok(Some(s)) => s,
        Ok(None) | Err(_) => {
            return Err(ApiError::ServiceUnavailable("Settings unavailable".into()));
        }
    };
    // Filing takes its own connection; don't hold two.
    drop(conn);

    if !settings.guest_tickets_enabled {
        return Err(ApiError::Forbidden(
            "Guest ticket submission is disabled".into(),
        ));
    }

    // Honeypot: a non-empty `website` field means a bot filled a hidden
    // input. Return a plausible-looking 400 rather than surfacing that the
    // trap worked — keeps the bot from learning to avoid it.
    if body
        .website
        .as_deref()
        .map(str::trim)
        .is_some_and(|s| !s.is_empty())
    {
        debug!(ip = ?client_ip(&req), "Guest ticket submission tripped honeypot");
        return Err(ApiError::BadRequest("Invalid submission".into()));
    }

    if let Err(why) = check_challenge(ws.workspace_id, body.challenge.as_ref()).await {
        debug!("Guest ticket submission failed the form challenge ({why})");
        return Ok(errors::bad_request_with_code(
            "Please try sending that again.",
            "challenge_failed",
        ));
    }

    // Basic input validation
    let name = body.name.trim();
    let email = body.email.trim();
    let title = body.title.trim();
    let description = body.description.trim();

    if name.is_empty() || name.len() > GUEST_MAX_NAME_LENGTH {
        return Err(ApiError::BadRequest("Invalid name".into()));
    }
    if !valid_email(email) {
        return Err(ApiError::BadRequest("Invalid email".into()));
    }
    // MX pre-check: reject addresses whose domain has no mail servers. This
    // is a cheap filter that catches `random@madeupdomain.tld` garbage
    // before we commit to creating a user + sending a confirmation email
    // that will just hard-bounce. The check fails open on resolver errors
    // so flaky upstream DNS doesn't break legitimate submissions.
    if !email_domain_has_mx(email).await {
        return Err(ApiError::BadRequest(
            "We can't deliver mail to that address.".into(),
        ));
    }
    // A subject of a character or two doesn't tell anyone what the ticket is
    // about; the form asks for at least three.
    if title.chars().count() < GUEST_MIN_TITLE_CHARS || title.len() > 255 {
        return Err(ApiError::BadRequest("Invalid title".into()));
    }
    if description.is_empty() || description.len() > GUEST_MAX_DESCRIPTION_LENGTH {
        return Err(ApiError::BadRequest("Invalid description".into()));
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
                return Err(ApiError::BadRequest("Invalid priority".into()));
            }
        }
    };

    // Per-IP rate limit, sourced from site_settings. Uses the shared Redis
    // sliding-window helper (matches auth / MFA rate limiting). Fail-open on
    // Redis errors so a misconfigured cache doesn't take down submissions —
    // this mirrors the existing helper's behavior and the app's other
    // fail-open limiters. The scope's rate-limit middleware enforces the
    // coarse per-minute cap underneath, and fails open on Redis errors too.
    // Keyed per workspace (hosted tenants each set their own limit), and IPv6
    // by its /64.
    if let Some(ip) = client_ip(&req).map(|n| n.ip()) {
        let key = format!(
            "guest_tickets:{}:{}",
            ws.workspace_id,
            crate::utils::client_ip::limit_bucket(ip)
        );
        let max =
            u32::try_from(settings.guest_ticket_rate_limit_per_hour.max(1)).unwrap_or(u32::MAX);
        let redis_url = rate_limit::get_redis_url();
        match RateLimiter::check_rate_limit(&redis_url, &key, max, 3600).await {
            Ok(true) => { /* allowed */ }
            Ok(false) => {
                return Ok(errors::too_many_requests(
                    "Too many submissions from your network. Please try again later.",
                    3600,
                ));
            }
            Err(e) => {
                warn!(error = %e, "Guest-ticket rate limiter unavailable; allowing request");
            }
        }
    }

    // A ceiling for the whole workspace, whatever the spread of addresses:
    // far above any real day, low enough that a botnet can't bury the queue.
    {
        let key = format!("guest_tickets_day:{}", ws.workspace_id);
        match RateLimiter::check_rate_limit(
            &rate_limit::get_redis_url(),
            &key,
            GUEST_TICKETS_PER_WORKSPACE_PER_DAY,
            86_400,
        )
        .await
        {
            Ok(true) => {}
            Ok(false) => {
                return Ok(errors::too_many_requests(
                    "We're receiving a lot of requests right now. Please try again later.",
                    3600,
                ));
            }
            Err(e) => warn!(error = %e, "Guest-ticket daily limiter unavailable; allowing request"),
        }
    }

    let filing = GuestFiling {
        email: email.to_string(),
        name: name.to_string(),
        title: title.to_string(),
        description: description.to_string(),
        priority,
        category_id: body.category_id,
        verification_required: settings.guest_ticket_email_verification,
        attachment_tokens: if settings.guest_ticket_attachments_enabled {
            body.attachment_tokens.clone()
        } else {
            Vec::new()
        },
    };
    let verification_required = filing.verification_required;

    // Everything past here depends on the address (whether an account holds
    // it, how many of its requests wait), so it runs after the answer goes
    // out, and the answer, its status and its timing are the same for all.
    let pool = pool.get_ref().clone();
    let search = search_service.get_ref().clone();
    let storage = storage.get_ref().clone();
    let origin = RequestOrigin::of(&req);
    let workspace_id = ws.workspace_id;
    spawn_filing(async move {
        if let Err(e) = file_guest_submission(pool, search, storage, origin, ws, filing).await {
            error!(workspace_id, error = %e, "Guest submission could not be filed");
        }
    });
    Ok(submission_answer(&req, verification_required))
}

/// Filings answered but not yet finished; graceful shutdown waits for them
/// ([`drain_filings`]).
fn filings() -> &'static tokio_util::task::TaskTracker {
    static FILINGS: std::sync::OnceLock<tokio_util::task::TaskTracker> = std::sync::OnceLock::new();
    FILINGS.get_or_init(tokio_util::task::TaskTracker::new)
}

/// Filings that may run at once. The rest wait for a slot after their
/// submitter has been answered, so a burst of submissions can't take every
/// database connection.
const CONCURRENT_FILINGS: usize = 4;

fn filing_slots() -> Arc<tokio::sync::Semaphore> {
    static SLOTS: std::sync::OnceLock<Arc<tokio::sync::Semaphore>> = std::sync::OnceLock::new();
    SLOTS
        .get_or_init(|| Arc::new(tokio::sync::Semaphore::new(CONCURRENT_FILINGS)))
        .clone()
}

/// Run a filing on the system arbiter, which outlives the HTTP workers
/// through a graceful shutdown, in turn with the others.
fn spawn_filing(filing: impl std::future::Future<Output = ()> + Send + 'static) {
    let task = filings().track_future(async move {
        let Ok(_slot) = filing_slots().acquire_owned().await else {
            return;
        };
        filing.await;
    });
    if !actix_web::rt::System::current().arbiter().spawn(task) {
        error!("Guest submission could not be filed: the system arbiter has stopped");
    }
}

/// Wait up to `timeout` for the filings already answered to finish, taking no
/// more. True when they all did.
pub async fn drain_filings(timeout: std::time::Duration) -> bool {
    let tracker = filings();
    tracker.close();
    tokio::time::timeout(timeout, tracker.wait()).await.is_ok()
}

/// A validated request-form submission that passed the limits, owned so it
/// can be filed after the submitter is answered.
#[derive(Clone, Debug)]
pub struct GuestFiling {
    pub email: String,
    pub name: String,
    pub title: String,
    pub description: String,
    pub priority: TicketPriority,
    pub category_id: Option<i32>,
    pub verification_required: bool,
    /// Claim tokens for uploads to attach; empty when attachments are off.
    pub attachment_tokens: Vec<String>,
}

/// What became of a filed submission. Never told to the submitter.
#[derive(Debug, PartialEq, Eq)]
pub enum Filed {
    /// A ticket was filed (held until confirmation, when that is on).
    Ticket { ticket_id: i32 },
    /// The address belongs to an account that signs in instead; a sign-in
    /// link was offered.
    Claimed,
    /// The address has too many requests awaiting confirmation.
    TooManyPending,
    /// Its confirmation was held and no earlier link could carry it, so
    /// nothing was filed.
    Held,
}

/// File a submission answered already with [`submission_answer`]: find or
/// make its requester, file the ticket and its first comment, attach its
/// uploads, record it, and send the confirmation. The database work runs on
/// the blocking pool.
///
/// When email verification is enabled, tickets start in a `pending` state
/// invisible to techs (see ticket_query::apply_filters). The verification
/// link in the invitation email flips them to `verified` atomically in
/// accept_invitation::verify_pending_tickets_for_user.
pub async fn file_guest_submission(
    pool: Pool,
    search: Arc<SearchService>,
    storage: Arc<dyn Storage>,
    origin: RequestOrigin,
    ws: WorkspaceContext,
    filing: GuestFiling,
) -> Result<Filed, ApiError> {
    let actor = guest_actor(&ws, "guest:ticket_create");
    let workspace_id = ws.workspace_id;
    let verification_required = filing.verification_required;

    // One transaction: the requester, the ticket, its first comment, and the
    // confirmation (its token and its queued mail). A held request whose
    // confirmation can't be queued is rolled back with it, so nothing is left
    // waiting on a link nobody was sent. With confirmation off the request is
    // live either way; the courtesy confirmation is a savepoint whose failure
    // is logged and leaves the request filed.
    let create_result = {
        let (pool, search, actor, filing, origin) = (
            pool.clone(),
            search.clone(),
            actor.clone(),
            filing.clone(),
            origin.clone(),
        );
        web::block(move || {
            let mut conn = pool.get().map_err(|_| CreateError::Internal)?;
            let submission = GuestSubmission {
                email: &filing.email,
                name: &filing.name,
                title: &filing.title,
                description: &filing.description,
                priority: filing.priority,
                category_id: filing.category_id,
                verification_required,
            };
            session::with_actor_context::<_, CreateError>(&mut conn, &actor, |conn| {
                let created = create_guest_submission(conn, workspace_id, &search, &submission)?;
                if created.held || created.is_new_guest {
                    match queue_confirmation(
                        conn,
                        &origin,
                        &created,
                        &filing.email,
                        workspace_id,
                        created.held,
                    ) {
                        Ok(()) => {}
                        Err(Unqueued::Held) => {
                            warn!(
                                ticket_id = created.ticket.id,
                                "Guest confirmation held and no earlier link is outstanding; the request is not filed"
                            );
                            return Err(CreateError::ConfirmationHeld);
                        }
                        Err(Unqueued::Failed(why)) if created.held => {
                            warn!(
                                ticket_id = created.ticket.id,
                                result = %why,
                                "Guest confirmation could not be queued; the request is not filed"
                            );
                            return Err(CreateError::ConfirmationNotQueued);
                        }
                        Err(Unqueued::Failed(why)) => warn!(
                            ticket_id = created.ticket.id,
                            result = %why,
                            "Failed to send guest ticket confirmation email"
                        ),
                    }
                }
                Ok(created)
            })
        })
        .await
        .map_err(|_| ApiError::Internal("Filing task failed".into()))?
    };

    // An address with an account gets a sign-in link instead, and one with
    // too many unconfirmed requests gets nothing more until it confirms.
    let created = match create_result {
        Ok(c) => c,
        Err(CreateError::EmailClaimed) => {
            crate::handlers::portal::spawn_sign_in_link(pool, ws, filing.email);
            return Ok(Filed::Claimed);
        }
        Err(CreateError::TooManyPending) => return Ok(Filed::TooManyPending),
        Err(CreateError::ConfirmationHeld) => return Ok(Filed::Held),
        Err(CreateError::ConfirmationNotQueued) => {
            return Err(ApiError::Internal(
                "The confirmation could not be queued".into(),
            ));
        }
        Err(CreateError::Internal) => {
            return Err(ApiError::Internal("Failed to create ticket".into()));
        }
    };
    let ticket_id = created.ticket.id;
    let user_uuid = created.user.uuid;

    // Claim any referenced attachments. Cap at GUEST_MAX_FILES_PER_TICKET;
    // silently truncate extras rather than rejecting the submission.
    if let Some(comment_id) = created.first_comment_id {
        if !filing.attachment_tokens.is_empty() {
            // A token that does not verify is dropped rather than rejected, the
            // same treatment an expired or already-claimed id has always had:
            // one stale attachment should not cost the submitter their ticket.
            let ids: Vec<i32> = filing
                .attachment_tokens
                .iter()
                .take(GUEST_MAX_FILES_PER_TICKET)
                .filter_map(|token| {
                    crate::utils::guest_attachment_token::verify(workspace_id, token)
                })
                .collect();
            // Scope storage to this workspace so the temp->ticket move
            // stays under the ws/{id}/ prefix.
            let scoped_storage = WorkspaceScopedStorage::arc(storage.clone(), workspace_id);
            claim_guest_attachments(
                &pool,
                &scoped_storage,
                ids,
                ticket_id,
                comment_id,
                user_uuid,
                &actor,
            )
            .await;
        }
    }

    // Best-effort record, in its own transaction so it can't undo the filing.
    {
        let (pool, actor) = (pool.clone(), actor.clone());
        let is_new_guest = created.is_new_guest;
        let email_domain = filing.email.rsplit('@').next().unwrap_or("").to_string();
        let _ = web::block(move || {
            let Ok(mut conn) = pool.get() else {
                warn!(ticket_id, "Guest submission: no connection to record it");
                return;
            };
            let _ =
                session::with_actor_context::<_, diesel::result::Error>(&mut conn, &actor, |c| {
                    log_guest_event(
                        c,
                        user_uuid,
                        "guest_ticket_submitted",
                        &origin,
                        json!({
                            "ticket_id": ticket_id,
                            "email_domain": email_domain,
                            "new_account": is_new_guest,
                        }),
                    );
                    Ok(())
                });
        })
        .await;
    }

    // Only surface the ticket to techs once it's actually verified. Pending
    // tickets skip search indexing entirely; accept_invitation picks them
    // up at verification time. The verified ticket reaches clients through
    // the sync pool (the repository write emits `ticket.created`).
    if !created.held {
        indexing_tasks::spawn_index_ticket(search, created.ticket, None);
    }

    info!(ticket_id, verification_required, "Guest ticket submitted");

    Ok(Filed::Ticket { ticket_id })
}

/// Queue a submission's confirmation in a savepoint of the filing's
/// transaction: its token and its mail, or neither. `Err` says why nothing
/// was queued.
///
/// Email dispatch:
///   - verification on: send confirmation every time (the link is also the
///     "release my pending ticket" trigger, so even Existing guests with
///     multiple pending tickets need a fresh, working link).
///   - verification off: only send on new accounts (no ticket-gating concern,
///     so re-sending to returning submitters would just be noise).
///
/// The email itself is framed as "confirm your ticket submission" rather than
/// the generic "welcome / invitation" copy; see
/// EmailService::send_guest_ticket_confirmation_email. When the confirmation
/// is held (the address's daily cap, its stop link, the workspace's ceiling on
/// new addresses) nothing more is sent. A held request is then named on the
/// newest link still outstanding to the address from this workspace, so that
/// earlier link releases it too; with none, [`Unqueued::Held`]. With
/// confirmation off the request is live, so a hold only skips the courtesy
/// mail.
fn queue_confirmation(
    conn: &mut DbConnection,
    origin: &RequestOrigin,
    created: &CreatedSubmission,
    email: &str,
    workspace_id: i32,
    verification_required: bool,
) -> Result<(), Unqueued> {
    use crate::handlers::users::SendInvitationResult;
    enum NotQueued {
        Db(diesel::result::Error),
        Refused(String),
        Held,
    }
    impl From<diesel::result::Error> for NotQueued {
        fn from(e: diesel::result::Error) -> Self {
            Self::Db(e)
        }
    }
    let outcome = conn.transaction::<_, NotQueued, _>(|c| {
        match crate::handlers::users::send_guest_ticket_confirmation(
            c,
            origin,
            created.user.uuid,
            email,
            workspace_id,
            created.ticket.id,
        ) {
            SendInvitationResult::Capped if verification_required => {
                if repository::reset_tokens::name_ticket_on_outstanding_confirmation(
                    c,
                    created.user.uuid,
                    workspace_id,
                    email,
                    created.ticket.id,
                )? {
                    Ok(())
                } else {
                    Err(NotQueued::Held)
                }
            }
            SendInvitationResult::Success | SendInvitationResult::Capped => Ok(()),
            other => Err(NotQueued::Refused(other.to_string())),
        }
    });
    match outcome {
        Ok(()) => Ok(()),
        Err(NotQueued::Db(e)) => Err(Unqueued::Failed(e.to_string())),
        Err(NotQueued::Refused(why)) => Err(Unqueued::Failed(why)),
        Err(NotQueued::Held) => Err(Unqueued::Held),
    }
}

/// Why [`queue_confirmation`] queued nothing.
enum Unqueued {
    /// The confirmation was held and no earlier link could carry the request.
    Held,
    /// It failed, for this reason.
    Failed(String),
}

// ---------- Public documentation ----------

/// GET /api/public/docs
pub async fn list_public_docs(
    pool: web::Data<Pool>,
    ws: WorkspaceContext,
) -> Result<HttpResponse, ApiError> {
    let mut conn = helpers::db_conn(&pool)?;

    let actor = guest_actor(&ws, "guest:public_docs");
    let result = session::with_actor_context(&mut conn, &actor, |conn| {
        let settings = match get_settings(conn) {
            Some(s) => s,
            None => return Ok(Err("unavailable")),
        };
        if !settings.guest_public_docs_enabled {
            return Ok(Err("disabled"));
        }

        use crate::schema::documentation_pages::dsl::*;
        let rows = documentation_pages
            .filter(is_public.eq(true))
            .filter(deleted_at.is_null())
            .select((id, uuid, title, slug, icon, updated_at))
            .load::<(
                i32,
                Uuid,
                String,
                String,
                Option<String>,
                chrono::NaiveDateTime,
            )>(conn)?;
        let rows = open_to_guests(conn, rows, |r| r.0)?;
        Ok::<_, diesel::result::Error>(Ok(rows))
    });

    match result {
        Ok(Ok(list)) => {
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
            Ok(HttpResponse::Ok().json(items))
        }
        Ok(Err("unavailable")) => Err(ApiError::ServiceUnavailable("Service unavailable".into())),
        Ok(Err(_)) => Err(ApiError::Forbidden(
            "Public documentation is disabled".into(),
        )),
        Err(e) => {
            error!(error = %e, "Failed to list public docs");
            Err(ApiError::Internal("Internal server error".into()))
        }
    }
}

/// The rows (pages, named by `page_id`) the guest portal may show: published
/// to guests, out of the trash, and covered by no restriction. A page marked
/// public inside a restricted collection stays restricted.
fn open_to_guests<T>(
    conn: &mut crate::db::DbConnection,
    rows: Vec<T>,
    page_id: impl Fn(&T) -> i32,
) -> diesel::QueryResult<Vec<T>> {
    let ids: Vec<i32> = rows.iter().map(&page_id).collect();
    let hidden = crate::repository::PageAudience::Guest.hidden_pages(conn, &ids)?;
    Ok(rows
        .into_iter()
        .filter(|r| !hidden.contains(&page_id(r)))
        .collect())
}

/// GET /api/public/docs/{slug}
pub async fn get_public_doc(
    pool: web::Data<Pool>,
    ws: WorkspaceContext,
    path: web::Path<String>,
) -> Result<HttpResponse, ApiError> {
    let slug_param = path.into_inner();
    let mut conn = helpers::db_conn(&pool)?;

    let actor = guest_actor(&ws, "guest:public_doc");
    let result = session::with_actor_context(&mut conn, &actor, |conn| {
        let settings = match get_settings(conn) {
            Some(s) => s,
            None => return Ok(Err("unavailable")),
        };
        if !settings.guest_public_docs_enabled {
            return Ok(Err("disabled"));
        }

        use crate::schema::documentation_pages::dsl::*;
        let page = documentation_pages
            .filter(is_public.eq(true))
            .filter(deleted_at.is_null())
            .filter(slug.eq(&slug_param))
            .select((id, uuid, title, slug, icon, yjs_document, updated_at))
            .first::<(
                i32,
                Uuid,
                String,
                String,
                Option<String>,
                Option<Vec<u8>>,
                chrono::NaiveDateTime,
            )>(conn)
            .optional()?;
        let page = open_to_guests(conn, page.into_iter().collect(), |r| r.0)?.pop();
        Ok::<_, diesel::result::Error>(Ok(page))
    });

    match result {
        Ok(Ok(Some((pid, puuid, ptitle, pslug, picon, pdoc, pupdated)))) => Ok(HttpResponse::Ok()
            .json(json!({
                "id": pid,
                "uuid": puuid,
                "title": ptitle,
                "slug": pslug,
                "icon": picon,
                "yjs_document": pdoc,
                "updated_at": pupdated,
            }))),
        Ok(Ok(None)) => Err(ApiError::NotFoundMsg("Not found".into())),
        Ok(Err("unavailable")) => Err(ApiError::ServiceUnavailable("Service unavailable".into())),
        Ok(Err(_)) => Err(ApiError::Forbidden(
            "Public documentation is disabled".into(),
        )),
        Err(e) => {
            error!(error = %e, "Failed to load public doc");
            Err(ApiError::Internal("Internal server error".into()))
        }
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
    ws: WorkspaceContext,
    search_service: web::Data<Arc<SearchService>>,
    query: web::Query<PublicDocsSearchQuery>,
) -> Result<HttpResponse, ApiError> {
    let q = query.q.trim().to_string();
    if q.is_empty() || q.len() > GUEST_DOC_SEARCH_MAX_QUERY_LENGTH {
        return Err(ApiError::BadRequest("Invalid query".into()));
    }

    // Full-text over title and body, so "printer error 49" finds "Printer
    // troubleshooting". Candidates are re-checked against the database below,
    // which alone decides what's public. Title matching is the fallback when
    // the index can't answer.
    let ranked: Option<Vec<i32>> = search_service
        .search(
            &crate::services::search::SearchQuery {
                q: q.clone(),
                limit: 50,
                types: Some("documentation".into()),
                sort: None,
                author: None,
            },
            false,
            ws.workspace_id as i64,
        )
        .map(|r| {
            r.results
                .iter()
                .filter(|hit| hit.entity_type == "documentation")
                .filter_map(|hit| i32::try_from(hit.entity_id).ok())
                .collect()
        })
        .map_err(|e| warn!(error = %e, "public doc search: index unavailable, matching titles"))
        .ok();

    let mut conn = helpers::db_conn(&pool)?;

    let actor = guest_actor(&ws, "guest:public_docs_search");
    let result = session::with_actor_context(&mut conn, &actor, |conn| {
        let settings = match get_settings(conn) {
            Some(s) => s,
            None => return Ok(Err("unavailable")),
        };
        if !settings.guest_kb_search_enabled || !settings.guest_public_docs_enabled {
            return Ok(Err("disabled"));
        }

        use crate::schema::documentation_pages::dsl::*;
        let public = documentation_pages
            .filter(is_public.eq(true))
            .filter(deleted_at.is_null())
            .select((id, uuid, title, slug, icon, updated_at))
            .into_boxed();
        type Row = (
            i32,
            Uuid,
            String,
            String,
            Option<String>,
            chrono::NaiveDateTime,
        );
        let rows: Vec<Row> = match &ranked {
            Some(ids) => {
                let found: Vec<Row> = public.filter(id.eq_any(ids)).load(conn)?;
                // Keep the index's ranking.
                let mut ordered: Vec<Row> = ids
                    .iter()
                    .filter_map(|pid| found.iter().find(|r| r.0 == *pid).cloned())
                    .collect();
                ordered.truncate(GUEST_DOC_SEARCH_RESULT_LIMIT as usize);
                ordered
            }
            None => public
                .filter(title.ilike(format!("%{}%", crate::repository::escape_like(&q))))
                .limit(GUEST_DOC_SEARCH_RESULT_LIMIT)
                .load(conn)?,
        };
        let rows = open_to_guests(conn, rows, |r| r.0)?;
        Ok::<_, diesel::result::Error>(Ok(rows))
    });

    match result {
        Ok(Ok(list)) => {
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
            Ok(HttpResponse::Ok().json(items))
        }
        Ok(Err("unavailable")) => Err(ApiError::ServiceUnavailable("Service unavailable".into())),
        Ok(Err(_)) => Err(ApiError::Forbidden(
            "Public documentation search is disabled".into(),
        )),
        Err(e) => {
            error!(error = %e, "Public doc search failed");
            Err(ApiError::Internal("Internal server error".into()))
        }
    }
}

// ---------- Guest attachment upload ----------

pub(crate) enum UploadRejected {
    Invalid(String),
    TooLarge(String),
    /// The intake couldn't take it (the spool budget is spent, or the spool
    /// file couldn't be written): its own response.
    Intake(IntakeError),
}

impl UploadRejected {
    /// The response both callers give: 400 with the reason, 413 over the
    /// cap, the intake's own otherwise (503 with Retry-After when busy).
    pub(crate) fn response(self) -> Result<HttpResponse, ApiError> {
        match self {
            Self::Invalid(msg) => Err(ApiError::BadRequest(msg)),
            Self::TooLarge(msg) => Ok(errors::payload_too_large(msg)),
            Self::Intake(e) => Ok(e.into_response()),
        }
    }
}

/// Read the one `file` field of a multipart upload through the intake with
/// the guest rules: spooled to disk under the guest cap, its type checked
/// from its first bytes against the guest allowlist, hashed as it arrives.
/// Shared by the guest form and the customer portal, which accept the same
/// kinds of file from people outside the team.
pub(crate) async fn read_validated_upload(
    req: &HttpRequest,
    payload: &mut Multipart,
) -> Result<Intaken, UploadRejected> {
    let invalid = |m: &str| UploadRejected::Invalid(m.to_string());
    let too_large =
        || UploadRejected::TooLarge(format!("File exceeds {GUEST_MAX_FILE_SIZE_MB}MB limit"));
    let policy = UploadPolicy::guest();
    // A body too big to hold one file at the cap is refused unread.
    if intake::check_length(intake::content_length(req), &policy).is_err() {
        return Err(too_large());
    }
    let mut field = match payload.try_next().await {
        Ok(Some(f)) => f,
        Ok(None) => return Err(invalid("No file in request")),
        Err(e) => {
            debug!(error = %e, "Multipart parse error");
            return Err(invalid("Could not read upload"));
        }
    };
    if field.name() != policy.file_field {
        return Err(invalid("Expected field 'file'"));
    }
    let original_filename = match field
        .content_disposition()
        .get_filename()
        .map(|s| s.to_string())
    {
        Some(n) if !n.is_empty() => n,
        _ => return Err(invalid("Filename is required")),
    };

    match intake::area(req)
        .stream(&mut field, &original_filename, &policy)
        .await
    {
        Ok(file) => Ok(file),
        Err(IntakeError::Refused { reason, .. }) => match reason {
            Refusal::Validation(FileValidationError::FileTooLarge { .. }) => Err(too_large()),
            Refusal::Validation(e) => {
                debug!(error = ?e, "Upload rejected");
                Err(UploadRejected::Invalid(e.to_string()))
            }
            Refusal::NotAnImage | Refusal::UnsupportedImage { .. } => {
                Err(invalid("File type is not allowed"))
            }
        },
        Err(IntakeError::Unreadable(e)) => {
            debug!(error = %e, "Read chunk error");
            Err(invalid("Upload interrupted"))
        }
        Err(e) => Err(UploadRejected::Intake(e)),
    }
}

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
    storage: crate::extractors::ScopedStorage,
    ws: WorkspaceContext,
    req: HttpRequest,
    mut payload: Multipart,
) -> Result<HttpResponse, ApiError> {
    let mut conn = helpers::db_conn(&pool)?;

    // Scope the settings read to the workspace (RLS), matching every other
    // guest handler; a bare conn reads zero rows under hosted mode.
    let settings_actor = guest_actor(&ws, "guest:attachment_upload");
    let settings = match session::with_actor_context(&mut conn, &settings_actor, |c| {
        Ok::<_, diesel::result::Error>(get_settings(c))
    }) {
        Ok(Some(s)) => s,
        Ok(None) | Err(_) => {
            return Err(ApiError::ServiceUnavailable("Settings unavailable".into()));
        }
    };

    if !settings.guest_tickets_enabled || !settings.guest_ticket_attachments_enabled {
        return Err(ApiError::Forbidden("Attachments are not accepted".into()));
    }

    // Dedicated per-IP rate limiter for uploads. Keeping it distinct from
    // the submit rate limiter matters: an attacker can upload-and-abandon
    // to fill disk without ever completing a submission.
    if let Some(ip) = client_ip(&req).map(|n| n.ip()) {
        let key = format!(
            "guest_upload:{}:{}",
            ws.workspace_id,
            crate::utils::client_ip::limit_bucket(ip)
        );
        let redis_url = rate_limit::get_redis_url();
        match RateLimiter::check_rate_limit(&redis_url, &key, GUEST_UPLOADS_PER_HOUR, 3600).await {
            Ok(true) => { /* allowed */ }
            Ok(false) => {
                return Ok(errors::too_many_requests(
                    "Too many uploads from your network. Please try again later.",
                    3600,
                ));
            }
            Err(e) => {
                warn!(error = %e, "Guest upload rate limiter unavailable; allowing request");
            }
        }
    }

    // One file per request; the client loops if they pick several. This keeps
    // the per-request surface small and maps 1:1 with the rate limiter.
    let file = match read_validated_upload(&req, &mut payload).await {
        Ok(file) => file,
        Err(rejected) => return rejected.response(),
    };

    let total_size = file.size;
    let detected_mime = file.mime.clone();
    let stored_file = match file.store(storage.0.as_ref(), "temp").await {
        Ok(sf) => sf,
        Err(e) => {
            error!(error = ?e, "Failed to store guest upload");
            return Err(ApiError::Internal("Failed to store file".into()));
        }
    };

    let new_attachment = NewAttachment {
        url: stored_file.url.clone(),
        name: file.display_name.clone(),
        file_size: Some(total_size as i64),
        mime_type: Some(detected_mime.clone()),
        checksum: Some(file.sha256.clone()),
        // Claimed by submit_guest_ticket when the submission arrives.
        comment_id: None,
        uploaded_by: None,
        transcription: None,
    };

    // Pin the workspace so `attachments.workspace_id` (NOT NULL, defaulted
    // from `app.workspace_id`, FORCE RLS) is populated. Mirrors the guest
    // ticket-create path; the temp upload is an orphan row claimed at submit.
    let actor = guest_actor(&ws, "guest:attachment_upload");
    match session::with_actor_context::<_, diesel::result::Error>(&mut conn, &actor, |conn| {
        repository::create_attachment(conn, new_attachment)
    }) {
        Ok(att) => {
            info!(
                attachment_id = att.id,
                size = total_size,
                mime = %detected_mime,
                "Guest upload stored"
            );
            // The token IS the capability now; the id is returned only so the
            // client can key its own list and offer a remove button.
            let claim_token =
                match crate::utils::guest_attachment_token::sign(ws.workspace_id, att.id) {
                    Some(t) => t,
                    None => {
                        error!("JWT_SECRET unset; cannot issue a guest attachment claim token");
                        return Err(ApiError::Internal("Failed to save attachment".into()));
                    }
                };
            Ok(HttpResponse::Created().json(json!({
                "id": att.id,
                "claim_token": claim_token,
                "name": att.name,
                "size": total_size,
                "mime_type": detected_mime,
            })))
        }
        Err(e) => {
            // Best-effort cleanup of the orphaned file. If this fails,
            // the daily temp-cleanup job will pick it up.
            let _ = storage.0.delete_file(&stored_file.path).await;
            error!(error = ?e, "Failed to record guest upload");
            Err(ApiError::Internal("Failed to save attachment".into()))
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
    pool: &Pool,
    storage: &Arc<dyn Storage>,
    attachment_ids: Vec<i32>,
    ticket_id: i32,
    comment_id: i32,
    requester_uuid: Uuid,
    actor: &ActorContext,
) {
    use crate::schema::attachments;

    let cutoff = Utc::now().naive_utc() - chrono::Duration::minutes(GUEST_ATTACHMENT_TTL_MINUTES);

    // Load + filter in one query: only rows that are still eligible to be
    // claimed. The IN list is already capped by the caller at
    // GUEST_MAX_FILES_PER_TICKET, so this scales fine. Every id here came out
    // of a verified claim token, so these filters answer "is it still
    // claimable", not "is it yours"; that question was settled by the
    // signature. The database work runs on the blocking pool.
    let candidates = {
        let (pool, actor) = (pool.clone(), actor.clone());
        web::block(move || {
            let mut conn = pool.get().map_err(|e| e.to_string())?;
            session::with_actor_context(&mut conn, &actor, |c| {
                attachments::table
                    .filter(attachments::id.eq_any(&attachment_ids))
                    .filter(attachments::comment_id.is_null())
                    .filter(attachments::uploaded_by.is_null())
                    .filter(attachments::created_at.ge(cutoff))
                    .load::<crate::models::Attachment>(c)
            })
            .map_err(|e| e.to_string())
        })
        .await
    };
    let candidates = match candidates {
        Ok(Ok(rows)) => rows,
        Ok(Err(e)) => {
            warn!(error = %e, ticket_id, "Failed to load guest attachments for claim");
            return;
        }
        Err(e) => {
            warn!(error = %e, ticket_id, "Failed to load guest attachments for claim");
            return;
        }
    };

    for att in candidates {
        // URL looks like "/uploads/temp/<uuid>_<name>"; the storage path
        // the trait wants is relative to uploads/.
        let temp_storage_path = att.url.trim_start_matches("/uploads/").to_string();
        let file_only = temp_storage_path.trim_start_matches("temp/").to_string();
        let new_storage_path = format!("tickets/{ticket_id}/{file_only}");

        match storage
            .move_file(&temp_storage_path, &new_storage_path)
            .await
        {
            Ok(_) => {
                let new_url = format!("/uploads/{new_storage_path}");
                let attachment_id = att.id;
                let (pool, actor) = (pool.clone(), actor.clone());
                let update_result = web::block(move || {
                    let mut conn = pool.get().map_err(|e| e.to_string())?;
                    session::with_actor_context(&mut conn, &actor, |c| {
                        crate::repository::comments::reparent_attachment(
                            c,
                            attachment_id,
                            &new_url,
                            comment_id,
                            requester_uuid,
                        )
                    })
                    .map(|_| ())
                    .map_err(|e| e.to_string())
                })
                .await;
                match update_result {
                    Ok(Ok(())) => {}
                    Ok(Err(e)) => {
                        warn!(error = %e, attachment_id, "Failed to update claimed attachment")
                    }
                    Err(e) => {
                        warn!(error = %e, attachment_id, "Failed to update claimed attachment")
                    }
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
        assert!(matches!(
            parse_priority(Some("low")),
            Some(TicketPriority::Low)
        ));
        assert!(matches!(
            parse_priority(Some("Low")),
            Some(TicketPriority::Low)
        ));
        assert!(matches!(
            parse_priority(Some("LOW")),
            Some(TicketPriority::Low)
        ));
        assert!(matches!(
            parse_priority(Some("medium")),
            Some(TicketPriority::Medium)
        ));
        assert!(matches!(
            parse_priority(Some("high")),
            Some(TicketPriority::High)
        ));
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
}
