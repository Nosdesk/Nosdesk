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

/// An attachment that couldn't be stored, and why, for the report.
pub(super) type Missing = String;

pub(super) struct Http {
    client: reqwest::Client,
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
    /// attachment hosts over https, nowhere else. A comment's attachment
    /// can live on any host, and Zendesk's docs warn about exactly this.
    fn may_send_credentials(&self, url: &Url) -> bool {
        if same_origin(url, &self.base) {
            return true;
        }
        let host = url.host_str().unwrap_or("").to_ascii_lowercase();
        url.scheme() == "https"
            && (host.ends_with(".zendesk.com") || host.ends_with(".zdusercontent.com"))
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
        egress_check(url, self.trusted_host.as_deref()).map_err(SourceError::Network)?;
        let with_auth = self.may_send_credentials(url);
        let mut failures = 0u32;
        let mut waits = 0u32;
        loop {
            if cancel.is_cancelled() {
                return Err(SourceError::Cancelled);
            }
            let mut req = self.client.get(url.clone()).timeout(timeout);
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
                                "{} still rate limited after {} waits",
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
    /// back as the inner `Missing` so one bad attachment never stops an
    /// import. A file already at `dest` with the expected size is kept, so
    /// a resumed fetch doesn't download it twice.
    pub async fn download(
        &self,
        raw_url: &str,
        dest: &Path,
        cap: u64,
        expected_size: Option<u64>,
        cancel: &CancellationToken,
    ) -> Result<Result<u64, Missing>, SourceError> {
        if let (Some(expected), Ok(meta)) = (expected_size, tokio::fs::metadata(dest).await) {
            if meta.is_file() && meta.len() == expected {
                return Ok(Ok(expected));
            }
        }
        if expected_size.is_some_and(|s| s > cap) {
            return Ok(Err(format!(
                "larger than the {} MB limit",
                cap / (1024 * 1024)
            )));
        }
        let url = match Url::parse(raw_url) {
            Ok(u) => u,
            Err(e) => return Ok(Err(format!("unreadable download link: {e}"))),
        };
        let mut resp = match self.send(&url, DOWNLOAD_TIMEOUT, cancel).await {
            Ok(r) => r,
            Err(SourceError::Cancelled) => return Err(SourceError::Cancelled),
            Err(e) => return Ok(Err(e.to_string())),
        };
        let status = resp.status();
        if (status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN)
            && self.auth.is_none()
        {
            return Ok(Err(
                "needs sign-in; re-run with a Zendesk credential".to_string()
            ));
        }
        if !status.is_success() {
            return Ok(Err(format!("download answered {status}")));
        }
        if resp.content_length().is_some_and(|len| len > cap) {
            return Ok(Err(format!(
                "larger than the {} MB limit",
                cap / (1024 * 1024)
            )));
        }

        let part = dest.with_extension("part");
        let outcome = stream_to(&mut resp, &part, cap, cancel).await;
        match outcome {
            Ok(Ok(len)) => match tokio::fs::rename(&part, dest).await {
                Ok(()) => Ok(Ok(len)),
                Err(e) => {
                    let _ = tokio::fs::remove_file(&part).await;
                    Ok(Err(format!("could not store the file: {e}")))
                }
            },
            other => {
                let _ = tokio::fs::remove_file(&part).await;
                other
            }
        }
    }
}

async fn stream_to(
    resp: &mut Response,
    part: &Path,
    cap: u64,
    cancel: &CancellationToken,
) -> Result<Result<u64, Missing>, SourceError> {
    let mut file = match tokio::fs::File::create(part).await {
        Ok(f) => f,
        Err(e) => return Ok(Err(format!("could not store the file: {e}"))),
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
                    return Ok(Err(format!(
                        "larger than the {} MB limit",
                        cap / (1024 * 1024)
                    )));
                }
                if let Err(e) = file.write_all(&bytes).await {
                    return Ok(Err(format!("could not store the file: {e}")));
                }
            }
            Ok(None) => break,
            Err(e) => return Ok(Err(format!("download interrupted: {e}"))),
        }
    }
    if let Err(e) = file.flush().await {
        return Ok(Err(format!("could not store the file: {e}")));
    }
    if let Err(e) = file.sync_all().await {
        return Ok(Err(format!("could not store the file: {e}")));
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
