//! Talking to Zendesk: one SSRF-safe client, credentials only where they
//! belong, and patience when Zendesk asks for it.
//!
//! Every request, API or attachment, goes through [`Http::send`], which
//! checks the destination against the egress policy, waits out 429s for as
//! long as `Retry-After` says (those don't use up the retry budget: the
//! incremental exports allow ten requests a minute, so a big account meets
//! the limit as a matter of course), backs off with jitter on 5xx and
//! network errors, and stops promptly when the run is cancelled.

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use reqwest::{RequestBuilder, Response, StatusCode};
use serde::de::DeserializeOwned;
use tokio::io::AsyncWriteExt;
use tokio_util::sync::CancellationToken;
use url::Url;

use super::ZendeskAuth;
use crate::services::imports::helpdesk::source::SourceError;
use crate::utils::{egress, safe_http};

/// A page of JSON is small; a minute covers a slow Zendesk.
const API_TIMEOUT: Duration = Duration::from_secs(60);
/// An attachment can be large and its host slow; the cap bounds the size,
/// this bounds the time.
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(30 * 60);

/// How hard to try before giving up on a request.
#[derive(Debug, Clone, Copy)]
pub(super) struct RetryPolicy {
    /// Attempts at a request that fails with a 5xx or a network error,
    /// counting the first.
    pub max_attempts: u32,
    pub initial_backoff: Duration,
    pub max_backoff: Duration,
    /// 429s waited out for one request before calling it stuck.
    pub max_throttle_waits: u32,
    /// The longest single `Retry-After` honoured; a longer one is clipped
    /// and the request simply asked again.
    pub max_retry_after: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        RetryPolicy {
            max_attempts: 5,
            initial_backoff: Duration::from_secs(1),
            max_backoff: Duration::from_secs(60),
            max_throttle_waits: 60,
            max_retry_after: Duration::from_secs(300),
        }
    }
}

impl RetryPolicy {
    /// Exponential backoff before retry `n` (1-based), with jitter so a
    /// burst of failed requests doesn't come back in lockstep.
    fn backoff(&self, n: u32) -> Duration {
        let exp = 2u32.saturating_pow(n.saturating_sub(1).min(16));
        let base = self
            .initial_backoff
            .saturating_mul(exp)
            .min(self.max_backoff);
        let factor: f64 = rand::random_range(0.5..=1.0);
        base.mul_f64(factor)
    }
}

/// An attachment that couldn't be stored, and why, for the report. The
/// reason never carries the link, which can hold a token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Missing {
    pub reason: String,
    /// The cause may pass (a dropped connection, a short read, a busy
    /// server), so a resumed fetch tries the file again.
    pub retry: bool,
}

impl Missing {
    fn lasting(reason: impl Into<String>) -> Self {
        Missing {
            reason: reason.into(),
            retry: false,
        }
    }

    fn passing(reason: impl Into<String>) -> Self {
        Missing {
            reason: reason.into(),
            retry: true,
        }
    }
}

/// Begins a [`SourceError::Network`] for a request the outbound policy
/// refused before it was sent.
pub(super) const EGRESS_REFUSED: &str = "refused by the outbound policy:";

/// In a [`SourceError::Network`] for a request Zendesk kept throttling past
/// the retry budget.
pub(super) const RATE_LIMITED: &str = "still rate limited";

/// `reason` with `url` (and its path, which can hold a token) taken out.
fn redact(reason: &str, url: &Url) -> String {
    let mut out = reason.replace(url.as_str(), "the download link");
    if url.path().len() > 1 {
        out = out.replace(url.path(), "the download link");
    }
    out
}

pub(super) struct Http {
    client: reqwest::Client,
    /// For attachments: follows no redirect itself, so [`Http::download`]
    /// checks every hop against the attachment host rules.
    download_client: reqwest::Client,
    /// Scheme, host and port of the Zendesk account (or the operator's
    /// allowlisted stand-in). Every API URL is built from it.
    base: Url,
    /// None when reading an export without a credential: nothing is ever
    /// signed.
    auth: Option<ZendeskAuth>,
    /// The host of an operator's stand-in base URL, which was checked
    /// against `NOSDESK_OUTBOUND_ALLOWED_HOSTS` when the source was built
    /// and stays trusted for the run. None for a real Zendesk account,
    /// whose hostname the safe_http resolver vets on every connect.
    trusted_host: Option<String>,
    pub(super) policy: RetryPolicy,
    throttled: AtomicU64,
}

impl Http {
    /// `https_only` is true for every real Zendesk account; only an
    /// allowlisted local stand-in may be plain http.
    pub fn new(
        base: Url,
        auth: Option<ZendeskAuth>,
        https_only: bool,
        trusted_host: Option<String>,
    ) -> reqwest::Result<Self> {
        let client = if https_only {
            safe_http::https_only_client(API_TIMEOUT)?
        } else {
            safe_http::client(API_TIMEOUT)?
        };
        Ok(Http {
            client,
            download_client: safe_http::no_redirect_client(DOWNLOAD_TIMEOUT)?,
            base,
            auth,
            trusted_host,
            policy: RetryPolicy::default(),
            throttled: AtomicU64::new(0),
        })
    }

    /// Requests that waited on a 429 so far.
    pub fn throttled(&self) -> u64 {
        self.throttled.load(Ordering::Relaxed)
    }

    /// An API URL on the account. Cursors go in as query values, so a
    /// next-page link from Zendesk is never followed as a URL.
    pub fn url(&self, path: &str, query: &[(&str, &str)]) -> Url {
        let mut url = self.base.clone();
        url.set_path(path);
        url.set_query(None);
        if !query.is_empty() {
            let mut pairs = url.query_pairs_mut();
            for (k, v) in query {
                pairs.append_pair(k, v);
            }
        }
        url
    }

    /// Zendesk credentials go to the account itself and to Zendesk's own
    /// attachment host over https, nowhere else: not to another Zendesk
    /// account, and not to wherever a comment's link points.
    fn may_send_credentials(&self, url: &Url) -> bool {
        same_origin(url, &self.base) || is_zendesk_file_host(url)
    }

    /// Whether an attachment may be fetched from `url`: the account itself
    /// (or the operator's stand-in for it), or Zendesk's attachment host over
    /// https. A link anywhere else is the bundle's word, not Zendesk's, and
    /// its bytes would be stored and served back here, so it isn't followed;
    /// `NOSDESK_OUTBOUND_ALLOWED_HOSTS` doesn't widen this.
    pub fn attachment_link_allowed(&self, url: &Url) -> Result<(), String> {
        if same_origin(url, &self.base) || is_zendesk_file_host(url) {
            return Ok(());
        }
        Err(format!(
            "the link points off the Zendesk account (to {}), so it wasn't fetched",
            url.host_str().unwrap_or("no host")
        ))
    }

    pub fn has_credential(&self) -> bool {
        self.auth.is_some()
    }

    fn authorise(&self, req: RequestBuilder) -> RequestBuilder {
        match &self.auth {
            Some(ZendeskAuth::Bearer(token)) => req.bearer_auth(token),
            Some(ZendeskAuth::Basic { email, token }) => {
                req.basic_auth(format!("{email}/token"), Some(token))
            }
            None => req,
        }
    }

    /// Sends a GET, retrying as the module docs describe. Returns whatever
    /// status Zendesk settled on; the caller decides what a 4xx means.
    async fn send(
        &self,
        url: &Url,
        timeout: Duration,
        cancel: &CancellationToken,
    ) -> Result<Response, SourceError> {
        self.send_with(&self.client, url, timeout, false, cancel)
            .await
    }

    /// [`Self::send`] through `client`. `strict` refuses an IP literal the
    /// operator allowlisted, unless it is the stand-in's own host: for an
    /// attachment, whose bytes are stored and served back.
    async fn send_with(
        &self,
        client: &reqwest::Client,
        url: &Url,
        timeout: Duration,
        strict: bool,
        cancel: &CancellationToken,
    ) -> Result<Response, SourceError> {
        if strict {
            strict_egress_check(url, self.trusted_host.as_deref())
        } else {
            egress_check(url, self.trusted_host.as_deref())
        }
        .map_err(|e| SourceError::Network(format!("{EGRESS_REFUSED} {e}")))?;
        let with_auth = self.may_send_credentials(url);
        let mut failures = 0u32;
        let mut waits = 0u32;
        loop {
            if cancel.is_cancelled() {
                return Err(SourceError::Cancelled);
            }
            let mut req = client.get(url.clone()).timeout(timeout);
            if with_auth {
                req = self.authorise(req);
            }
            let sent = tokio::select! {
                r = req.send() => r,
                _ = cancel.cancelled() => return Err(SourceError::Cancelled),
            };
            match sent {
                Ok(resp) => {
                    let status = resp.status();
                    let retry_after = retry_after(&resp);
                    if status == StatusCode::TOO_MANY_REQUESTS
                        || (status == StatusCode::SERVICE_UNAVAILABLE && retry_after.is_some())
                    {
                        waits += 1;
                        if waits > self.policy.max_throttle_waits {
                            return Err(SourceError::Network(format!(
                                "{} {RATE_LIMITED} after {} waits",
                                url.path(),
                                waits - 1
                            )));
                        }
                        let wait = retry_after.unwrap_or_else(|| self.policy.backoff(waits));
                        self.throttled.fetch_add(1, Ordering::Relaxed);
                        tracing::info!(
                            source = "zendesk",
                            attempt = waits,
                            "zendesk asked us to slow down; waiting {}s",
                            wait.as_secs()
                        );
                        sleep(wait.min(self.policy.max_retry_after), cancel).await?;
                        continue;
                    }
                    if status.is_server_error() {
                        failures += 1;
                        if failures >= self.policy.max_attempts {
                            return Err(SourceError::Network(format!(
                                "{} answered {status}",
                                url.path()
                            )));
                        }
                        sleep(self.policy.backoff(failures), cancel).await?;
                        continue;
                    }
                    return Ok(resp);
                }
                Err(e) => {
                    // A refused redirect or a request we built wrong won't
                    // get better by asking again.
                    if e.is_builder() || e.is_redirect() {
                        return Err(SourceError::Network(e.to_string()));
                    }
                    failures += 1;
                    if failures >= self.policy.max_attempts {
                        return Err(SourceError::Network(e.to_string()));
                    }
                    tracing::warn!(source = "zendesk", attempt = failures, error = %e, "zendesk request failed, retrying");
                    sleep(self.policy.backoff(failures), cancel).await?;
                }
            }
        }
    }

    /// GETs a page of JSON. `Ok(None)` is a 404: the record went away
    /// between listing and fetching, which happens on a live account.
    pub async fn get_json<T: DeserializeOwned>(
        &self,
        url: &Url,
        cancel: &CancellationToken,
    ) -> Result<Option<T>, SourceError> {
        let mut failures = 0u32;
        loop {
            let resp = self.send(url, API_TIMEOUT, cancel).await?;
            let status = resp.status();
            match status {
                StatusCode::UNAUTHORIZED => {
                    return Err(SourceError::Unauthorized(format!(
                        "{} answered 401",
                        url.path()
                    )))
                }
                StatusCode::FORBIDDEN => {
                    return Err(SourceError::Unauthorized(format!(
                        "{} answered 403: the credential lacks permission",
                        url.path()
                    )))
                }
                StatusCode::NOT_FOUND => return Ok(None),
                s if !s.is_success() => {
                    return Err(SourceError::Protocol(format!(
                        "{} answered {s}",
                        url.path()
                    )));
                }
                _ => {}
            }
            let bytes = tokio::select! {
                b = resp.bytes() => b,
                _ = cancel.cancelled() => return Err(SourceError::Cancelled),
            };
            match bytes {
                Ok(bytes) => {
                    return serde_json::from_slice(&bytes)
                        .map(Some)
                        .map_err(|e| SourceError::Protocol(format!("{}: {e}", url.path())));
                }
                // The connection dropped mid-body: ask for the page again.
                Err(e) => {
                    failures += 1;
                    if failures >= self.policy.max_attempts {
                        return Err(SourceError::Network(e.to_string()));
                    }
                    sleep(self.policy.backoff(failures), cancel).await?;
                }
            }
        }
    }

    /// Streams `raw_url` to `dest`, at most `cap` bytes. The outer error is
    /// only for a cancelled run; anything wrong with the file itself comes
    /// back as the inner [`Missing`] so one bad attachment never stops an
    /// import. A file already at `dest` with the expected size is kept, so
    /// a resumed fetch doesn't download it twice. A download that breaks
    /// off, or ends short of the size Zendesk gave, is tried again.
    pub async fn download(
        &self,
        raw_url: &str,
        dest: &Path,
        cap: u64,
        expected_size: Option<u64>,
        declared_type: Option<&str>,
        cancel: &CancellationToken,
    ) -> Result<Result<u64, Missing>, SourceError> {
        if let (Some(expected), Ok(meta)) = (expected_size, tokio::fs::symlink_metadata(dest).await)
        {
            if meta.is_file() && meta.len() == expected {
                return Ok(Ok(expected));
            }
        }
        if expected_size.is_some_and(|s| s > cap) {
            return Ok(Err(too_large(cap)));
        }
        let url = match Url::parse(raw_url) {
            Ok(u) => u,
            Err(e) => {
                return Ok(Err(Missing::lasting(format!(
                    "unreadable download link: {e}"
                ))))
            }
        };
        if let Err(why) = self.attachment_link_allowed(&url) {
            return Ok(Err(Missing::lasting(why)));
        }
        let mut failures = 0u32;
        loop {
            match self
                .download_once(&url, dest, cap, expected_size, declared_type, cancel)
                .await?
            {
                Err(missing) if missing.retry => {
                    failures += 1;
                    if failures >= self.policy.max_attempts {
                        return Ok(Err(missing));
                    }
                    sleep(self.policy.backoff(failures), cancel).await?;
                }
                done => return Ok(done),
            }
        }
    }

    async fn download_once(
        &self,
        url: &Url,
        dest: &Path,
        cap: u64,
        expected_size: Option<u64>,
        declared_type: Option<&str>,
        cancel: &CancellationToken,
    ) -> Result<Result<u64, Missing>, SourceError> {
        // Redirects are followed here, one hop at a time, so each hop meets
        // the same host rule (and gets credentials only where they belong).
        let mut current = url.clone();
        let mut hops = 0;
        let mut resp = loop {
            let resp = match self
                .send_with(
                    &self.download_client,
                    &current,
                    DOWNLOAD_TIMEOUT,
                    true,
                    cancel,
                )
                .await
            {
                Ok(r) => r,
                Err(SourceError::Cancelled) => return Err(SourceError::Cancelled),
                Err(e) => return Ok(Err(Missing::passing(redact(&e.to_string(), &current)))),
            };
            if !resp.status().is_redirection() {
                break resp;
            }
            hops += 1;
            let next = resp
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|l| l.to_str().ok())
                .and_then(|l| current.join(l).ok());
            let Some(next) = next.filter(|_| hops <= 5) else {
                return Ok(Err(Missing::lasting(
                    "the download redirected too often or nowhere",
                )));
            };
            if let Err(why) = self.attachment_link_allowed(&next) {
                return Ok(Err(Missing::lasting(why)));
            }
            current = next;
        };

        let status = resp.status();
        if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
            return Ok(Err(Missing::lasting(if self.auth.is_none() {
                "needs sign-in; re-run with a Zendesk credential".to_string()
            } else {
                format!("download answered {status}")
            })));
        }
        if !status.is_success() {
            let missing = format!("download answered {status}");
            return Ok(Err(if status.is_server_error() {
                Missing::passing(missing)
            } else {
                Missing::lasting(missing)
            }));
        }
        if resp.content_length().is_some_and(|len| len > cap) {
            return Ok(Err(too_large(cap)));
        }
        // A sign-in or error page answered with 200 is not the file.
        let served = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_ascii_lowercase();
        let declared_html = declared_type.is_some_and(|t| t.to_ascii_lowercase().contains("html"));
        if served.starts_with("text/html") && !declared_html {
            return Ok(Err(Missing::lasting(
                "Zendesk answered with a web page instead of the file",
            )));
        }

        let part = dest.with_extension("part");
        let outcome = match stream_to(&mut resp, &part, cap, cancel).await {
            Ok(Ok(len)) if expected_size.is_some_and(|e| e != len) => {
                Ok(Err(Missing::passing(format!(
                    "the download ended at {len} of {} bytes",
                    expected_size.unwrap_or_default()
                ))))
            }
            other => other,
        };
        match outcome {
            Ok(Ok(len)) => match tokio::fs::rename(&part, dest).await {
                Ok(()) => Ok(Ok(len)),
                Err(e) => {
                    let _ = tokio::fs::remove_file(&part).await;
                    Ok(Err(Missing::lasting(format!(
                        "could not store the file: {e}"
                    ))))
                }
            },
            other => {
                let _ = tokio::fs::remove_file(&part).await;
                other
            }
        }
    }
}

fn too_large(cap: u64) -> Missing {
    Missing::lasting(format!("larger than the {} MB limit", cap / (1024 * 1024)))
}

/// Zendesk's attachment host, over https.
fn is_zendesk_file_host(url: &Url) -> bool {
    url.scheme() == "https"
        && url
            .host_str()
            .is_some_and(|h| h.to_ascii_lowercase().ends_with(".zdusercontent.com"))
}

async fn stream_to(
    resp: &mut Response,
    part: &Path,
    cap: u64,
    cancel: &CancellationToken,
) -> Result<Result<u64, Missing>, SourceError> {
    // Never through a link: a `.part` planted as a link to somewhere else
    // would have the download written there.
    let mut options = tokio::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    options.custom_flags(libc::O_NOFOLLOW);
    let mut file = match options.open(part).await {
        Ok(f) => f,
        Err(e) => {
            return Ok(Err(Missing::lasting(format!(
                "could not store the file: {e}"
            ))))
        }
    };
    let mut written = 0u64;
    loop {
        let chunk = tokio::select! {
            c = resp.chunk() => c,
            _ = cancel.cancelled() => return Err(SourceError::Cancelled),
        };
        match chunk {
            Ok(Some(bytes)) => {
                written += bytes.len() as u64;
                if written > cap {
                    return Ok(Err(too_large(cap)));
                }
                if let Err(e) = file.write_all(&bytes).await {
                    return Ok(Err(Missing::lasting(format!(
                        "could not store the file: {e}"
                    ))));
                }
            }
            Ok(None) => break,
            Err(e) => {
                return Ok(Err(Missing::passing(format!(
                    "download interrupted: {}",
                    e.without_url()
                ))))
            }
        }
    }
    if let Err(e) = file.flush().await {
        return Ok(Err(Missing::lasting(format!(
            "could not store the file: {e}"
        ))));
    }
    if let Err(e) = file.sync_all().await {
        return Ok(Err(Missing::lasting(format!(
            "could not store the file: {e}"
        ))));
    }
    Ok(Ok(written))
}

/// The egress policy for one URL. Hostnames are vetted by the safe_http
/// resolver at connect time; an IP literal bypasses the resolver, so it is
/// vetted here, unless the operator allowlisted it (now, or when the
/// source was built, for the stand-in's own host).
fn egress_check(url: &Url, trusted_host: Option<&str>) -> Result<(), String> {
    let host = url.host_str().ok_or_else(|| format!("{url}: no host"))?;
    if trusted_host.is_some_and(|t| t.eq_ignore_ascii_case(host))
        || egress::is_host_allowlisted(host)
    {
        return Ok(());
    }
    safe_http::reject_unsafe_ip_literal(url.as_str()).map_err(|e| format!("{host}: {e}"))
}

/// [`egress_check`] for an attachment: an IP literal passes only as the
/// stand-in's own host, never through the operator's allowlist.
fn strict_egress_check(url: &Url, trusted_host: Option<&str>) -> Result<(), String> {
    let host = url.host_str().ok_or_else(|| format!("{url}: no host"))?;
    if trusted_host.is_some_and(|t| t.eq_ignore_ascii_case(host)) {
        return Ok(());
    }
    safe_http::reject_unsafe_ip_literal(url.as_str()).map_err(|e| format!("{host}: {e}"))
}

fn same_origin(a: &Url, b: &Url) -> bool {
    a.scheme() == b.scheme()
        && a.host_str().map(str::to_ascii_lowercase) == b.host_str().map(str::to_ascii_lowercase)
        && a.port_or_known_default() == b.port_or_known_default()
}

/// `Retry-After` as seconds or an HTTP date.
fn retry_after(resp: &Response) -> Option<Duration> {
    let raw = resp
        .headers()
        .get(reqwest::header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim();
    if let Ok(secs) = raw.parse::<u64>() {
        return Some(Duration::from_secs(secs));
    }
    let at = chrono::DateTime::parse_from_rfc2822(raw).ok()?;
    let wait = at.with_timezone(&chrono::Utc) - chrono::Utc::now();
    Some(wait.to_std().unwrap_or(Duration::ZERO))
}

/// Sleeps, or stops early when the run is cancelled.
async fn sleep(d: Duration, cancel: &CancellationToken) -> Result<(), SourceError> {
    tokio::select! {
        _ = tokio::time::sleep(d) => Ok(()),
        _ = cancel.cancelled() => Err(SourceError::Cancelled),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn http(base: &str) -> Http {
        let base = Url::parse(base).expect("url");
        Http::new(base, Some(ZendeskAuth::Bearer("t".into())), true, None).expect("client")
    }

    #[test]
    fn credentials_go_only_to_zendesk_hosts_over_https() {
        let h = http("https://acme.zendesk.com");
        let ok = |u: &str| h.may_send_credentials(&Url::parse(u).expect("url"));
        assert!(ok("https://acme.zendesk.com/api/v2/x"));
        assert!(ok(
            "https://acme.zendesk.com/attachments/token/abc/?name=a.png"
        ));
        assert!(ok("https://p1.zdusercontent.com/attachment/1"));
        assert!(
            !ok("https://other.zendesk.com/attachments/token/abc/"),
            "another Zendesk account is not this one"
        );
        assert!(
            !ok("http://acme.zendesk.com/x"),
            "plain http to another origin"
        );
        assert!(
            !ok("https://evilzendesk.com/x"),
            "suffix match needs the dot"
        );
        assert!(!ok("https://acme.zendesk.com.evil.com/x"));
        assert!(!ok("https://s3.amazonaws.com/bucket/x"));
    }

    #[test]
    fn attachments_come_only_from_the_account_or_zendesk_file_hosts() {
        let h = http("https://acme.zendesk.com");
        let ok = |u: &str| {
            h.attachment_link_allowed(&Url::parse(u).expect("url"))
                .is_ok()
        };
        assert!(ok(
            "https://acme.zendesk.com/attachments/token/abc/?name=a.png"
        ));
        assert!(ok("https://p1.zdusercontent.com/attachment/1"));
        assert!(!ok("http://p1.zdusercontent.com/attachment/1"), "http");
        assert!(!ok("https://other.zendesk.com/attachments/1"));
        assert!(!ok("https://intranet.example/secret"));
        assert!(!ok("https://10.0.0.5/x"));
    }

    #[test]
    fn reasons_never_carry_the_link() {
        let url =
            Url::parse("https://acme.zendesk.com/attachments/token/SECRET/?name=a").expect("url");
        let reason = redact(
            &format!("error sending request for url ({url}) at {}", url.path()),
            &url,
        );
        assert!(!reason.contains("SECRET"), "{reason}");
    }

    #[test]
    fn backoff_grows_and_stays_bounded() {
        let p = RetryPolicy::default();
        assert!(p.backoff(1) <= Duration::from_secs(1));
        assert!(p.backoff(1) >= Duration::from_millis(500));
        assert!(p.backoff(30) <= p.max_backoff);
    }

    #[test]
    fn urls_carry_cursors_as_query_values() {
        let h = http("https://acme.zendesk.com");
        let u = h.url(
            "/api/v2/tickets/1/comments.json",
            &[("page[size]", "100"), ("page[after]", "a&b=c")],
        );
        assert_eq!(u.host_str(), Some("acme.zendesk.com"));
        let pairs: Vec<(String, String)> = u.query_pairs().into_owned().collect();
        assert_eq!(pairs[1], ("page[after]".to_string(), "a&b=c".to_string()));
    }
}
