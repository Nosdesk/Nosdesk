//! Native push senders — APNs (token auth) + FCM HTTP v1.
//!
//! Concrete [`PushSender`] implementation using only existing deps: `reqwest`
//! (HTTP/2, required by APNs) and `jsonwebtoken` (ES256 for the APNs bearer,
//! RS256 for the FCM service-account OAuth grant). No provider SDK crates, so
//! nothing new to vet in `deny.toml`.
//!
//! Construction is env-gated via [`NativePushSender::from_env`], which returns
//! `None` when neither provider is configured — the composition root then keeps
//! the inert `NoopPushSender`, so an un-provisioned deploy is unchanged and push
//! preferences stay visible-but-inert (see `channels/push.rs`).
//!
//! **Privacy:** request bodies carry only the generic title + entity refs from
//! [`PushPayload`] — never ticket subject/body. The device fetches real content
//! after the tap, so Apple/Google never see customer data.
//!
//! **Runtime-unverified:** wired against the documented APNs/FCM contracts but
//! not yet exercised against live credentials + a real device. First live test
//! happens with Step 5 (mobile token registration).

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

use super::push::{PushPayload, PushSender, PushTarget};

/// Outcome of a single-device send — drives token pruning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SendOutcome {
    /// Accepted by the provider.
    Sent,
    /// Provider reports the token is permanently invalid (unregistered / wrong
    /// topic / malformed) — the channel revokes it.
    Invalid,
    /// Transient or configuration failure — logged, token kept.
    Failed,
}

/// Read an env var as a non-empty trimmed string, or `None`.
/// Total per-request budget for a provider call, and the slice of it a
/// connection may take. Sends are sequential per target and per channel, so
/// without an explicit timeout one hung connection stalls the whole
/// notification: reqwest sets no default. Five seconds is generous for an
/// HTTP/2 POST to APNs or FCM, which normally answer well inside a second.
const PROVIDER_REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const PROVIDER_CONNECT_TIMEOUT: Duration = Duration::from_secs(3);

/// HTTP client for a push provider, with the timeouts above applied.
fn provider_http_client(provider: &str) -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(PROVIDER_REQUEST_TIMEOUT)
        .connect_timeout(PROVIDER_CONNECT_TIMEOUT)
        .build()
        .with_context(|| format!("building the {provider} HTTP client"))
}

fn env_nonempty(key: &str) -> Option<String> {
    match std::env::var(key) {
        Ok(v) if !v.trim().is_empty() => Some(v),
        _ => None,
    }
}

/// Resolve credential material provided either inline (`*_KEY`) or as a file
/// path (`*_PATH`). Inline wins. Returns `None` when neither is set.
fn env_material(inline_key: &str, path_key: &str) -> Result<Option<String>> {
    if let Some(inline) = env_nonempty(inline_key) {
        return Ok(Some(inline));
    }
    if let Some(path) = env_nonempty(path_key) {
        let contents = std::fs::read_to_string(&path)
            .with_context(|| format!("{path_key} points at {path} which could not be read"))?;
        return Ok(Some(contents));
    }
    Ok(None)
}

// ---------------------------------------------------------------------------
// APNs
// ---------------------------------------------------------------------------

/// A cached APNs bearer JWT. Apple accepts a provider token for up to 60 min
/// and rejects one older than ~60 min; we refresh well inside that window.
struct CachedJwt {
    token: String,
    minted: Instant,
}

/// APNs reasons that mean the device token will never work again.
///
/// Deliberately a closed set matched against the parsed `reason` field, not a
/// substring search of the body. Pruning on an unrecognised reason discards a
/// live registration, and a device that has been silently deregistered has no
/// way back short of the user reinstalling.
const APNS_PRUNABLE_REASONS: &[&str] =
    &["BadDeviceToken", "DeviceTokenNotForTopic", "Unregistered"];

/// Classify an APNs response.
///
/// 410 means the token is no longer active for this topic. A 400 is prunable
/// only when the reason names the token; a 400 for a malformed payload must not
/// cost the recipient their registration. 403 is handled by the caller, which
/// re-mints the bearer: it indicts our credentials, not the device.
fn classify_apns(status: u16, body: &str) -> SendOutcome {
    if (200..300).contains(&status) {
        return SendOutcome::Sent;
    }
    if status == 410 {
        return SendOutcome::Invalid;
    }
    if status == 400 {
        let reason = serde_json::from_str::<Value>(body)
            .ok()
            .and_then(|v| v.get("reason").and_then(Value::as_str).map(str::to_owned));
        if let Some(reason) = reason {
            if APNS_PRUNABLE_REASONS.contains(&reason.as_str()) {
                return SendOutcome::Invalid;
            }
        }
    }
    SendOutcome::Failed
}

/// The provider's machine-readable reason code, bounded for logging.
///
/// The raw response body must not reach the log pipeline. It is third-party
/// text of unbounded shape, and an FCM error echoes the device token back
/// inside `error.message`, so the field that would tell you *why* a send failed
/// would also ship the credential that identifies the handset. This
/// lifts just the code (APNs' `reason`, FCM's `error.status`) and only when it
/// still looks like the enum constant it is meant to be, which keeps the field
/// a closed set in practice without hardcoding a list that Apple and Google
/// extend without asking.
fn provider_reason(body: &str) -> String {
    let Ok(v) = serde_json::from_str::<Value>(body) else {
        return "unparsed".to_owned();
    };
    let code = v
        .get("reason")
        .and_then(Value::as_str)
        .or_else(|| v.pointer("/error/status").and_then(Value::as_str));
    match code {
        Some(c)
            if !c.is_empty()
                && c.len() <= 48
                && c.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') =>
        {
            c.to_owned()
        }
        Some(_) => "unrecognised".to_owned(),
        None => "unparsed".to_owned(),
    }
}

/// Classify an FCM v1 response.
///
/// FCM returns `INVALID_ARGUMENT` for a bad token **and** for a malformed
/// message, so the previous substring search for that string anywhere in the
/// body pruned good tokens whenever our own payload was wrong — and because
/// device tokens load across workspaces, one payload bug silently deregistered
/// every device that recipient owned.
///
/// A 404 means the registration is gone. A 400 is prunable only when the error
/// explicitly names the token: an `UNREGISTERED` detail, a `NOT_FOUND` status,
/// or a field violation on the token field.
fn classify_fcm(status: u16, body: &str) -> SendOutcome {
    if (200..300).contains(&status) {
        return SendOutcome::Sent;
    }
    if status == 404 {
        return SendOutcome::Invalid;
    }
    if status == 400 && fcm_error_blames_token(body) {
        return SendOutcome::Invalid;
    }
    SendOutcome::Failed
}

/// Whether an FCM error body attributes the failure to the device token itself.
fn fcm_error_blames_token(body: &str) -> bool {
    let Ok(v) = serde_json::from_str::<Value>(body) else {
        return false;
    };
    let Some(error) = v.get("error") else {
        return false;
    };

    if error.get("status").and_then(Value::as_str) == Some("NOT_FOUND") {
        return true;
    }

    let Some(details) = error.get("details").and_then(Value::as_array) else {
        return false;
    };
    details.iter().any(|d| {
        if d.get("errorCode").and_then(Value::as_str) == Some("UNREGISTERED") {
            return true;
        }
        d.get("fieldViolations")
            .and_then(Value::as_array)
            .is_some_and(|violations| {
                violations.iter().any(|fv| {
                    fv.get("field")
                        .and_then(Value::as_str)
                        .is_some_and(|f| f.ends_with("token"))
                })
            })
    })
}

/// APNs token-based sender. One JWT (ES256, signed by the `.p8` auth key) is
/// reused across sends and refreshed every ~50 min.
struct ApnsClient {
    http: reqwest::Client,
    encoding_key: EncodingKey,
    key_id: String,
    team_id: String,
    /// The app bundle id, sent as `apns-topic`.
    topic: String,
    /// Production vs sandbox APNs host.
    production: bool,
    jwt: Mutex<Option<CachedJwt>>,
}

/// APNs JWT claims (`iss` = team id, `iat` = issued-at).
#[derive(Serialize)]
struct ApnsClaims {
    iss: String,
    iat: i64,
}

impl ApnsClient {
    /// Build from env. Gated on `NOSDESK_APNS_KEY_ID`; once that's set the key
    /// material + team id + topic are required (partial config is an error, so a
    /// half-provisioned deploy fails loudly rather than silently not sending).
    fn from_env() -> Result<Option<Self>> {
        let Some(key_id) = env_nonempty("NOSDESK_APNS_KEY_ID") else {
            return Ok(None);
        };
        let key_pem =
            env_material("NOSDESK_APNS_KEY_P8", "NOSDESK_APNS_KEY_PATH")?.ok_or_else(|| {
                anyhow!("NOSDESK_APNS_KEY_ID set but NOSDESK_APNS_KEY_P8/PATH missing")
            })?;
        let team_id = env_nonempty("NOSDESK_APNS_TEAM_ID")
            .ok_or_else(|| anyhow!("NOSDESK_APNS_KEY_ID set but NOSDESK_APNS_TEAM_ID missing"))?;
        let topic = env_nonempty("NOSDESK_APNS_TOPIC")
            .ok_or_else(|| anyhow!("NOSDESK_APNS_KEY_ID set but NOSDESK_APNS_TOPIC missing"))?;
        // Default to production; only sandbox when explicitly opted in.
        let production = !matches!(
            env_nonempty("NOSDESK_APNS_PRODUCTION").as_deref(),
            Some("false") | Some("0") | Some("no")
        );

        // Name the environment at startup. Which Apple environment we talk to
        // decides whether a given device token can ever work, and a mismatch
        // presents as push that works once and then never again: sandbox
        // answers a production token with BadDeviceToken, which is correctly
        // classified permanently invalid, so the registration is pruned and the
        // evidence deletes itself. The relay logs this; the product did not.
        info!(
            environment = if production { "production" } else { "sandbox" },
            "APNs configured"
        );

        let encoding_key = EncodingKey::from_ec_pem(key_pem.as_bytes())
            .context("NOSDESK_APNS_KEY_P8 is not a valid EC private key (.p8 PEM)")?;

        Ok(Some(Self {
            http: provider_http_client("APNs")?,
            encoding_key,
            key_id,
            team_id,
            topic,
            production,
            jwt: Mutex::new(None),
        }))
    }

    /// The APNs host for the configured environment.
    fn host(&self) -> &'static str {
        if self.production {
            "api.push.apple.com"
        } else {
            "api.sandbox.push.apple.com"
        }
    }

    /// A valid bearer JWT, minting (and caching) a fresh one when the cached
    /// token is older than 50 minutes.
    async fn bearer(&self) -> Result<String> {
        let mut guard = self.jwt.lock().await;
        if let Some(cached) = guard.as_ref() {
            if cached.minted.elapsed() < Duration::from_secs(50 * 60) {
                return Ok(cached.token.clone());
            }
        }
        let mut header = Header::new(Algorithm::ES256);
        header.kid = Some(self.key_id.clone());
        let claims = ApnsClaims {
            iss: self.team_id.clone(),
            iat: chrono::Utc::now().timestamp(),
        };
        let token = encode(&header, &claims, &self.encoding_key)
            .context("failed to sign APNs provider JWT")?;
        *guard = Some(CachedJwt {
            token: token.clone(),
            minted: Instant::now(),
        });
        Ok(token)
    }

    /// Drop the cached provider JWT so the next send mints a fresh one.
    /// Without this a bearer rejected by Apple stays cached until the
    /// 50-minute age check expires, failing every send in between.
    async fn invalidate_bearer(&self) {
        *self.jwt.lock().await = None;
    }

    async fn send_one(&self, device_token: &str, payload: &PushPayload) -> SendOutcome {
        let bearer = match self.bearer().await {
            Ok(b) => b,
            Err(e) => {
                warn!(error = %e, "apns: could not mint provider JWT");
                return SendOutcome::Failed;
            }
        };

        // `alert.body` is present only in the workspace's `detailed` mode. Lock-
        // screen privacy relies on the OS "Show Previews: When Unlocked" default
        // (there is no server-side hide-on-lock flag for a standard iOS alert).
        let mut alert = serde_json::Map::new();
        alert.insert("title".into(), json!(payload.title));
        if let Some(body_text) = &payload.body {
            alert.insert("body".into(), json!(body_text));
        }
        let body = json!({
            "aps": { "alert": alert, "sound": "default" },
            "nd_type": payload.notification_type,
            "entity_type": payload.entity_type,
            "entity_id": payload.entity_id,
            "ticket_id": payload.ticket_id,
        });

        let url = format!("https://{}/3/device/{}", self.host(), device_token);
        let resp = self
            .http
            .post(&url)
            .header("authorization", format!("bearer {bearer}"))
            .header("apns-topic", &self.topic)
            .header("apns-push-type", "alert")
            .header("apns-priority", "10")
            .json(&body)
            .send()
            .await;

        match resp {
            Ok(r) => {
                let status = r.status();
                if status.is_success() {
                    return SendOutcome::Sent;
                }
                let reason = r.text().await.unwrap_or_default();
                // 403 is ExpiredProviderToken / InvalidProviderToken: the fault
                // is the cached bearer, not the device token. Drop it so the
                // next send re-mints rather than repeating the failure.
                if status.as_u16() == 403 {
                    self.invalidate_bearer().await;
                    warn!(
                        %status,
                        provider_reason = %provider_reason(&reason),
                        "apns: provider token rejected, dropped the cached bearer"
                    );
                    return SendOutcome::Failed;
                }
                match classify_apns(status.as_u16(), &reason) {
                    SendOutcome::Invalid => {
                        debug!(%status, "apns: pruning invalid device token");
                        SendOutcome::Invalid
                    }
                    outcome => {
                        warn!(%status, provider_reason = %provider_reason(&reason), "apns: send failed");
                        outcome
                    }
                }
            }
            Err(e) => {
                // The device token is a path segment of the request URL, and
                // reqwest's Error Display renders the URL, so logging `e`
                // directly would print the token. reqwest attaches the URL to
                // timeout errors too, which is why this guard belongs with the
                // timeouts configured above, not after them.
                warn!(error = %e.without_url(), "apns: request error");
                SendOutcome::Failed
            }
        }
    }
}

// ---------------------------------------------------------------------------
// FCM (HTTP v1)
// ---------------------------------------------------------------------------

/// The subset of a Google service-account JSON we need to mint OAuth tokens.
#[derive(Deserialize)]
struct ServiceAccount {
    client_email: String,
    private_key: String,
    #[serde(default = "default_token_uri")]
    token_uri: String,
    project_id: String,
}

fn default_token_uri() -> String {
    "https://oauth2.googleapis.com/token".to_string()
}

/// A cached FCM OAuth access token with its computed expiry.
struct CachedToken {
    token: String,
    expires_at: Instant,
}

/// Google OAuth JWT-bearer assertion claims.
#[derive(Serialize)]
struct GoogleClaims<'a> {
    iss: &'a str,
    scope: &'a str,
    aud: &'a str,
    iat: i64,
    exp: i64,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: i64,
}

/// FCM HTTP v1 sender. Mints a short-lived OAuth access token from the service
/// account (RS256 JWT → token endpoint) and reuses it until near expiry.
struct FcmClient {
    http: reqwest::Client,
    encoding_key: EncodingKey,
    client_email: String,
    token_uri: String,
    project_id: String,
    token: Mutex<Option<CachedToken>>,
}

impl FcmClient {
    /// Build from env. Gated on the service-account JSON
    /// (`NOSDESK_FCM_SERVICE_ACCOUNT` inline or `..._PATH`); `project_id` comes
    /// from the JSON but `NOSDESK_FCM_PROJECT_ID` overrides it.
    fn from_env() -> Result<Option<Self>> {
        let Some(json_str) = env_material(
            "NOSDESK_FCM_SERVICE_ACCOUNT",
            "NOSDESK_FCM_SERVICE_ACCOUNT_PATH",
        )?
        else {
            return Ok(None);
        };
        let sa: ServiceAccount = serde_json::from_str(&json_str)
            .context("NOSDESK_FCM_SERVICE_ACCOUNT is not valid service-account JSON")?;
        let encoding_key = EncodingKey::from_rsa_pem(sa.private_key.as_bytes())
            .context("service-account private_key is not a valid RSA PEM")?;
        let project_id = env_nonempty("NOSDESK_FCM_PROJECT_ID").unwrap_or(sa.project_id);

        Ok(Some(Self {
            http: provider_http_client("FCM")?,
            encoding_key,
            client_email: sa.client_email,
            token_uri: sa.token_uri,
            project_id,
            token: Mutex::new(None),
        }))
    }

    /// A valid OAuth access token, refreshing when the cached one is within
    /// 60s of expiry.
    async fn access_token(&self) -> Result<String> {
        let mut guard = self.token.lock().await;
        if let Some(cached) = guard.as_ref() {
            if cached.expires_at > Instant::now() + Duration::from_secs(60) {
                return Ok(cached.token.clone());
            }
        }

        let now = chrono::Utc::now().timestamp();
        let claims = GoogleClaims {
            iss: &self.client_email,
            scope: "https://www.googleapis.com/auth/firebase.messaging",
            aud: &self.token_uri,
            iat: now,
            exp: now + 3600,
        };
        let assertion = encode(&Header::new(Algorithm::RS256), &claims, &self.encoding_key)
            .context("failed to sign FCM OAuth assertion")?;

        let resp: TokenResponse = self
            .http
            .post(&self.token_uri)
            .form(&[
                ("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"),
                ("assertion", &assertion),
            ])
            .send()
            .await
            .context("FCM token request failed")?
            .error_for_status()
            .context("FCM token endpoint returned an error")?
            .json()
            .await
            .context("FCM token response was not JSON")?;

        let expires_at = Instant::now() + Duration::from_secs(resp.expires_in.max(0) as u64);
        *guard = Some(CachedToken {
            token: resp.access_token.clone(),
            expires_at,
        });
        Ok(resp.access_token)
    }

    async fn send_one(&self, device_token: &str, payload: &PushPayload) -> SendOutcome {
        let access = match self.access_token().await {
            Ok(t) => t,
            Err(e) => {
                warn!(error = %e, "fcm: could not obtain access token");
                return SendOutcome::Failed;
            }
        };

        // `notification.body` only in the workspace's `detailed` mode. FCM data
        // values must be strings. `visibility: PRIVATE` hides the content on the
        // Android secure lock screen (a redacted public version shows instead).
        let mut notif = serde_json::Map::new();
        notif.insert("title".into(), json!(payload.title));
        if let Some(body_text) = &payload.body {
            notif.insert("body".into(), json!(body_text));
        }
        let body = json!({
            "message": {
                "token": device_token,
                "notification": notif,
                "android": { "notification": { "visibility": "PRIVATE" } },
                "data": {
                    "nd_type": payload.notification_type,
                    "entity_type": payload.entity_type,
                    "entity_id": payload.entity_id.to_string(),
                    "ticket_id": payload.ticket_id.to_string(),
                }
            }
        });

        let url = format!(
            "https://fcm.googleapis.com/v1/projects/{}/messages:send",
            self.project_id
        );
        let resp = self
            .http
            .post(&url)
            .bearer_auth(access)
            .json(&body)
            .send()
            .await;

        match resp {
            Ok(r) => {
                let status = r.status();
                if status.is_success() {
                    return SendOutcome::Sent;
                }
                let reason = r.text().await.unwrap_or_default();
                match classify_fcm(status.as_u16(), &reason) {
                    SendOutcome::Invalid => {
                        debug!(%status, "fcm: pruning invalid device token");
                        SendOutcome::Invalid
                    }
                    outcome => {
                        warn!(%status, provider_reason = %provider_reason(&reason), "fcm: send failed");
                        outcome
                    }
                }
            }
            Err(e) => {
                warn!(error = %e, "fcm: request error");
                SendOutcome::Failed
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Combined sender
// ---------------------------------------------------------------------------

/// The concrete push sender: routes each target to APNs (ios) or FCM
/// (android/web), whichever is configured.
pub struct NativePushSender {
    apns: Option<ApnsClient>,
    fcm: Option<FcmClient>,
}

impl NativePushSender {
    /// Build from env. Returns `Some` when at least one provider is configured,
    /// `None` when neither is — the caller keeps the inert sender in that case.
    /// A configured-but-malformed provider is a hard error (fail loudly).
    pub fn from_env() -> Result<Option<Arc<dyn PushSender>>> {
        let apns = ApnsClient::from_env()?;
        let fcm = FcmClient::from_env()?;
        if apns.is_none() && fcm.is_none() {
            return Ok(None);
        }
        info!(
            apns = apns.is_some(),
            fcm = fcm.is_some(),
            "push: native sender configured"
        );
        Ok(Some(Arc::new(Self { apns, fcm })))
    }
}

#[async_trait::async_trait]
impl PushSender for NativePushSender {
    fn name(&self) -> &'static str {
        "native"
    }

    fn is_configured(&self) -> bool {
        self.apns.is_some() || self.fcm.is_some()
    }

    async fn send(&self, targets: &[PushTarget], payload: &PushPayload) -> Vec<String> {
        let mut invalid = Vec::new();
        // Per-platform tallies, matching what the relay reports. Without them a
        // successful native send logs nothing at all, so "did the iPhone get
        // it" is unanswerable from the server side -- and an aggregate count
        // cannot answer it either when one platform works and another does not.
        // Platform is a closed set, not personal data, so it is safe to log.
        let (mut sent_ios, mut sent_android) = (0usize, 0usize);
        let (mut failed, mut invalid_ios, mut invalid_android) = (0usize, 0usize, 0usize);

        for target in targets {
            let is_ios = target.platform == "ios";
            let outcome = match target.platform.as_str() {
                "ios" => match &self.apns {
                    Some(client) => client.send_one(&target.token, payload).await,
                    None => continue,
                },
                "android" | "web" => match &self.fcm {
                    Some(client) => client.send_one(&target.token, payload).await,
                    None => continue,
                },
                other => {
                    warn!(platform = %other, "push: unknown device platform, skipping");
                    continue;
                }
            };
            match outcome {
                SendOutcome::Sent if is_ios => sent_ios += 1,
                SendOutcome::Sent => sent_android += 1,
                SendOutcome::Failed => failed += 1,
                SendOutcome::Invalid => {
                    invalid.push(target.token.clone());
                    if is_ios {
                        invalid_ios += 1;
                    } else {
                        invalid_android += 1;
                    }
                }
            }
        }

        info!(
            sent_ios,
            sent_android,
            failed,
            invalid_ios,
            invalid_android,
            targets = targets.len(),
            "Push dispatched"
        );
        invalid
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Throwaway keys generated solely for these tests — not credentials.
    const TEST_EC_P8: &str = "-----BEGIN PRIVATE KEY-----\nMIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQgC68iXio5oruAH0in\nOolSIAyHVqI4alEcCA9rX86Jll+hRANCAARuYF6KQ8PrEI5hsNDbvX0u1IqxGGc3\nnopfyywzW9YYuw01QqrDTmaO4CGPoywVnzhr9PqmpsJWpA8seCJfbxjD\n-----END PRIVATE KEY-----\n";

    fn apns_test_client() -> ApnsClient {
        ApnsClient {
            http: reqwest::Client::new(),
            encoding_key: EncodingKey::from_ec_pem(TEST_EC_P8.as_bytes()).unwrap(),
            key_id: "ABC123DEFG".to_string(),
            team_id: "TEAM123456".to_string(),
            topic: "com.nosdesk.app".to_string(),
            production: true,
            jwt: Mutex::new(None),
        }
    }

    #[tokio::test]
    async fn apns_mints_and_caches_es256_jwt() {
        let client = apns_test_client();
        let first = client.bearer().await.expect("mint");
        // Three base64url segments = a well-formed JWS.
        assert_eq!(first.split('.').count(), 3);
        // Cached: a second call returns the same token.
        let second = client.bearer().await.expect("cached");
        assert_eq!(first, second);
    }

    #[tokio::test]
    async fn apns_invalidates_the_cached_bearer() {
        let client = apns_test_client();
        client.bearer().await.expect("mint");
        assert!(client.jwt.lock().await.is_some());

        client.invalidate_bearer().await;
        assert!(
            client.jwt.lock().await.is_none(),
            "a bearer Apple rejected with 403 must not stay cached"
        );

        // The next send re-mints instead of replaying the rejected token.
        client.bearer().await.expect("re-mint");
        assert!(client.jwt.lock().await.is_some());
    }

    #[test]
    fn provider_client_builds_with_timeouts() {
        // Guards the builder config itself: an invalid combination would
        // otherwise only surface as a panic at boot.
        provider_http_client("APNs").expect("APNs client");
        provider_http_client("FCM").expect("FCM client");
    }

    #[test]
    fn apns_host_switches_on_environment() {
        let mut client = apns_test_client();
        assert_eq!(client.host(), "api.push.apple.com");
        client.production = false;
        assert_eq!(client.host(), "api.sandbox.push.apple.com");
    }

    #[test]
    fn service_account_json_parses() {
        let json = r#"{
            "type": "service_account",
            "project_id": "nosdesk-test",
            "private_key": "-----BEGIN PRIVATE KEY-----\nMII...\n-----END PRIVATE KEY-----\n",
            "client_email": "fcm@nosdesk-test.iam.gserviceaccount.com"
        }"#;
        let sa: ServiceAccount = serde_json::from_str(json).unwrap();
        assert_eq!(sa.project_id, "nosdesk-test");
        assert_eq!(sa.token_uri, "https://oauth2.googleapis.com/token");
        assert!(sa.client_email.starts_with("fcm@"));
    }

    #[tokio::test]
    async fn unconfigured_sender_reports_and_sends_nothing() {
        let sender = NativePushSender {
            apns: None,
            fcm: None,
        };
        assert!(!sender.is_configured());
        let targets = vec![PushTarget {
            platform: "ios".to_string(),
            token: "abc".to_string(),
        }];
        let payload = PushPayload {
            title: "t".to_string(),
            body: Some("Assigned to you".to_string()),
            notification_type: "ticket_assigned".to_string(),
            entity_type: "ticket".to_string(),
            entity_id: 1,
            ticket_id: 1,
        };
        // No provider for the platform → skipped, no panic, nothing pruned.
        assert!(sender.send(&targets, &payload).await.is_empty());
    }
}

// ---------------------------------------------------------------------------
// Classifier tests (D5)
//
// The whole reason these live in pure functions: the decision to permanently
// deregister someone's device is the highest-consequence branch in this file,
// and it was previously reachable only through a network call.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod classifier_tests {
    use super::*;

    #[test]
    fn fcm_success_is_sent() {
        assert_eq!(classify_fcm(200, "{}"), SendOutcome::Sent);
    }

    #[test]
    fn fcm_404_prunes() {
        assert_eq!(classify_fcm(404, "{}"), SendOutcome::Invalid);
    }

    /// The D5 defect. FCM returns INVALID_ARGUMENT for a malformed *message*
    /// as well as for a bad token, so the old substring match deregistered
    /// every device the recipient owned whenever our own payload was wrong.
    #[test]
    fn fcm_does_not_prune_when_the_message_is_malformed() {
        let body = r#"{"error":{"code":400,"status":"INVALID_ARGUMENT",
             "message":"Invalid JSON payload received. Unknown name \"titel\" at 'message.notification'",
             "details":[{"fieldViolations":[
               {"field":"message.notification","description":"Invalid JSON payload"}]}]}}"#;
        assert_eq!(
            classify_fcm(400, body),
            SendOutcome::Failed,
            "a payload bug must never cost the recipient their registrations"
        );
    }

    #[test]
    fn fcm_prunes_on_an_unregistered_detail() {
        let body = r#"{"error":{"code":400,"status":"INVALID_ARGUMENT","details":[
             {"@type":"type.googleapis.com/google.firebase.fcm.v1.FcmError",
              "errorCode":"UNREGISTERED"}]}}"#;
        assert_eq!(classify_fcm(400, body), SendOutcome::Invalid);
    }

    #[test]
    fn fcm_prunes_when_a_field_violation_names_the_token() {
        let body = r#"{"error":{"code":400,"status":"INVALID_ARGUMENT","details":[
             {"fieldViolations":[
               {"field":"message.token","description":"Invalid registration token"}]}]}}"#;
        assert_eq!(classify_fcm(400, body), SendOutcome::Invalid);
    }

    #[test]
    fn fcm_prunes_on_a_not_found_status() {
        let body = r#"{"error":{"code":404,"status":"NOT_FOUND"}}"#;
        assert_eq!(classify_fcm(400, body), SendOutcome::Invalid);
    }

    /// A body that is not JSON at all (a proxy error page, a truncated
    /// response) must not be read as permission to prune.
    #[test]
    fn fcm_keeps_the_token_when_the_body_is_not_json() {
        assert_eq!(
            classify_fcm(400, "<html>502 Bad Gateway</html>"),
            SendOutcome::Failed
        );
        assert_eq!(classify_fcm(500, ""), SendOutcome::Failed);
    }

    #[test]
    fn apns_410_prunes_and_success_sends() {
        assert_eq!(classify_apns(200, ""), SendOutcome::Sent);
        assert_eq!(classify_apns(410, ""), SendOutcome::Invalid);
    }

    #[test]
    fn apns_prunes_only_on_a_reason_that_names_the_token() {
        for reason in ["BadDeviceToken", "DeviceTokenNotForTopic", "Unregistered"] {
            let body = format!(r#"{{"reason":"{reason}"}}"#);
            assert_eq!(classify_apns(400, &body), SendOutcome::Invalid, "{reason}");
        }
    }

    /// PayloadTooLarge is our fault, not the device's.
    #[test]
    fn apns_does_not_prune_on_a_payload_fault() {
        let body = r#"{"reason":"PayloadTooLarge"}"#;
        assert_eq!(classify_apns(400, body), SendOutcome::Failed);
    }

    /// An unrecognised reason is kept, not guessed at: pruning wrongly costs a
    /// user their push with no way back short of reinstalling.
    #[test]
    fn apns_keeps_the_token_on_an_unknown_reason_or_bad_body() {
        assert_eq!(
            classify_apns(400, r#"{"reason":"SomeNewAppleReason"}"#),
            SendOutcome::Failed
        );
        assert_eq!(classify_apns(400, "not json"), SendOutcome::Failed);
    }

    /// A reason string appearing somewhere other than the `reason` field must
    /// not trigger a prune. This is what the old substring match got wrong.
    #[test]
    fn apns_ignores_a_prunable_word_outside_the_reason_field() {
        let body = r#"{"reason":"PayloadTooLarge","debug":"see BadDeviceToken docs"}"#;
        assert_eq!(classify_apns(400, body), SendOutcome::Failed);
    }

    #[test]
    fn provider_reason_lifts_the_code_from_either_provider() {
        assert_eq!(
            provider_reason(r#"{"reason":"BadDeviceToken"}"#),
            "BadDeviceToken"
        );
        assert_eq!(
            provider_reason(r#"{"error":{"status":"NOT_FOUND","message":"x"}}"#),
            "NOT_FOUND"
        );
    }

    /// The point of the helper: what gets logged is the code, never the body.
    /// An FCM error echoes the device token inside `error.message`, so a field
    /// carrying the body would ship the credential with the diagnosis.
    #[test]
    fn provider_reason_never_carries_the_body_through() {
        let token = "fT9Zx_secret_device_token_value";
        let body = format!(
            r#"{{"error":{{"status":"INVALID_ARGUMENT","message":"The registration token {token} is not valid"}}}}"#
        );
        let logged = provider_reason(&body);
        assert_eq!(logged, "INVALID_ARGUMENT");
        assert!(
            !logged.contains(token),
            "the token must not survive: {logged}"
        );
    }

    /// A code that is not an enum constant is reported as such rather than
    /// emitted, so the field cannot become a hole for free text.
    #[test]
    fn provider_reason_rejects_anything_that_is_not_a_constant() {
        assert_eq!(provider_reason("not json"), "unparsed");
        assert_eq!(provider_reason(r#"{"nothing":"here"}"#), "unparsed");
        assert_eq!(
            provider_reason(r#"{"reason":"user kyle@nosdesk.com failed"}"#),
            "unrecognised"
        );
        assert_eq!(
            provider_reason(&format!(r#"{{"reason":"{}"}}"#, "A".repeat(49))),
            "unrecognised"
        );
    }
}
