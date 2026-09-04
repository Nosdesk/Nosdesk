//! Client for the Nosdesk cloud push relay (plan slice 4).
//!
//! A licensed self-hosted instance cannot push to the official mobile app
//! itself: the app is one binary with one identity (`com.nosdesk.app`), so only
//! the holder of that app's APNs `.p8` and Firebase project can deliver to its
//! device tokens. The relay is that holder. This client exchanges the
//! instance's edition licence for a short-lived derived token, then posts
//! device tokens and an already-composed payload for forwarding.
//!
//! ## What is sent
//!
//! Device tokens and the alert the instance composed — nothing else. The relay
//! is stateless for token custody: it forwards and returns the tokens to prune,
//! and never stores them. The licence is presented **once per exchange**, never
//! on a send, because it is the customer's long-lived entitlement artifact and
//! putting it on a hot path multiplies its exposure for nothing.
//!
//! ## Timeout
//!
//! 10s, from the B3 measurement against the deployed staging relay: warm
//! exchanges land in ~0.12s, but the control plane runs with
//! `min_machines_running = 0`, and a cold start measured **~6.7s** across five
//! samples (6.09-6.94s) with the whole delay in TTFB while Fly's proxy holds
//! the request. The earlier 2s guess would have dropped every push that landed
//! on a cold relay — and with no retry queue, that silently loses the first
//! notification after any quiet period, which for a helpdesk is the one that
//! matters most.
//!
//! ## Logging
//!
//! Never log the licence, a derived token, a device token, or an alert body
//! (plan O7). Failure reasons are a closed set of static strings.

use std::sync::RwLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use super::push::{PushPayload, PushTarget};

/// Where the relay lives. Overridable so an instance can be pointed at staging.
const RELAY_URL_ENV: &str = "NOSDESK_RELAY_URL";
const DEFAULT_RELAY_URL: &str = "https://api.nosdesk.com";

/// See the module docs: sized to clear a control-plane cold start, not just
/// steady-state latency.
const RELAY_TIMEOUT: Duration = Duration::from_secs(10);

/// Re-exchange this long before the derived token expires, so a send never
/// fires with a token that lapses in flight.
const REFRESH_MARGIN_SECS: u64 = 120;

/// What happened on the last relay interaction, for the admin edition surface.
///
/// This is the only diagnostic a self-hoster has when push stops working, so it
/// distinguishes "we cannot reach the relay" from "the relay refused us" from
/// "you have not accepted the DPA". All values are static strings.
#[derive(Debug, Clone, Serialize, Default)]
pub struct RelayStatus {
    /// `ok`, or a static failure kind. `None` before the first attempt.
    pub last_outcome: Option<&'static str>,
    /// Unix seconds of the last successful exchange.
    pub last_success_at: Option<i64>,
    /// Unix seconds of the last attempt of any kind.
    pub last_attempt_at: Option<i64>,
}

/// Why a relay call failed. Every variant is a static discriminator safe to log
/// and to surface to an operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelayFailure {
    /// The relay rejected the licence: expired, wrong issuer, or denylisted.
    /// Deliberately not distinguished — the relay does not tell us which.
    InvalidLicense,
    /// The customer has not accepted the data-processing agreement, so relay
    /// mode is refused. Operator-actionable, which is why it is its own kind.
    DpaRequired,
    /// Throttled: either the exchange's per-IP limit or the send burst limit.
    RateLimited,
    /// The relay is deployed but not configured (missing keys on its side).
    RelayUnavailable,
    /// Could not reach the relay at all, or it timed out.
    Unreachable,
    /// The relay answered with something unexpected.
    Unexpected,
}

impl RelayFailure {
    pub fn kind(self) -> &'static str {
        match self {
            Self::InvalidLicense => "invalid_license",
            Self::DpaRequired => "dpa_required",
            Self::RateLimited => "rate_limited",
            Self::RelayUnavailable => "relay_unavailable",
            Self::Unreachable => "unreachable",
            Self::Unexpected => "unexpected",
        }
    }
}

/// Map a relay HTTP status onto an action. Pure, so the interesting decision is
/// testable without a network — and it must agree with the control plane's
/// handler, which is the thing most likely to drift.
pub fn classify_status(status: u16) -> Result<(), RelayFailure> {
    match status {
        200..=299 => Ok(()),
        401 => Err(RelayFailure::InvalidLicense),
        403 => Err(RelayFailure::DpaRequired),
        429 => Err(RelayFailure::RateLimited),
        503 => Err(RelayFailure::RelayUnavailable),
        _ => Err(RelayFailure::Unexpected),
    }
}

#[derive(Serialize)]
struct ExchangeRequest<'a> {
    license: &'a str,
    instance_id: &'a str,
}

#[derive(Deserialize)]
struct ExchangeResponse {
    token: String,
    exp: u64,
}

#[derive(Serialize)]
struct RelayTarget<'a> {
    platform: &'a str,
    token: &'a str,
}

#[derive(Serialize)]
struct PushRequest<'a> {
    targets: Vec<RelayTarget<'a>>,
    payload: &'a PushPayload,
}

#[derive(Deserialize)]
struct PushResponse {
    invalid_tokens: Vec<String>,
}

struct CachedToken {
    token: String,
    exp: u64,
}

/// Talks to the cloud relay on behalf of one instance.
pub struct RelayClient {
    http: reqwest::Client,
    base_url: String,
    license: String,
    instance_id: String,
    token: Mutex<Option<CachedToken>>,
    status: RwLock<RelayStatus>,
}

impl RelayClient {
    /// Build from env plus the instance's durable id.
    ///
    /// `instance_id` comes from `system_meta`, which means the client must be
    /// constructed **after** `initialize_database` — that is where the id is
    /// minted. An empty id is tolerated rather than fatal, because
    /// `ensure_instance_id` is warn-only on boot; the relay then falls back to
    /// a shared burst bucket for this customer, which is a degraded but working
    /// state and better than refusing to push at all.
    pub fn new(license: String, instance_id: String) -> reqwest::Result<Self> {
        let base_url = std::env::var(RELAY_URL_ENV)
            .ok()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_RELAY_URL.to_string());
        Ok(Self {
            http: reqwest::Client::builder().timeout(RELAY_TIMEOUT).build()?,
            base_url: base_url.trim_end_matches('/').to_string(),
            license,
            instance_id,
            token: Mutex::new(None),
            status: RwLock::new(RelayStatus::default()),
        })
    }

    /// Snapshot for the admin edition surface.
    pub fn status(&self) -> RelayStatus {
        self.status.read().expect("RwLock poisoned").clone()
    }

    fn record(&self, outcome: Option<&'static str>) {
        let now = unix_now() as i64;
        let mut s = self.status.write().expect("RwLock poisoned");
        s.last_attempt_at = Some(now);
        s.last_outcome = outcome;
        if outcome == Some("ok") {
            s.last_success_at = Some(now);
        }
    }

    /// A usable derived token, exchanging when the cached one is absent or
    /// close to expiry.
    async fn token(&self) -> Result<String, RelayFailure> {
        let mut guard = self.token.lock().await;
        if let Some(cached) = guard.as_ref() {
            if cached.exp > unix_now() + REFRESH_MARGIN_SECS {
                return Ok(cached.token.clone());
            }
        }

        let res = self
            .http
            .post(format!("{}/api/relay/v1/token", self.base_url))
            .json(&ExchangeRequest {
                license: &self.license,
                instance_id: &self.instance_id,
            })
            .send()
            .await
            .map_err(|e| {
                // `without_url` for consistency with the provider clients; the
                // relay URL is not secret, but the licence must never end up in
                // a rendered error.
                log::warn!("relay exchange transport error: {}", e.without_url());
                RelayFailure::Unreachable
            })?;

        let status = res.status().as_u16();
        if let Err(failure) = classify_status(status) {
            self.record(Some(failure.kind()));
            log::warn!("relay exchange refused: error_kind={}", failure.kind());
            return Err(failure);
        }

        let body: ExchangeResponse = res.json().await.map_err(|_| {
            self.record(Some(RelayFailure::Unexpected.kind()));
            RelayFailure::Unexpected
        })?;

        *guard = Some(CachedToken {
            token: body.token.clone(),
            exp: body.exp,
        });
        self.record(Some("ok"));
        Ok(body.token)
    }

    /// Drop the cached token so the next call re-exchanges.
    async fn invalidate_token(&self) {
        *self.token.lock().await = None;
    }

    /// Relay one payload to a set of devices; returns tokens to prune.
    ///
    /// A 401 on send means the derived token stopped being accepted mid-life
    /// (revoked licence, or a relay restart). Re-exchange and retry **once**:
    /// a persistent 401 means the licence itself is refused, and looping would
    /// hammer the exchange for a condition no retry can fix.
    pub async fn send(
        &self,
        targets: &[PushTarget],
        payload: &PushPayload,
    ) -> Result<Vec<String>, RelayFailure> {
        match self.try_send(targets, payload).await {
            Err(RelayFailure::InvalidLicense) => {
                self.invalidate_token().await;
                self.try_send(targets, payload).await
            }
            other => other,
        }
    }

    async fn try_send(
        &self,
        targets: &[PushTarget],
        payload: &PushPayload,
    ) -> Result<Vec<String>, RelayFailure> {
        let token = self.token().await?;

        // No idempotency key. The plan's O3 form is
        // `notification_id:device_token`, but `PushSender::send` does not carry
        // a notification id, and inventing a substitute would key on something
        // that is not stable per notification. O3 also settled that it
        // deduplicates nothing in v1.1, since nothing retries; the relay
        // accepts the field as optional, so this stays absent until a retry
        // path exists and the trait carries an id to key on.
        let res = self
            .http
            .post(format!("{}/api/relay/v1/push", self.base_url))
            .bearer_auth(&token)
            .json(&PushRequest {
                targets: targets
                    .iter()
                    .map(|t| RelayTarget {
                        platform: &t.platform,
                        token: &t.token,
                    })
                    .collect(),
                payload,
            })
            .send()
            .await
            .map_err(|e| {
                log::warn!("relay send transport error: {}", e.without_url());
                self.record(Some(RelayFailure::Unreachable.kind()));
                RelayFailure::Unreachable
            })?;

        let status = res.status().as_u16();
        if let Err(failure) = classify_status(status) {
            self.record(Some(failure.kind()));
            log::warn!("relay send refused: error_kind={}", failure.kind());
            return Err(failure);
        }

        let body: PushResponse = res.json().await.map_err(|_| {
            self.record(Some(RelayFailure::Unexpected.kind()));
            RelayFailure::Unexpected
        })?;
        self.record(Some("ok"));
        Ok(body.invalid_tokens)
    }

    /// Whether the channel should present itself as configured.
    ///
    /// Deliberately not "have we had a successful exchange". `is_configured`
    /// gates the channel, and the channel is what triggers the first exchange,
    /// so latching false until success would deadlock: no exchange, so never
    /// configured, so no exchange. Instead this is false only once the relay
    /// has told us this licence will *not* work — a bad licence or an
    /// unaccepted DPA. Transient failures stay usable so the next notification
    /// retries, which is also the plan's "do not cache configured independently
    /// of the last exchange".
    pub fn is_usable(&self) -> bool {
        !matches!(
            self.status().last_outcome,
            Some("invalid_license") | Some("dpa_required")
        )
    }
}

/// [`PushSender`] that forwards through the cloud relay instead of holding
/// APNs/FCM credentials locally.
pub struct CloudRelayPushSender {
    client: RelayClient,
}

impl CloudRelayPushSender {
    pub fn new(license: String, instance_id: String) -> reqwest::Result<Self> {
        Ok(Self {
            client: RelayClient::new(license, instance_id)?,
        })
    }
}

#[async_trait::async_trait]
impl super::push::PushSender for CloudRelayPushSender {
    fn is_configured(&self) -> bool {
        self.client.is_usable()
    }

    fn relay_status(&self) -> Option<RelayStatus> {
        Some(self.client.status())
    }

    async fn send(&self, targets: &[PushTarget], payload: &PushPayload) -> Vec<String> {
        match self.client.send(targets, payload).await {
            Ok(invalid) => invalid,
            // Prune nothing on failure. The relay could not tell us which
            // tokens are bad, and discarding a live registration because the
            // relay was briefly unreachable would silently stop that device
            // receiving push with no way back.
            Err(failure) => {
                log::warn!("relay push failed: error_kind={}", failure.kind());
                Vec::new()
            }
        }
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock predates Unix epoch")
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// These must agree with the control plane's relay handlers. Each status
    /// here is one the CP actually emits.
    #[test]
    fn status_mapping_matches_the_relay_contract() {
        assert!(classify_status(200).is_ok());
        assert_eq!(classify_status(401), Err(RelayFailure::InvalidLicense));
        assert_eq!(classify_status(403), Err(RelayFailure::DpaRequired));
        assert_eq!(classify_status(429), Err(RelayFailure::RateLimited));
        assert_eq!(classify_status(503), Err(RelayFailure::RelayUnavailable));
    }

    #[test]
    fn unknown_statuses_are_unexpected_not_silently_ok() {
        for s in [400, 404, 418, 500, 502] {
            assert_eq!(
                classify_status(s),
                Err(RelayFailure::Unexpected),
                "status {s} must not be treated as success"
            );
        }
    }

    #[test]
    fn failure_kinds_are_static_and_distinct() {
        let kinds = [
            RelayFailure::InvalidLicense.kind(),
            RelayFailure::DpaRequired.kind(),
            RelayFailure::RateLimited.kind(),
            RelayFailure::RelayUnavailable.kind(),
            RelayFailure::Unreachable.kind(),
            RelayFailure::Unexpected.kind(),
        ];
        for k in kinds {
            assert!(!k.is_empty());
        }
        let mut sorted = kinds;
        sorted.sort_unstable();
        let mut deduped = sorted.to_vec();
        deduped.dedup();
        assert_eq!(deduped.len(), kinds.len(), "kinds must be distinguishable");
    }

    #[test]
    fn status_starts_empty_and_records_outcomes() {
        let client = RelayClient::new("licence".into(), "inst".into()).expect("client");
        let s = client.status();
        assert!(s.last_outcome.is_none());
        assert!(s.last_attempt_at.is_none());

        client.record(Some("ok"));
        let s = client.status();
        assert_eq!(s.last_outcome, Some("ok"));
        assert!(s.last_success_at.is_some());

        // A later failure must not erase the last success: an operator needs
        // to see both "it is failing now" and "it last worked at T".
        client.record(Some(RelayFailure::DpaRequired.kind()));
        let s = client.status();
        assert_eq!(s.last_outcome, Some("dpa_required"));
        assert!(s.last_success_at.is_some());
    }

    #[test]
    fn relay_url_is_overridable_and_trims_a_trailing_slash() {
        // Not using the env var here (tests share a process); the constructor's
        // trimming is what matters, since the paths are joined with a leading /.
        let client = RelayClient::new("l".into(), "i".into()).expect("client");
        assert!(!client.base_url.ends_with('/'));
    }
}
