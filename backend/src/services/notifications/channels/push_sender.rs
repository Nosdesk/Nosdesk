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
use serde_json::json;
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

use super::push::{PushPayload, PushSender, PushTarget};

/// Outcome of a single-device send — drives token pruning.
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

        let encoding_key = EncodingKey::from_ec_pem(key_pem.as_bytes())
            .context("NOSDESK_APNS_KEY_P8 is not a valid EC private key (.p8 PEM)")?;

        Ok(Some(Self {
            http: reqwest::Client::new(),
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
                // 410 = the token is no longer active for this topic; a 400 with
                // one of these reasons means the token is permanently unusable.
                if status.as_u16() == 410
                    || (status.as_u16() == 400
                        && (reason.contains("BadDeviceToken")
                            || reason.contains("DeviceTokenNotForTopic")
                            || reason.contains("Unregistered")))
                {
                    debug!(%status, "apns: pruning invalid device token");
                    SendOutcome::Invalid
                } else {
                    warn!(%status, reason = %reason, "apns: send failed");
                    SendOutcome::Failed
                }
            }
            Err(e) => {
                warn!(error = %e, "apns: request error");
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
            http: reqwest::Client::new(),
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
                // 404 = token unregistered; a 400 UNREGISTERED/INVALID_ARGUMENT on
                // the token means it's permanently unusable.
                if status.as_u16() == 404
                    || (status.as_u16() == 400
                        && (reason.contains("UNREGISTERED") || reason.contains("INVALID_ARGUMENT")))
                {
                    debug!(%status, "fcm: pruning invalid device token");
                    SendOutcome::Invalid
                } else {
                    warn!(%status, reason = %reason, "fcm: send failed");
                    SendOutcome::Failed
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
    fn is_configured(&self) -> bool {
        self.apns.is_some() || self.fcm.is_some()
    }

    async fn send(&self, targets: &[PushTarget], payload: &PushPayload) -> Vec<String> {
        let mut invalid = Vec::new();
        for target in targets {
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
            if let SendOutcome::Invalid = outcome {
                invalid.push(target.token.clone());
            }
        }
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
