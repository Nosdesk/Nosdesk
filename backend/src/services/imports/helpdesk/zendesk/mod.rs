//! The Zendesk connector: reads a Zendesk Support account through its API
//! (or a full JSON export, see [`export`]) into a [`Bundle`].
//!
//! Order of work, each step resumable from `state.json`:
//!
//! 1. Organisations (for names), then every user, from the incremental
//!    user export, into `people.jsonl`. Redone whole if interrupted; it is
//!    cheap next to the tickets. The export's cursor is kept.
//! 2. Custom ticket field titles, every run.
//! 3. Tickets from the incremental ticket export (which includes archived
//!    tickets) with their metrics sideloaded (`include=metric_sets`, for
//!    when each was solved; the sideload's shape is taken from Zendesk's
//!    API reference and not yet seen on a real account), a page at a time.
//!    For each ticket its comments, and for each comment its attachments,
//!    downloaded to `files/`. A page is checkpointed only once all of it is
//!    on disk, together with the cursor for the next one, so an interrupted
//!    fetch redoes at most one page.
//! 4. Users again, from the kept cursor: anyone created while the tickets
//!    were fetched, so a ticket raised during a long fetch keeps its
//!    requester.
//! 5. On a resumed fetch, attachments that went missing for a reason that
//!    may pass (a dropped connection, a short read) are tried again.
//! 6. The manifest, last.
//!
//! Credentials: an OAuth bearer token, or (until Zendesk switches them off
//! on 2027-04-30) an API token. Neither is refreshed here; a 401 mid-run
//! stops the fetch with [`SourceError::Unauthorized`] and the saved state
//! lets a run with a fresh credential carry on.

mod api;
mod client;
pub mod export;
mod map;
#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;
use std::sync::OnceLock;

use chrono::Utc;
use futures::stream::{self, StreamExt, TryStreamExt};
use regex::Regex;
use serde::Deserialize;
use tokio_util::sync::CancellationToken;
use url::Url;

use super::bundle::{self, Attachment, Bundle, FetchState, Manifest};
use super::source::{FetchProgress, HelpdeskSource, SourceError};
use crate::utils::egress;
use client::Http;
use map::FieldCatalog;

pub use export::read_export;

/// The biggest attachment carried. Anything larger is listed with a reason
/// instead, and the loader reports it.
pub const MAX_ATTACHMENT_BYTES: u64 = 100 * 1024 * 1024;

/// Tickets per incremental page. Zendesk allows 1000, but a page is the
/// unit of resume and each ticket costs at least one comments request, so
/// a smaller page loses less work to an interruption. The ten-a-minute
/// limit on the incremental endpoint is never the bottleneck here.
const TICKET_PAGE_SIZE: u32 = 100;
const USER_PAGE_SIZE: u32 = 1000;
const LIST_PAGE_SIZE: u32 = 100;
/// Tickets fetched at once within a page. Small, to stay well inside the
/// account-wide rate limit of the smaller Zendesk plans.
const TICKET_CONCURRENCY: usize = 4;

/// How the connector signs in to Zendesk.
#[derive(Clone)]
pub enum ZendeskAuth {
    /// An OAuth access token.
    Bearer(String),
    /// A legacy API token: sent as `{email}/token` and the token. Zendesk
    /// stopped issuing these on 2026-10-27 and revokes them on 2027-04-30.
    Basic { email: String, token: String },
}

impl fmt::Debug for ZendeskAuth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ZendeskAuth::Bearer(_) => f.write_str("Bearer(<redacted>)"),
            ZendeskAuth::Basic { email, .. } => {
                write!(f, "Basic {{ email: {email:?}, token: <redacted> }}")
            }
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ZendeskConfigError {
    #[error("'{0}' is not a Zendesk subdomain")]
    InvalidSubdomain(String),
    #[error("base URL refused: {0}")]
    BaseUrlRefused(String),
    #[error("could not build the HTTP client: {0}")]
    Client(String),
}

pub struct ZendeskSource {
    scope: String,
    http: Http,
    max_attachment_bytes: u64,
    ticket_page_size: u32,
}

impl ZendeskSource {
    /// The account at `https://{subdomain}.zendesk.com`. Accepts the bare
    /// subdomain in any case, or `acme.zendesk.com`; refuses anything that
    /// could steer the request elsewhere.
    pub fn new(subdomain: &str, auth: ZendeskAuth) -> Result<Self, ZendeskConfigError> {
        Self::at_subdomain(subdomain, Some(auth))
    }

    /// The account at `https://{subdomain}.zendesk.com` with no credential,
    /// for [`read_export`] when the customer sends their export instead of
    /// granting API access. Attachments are fetched unsigned (export links
    /// usually carry their own token); custom field titles, which only the
    /// API has, are left as ids. [`HelpdeskSource::fetch`] on such a source
    /// is refused as [`SourceError::Unauthorized`].
    pub fn for_export(subdomain: &str) -> Result<Self, ZendeskConfigError> {
        Self::at_subdomain(subdomain, None)
    }

    fn at_subdomain(
        subdomain: &str,
        auth: Option<ZendeskAuth>,
    ) -> Result<Self, ZendeskConfigError> {
        let subdomain = normalise_subdomain(subdomain)?;
        let base = Url::parse(&format!("https://{subdomain}.zendesk.com"))
            .map_err(|e| ZendeskConfigError::InvalidSubdomain(e.to_string()))?;
        let http = Http::new(base, auth, true, None)
            .map_err(|e| ZendeskConfigError::Client(e.to_string()))?;
        Ok(Self::build(subdomain, http))
    }

    /// A Zendesk stand-in at `base_url`: a local mock in tests, or an
    /// operator's proxy. Refused unless its host is in
    /// `NOSDESK_OUTBOUND_ALLOWED_HOSTS`, the same allowlist that lets any
    /// outbound request reach a private address. The allowlist is read here,
    /// once: the host stays trusted for this source's life. The scope is the
    /// host (and port), so its ids never mix with a real account's.
    pub fn with_base_url(base_url: &str, auth: ZendeskAuth) -> Result<Self, ZendeskConfigError> {
        Self::at_base_url(base_url, Some(auth))
    }

    /// [`Self::for_export`] against a stand-in, under the same allowlist
    /// rule as [`Self::with_base_url`].
    pub fn for_export_with_base_url(base_url: &str) -> Result<Self, ZendeskConfigError> {
        Self::at_base_url(base_url, None)
    }

    fn at_base_url(base_url: &str, auth: Option<ZendeskAuth>) -> Result<Self, ZendeskConfigError> {
        let url =
            Url::parse(base_url).map_err(|e| ZendeskConfigError::BaseUrlRefused(e.to_string()))?;
        let host = url
            .host_str()
            .ok_or_else(|| ZendeskConfigError::BaseUrlRefused("no host".into()))?
            .to_string();
        if !matches!(url.scheme(), "http" | "https") {
            return Err(ZendeskConfigError::BaseUrlRefused("not http(s)".into()));
        }
        if !egress::is_host_allowlisted(&host) {
            return Err(ZendeskConfigError::BaseUrlRefused(format!(
                "{host} is not in NOSDESK_OUTBOUND_ALLOWED_HOSTS"
            )));
        }
        let mut base = url.clone();
        base.set_path("/");
        base.set_query(None);
        base.set_fragment(None);
        let https_only = url.scheme() == "https";
        let http = Http::new(base, auth, https_only, Some(host.clone()))
            .map_err(|e| ZendeskConfigError::Client(e.to_string()))?;
        let scope = match url.port() {
            Some(p) => format!("{host}:{p}"),
            None => host,
        };
        Ok(Self::build(scope, http))
    }

    fn build(scope: String, http: Http) -> Self {
        ZendeskSource {
            scope,
            http,
            max_attachment_bytes: MAX_ATTACHMENT_BYTES,
            ticket_page_size: TICKET_PAGE_SIZE,
        }
    }

    /// Every item of a cursor-paginated list (`page[size]`, `meta`).
    async fn list_all<P, T>(
        &self,
        path: &str,
        split: fn(P) -> (Vec<T>, api::Meta),
        cancel: &CancellationToken,
    ) -> Result<Vec<T>, SourceError>
    where
        P: serde::de::DeserializeOwned,
    {
        let size = LIST_PAGE_SIZE.to_string();
        let mut out = Vec::new();
        let mut after: Option<String> = None;
        loop {
            let mut query = vec![("page[size]", size.as_str())];
            if let Some(a) = after.as_deref() {
                query.push(("page[after]", a));
            }
            let url = self.http.url(path, &query);
            let page: P = self
                .http
                .get_json(&url, cancel)
                .await?
                .ok_or_else(|| SourceError::Protocol(format!("{path} answered 404")))?;
            let (items, meta) = split(page);
            out.extend(items);
            match meta.after_cursor {
                Some(next) if meta.has_more && after.as_deref() != Some(next.as_str()) => {
                    after = Some(next)
                }
                _ => return Ok(out),
            }
        }
    }

    async fn fetch_people(
        &self,
        bundle: &Bundle,
        run: &mut Run<'_>,
        cancel: &CancellationToken,
    ) -> Result<Option<String>, SourceError> {
        let orgs = self
            .list_all(
                "/api/v2/organizations.json",
                |p: api::OrganizationsPage| (p.organizations, p.meta),
                cancel,
            )
            .await?;
        let orgs = map::organizations(orgs);

        let mut writer = bundle.people_writer(true)?;
        let mut seen = HashSet::new();
        let mut people = 0u64;
        let cursor = self
            .users_since(
                None,
                &orgs,
                &mut writer,
                &mut seen,
                &mut people,
                run,
                cancel,
            )
            .await?;
        writer.checkpoint()?;
        run.counts.insert("people".into(), people);
        Ok(cursor)
    }

    /// The users created or changed since the people were fetched, added to
    /// people.jsonl when not already there. Returns the export's cursor for
    /// next time.
    async fn refresh_people(
        &self,
        bundle: &Bundle,
        cursor: Option<String>,
        run: &mut Run<'_>,
        cancel: &CancellationToken,
    ) -> Result<Option<String>, SourceError> {
        let Some(cursor) = cursor else {
            return Ok(None);
        };
        let orgs = map::organizations(
            self.list_all(
                "/api/v2/organizations.json",
                |p: api::OrganizationsPage| (p.organizations, p.meta),
                cancel,
            )
            .await?,
        );
        let mut seen: HashSet<i64> = bundle
            .people()?
            .filter_map(Result::ok)
            .filter_map(|p| p.source_id.parse().ok())
            .collect();
        let mut people = run.counts.get("people").copied().unwrap_or(0);
        let mut writer = bundle.people_writer(false)?;
        let cursor = self
            .users_since(
                Some(cursor),
                &orgs,
                &mut writer,
                &mut seen,
                &mut people,
                run,
                cancel,
            )
            .await?;
        writer.checkpoint()?;
        run.counts.insert("people".into(), people);
        Ok(cursor)
    }

    /// Pages through the incremental user export from `cursor` (or the
    /// start), writing each user not `seen` yet.
    #[allow(clippy::too_many_arguments)]
    async fn users_since(
        &self,
        mut cursor: Option<String>,
        orgs: &map::Organizations,
        writer: &mut bundle::JsonlWriter,
        seen: &mut HashSet<i64>,
        people: &mut u64,
        run: &mut Run<'_>,
        cancel: &CancellationToken,
    ) -> Result<Option<String>, SourceError> {
        let per_page = USER_PAGE_SIZE.to_string();
        loop {
            let query: Vec<(&str, &str)> = match cursor.as_deref() {
                Some(c) => vec![("cursor", c), ("per_page", &per_page)],
                None => vec![("start_time", "0"), ("per_page", &per_page)],
            };
            let url = self
                .http
                .url("/api/v2/incremental/users/cursor.json", &query);
            let page: api::UsersPage = self
                .http
                .get_json(&url, cancel)
                .await?
                .ok_or_else(|| SourceError::Protocol("the user export answered 404".into()))?;
            for u in &page.users {
                // A user updated during the export comes round again.
                if seen.insert(u.id) {
                    writer.write(&map::person(u, orgs))?;
                    *people += 1;
                }
            }
            run.progress.people = *people;
            run.report(self);
            match page.after_cursor {
                Some(next) if !page.end_of_stream && cursor.as_deref() != Some(next.as_str()) => {
                    cursor = Some(next)
                }
                // The end: the cursor Zendesk gave stays good for what
                // changes after it.
                next => return Ok(next.or(cursor)),
            }
        }
    }

    /// One ticket with its conversation and files. None when it can't be
    /// carried (gone since it was listed); the reason is counted.
    async fn fetch_ticket(
        &self,
        bundle: &Bundle,
        t: &api::Ticket,
        solved_at: Option<chrono::DateTime<Utc>>,
        fields: &FieldCatalog,
        cancel: &CancellationToken,
    ) -> Result<TicketOutcome, SourceError> {
        let Some(created_at) = map::ticket_created_at(t) else {
            return Ok(TicketOutcome::Skipped("tickets_without_dates"));
        };
        // Joined, not formatted: the ticket-link lint reads any formatted
        // `/tickets/` path as a hand-built Nosdesk link.
        let path = ["/api/v2/tickets", &t.id.to_string(), "comments.json"].join("/");
        let size = LIST_PAGE_SIZE.to_string();
        let mut raw = Vec::new();
        let mut after: Option<String> = None;
        loop {
            let mut query = vec![("page[size]", size.as_str())];
            if let Some(a) = after.as_deref() {
                query.push(("page[after]", a));
            }
            let url = self.http.url(&path, &query);
            let Some(page) = self
                .http
                .get_json::<api::CommentsPage>(&url, cancel)
                .await?
            else {
                return Ok(TicketOutcome::Skipped("tickets_vanished"));
            };
            raw.extend(page.comments);
            match page.meta.after_cursor {
                Some(next) if page.meta.has_more && after.as_deref() != Some(next.as_str()) => {
                    after = Some(next)
                }
                _ => break,
            }
        }
        let (ticket, stats) = self
            .assemble(bundle, t, created_at, solved_at, fields, &raw, cancel)
            .await?;
        Ok(TicketOutcome::Fetched(Box::new(ticket), stats))
    }

    /// Maps a ticket and its raw comments, downloading every attachment.
    /// Shared with the export path.
    #[allow(clippy::too_many_arguments)]
    async fn assemble(
        &self,
        bundle: &Bundle,
        t: &api::Ticket,
        created_at: chrono::DateTime<Utc>,
        solved_at: Option<chrono::DateTime<Utc>>,
        fields: &FieldCatalog,
        raw: &[api::Comment],
        cancel: &CancellationToken,
    ) -> Result<(bundle::Ticket, Stats), SourceError> {
        let mut stats = Stats::default();
        let mut comments = Vec::with_capacity(raw.len());
        for c in raw {
            let mut attachments = Vec::with_capacity(c.attachments.len());
            for a in &c.attachments {
                let att = self.attachment(bundle, a, cancel).await?;
                stats.attachments += 1;
                if !att.stored {
                    stats.attachments_missing += 1;
                }
                attachments.push(att);
            }
            let comment = map::comment(c, created_at, attachments);
            if map::has_zendesk_inline_image(&comment.body) {
                stats.inline_images += 1;
            }
            stats.comments += 1;
            comments.push(comment);
        }
        Ok((
            map::ticket(t, created_at, fields, comments, solved_at),
            stats,
        ))
    }

    async fn attachment(
        &self,
        bundle: &Bundle,
        a: &api::Attachment,
        cancel: &CancellationToken,
    ) -> Result<Attachment, SourceError> {
        let source_id = a.id.to_string();
        let file_name = a
            .file_name
            .as_deref()
            .map(map::text)
            .filter(|n| !n.trim().is_empty())
            .unwrap_or_else(|| format!("attachment-{}", a.id));
        let link = a.content_url.clone().filter(|u| !u.is_empty());
        let mut att = Attachment {
            source_id,
            file_name,
            content_type: a.content_type.clone(),
            size: a.size,
            stored: false,
            missing_reason: None,
            source_url: None,
            retry: false,
        };
        match link {
            Some(u) => {
                // Kept only when it is a link the connector would follow.
                let allowed = url::Url::parse(&u)
                    .ok()
                    .is_some_and(|parsed| self.http.attachment_link_allowed(&parsed).is_ok());
                if allowed {
                    att.source_url = Some(u.clone());
                }
                self.download_into(bundle, &mut att, &u, cancel).await?;
            }
            None => att.missing_reason = Some("Zendesk gave no download link".to_string()),
        }
        Ok(att)
    }

    /// Downloads `att` from `link` into the bundle, recording how it went.
    async fn download_into(
        &self,
        bundle: &Bundle,
        att: &mut Attachment,
        link: &str,
        cancel: &CancellationToken,
    ) -> Result<(), SourceError> {
        let dest = bundle.file_path_to_write(&att.source_id)?;
        let outcome = self
            .http
            .download(
                link,
                &dest,
                self.max_attachment_bytes,
                att.size,
                att.content_type.as_deref(),
                cancel,
            )
            .await?;
        match outcome {
            Ok(len) => {
                att.size = Some(len);
                att.stored = true;
                att.missing_reason = None;
                att.retry = false;
            }
            Err(missing) => {
                att.stored = false;
                att.missing_reason = Some(missing.reason);
                att.retry = missing.retry;
            }
        }
        Ok(())
    }

    /// Tries again every attachment the bundle lacks for a reason that may
    /// have passed, rewriting tickets.jsonl once if any came through.
    async fn retry_missing(
        &self,
        bundle: &Bundle,
        state: &mut FetchState,
        run: &mut Run<'_>,
        cancel: &CancellationToken,
    ) -> Result<(), SourceError> {
        let mut tickets = bundle.tickets()?.collect::<Result<Vec<_>, _>>()?;
        let mut recovered = 0u64;
        for t in &mut tickets {
            for c in &mut t.comments {
                for a in &mut c.attachments {
                    let Some(link) = a.source_url.clone().filter(|_| !a.stored && a.retry) else {
                        continue;
                    };
                    self.download_into(bundle, a, &link, cancel).await?;
                    if a.stored {
                        recovered += 1;
                    }
                }
            }
        }
        if recovered > 0 {
            state.tickets_len = bundle.rewrite_tickets(&tickets)?;
            let missing = run.counts.entry("attachments_missing".into()).or_default();
            *missing = missing.saturating_sub(recovered);
            state.counts = run.counts.clone();
            bundle.write_state(state)?;
        }
        Ok(())
    }

    async fn fetch_tickets(
        &self,
        bundle: &Bundle,
        state: &mut FetchState,
        run: &mut Run<'_>,
        cancel: &CancellationToken,
    ) -> Result<(), SourceError> {
        let fields = FieldCatalog::new(
            self.list_all(
                "/api/v2/ticket_fields.json",
                |p: api::TicketFieldsPage| (p.ticket_fields, p.meta),
                cancel,
            )
            .await?,
        );

        let mut writer = bundle.tickets_writer(state.tickets_len)?;
        let mut seen = written_ticket_ids(bundle, state.tickets_len)?;
        let per_page = self.ticket_page_size.to_string();
        let mut cursor = state.ticket_cursor.clone();

        loop {
            if cancel.is_cancelled() {
                return Err(SourceError::Cancelled);
            }
            let query: Vec<(&str, &str)> = match cursor.as_deref() {
                Some(c) => vec![
                    ("cursor", c),
                    ("per_page", &per_page),
                    ("include", "metric_sets"),
                ],
                None => vec![
                    ("start_time", "0"),
                    ("exclude_deleted", "true"),
                    ("per_page", &per_page),
                    ("include", "metric_sets"),
                ],
            };
            let url = self
                .http
                .url("/api/v2/incremental/tickets/cursor.json", &query);
            let page: api::TicketsPage =
                self.http.get_json(&url, cancel).await?.ok_or_else(|| {
                    SourceError::Protocol("the ticket export answered 404".into())
                })?;

            // Incremental exports run in update order, so a ticket touched
            // during the fetch comes round again; the first copy stands.
            let mut page_ids = HashSet::new();
            let mut duplicates = 0u64;
            let mut deleted = 0u64;
            let todo: Vec<&api::Ticket> = page
                .tickets
                .iter()
                .filter(|t| {
                    if map::is_deleted(t) {
                        deleted += 1;
                        return false;
                    }
                    let fresh = !seen.contains(&t.id.to_string()) && page_ids.insert(t.id);
                    if !fresh {
                        duplicates += 1;
                    }
                    fresh
                })
                .collect();

            // Futures built up front rather than in a `.map` closure: a
            // closure over borrowed tickets trips the compiler's `Send`
            // check on the fetch future.
            let solved: HashMap<i64, chrono::DateTime<Utc>> = page
                .metric_sets
                .iter()
                .filter_map(|m| Some((m.ticket_id?, m.solved_at?)))
                .collect();
            let pending: Vec<_> = todo
                .into_iter()
                .map(|t| {
                    let solved_at = solved.get(&t.id).copied();
                    self.fetch_ticket(bundle, t, solved_at, &fields, cancel)
                })
                .collect();
            let outcomes: Vec<TicketOutcome> = stream::iter(pending)
                .buffered(TICKET_CONCURRENCY)
                .try_collect()
                .await?;

            // Everything on this page is fetched; write it and only then
            // move the checkpoint past it.
            let mut page_stats = Stats::default();
            for outcome in outcomes {
                match outcome {
                    TicketOutcome::Fetched(ticket, stats) => {
                        writer.write(&ticket)?;
                        seen.insert(ticket.source_id.clone());
                        page_stats.tickets += 1;
                        page_stats.add(&stats);
                    }
                    TicketOutcome::Skipped(why) => *run.counts.entry(why.into()).or_default() += 1,
                }
            }
            *run.counts
                .entry("duplicate_tickets_skipped".into())
                .or_default() += duplicates;
            *run.counts
                .entry("deleted_tickets_skipped".into())
                .or_default() += deleted;
            run.add(&page_stats);

            let done = page.end_of_stream || page.after_cursor.is_none();
            state.tickets_len = writer.checkpoint()?;
            state.ticket_cursor = page.after_cursor.clone().or(cursor.clone());
            state.tickets_done = done;
            state.counts = run.counts.clone();
            bundle.write_state(state)?;
            run.report(self);

            if done {
                return Ok(());
            }
            match page.after_cursor {
                Some(next) if cursor.as_deref() != Some(next.as_str()) => cursor = Some(next),
                // Zendesk handed back the cursor we just used: no progress
                // is possible, and looping would never end.
                _ => {
                    return Err(SourceError::Protocol(
                        "the ticket export repeated its cursor".into(),
                    ))
                }
            }
        }
    }
}

impl HelpdeskSource for ZendeskSource {
    fn source(&self) -> &'static str {
        "zendesk"
    }

    fn scope(&self) -> String {
        self.scope.clone()
    }

    async fn fetch(
        &mut self,
        bundle: &Bundle,
        progress: &(dyn Fn(&FetchProgress) + Send + Sync),
        cancel: CancellationToken,
    ) -> Result<Manifest, SourceError> {
        if !self.http.has_credential() {
            return Err(SourceError::Unauthorized(
                "no Zendesk credential: this source can only read an export".into(),
            ));
        }
        let mut state = bundle.read_state()?;
        let resumed = state != FetchState::default();
        let mut run = Run::new(&state, progress);

        if !state.people_done {
            state.people_cursor = self.fetch_people(bundle, &mut run, &cancel).await?;
            state.people_done = true;
            state.counts = run.counts.clone();
            bundle.write_state(&state)?;
        }
        if !state.tickets_done {
            self.fetch_tickets(bundle, &mut state, &mut run, &cancel)
                .await?;
        }
        state.people_cursor = self
            .refresh_people(bundle, state.people_cursor.take(), &mut run, &cancel)
            .await?;
        state.counts = run.counts.clone();
        bundle.write_state(&state)?;
        if resumed {
            self.retry_missing(bundle, &mut state, &mut run, &cancel)
                .await?;
        }

        let manifest = run.manifest(self.source(), &self.scope, &state.notes);
        bundle.write_manifest(&manifest)?;
        Ok(manifest)
    }
}

/// Accepts `acme`, `ACME` and `acme.zendesk.com`; returns `acme`.
fn normalise_subdomain(raw: &str) -> Result<String, ZendeskConfigError> {
    static PATTERN: OnceLock<Option<Regex>> = OnceLock::new();
    let lowered = raw.trim().to_ascii_lowercase();
    let candidate = lowered.strip_suffix(".zendesk.com").unwrap_or(&lowered);
    let valid = PATTERN
        .get_or_init(|| Regex::new(r"^[a-z0-9][a-z0-9-]{0,62}$").ok())
        .as_ref()
        .is_some_and(|re| re.is_match(candidate));
    if valid {
        Ok(candidate.to_string())
    } else {
        Err(ZendeskConfigError::InvalidSubdomain(raw.to_string()))
    }
}

/// The ids already in tickets.jsonl up to the checkpoint, so a resumed
/// fetch doesn't write a ticket twice when Zendesk lists it again.
fn written_ticket_ids(bundle: &Bundle, len: u64) -> Result<HashSet<String>, SourceError> {
    #[derive(Deserialize)]
    struct Id {
        source_id: String,
    }
    if len == 0 {
        return Ok(HashSet::new());
    }
    let path = bundle.root().join(bundle::TICKETS);
    let file = std::fs::File::open(path)?;
    let mut ids = HashSet::new();
    for line in std::io::BufRead::lines(std::io::BufReader::new(file)) {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let id: Id = serde_json::from_str(&line).map_err(|e| {
            SourceError::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, e))
        })?;
        ids.insert(id.source_id);
    }
    Ok(ids)
}

enum TicketOutcome {
    Fetched(Box<bundle::Ticket>, Stats),
    /// Not carried; the name is the count it goes under.
    Skipped(&'static str),
}

#[derive(Debug, Default, Clone, Copy)]
struct Stats {
    tickets: u64,
    comments: u64,
    attachments: u64,
    attachments_missing: u64,
    inline_images: u64,
}

impl Stats {
    fn add(&mut self, o: &Stats) {
        self.tickets += o.tickets;
        self.comments += o.comments;
        self.attachments += o.attachments;
        self.attachments_missing += o.attachments_missing;
        self.inline_images += o.inline_images;
    }
}

/// Running counts for one fetch, seeded from the saved state so a resumed
/// fetch reports the whole bundle, not just its own share.
struct Run<'a> {
    counts: BTreeMap<String, u64>,
    progress: FetchProgress,
    callback: &'a (dyn Fn(&FetchProgress) + Send + Sync),
}

/// The counts the manifest carries. Everything else in `counts` is
/// bookkeeping that becomes a note.
const MANIFEST_COUNTS: [&str; 5] = [
    "people",
    "tickets",
    "comments",
    "attachments",
    "attachments_missing",
];

impl<'a> Run<'a> {
    fn new(state: &FetchState, callback: &'a (dyn Fn(&FetchProgress) + Send + Sync)) -> Self {
        let counts = state.counts.clone();
        let get = |k: &str| counts.get(k).copied().unwrap_or(0);
        let progress = FetchProgress {
            people: get("people"),
            tickets: get("tickets"),
            comments: get("comments"),
            attachments: get("attachments"),
            throttled: 0,
        };
        Run {
            counts,
            progress,
            callback,
        }
    }

    fn add(&mut self, s: &Stats) {
        for (k, v) in [
            ("tickets", s.tickets),
            ("comments", s.comments),
            ("attachments", s.attachments),
            ("attachments_missing", s.attachments_missing),
            ("inline_image_comments", s.inline_images),
        ] {
            *self.counts.entry(k.into()).or_default() += v;
        }
        self.progress.tickets += s.tickets;
        self.progress.comments += s.comments;
        self.progress.attachments += s.attachments;
    }

    fn report(&mut self, source: &ZendeskSource) {
        self.progress.throttled = source.http.throttled();
        (self.callback)(&self.progress);
    }

    fn manifest(&self, source: &str, scope: &str, saved_notes: &[String]) -> Manifest {
        let counts: BTreeMap<String, u64> = MANIFEST_COUNTS
            .iter()
            .map(|k| (k.to_string(), self.counts.get(*k).copied().unwrap_or(0)))
            .collect();
        let mut notes = saved_notes.to_vec();
        notes.extend(notes_from_counts(&self.counts));
        Manifest {
            format_version: bundle::FORMAT_VERSION,
            source: source.to_string(),
            source_scope: scope.to_string(),
            extracted_at: Utc::now(),
            counts,
            notes,
        }
    }
}

/// The bookkeeping counts, in words for the report.
fn notes_from_counts(counts: &BTreeMap<String, u64>) -> Vec<String> {
    let get = |k: &str| counts.get(k).copied().unwrap_or(0);
    let mut notes = Vec::new();
    let n = get("inline_image_comments");
    if n > 0 {
        notes.push(format!(
            "{n} comments show images hosted by Zendesk inline; the HTML still points at Zendesk"
        ));
    }
    let n = get("attachments_missing");
    if n > 0 {
        notes.push(format!(
            "{n} attachments could not be downloaded; each says why"
        ));
    }
    let n = get("tickets_vanished");
    if n > 0 {
        notes.push(format!(
            "{n} tickets were deleted in Zendesk while the fetch ran and were left out"
        ));
    }
    let n = get("tickets_without_dates");
    if n > 0 {
        notes.push(format!(
            "{n} tickets had no creation date and were left out"
        ));
    }
    let n = get("deleted_tickets_skipped");
    if n > 0 {
        notes.push(format!("{n} deleted tickets were left out"));
    }
    let n = get("tickets_without_comments");
    if n > 0 {
        notes.push(format!(
            "{n} tickets came without their conversation (Zendesk leaves comments out of export records over 1 MB); their description stands in as the first comment"
        ));
    }
    notes
}
