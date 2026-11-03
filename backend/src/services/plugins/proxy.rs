//! Plugin Proxy Service
//!
//! Proxies external HTTP requests for plugins, providing:
//! - Permission validation (plugins can only access whitelisted domains)
//! - Declarative auth injection (manifest-driven, no hardcoded domain checks)
//! - OAuth2 client credentials and private-key JWT exchange, with token caching
//! - Request logging for audit
//! - Rate limiting (future)
//! - Response sanitization

use actix_web::http::StatusCode;
use base64::Engine;
use reqwest::{Client, Method};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tracing::{debug, error, info, warn};

use crate::models::{PluginAuthConfig, PluginManifest, PluginProxyRequest, PluginProxyResponse};

/// A structured plugin-proxy failure. Replaces the old `Result<_, String>` so
/// the handler maps each failure to the right HTTP status (rather than a flat
/// 400) and the typed cause (the SSRF-guard rejection, the reqwest error kind)
/// survives the seam. Upstream non-2xx responses are NOT errors here: the proxy
/// relays the upstream status in `PluginProxyResponse`, as it does today.
#[derive(Debug, thiserror::Error)]
pub enum PluginProxyError {
    /// The target URL isn't covered by the manifest's network permissions.
    #[error("plugin '{plugin}' does not have permission to access '{url}'")]
    PermissionDenied { plugin: String, url: String },

    /// The request named an HTTP method the proxy doesn't support.
    #[error("unsupported HTTP method: {0}")]
    UnsupportedMethod(String),

    /// The SSRF guard rejected the destination (IP-literal / non-routable).
    #[error("request blocked by egress guard: {0}")]
    Blocked(#[from] crate::utils::safe_http::SafeHttpError),

    /// The request failed at the transport layer (connect, timeout, TLS, DNS)
    /// before or without an HTTP response. Preserves the reqwest kind so the
    /// handler can distinguish a timeout (504) from other network faults (502).
    #[error("upstream request failed: {0}")]
    Network(#[source] reqwest::Error),

    /// The manifest DECLARED auth for the target host but it couldn't be
    /// resolved: a missing secret, a disallowed / SSRF-blocked OAuth token URL,
    /// or a failed token exchange. We fail closed rather than send the request
    /// unauthenticated (OWASP "fail securely"); a host with no declared auth is
    /// `Ok(None)`, not this error, and proceeds normally.
    #[error("could not resolve declared auth for '{url}': {reason}")]
    AuthResolution { url: String, reason: String },

    /// A `private_key_jwt` credential couldn't be made: the token endpoint
    /// refused the exchange (its message is kept), or the settings it needs are
    /// missing or malformed. Not sent upstream. The handler answers with the
    /// 401-like `auth_failed` response (see `auth_failed_body`) so a plugin's
    /// settings page can show the reason.
    #[error("auth_failed: {message}")]
    AuthFailed { message: String },
}

impl PluginProxyError {
    /// The HTTP status the proxy handler should return for this failure.
    pub fn status_code(&self) -> StatusCode {
        match self {
            PluginProxyError::PermissionDenied { .. } | PluginProxyError::Blocked(_) => {
                StatusCode::FORBIDDEN
            }
            PluginProxyError::UnsupportedMethod(_) => StatusCode::BAD_REQUEST,
            PluginProxyError::Network(e) if e.is_timeout() => StatusCode::GATEWAY_TIMEOUT,
            // A declared-auth resolution failure and other network faults are
            // both a failure to establish the authenticated upstream call.
            PluginProxyError::Network(_) | PluginProxyError::AuthResolution { .. } => {
                StatusCode::BAD_GATEWAY
            }
            // Relayed as a 401 inside a successful proxy response, never as an
            // HTTP 401 from this API (which the app reads as a lost session).
            PluginProxyError::AuthFailed { .. } => StatusCode::OK,
        }
    }
}

/// The body of the 401-like response a plugin gets for `auth_failed`, so it
/// can tell this apart from an upstream 401: `{"error":"auth_failed",
/// "message": ...}`, with the `x-nosdesk-proxy-error: auth_failed` header.
pub fn auth_failed_body(message: &str) -> String {
    serde_json::json!({ "error": "auth_failed", "message": message }).to_string()
}

/// What the proxy authenticates a plugin's requests with: its decrypted
/// `secret` settings, its other settings as text (for `{key}` placeholders and
/// `issuer`), and the install the exchanged tokens are cached for.
#[derive(Debug, Clone, Default)]
pub struct PluginCredentials {
    pub secrets: HashMap<String, String>,
    pub settings: HashMap<String, String>,
    /// Identifies the install (workspace and plugin) in the token cache key.
    pub cache_scope: String,
}

impl PluginCredentials {
    pub fn from_secrets(secrets: HashMap<String, String>) -> Self {
        Self {
            secrets,
            ..Self::default()
        }
    }
}

/// The claims of the RS256 JWT a `private_key_jwt` exchange presents.
#[derive(serde::Serialize)]
struct ExchangeClaims<'a> {
    iss: &'a str,
    iat: i64,
    exp: i64,
}

/// Rebuild a PEM whose line breaks were lost, as happens when it is pasted
/// into a single-line input: keep the BEGIN / END lines and re-wrap the base64
/// between them at 64 columns. A PEM that already has its lines is returned
/// in the same form.
fn normalize_pem(pem: &str) -> Option<String> {
    let pem = pem.trim();
    let begin_end = pem.find("-----BEGIN ")?;
    let header_end = pem[begin_end + 11..].find("-----")? + begin_end + 11 + 5;
    let header = &pem[begin_end..header_end];
    let footer_start = pem.rfind("-----END ")?;
    let footer = pem[footer_start..].trim();
    if footer_start < header_end {
        return None;
    }
    let body: String = pem[header_end..footer_start]
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    if body.is_empty() {
        return None;
    }
    let mut out = String::with_capacity(pem.len() + body.len() / 64 + 4);
    out.push_str(header);
    out.push('\n');
    for chunk in body.as_bytes().chunks(64) {
        out.push_str(std::str::from_utf8(chunk).ok()?);
        out.push('\n');
    }
    out.push_str(footer);
    out.push('\n');
    Some(out)
}

/// Fill each `{key}` in `template` with the setting's value, path-encoded.
/// Refuses an unknown or empty setting, or an unclosed brace.
fn fill_token_url(template: &str, settings: &HashMap<String, String>) -> Result<String, String> {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let close = after
            .find('}')
            .ok_or_else(|| "token_url has an unclosed {".to_string())?;
        let key = &after[..close];
        let value = settings
            .get(key)
            .map(|v| v.trim())
            .filter(|v| !v.is_empty())
            .ok_or_else(|| format!("the {key} setting is empty"))?;
        out.push_str(&urlencoding::encode(value));
        rest = &after[close + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

/// The upstream's own reason from an error body: GitHub's `message`, OAuth's
/// `error_description` or `error`, else the start of the text.
fn upstream_reason(status: reqwest::StatusCode, text: &str) -> String {
    let parsed: Option<serde_json::Value> = serde_json::from_str(text).ok();
    let field = |k: &str| {
        parsed
            .as_ref()
            .and_then(|v| v.get(k))
            .and_then(|v| v.as_str())
            .map(str::to_string)
    };
    field("message")
        .or_else(|| field("error_description"))
        .or_else(|| field("error"))
        .unwrap_or_else(|| {
            let snippet: String = text.trim().chars().take(200).collect();
            if snippet.is_empty() {
                format!("the token endpoint answered {status}")
            } else {
                snippet
            }
        })
}

/// Seconds until the token expires, from `expires_field`: an RFC 3339 time
/// (GitHub's `expires_at`) or a number of seconds (`expires_in`).
fn expires_in_secs(value: Option<&serde_json::Value>) -> Option<i64> {
    match value? {
        serde_json::Value::Number(n) => n.as_i64(),
        serde_json::Value::String(s) => {
            if let Ok(secs) = s.trim().parse::<i64>() {
                return Some(secs);
            }
            chrono::DateTime::parse_from_rfc3339(s.trim())
                .ok()
                .map(|t| (t.with_timezone(&chrono::Utc) - chrono::Utc::now()).num_seconds())
        }
        _ => None,
    }
}

/// Used when the token response names no expiry: short, so a token is never
/// held past a lifetime we can't see.
const DEFAULT_EXCHANGED_TOKEN_SECS: i64 = 300;

/// A cached token is dropped this long before it expires.
const TOKEN_EXPIRY_MARGIN_SECS: i64 = 60;

/// The `private_key_jwt` fields of a manifest `auth` entry.
struct PrivateKeyJwtConfig<'a> {
    private_key: &'a str,
    issuer: &'a str,
    token_url: &'a str,
    token_field: &'a str,
    expires_field: &'a str,
}

/// Cached OAuth2 token
struct CachedToken {
    token: String,
    expires_at: Instant,
}

/// Plugin Proxy Service
///
/// Handles proxying external HTTP requests for plugins. All plugin external requests
/// must go through this service to ensure:
/// - The plugin has permission to access the domain
/// - Auth is injected from the manifest's declarative auth config
/// - The request is logged for audit
/// - Rate limits are enforced
pub struct PluginProxyService {
    client: Client,
    token_cache: Mutex<HashMap<String, CachedToken>>,
}

impl PluginProxyService {
    /// Create a new proxy service
    pub fn new() -> Self {
        // The proxy fronts URLs declared in plugin manifests. Plugin
        // bundles are signature-pinned, but the network section of
        // the manifest is still author-supplied. Routing through
        // safe_http means the resolver refuses to dial private IPs
        // even if a manifest author lists a hostname that resolves
        // into RFC1918 / 169.254.x.x.
        //
        // `no_redirect_client`, not `client`: the shared redirect policy
        // re-checks each hop for private IPs but cannot see this plugin's
        // consented `network:` grants, so a 302 off a permitted host would
        // otherwise leave the allowlist entirely (carrying a manifest-declared
        // `apiKey` header with it, which reqwest does not treat as sensitive).
        // `follow_redirects` drives the chain instead, re-authorizing each hop.
        let client = crate::utils::safe_http::no_redirect_client(Duration::from_secs(30))
            .expect("Failed to create SSRF-safe HTTP client");

        Self {
            client,
            token_cache: Mutex::new(HashMap::new()),
        }
    }

    /// Check if a plugin has permission to access a URL.
    ///
    /// `network_perms` is the plugin's EFFECTIVE (consented) permission set, not
    /// the raw manifest, so egress is authorized from the same trusted grant the
    /// rest of the plugin surface enforces on (a scope the admin actually
    /// consented to, reflecting mid-session revoke). Both the URL host and each
    /// `network:<pattern>` permission go through the same `Host` / `HostPattern`
    /// parsers, so case, normalisation, and wildcard semantics are shared with
    /// the validator. Any URL that doesn't normalise to a valid named host (IP
    /// literals, malformed URLs) is refused outright.
    fn has_permission(&self, network_perms: &[String], url: &str) -> bool {
        let parsed = match url::Url::parse(url) {
            Ok(u) => u,
            Err(_) => return false,
        };
        let Some(host_str) = parsed.host_str() else {
            return false;
        };
        let Ok(host) = crate::services::plugins::types::Host::parse(host_str) else {
            return false;
        };

        network_perms.iter().any(|perm| {
            crate::services::plugins::types::Permission::parse(perm)
                .ok()
                .and_then(|p| p.network_pattern().map(|pattern| pattern.matches(&host)))
                .unwrap_or(false)
        })
    }

    /// Get the auth header name that the manifest's auth config would inject for a given URL.
    /// Used to strip plugin-supplied headers that would conflict with declared auth.
    fn get_auth_header_name(manifest: &PluginManifest, url: &str) -> Option<String> {
        let parsed = url::Url::parse(url).ok()?;
        let host_str = parsed.host_str()?;
        let host = crate::services::plugins::types::Host::parse(host_str).ok()?;

        let auth_config = manifest.auth.get(&host)?;

        match auth_config {
            PluginAuthConfig::ApiKey { header, .. } => Some(header.to_lowercase()),
            _ => Some("authorization".to_string()),
        }
    }

    /// Get the authorization header for a URL based on the plugin manifest's auth config.
    ///
    /// Reads the `auth` map from the manifest, matches the request host to a domain pattern,
    /// and resolves the auth strategy using decrypted plugin secrets.
    async fn get_auth_for_url(
        &self,
        plugin_name: &str,
        url: &str,
        manifest: &PluginManifest,
        network_perms: &[String],
        creds: &PluginCredentials,
    ) -> Result<Option<(String, String)>, PluginProxyError> {
        let secrets = &creds.secrets;
        // A URL that reached here already passed `has_permission` (which parses
        // it), so these are effectively unreachable; if the host can't be
        // determined, no declared auth can match, so proceed unauthenticated.
        let Some(parsed) = url::Url::parse(url).ok() else {
            return Ok(None);
        };
        let Some(host_str) = parsed.host_str() else {
            return Ok(None);
        };
        let Ok(host) = crate::services::plugins::types::Host::parse(host_str) else {
            return Ok(None);
        };

        // No auth declared for this host: the one legitimate "proceed
        // unauthenticated" case. Every failure below is declared-but-
        // unresolvable auth, which fails closed (OWASP "fail securely") rather
        // than silently sending the request without the intended credentials.
        let Some(auth_config) = manifest.auth.get(&host) else {
            return Ok(None);
        };

        let fail = |reason: String| PluginProxyError::AuthResolution {
            url: url.to_string(),
            reason,
        };

        match auth_config {
            PluginAuthConfig::Bearer { secret } => {
                let token = secrets
                    .get(secret)
                    .ok_or_else(|| fail(format!("missing secret '{secret}'")))?;
                Ok(Some(("Authorization".into(), format!("Bearer {token}"))))
            }
            PluginAuthConfig::Basic {
                username_secret,
                password_secret,
            } => {
                let user = secrets
                    .get(username_secret)
                    .ok_or_else(|| fail(format!("missing secret '{username_secret}'")))?;
                let pass = secrets
                    .get(password_secret)
                    .ok_or_else(|| fail(format!("missing secret '{password_secret}'")))?;
                let encoded =
                    base64::engine::general_purpose::STANDARD.encode(format!("{user}:{pass}"));
                Ok(Some(("Authorization".into(), format!("Basic {encoded}"))))
            }
            PluginAuthConfig::ApiKey { header, secret } => {
                let value = secrets
                    .get(secret)
                    .ok_or_else(|| fail(format!("missing secret '{secret}'")))?;
                Ok(Some((header.clone(), value.clone())))
            }
            PluginAuthConfig::Oauth2ClientCredentials {
                token_url,
                client_id_secret,
                client_secret_secret,
            } => {
                // The token endpoint is manifest-controlled and may
                // differ from the request host, so it must clear the
                // same gates as request.url: the network allowlist (or
                // a plugin could exfil the admin-configured client_secret
                // to any host) and the IP-literal SSRF guard (a literal
                // token_url host bypasses the resolver). See
                // security-audit-2026-06.
                if !self.has_permission(network_perms, token_url) {
                    warn!(
                        plugin = plugin_name,
                        token_url = token_url,
                        "OAuth2 token_url is not in the plugin's network permissions"
                    );
                    return Err(fail(
                        "OAuth2 token_url is not in the plugin's network permissions".to_string(),
                    ));
                }
                if let Err(e) = crate::utils::safe_http::reject_unsafe_ip_literal(token_url) {
                    warn!(
                        plugin = plugin_name,
                        token_url = token_url,
                        error = %e,
                        "OAuth2 token_url blocked by SSRF guard"
                    );
                    return Err(fail(format!("OAuth2 token_url blocked by SSRF guard: {e}")));
                }

                let client_id = secrets
                    .get(client_id_secret)
                    .ok_or_else(|| fail(format!("missing secret '{client_id_secret}'")))?;
                let client_secret = secrets
                    .get(client_secret_secret)
                    .ok_or_else(|| fail(format!("missing secret '{client_secret_secret}'")))?;

                // Check token cache
                let cache_key = format!("{plugin_name}:{host}");
                if let Ok(cache) = self.token_cache.lock() {
                    if let Some(cached) = cache.get(&cache_key) {
                        if cached.expires_at > Instant::now() {
                            return Ok(Some(("Authorization".into(), cached.token.clone())));
                        }
                    }
                }

                // Exchange credentials for a token
                // Send as query params with empty body (some servers like IIS
                // require Content-Length: 0 on POST with no body)
                let token_request_url = url::Url::parse_with_params(
                    token_url,
                    &[
                        ("grant_type", "client_credentials"),
                        ("scope", "all"),
                        ("client_id", client_id.as_str()),
                        ("client_secret", client_secret.as_str()),
                    ],
                )
                .map_err(|e| fail(format!("invalid OAuth2 token_url: {e}")))?;

                let resp = self
                    .client
                    .post(token_request_url)
                    .header("Content-Length", "0")
                    .send()
                    .await
                    .map_err(|e| {
                        error!(
                            plugin = plugin_name,
                            token_url = token_url,
                            error = %e,
                            "OAuth2 token exchange failed"
                        );
                        fail(format!("OAuth2 token exchange failed: {e}"))
                    })?;

                let resp_status = resp.status();
                let resp_text = resp.text().await.map_err(|e| {
                    error!(
                        plugin = plugin_name,
                        error = %e,
                        "Failed to read OAuth2 token response body"
                    );
                    fail(format!("failed to read OAuth2 token response: {e}"))
                })?;

                if !resp_status.is_success() {
                    error!(
                        plugin = plugin_name,
                        status = %resp_status,
                        body_preview = %resp_text.chars().take(500).collect::<String>(),
                        "OAuth2 token endpoint returned error"
                    );
                    return Err(fail(format!(
                        "OAuth2 token endpoint returned status {resp_status}"
                    )));
                }

                let json: serde_json::Value = serde_json::from_str(&resp_text).map_err(|e| {
                    error!(
                        plugin = plugin_name,
                        error = %e,
                        body_preview = %resp_text.chars().take(500).collect::<String>(),
                        "Failed to parse OAuth2 token response as JSON"
                    );
                    fail(format!("OAuth2 token response was not valid JSON: {e}"))
                })?;

                let access_token = json
                    .get("access_token")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        fail("OAuth2 token response had no access_token field".to_string())
                    })?;
                let bearer = format!("Bearer {access_token}");

                // Cache with TTL (use expires_in from response, default 50 min)
                let ttl = json
                    .get("expires_in")
                    .and_then(|v| v.as_u64())
                    .map(|s| s.saturating_sub(60)) // 1 min buffer
                    .unwrap_or(3000);

                if let Ok(mut cache) = self.token_cache.lock() {
                    cache.insert(
                        cache_key,
                        CachedToken {
                            token: bearer.clone(),
                            expires_at: Instant::now() + Duration::from_secs(ttl),
                        },
                    );
                }

                Ok(Some(("Authorization".into(), bearer)))
            }
            PluginAuthConfig::PrivateKeyJwt {
                private_key,
                issuer,
                token_url,
                token_field,
                expires_field,
            } => {
                let token = self
                    .exchange_private_key_jwt(
                        plugin_name,
                        manifest,
                        network_perms,
                        creds,
                        &host,
                        PrivateKeyJwtConfig {
                            private_key,
                            issuer,
                            token_url,
                            token_field,
                            expires_field,
                        },
                    )
                    .await?;
                Ok(Some(("Authorization".into(), token)))
            }
        }
    }

    /// Mint an access token for a `private_key_jwt` host: sign an RS256 JWT
    /// with the private key setting, POST it to the token URL, and cache the
    /// returned token per (install, settings) until shortly before it expires.
    /// Returns the `Authorization` value. The exchange is built here, not
    /// through `build_hop_request`, so it never carries an injected token.
    async fn exchange_private_key_jwt(
        &self,
        plugin_name: &str,
        manifest: &PluginManifest,
        network_perms: &[String],
        creds: &PluginCredentials,
        host: &crate::services::plugins::types::Host,
        config: PrivateKeyJwtConfig<'_>,
    ) -> Result<String, PluginProxyError> {
        let failed = |message: String| PluginProxyError::AuthFailed { message };

        let pem = creds
            .secrets
            .get(config.private_key)
            .filter(|v| !v.trim().is_empty())
            .ok_or_else(|| failed(format!("the {} setting is empty", config.private_key)))?;
        let issuer = creds
            .settings
            .get(config.issuer)
            .map(|v| v.trim())
            .filter(|v| !v.is_empty())
            .ok_or_else(|| failed(format!("the {} setting is empty", config.issuer)))?;
        let token_url = fill_token_url(config.token_url, &creds.settings).map_err(failed)?;

        // The URL is the manifest's with settings filled in, so it clears the
        // same gates as a request URL: the consented network allowlist and the
        // IP-literal SSRF guard (the resolver covers hostnames).
        if !self.has_permission(network_perms, &token_url) {
            return Err(PluginProxyError::AuthResolution {
                url: token_url,
                reason: "token_url is not in the plugin's network permissions".to_string(),
            });
        }
        if let Err(e) = crate::utils::safe_http::reject_unsafe_ip_literal(&token_url) {
            return Err(PluginProxyError::AuthResolution {
                url: token_url,
                reason: format!("token_url blocked by SSRF guard: {e}"),
            });
        }

        // One cache entry per install and per credential: changing the key,
        // issuer or any setting in the URL mints a fresh token.
        let cache_key = {
            let mut ctx = ring::digest::Context::new(&ring::digest::SHA256);
            for part in [
                creds.cache_scope.as_str(),
                host.as_str(),
                token_url.as_str(),
                issuer,
                pem.as_str(),
            ] {
                ctx.update(&(part.len() as u64).to_be_bytes());
                ctx.update(part.as_bytes());
            }
            format!("private_key_jwt:{}", hex::encode(ctx.finish().as_ref()))
        };
        if let Ok(cache) = self.token_cache.lock() {
            if let Some(cached) = cache.get(&cache_key) {
                if cached.expires_at > Instant::now() {
                    return Ok(cached.token.clone());
                }
            }
        }

        let pem = normalize_pem(pem).ok_or_else(|| {
            failed(format!(
                "the {} setting is not a PEM key",
                config.private_key
            ))
        })?;
        let key = jsonwebtoken::EncodingKey::from_rsa_pem(pem.as_bytes()).map_err(|_| {
            failed(format!(
                "the {} setting is not an RSA private key",
                config.private_key
            ))
        })?;
        let now = chrono::Utc::now().timestamp();
        let jwt = jsonwebtoken::encode(
            &jsonwebtoken::Header::new(jsonwebtoken::Algorithm::RS256),
            &ExchangeClaims {
                iss: issuer,
                iat: now - 60,
                exp: now + 9 * 60,
            },
            &key,
        )
        .map_err(|e| failed(format!("could not sign the token request: {e}")))?;

        let resp = self
            .client
            .post(&token_url)
            .header("Authorization", format!("Bearer {jwt}"))
            .header("Accept", "application/json")
            .header(
                "User-Agent",
                format!("Nosdesk-Plugin/{} ({})", manifest.version, plugin_name),
            )
            .header("Content-Length", "0")
            .send()
            .await
            .map_err(|e| {
                error!(plugin = plugin_name, error = %e, "private_key_jwt exchange failed");
                PluginProxyError::AuthResolution {
                    url: token_url.clone(),
                    reason: format!("token exchange failed: {e}"),
                }
            })?;
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            let reason = upstream_reason(status, &text);
            warn!(
                plugin = plugin_name,
                status = %status,
                error = %reason,
                "private_key_jwt token endpoint refused the exchange"
            );
            return Err(failed(reason));
        }

        let json: serde_json::Value = serde_json::from_str(&text)
            .map_err(|_| failed("the token response was not JSON".to_string()))?;
        let access_token = json
            .get(config.token_field)
            .and_then(|v| v.as_str())
            .filter(|v| !v.is_empty())
            .ok_or_else(|| {
                failed(format!(
                    "the token response had no {} field",
                    config.token_field
                ))
            })?;
        let value = format!("Bearer {access_token}");

        let lifetime =
            expires_in_secs(json.get(config.expires_field)).unwrap_or(DEFAULT_EXCHANGED_TOKEN_SECS);
        let keep = lifetime - TOKEN_EXPIRY_MARGIN_SECS;
        if keep > 0 {
            if let Ok(mut cache) = self.token_cache.lock() {
                let now = Instant::now();
                cache.retain(|_, t| t.expires_at > now);
                cache.insert(
                    cache_key,
                    CachedToken {
                        token: value.clone(),
                        expires_at: now + Duration::from_secs(keep as u64),
                    },
                );
            }
        }
        Ok(value)
    }

    /// Maximum redirect hops the proxy will follow. Matches the shared
    /// `safe_http` policy's cap.
    const MAX_REDIRECTS: usize = 5;

    /// Build one request against `url`, applying the allowlist and SSRF guards
    /// and resolving auth *for that URL's host*.
    ///
    /// Called once per hop, which is the point: authorization is a property of
    /// the URL being dialled, not of the call the plugin originally made. On a
    /// redirect to a host with no declared auth, `get_auth_for_url` returns
    /// `None` and no credential is attached.
    ///
    /// `include_body` is false on hops that a 301/302/303 has downgraded to GET.
    #[allow(clippy::too_many_arguments)]
    async fn build_hop_request(
        &self,
        url: &str,
        method: Method,
        plugin_name: &str,
        manifest: &PluginManifest,
        network_perms: &[String],
        creds: &PluginCredentials,
        request: &PluginProxyRequest,
        include_body: bool,
    ) -> Result<reqwest::RequestBuilder, PluginProxyError> {
        if !self.has_permission(network_perms, url) {
            warn!(
                plugin = plugin_name,
                url, "Plugin denied access to URL - no matching external permission"
            );
            return Err(PluginProxyError::PermissionDenied {
                plugin: plugin_name.to_string(),
                url: url.to_string(),
            });
        }

        // The safe_http resolver refuses internal IPs returned by DNS; this
        // catches the IP-literal case, which never reaches the resolver.
        if let Err(e) = crate::utils::safe_http::reject_unsafe_ip_literal(url) {
            warn!(
                plugin = plugin_name,
                url,
                error = %e,
                "plugin proxy blocked by SSRF guard"
            );
            return Err(PluginProxyError::Blocked(e));
        }

        // Which header the auth config will inject for THIS url, so a
        // plugin-supplied header cannot shadow it.
        let auth_header_name = Self::get_auth_header_name(manifest, url);

        let mut req = self.client.request(method.clone(), url);

        if let Some(headers) = request.headers.clone() {
            for (key, value) in headers {
                let key_lower = key.to_lowercase();
                // Always block host and user-agent overrides
                if key_lower == "host" || key_lower == "user-agent" {
                    continue;
                }
                // Block any header that the auth config will inject
                if let Some(ref auth_header) = auth_header_name {
                    if key_lower == *auth_header {
                        continue;
                    }
                }
                req = req.header(&key, &value);
            }
        }

        // Inject authorization from the manifest's auth config. `Ok(None)` means
        // no auth was declared for this host, so proceed unauthenticated; an
        // `Err` means auth WAS declared but couldn't be resolved, so `?` fails
        // closed rather than sending the request without the intended
        // credentials.
        if let Some((header_name, header_value)) = self
            .get_auth_for_url(plugin_name, url, manifest, network_perms, creds)
            .await?
        {
            debug!(
                plugin = plugin_name,
                "Injecting auth header from manifest config"
            );
            req = req.header(&header_name, &header_value);
        }

        // Add custom User-Agent
        req = req.header(
            "User-Agent",
            format!("Nosdesk-Plugin/{} ({})", manifest.version, plugin_name),
        );

        // Add body for methods that support it, or Content-Length: 0 for bodyless POSTs
        let body = if include_body {
            request.body.clone()
        } else {
            None
        };
        if let Some(body) = body {
            match request.content_type.as_deref() {
                Some("form") => {
                    if let serde_json::Value::Object(map) = &body {
                        let params: Vec<(String, String)> = map
                            .iter()
                            .map(|(k, v)| {
                                (
                                    k.clone(),
                                    match v {
                                        serde_json::Value::String(s) => s.clone(),
                                        other => other.to_string(),
                                    },
                                )
                            })
                            .collect();
                        req = req.form(&params);
                    } else {
                        req = req.json(&body);
                    }
                }
                _ => {
                    req = req.json(&body);
                }
            }
        } else if include_body
            && (method == Method::POST || method == Method::PUT || method == Method::PATCH)
        {
            // Some servers require Content-Length even for empty bodies
            req = req.header("Content-Length", "0");
        }

        Ok(req)
    }

    /// Send `req`, following any redirect chain by hand.
    ///
    /// The client is built with `Policy::none()` so nothing is followed
    /// implicitly. Each hop is rebuilt through [`Self::build_hop_request`],
    /// which re-runs the `network:` allowlist check, the SSRF guard, and auth
    /// resolution against the *new* URL.
    ///
    /// This exists because the two obvious alternatives are both wrong. Letting
    /// reqwest follow redirects leaves the consented allowlist entirely (the
    /// shared policy can only re-check the IP, not this plugin's grants) and
    /// carries a manifest-declared `apiKey` header along with it, because
    /// reqwest's cross-host header stripping only covers `Authorization`,
    /// `Cookie`, and `Proxy-Authorization`. Refusing to follow redirects at all
    /// would break plugins against ordinary APIs that redirect.
    #[allow(clippy::too_many_arguments)]
    async fn follow_redirects(
        &self,
        req: reqwest::RequestBuilder,
        plugin_name: &str,
        manifest: &PluginManifest,
        network_perms: &[String],
        creds: &PluginCredentials,
        request: &PluginProxyRequest,
        method: &Method,
    ) -> Result<reqwest::Response, PluginProxyError> {
        let mut req = req;
        let mut current_url = request.url.clone();
        let mut current_method = method.clone();
        let mut include_body = true;

        for hop in 0..=Self::MAX_REDIRECTS {
            let response = req.send().await.map_err(|e| {
                error!(
                    plugin = plugin_name,
                    url = current_url,
                    error = %e,
                    "Failed to execute proxied request"
                );
                PluginProxyError::Network(e)
            })?;

            if !response.status().is_redirection() {
                return Ok(response);
            }
            if hop == Self::MAX_REDIRECTS {
                warn!(
                    plugin = plugin_name,
                    url = current_url,
                    "plugin proxy stopped at the redirect cap"
                );
                return Ok(response);
            }

            // No usable Location: hand the 3xx back rather than guessing.
            let Some(location) = response
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|v| v.to_str().ok())
            else {
                return Ok(response);
            };

            // Location may be relative; resolve it against the URL we just
            // dialled, exactly as a browser or reqwest would.
            let base =
                url::Url::parse(&current_url).map_err(|_| PluginProxyError::PermissionDenied {
                    plugin: plugin_name.to_string(),
                    url: current_url.clone(),
                })?;
            let Ok(next) = base.join(location) else {
                return Ok(response);
            };

            let status = response.status().as_u16();
            // 303 always becomes GET; 301/302 do in practice for non-GET/HEAD.
            // 307/308 preserve method and body by definition.
            if status == 303 || ((status == 301 || status == 302) && current_method != Method::HEAD)
            {
                current_method = Method::GET;
                include_body = false;
            }

            let next_url = next.to_string();
            info!(
                plugin = plugin_name,
                from = current_url,
                to = next_url,
                status,
                "plugin proxy following redirect (re-authorizing)"
            );

            // Rebuilding re-runs the allowlist + SSRF guards and re-resolves
            // auth for the new host. A hop off the consented allowlist fails
            // here with PermissionDenied, and a hop to a host with no declared
            // auth carries no credential.
            req = self
                .build_hop_request(
                    &next_url,
                    current_method.clone(),
                    plugin_name,
                    manifest,
                    network_perms,
                    creds,
                    request,
                    include_body,
                )
                .await?;
            current_url = next_url;
        }

        unreachable!("redirect loop returns from inside the body")
    }

    /// Execute a proxied request for a plugin, authenticated with its secret
    /// settings alone. See [`Self::proxy_request_with`].
    pub async fn proxy_request(
        &self,
        plugin_name: &str,
        manifest: &PluginManifest,
        network_perms: &[String],
        request: PluginProxyRequest,
        secrets: &HashMap<String, String>,
    ) -> Result<PluginProxyResponse, PluginProxyError> {
        self.proxy_request_with(
            plugin_name,
            manifest,
            network_perms,
            request,
            &PluginCredentials::from_secrets(secrets.clone()),
        )
        .await
    }

    /// Execute a proxied request for a plugin
    ///
    /// `creds` carries the plugin's settings, which the manifest's `auth`
    /// config turns into the credential injected for the request's host.
    pub async fn proxy_request_with(
        &self,
        plugin_name: &str,
        manifest: &PluginManifest,
        network_perms: &[String],
        request: PluginProxyRequest,
        creds: &PluginCredentials,
    ) -> Result<PluginProxyResponse, PluginProxyError> {
        // Check permission
        if !self.has_permission(network_perms, &request.url) {
            warn!(
                plugin = plugin_name,
                url = request.url,
                "Plugin denied access to URL - no matching external permission"
            );
            return Err(PluginProxyError::PermissionDenied {
                plugin: plugin_name.to_string(),
                url: request.url.clone(),
            });
        }

        info!(
            plugin = plugin_name,
            url = request.url,
            method = request.method,
            "Proxying external request for plugin"
        );

        // Parse method
        let method = match request.method.to_uppercase().as_str() {
            "GET" => Method::GET,
            "POST" => Method::POST,
            "PUT" => Method::PUT,
            "PATCH" => Method::PATCH,
            "DELETE" => Method::DELETE,
            "HEAD" => Method::HEAD,
            "OPTIONS" => Method::OPTIONS,
            _ => return Err(PluginProxyError::UnsupportedMethod(request.method.clone())),
        };

        let req = self
            .build_hop_request(
                &request.url,
                method.clone(),
                plugin_name,
                manifest,
                network_perms,
                creds,
                &request,
                true,
            )
            .await?;

        // Execute the request, following redirects by hand so every hop is
        // re-authorized against this plugin's grants (see `follow_redirects`).
        let response = self
            .follow_redirects(
                req,
                plugin_name,
                manifest,
                network_perms,
                creds,
                &request,
                &method,
            )
            .await?;

        let status = response.status().as_u16();

        // Extract response headers
        let mut response_headers = HashMap::new();
        for (key, value) in response.headers() {
            if let Ok(v) = value.to_str() {
                response_headers.insert(key.to_string(), v.to_string());
            }
        }

        // Get response body — try JSON first, fall back to text string
        let body_text = response.text().await.ok();
        let body = body_text.and_then(|text| {
            serde_json::from_str::<serde_json::Value>(&text)
                .ok()
                .or(Some(serde_json::Value::String(text)))
        });

        if status >= 400 {
            warn!(
                plugin = plugin_name,
                url = request.url,
                status = status,
                response_body = ?body,
                "Proxied request failed"
            );
        } else {
            debug!(
                plugin = plugin_name,
                url = request.url,
                status = status,
                "Proxied request completed"
            );
        }

        Ok(PluginProxyResponse {
            status,
            headers: response_headers,
            body,
        })
    }
}

impl Default for PluginProxyService {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::plugins::types::{Host, Permission};

    fn create_test_manifest(permissions: Vec<&str>) -> PluginManifest {
        PluginManifest {
            manifest_version: 1,
            name: "test-plugin".to_string(),
            display_name: "Test Plugin".to_string(),
            version: "1.0.0".to_string(),
            description: None,
            license: None,
            author: None,
            repository: None,
            homepage: None,
            bugs: None,
            support_contact: None,
            engines: crate::models::PluginEngines {
                nosdesk: ">=0.0.0".into(),
                plugin_api: "1".into(),
            },
            dependencies: std::collections::BTreeMap::new(),
            categories: vec![],
            tags: vec![],
            screenshots: vec![],
            permissions: permissions
                .into_iter()
                .map(|p| Permission::parse(p).expect("test permission must be valid"))
                .collect(),
            permission_reasons: Default::default(),
            components: std::collections::BTreeMap::new(),
            events: vec![],
            settings: vec![],
            collections: std::collections::BTreeMap::new(),
            auth: std::collections::BTreeMap::new(),
            lifecycle: crate::models::PluginLifecyclePolicy::default(),
            commands: vec![],
            menus: std::collections::BTreeMap::new(),
            url_handlers: vec![],
            extensions: std::collections::BTreeMap::new(),
            i18n: std::collections::BTreeMap::new(),
        }
    }

    fn host(s: &str) -> Host {
        Host::parse(s).expect("test host must be valid")
    }

    /// Effective permission strings for a test manifest. In these unit tests the
    /// consented set equals the manifest (no reconsent state), so deriving it
    /// from the manifest mirrors what the handler passes at runtime.
    fn net_perms(manifest: &PluginManifest) -> Vec<String> {
        manifest.permissions.iter().map(|p| p.as_string()).collect()
    }

    /// Serialises the tests that drive real connections to localhost.
    ///
    /// They work by setting `NOSDESK_OUTBOUND_ALLOWED_HOSTS` so the SSRF
    /// resolver will dial loopback at all. That is process-global, so two of
    /// them in parallel will clear it out from under each other mid-request and
    /// the second fails with "resolves to non-routable address" rather than for
    /// any reason to do with the code under test.
    static ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    /// Minimal one-shot HTTP server on an ephemeral localhost port. Returns the
    /// bound port and a handle; the task serves exactly `responses.len()`
    /// connections, one canned response each, in order.
    ///
    /// Hand-rolled rather than pulled from a mock-server crate: the whole point
    /// is to exercise the real reqwest client through the real SSRF resolver,
    /// and the responses needed here are two lines each.
    async fn serve_canned(responses: Vec<String>) -> (u16, tokio::task::JoinHandle<()>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = tokio::spawn(async move {
            for body in responses {
                let Ok((mut sock, _)) = listener.accept().await else {
                    return;
                };
                let mut buf = [0u8; 2048];
                let _ = sock.read(&mut buf).await;
                let _ = sock.write_all(body.as_bytes()).await;
                let _ = sock.flush().await;
            }
        });
        (port, handle)
    }

    fn proxy_req(url: &str) -> PluginProxyRequest {
        PluginProxyRequest {
            url: url.to_string(),
            method: "GET".to_string(),
            headers: None,
            body: None,
            content_type: None,
        }
    }

    /// A redirect off the consented allowlist must be refused, not followed.
    ///
    /// Before this guard the proxy let reqwest follow redirects: the shared
    /// policy re-checked each hop's IP but not the plugin's `network:` grants,
    /// so any declared host could bounce the proxy to an arbitrary public host
    /// and read the response back. Regression test for S1
    /// (docs/security-audit-2026-08.md).
    #[tokio::test]
    async fn redirect_off_the_allowlist_is_refused() {
        let _guard = ENV_LOCK.lock().await;
        std::env::set_var("NOSDESK_OUTBOUND_ALLOWED_HOSTS", "localhost");

        let (port, _h) = serve_canned(vec!["HTTP/1.1 302 Found\r\nLocation: \
             http://not-granted.example.com/steal\r\nContent-Length: 0\r\n\r\n"
            .to_string()])
        .await;

        let service = PluginProxyService::new();
        // Only localhost is granted; the redirect target is not.
        let manifest = create_test_manifest(vec!["network:localhost"]);
        let result = service
            .proxy_request(
                "test-plugin",
                &manifest,
                &net_perms(&manifest),
                proxy_req(&format!("http://localhost:{port}/start")),
                &HashMap::new(),
            )
            .await;

        std::env::remove_var("NOSDESK_OUTBOUND_ALLOWED_HOSTS");

        match result {
            Err(PluginProxyError::PermissionDenied { url, .. }) => {
                assert!(
                    url.contains("not-granted.example.com"),
                    "should name the refused hop, got {url}"
                );
            }
            other => panic!("expected PermissionDenied on the redirect hop, got {other:?}"),
        }
    }

    /// The complement: a redirect that stays inside the granted host is still
    /// followed, so the guard above does not break ordinary APIs that redirect.
    #[tokio::test]
    async fn redirect_within_the_allowlist_is_followed() {
        let _guard = ENV_LOCK.lock().await;
        std::env::set_var("NOSDESK_OUTBOUND_ALLOWED_HOSTS", "localhost");

        let (port, _h) = serve_canned(vec![
            // Relative Location, resolved against the current URL.
            "HTTP/1.1 302 Found\r\nLocation: /landed\r\nContent-Length: 0\r\n\r\n".to_string(),
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 14\r\n\r\n\
             {\"ok\":\"true\"}"
                .to_string(),
        ])
        .await;

        let service = PluginProxyService::new();
        let manifest = create_test_manifest(vec!["network:localhost"]);
        let result = service
            .proxy_request(
                "test-plugin",
                &manifest,
                &net_perms(&manifest),
                proxy_req(&format!("http://localhost:{port}/start")),
                &HashMap::new(),
            )
            .await;

        std::env::remove_var("NOSDESK_OUTBOUND_ALLOWED_HOSTS");

        let response = result.expect("same-host redirect should be followed");
        assert_eq!(response.status, 200);
    }

    /// A manifest-declared `apiKey` header is resolved per hop, keyed on the
    /// host being dialled. reqwest only strips `Authorization` / `Cookie` /
    /// `Proxy-Authorization` across hosts, so a custom header would otherwise
    /// ride a redirect to the target. Asserting at the resolution layer: a host
    /// with no declared auth yields no header at all.
    #[tokio::test]
    async fn api_key_header_is_not_resolved_for_an_undeclared_host() {
        let mut manifest = create_test_manifest(vec![
            "network:api.vendor.com",
            "network:redirect-target.example.com",
        ]);
        manifest.auth.insert(
            host("api.vendor.com"),
            PluginAuthConfig::ApiKey {
                header: "X-API-Key".to_string(),
                secret: "vendor_key".to_string(),
            },
        );
        let secrets = HashMap::from([("vendor_key".to_string(), "super-secret-value".to_string())]);
        let service = PluginProxyService::new();
        let perms = net_perms(&manifest);

        // The declared host resolves the key.
        let declared = service
            .get_auth_for_url(
                "test-plugin",
                "https://api.vendor.com/v1/thing",
                &manifest,
                &perms,
                &PluginCredentials::from_secrets(secrets.clone()),
            )
            .await
            .expect("declared host resolves");
        assert_eq!(
            declared,
            Some(("X-API-Key".to_string(), "super-secret-value".to_string()))
        );

        // A different host, even one the plugin may reach, resolves nothing.
        let other = service
            .get_auth_for_url(
                "test-plugin",
                "https://redirect-target.example.com/steal",
                &manifest,
                &perms,
                &PluginCredentials::from_secrets(secrets.clone()),
            )
            .await
            .expect("undeclared host is not an error");
        assert_eq!(other, None, "the API key must not follow to another host");
    }

    #[test]
    fn test_exact_domain_permission() {
        let service = PluginProxyService::new();
        let manifest = create_test_manifest(vec!["network:api.example.com"]);

        assert!(service.has_permission(&net_perms(&manifest), "https://api.example.com/v1/data"));
        assert!(!service.has_permission(&net_perms(&manifest), "https://other.example.com/data"));
        assert!(!service.has_permission(&net_perms(&manifest), "https://api.other.com/data"));
    }

    #[test]
    fn test_wildcard_domain_permission() {
        let service = PluginProxyService::new();
        let manifest = create_test_manifest(vec!["network:*.example.com"]);

        assert!(service.has_permission(&net_perms(&manifest), "https://api.example.com/v1/data"));
        assert!(service.has_permission(&net_perms(&manifest), "https://www.example.com/data"));
        assert!(service.has_permission(&net_perms(&manifest), "https://example.com/data"));
        assert!(!service.has_permission(&net_perms(&manifest), "https://api.other.com/data"));
    }

    #[test]
    fn test_wildcard_does_not_match_deeper_subdomain() {
        // Single-label wildcard semantics: `*.example.com` does
        // NOT cover `v1.api.example.com`. This is the correct
        // outcome for the spec we documented; record it.
        let service = PluginProxyService::new();
        let manifest = create_test_manifest(vec!["network:*.example.com"]);
        assert!(!service.has_permission(&net_perms(&manifest), "https://v1.api.example.com/data"));
    }

    #[test]
    fn test_url_with_uppercase_host_normalises() {
        // Browsers and most HTTP clients lowercase the host, but
        // a curl-style direct call could include uppercase. The
        // typed `Host::parse` enforces lowercase normalisation on
        // both sides so the match is consistent.
        let service = PluginProxyService::new();
        let manifest = create_test_manifest(vec!["network:api.example.com"]);
        assert!(service.has_permission(&net_perms(&manifest), "https://API.EXAMPLE.COM/data"));
    }

    #[test]
    fn test_ip_literal_url_is_refused() {
        // The proxy refuses URL hosts that don't normalise to a
        // named host. `Host::parse` rejects IP literals.
        let service = PluginProxyService::new();
        let manifest = create_test_manifest(vec!["network:*.example.com"]);
        assert!(!service.has_permission(&net_perms(&manifest), "https://127.0.0.1/data"));
    }

    #[test]
    fn test_no_permission() {
        let service = PluginProxyService::new();
        let manifest = create_test_manifest(vec!["ticket:read"]);

        assert!(!service.has_permission(&net_perms(&manifest), "https://api.example.com/data"));
    }

    #[test]
    fn test_auth_header_name_bearer() {
        let mut auth = std::collections::BTreeMap::new();
        auth.insert(
            host("api.example.com"),
            PluginAuthConfig::Bearer {
                secret: "token".to_string(),
            },
        );
        let mut manifest = create_test_manifest(vec![]);
        manifest.auth = auth;

        assert_eq!(
            PluginProxyService::get_auth_header_name(&manifest, "https://api.example.com/test"),
            Some("authorization".to_string())
        );
    }

    #[test]
    fn test_auth_header_name_api_key() {
        let mut auth = std::collections::BTreeMap::new();
        auth.insert(
            host("api.example.com"),
            PluginAuthConfig::ApiKey {
                header: "X-API-Key".to_string(),
                secret: "key".to_string(),
            },
        );
        let mut manifest = create_test_manifest(vec![]);
        manifest.auth = auth;

        assert_eq!(
            PluginProxyService::get_auth_header_name(&manifest, "https://api.example.com/test"),
            Some("x-api-key".to_string())
        );
    }

    #[test]
    fn test_auth_header_name_no_match() {
        let manifest = create_test_manifest(vec![]);
        assert_eq!(
            PluginProxyService::get_auth_header_name(&manifest, "https://api.example.com/test"),
            None
        );
    }

    #[tokio::test]
    async fn test_bearer_auth() {
        let service = PluginProxyService::new();
        let mut auth = std::collections::BTreeMap::new();
        auth.insert(
            host("api.github.com"),
            PluginAuthConfig::Bearer {
                secret: "github_token".to_string(),
            },
        );
        let mut manifest = create_test_manifest(vec![]);
        manifest.auth = auth;

        let mut secrets = HashMap::new();
        secrets.insert("github_token".to_string(), "ghp_test123".to_string());

        let result = service
            .get_auth_for_url(
                "test",
                "https://api.github.com/repos",
                &manifest,
                &net_perms(&manifest),
                &PluginCredentials::from_secrets(secrets.clone()),
            )
            .await
            .expect("auth resolution should not error");

        assert_eq!(
            result,
            Some((
                "Authorization".to_string(),
                "Bearer ghp_test123".to_string()
            ))
        );
    }

    #[tokio::test]
    async fn test_basic_auth() {
        let service = PluginProxyService::new();
        let mut auth = std::collections::BTreeMap::new();
        auth.insert(
            host("api.example.com"),
            PluginAuthConfig::Basic {
                username_secret: "user".to_string(),
                password_secret: "pass".to_string(),
            },
        );
        let mut manifest = create_test_manifest(vec![]);
        manifest.auth = auth;

        let mut secrets = HashMap::new();
        secrets.insert("user".to_string(), "admin".to_string());
        secrets.insert("pass".to_string(), "secret".to_string());

        let result = service
            .get_auth_for_url(
                "test",
                "https://api.example.com/data",
                &manifest,
                &net_perms(&manifest),
                &PluginCredentials::from_secrets(secrets.clone()),
            )
            .await
            .expect("auth resolution should not error");

        let expected_encoded = base64::engine::general_purpose::STANDARD.encode("admin:secret");
        assert_eq!(
            result,
            Some((
                "Authorization".to_string(),
                format!("Basic {expected_encoded}")
            ))
        );
    }

    #[tokio::test]
    async fn test_api_key_auth() {
        let service = PluginProxyService::new();
        let mut auth = std::collections::BTreeMap::new();
        auth.insert(
            host("api.example.com"),
            PluginAuthConfig::ApiKey {
                header: "X-API-Key".to_string(),
                secret: "api_key".to_string(),
            },
        );
        let mut manifest = create_test_manifest(vec![]);
        manifest.auth = auth;

        let mut secrets = HashMap::new();
        secrets.insert("api_key".to_string(), "key123".to_string());

        let result = service
            .get_auth_for_url(
                "test",
                "https://api.example.com/data",
                &manifest,
                &net_perms(&manifest),
                &PluginCredentials::from_secrets(secrets.clone()),
            )
            .await
            .expect("auth resolution should not error");

        assert_eq!(
            result,
            Some(("X-API-Key".to_string(), "key123".to_string()))
        );
    }

    #[tokio::test]
    async fn test_no_auth_match() {
        let service = PluginProxyService::new();
        let manifest = create_test_manifest(vec![]);

        let secrets = HashMap::new();
        let result = service
            .get_auth_for_url(
                "test",
                "https://api.example.com/data",
                &manifest,
                &net_perms(&manifest),
                &PluginCredentials::from_secrets(secrets.clone()),
            )
            .await
            .expect("auth resolution should not error");

        assert_eq!(result, None);
    }

    #[tokio::test]
    async fn test_declared_auth_missing_secret_fails_closed() {
        // Auth IS declared for the host but the required secret isn't present.
        // The proxy must not silently send the request unauthenticated: it
        // fails closed with AuthResolution.
        let service = PluginProxyService::new();
        let mut auth = std::collections::BTreeMap::new();
        auth.insert(
            host("api.example.com"),
            PluginAuthConfig::Bearer {
                secret: "github_token".to_string(),
            },
        );
        let mut manifest = create_test_manifest(vec![]);
        manifest.auth = auth;

        // Empty secrets map: the declared secret can't be resolved.
        let secrets = HashMap::new();
        let result = service
            .get_auth_for_url(
                "test",
                "https://api.example.com/data",
                &manifest,
                &net_perms(&manifest),
                &PluginCredentials::from_secrets(secrets.clone()),
            )
            .await;

        assert!(
            matches!(result, Err(PluginProxyError::AuthResolution { .. })),
            "a missing declared secret must fail closed, got: {result:?}"
        );
    }

    // ---- private_key_jwt ----

    /// One 2048-bit key for every test (keygen is slow in a debug build), as
    /// PKCS#1 (what GitHub issues), PKCS#8, and the public half.
    fn test_rsa_key() -> &'static (String, String, String) {
        static KEY: std::sync::OnceLock<(String, String, String)> = std::sync::OnceLock::new();
        KEY.get_or_init(|| {
            use rsa::pkcs1::EncodeRsaPrivateKey;
            use rsa::pkcs8::{EncodePrivateKey, EncodePublicKey, LineEnding};
            let key = rsa::RsaPrivateKey::new(&mut rsa::rand_core::OsRng, 2048).unwrap();
            (
                key.to_pkcs1_pem(LineEnding::LF).unwrap().to_string(),
                key.to_pkcs8_pem(LineEnding::LF).unwrap().to_string(),
                key.to_public_key()
                    .to_public_key_pem(LineEnding::LF)
                    .unwrap(),
            )
        })
    }

    #[test]
    fn a_pem_pasted_on_one_line_is_rebuilt() {
        let (pkcs1, pkcs8, _) = test_rsa_key();
        for pem in [pkcs1, pkcs8] {
            assert_eq!(normalize_pem(pem).as_deref(), Some(pem.as_str()));
            let one_line = pem.replace('\n', " ");
            let rebuilt = normalize_pem(&one_line).expect("one-line PEM rebuilds");
            assert_eq!(&rebuilt, pem);
            jsonwebtoken::EncodingKey::from_rsa_pem(rebuilt.as_bytes())
                .expect("PKCS#1 and PKCS#8 keys both load");
            let joined = pem.replace('\n', "");
            assert_eq!(normalize_pem(&joined).as_deref(), Some(pem.as_str()));
        }
        assert_eq!(normalize_pem("not a key"), None);
        assert_eq!(
            normalize_pem("-----BEGIN RSA PRIVATE KEY----------END RSA PRIVATE KEY-----"),
            None
        );
    }

    #[test]
    fn token_url_placeholders_are_filled_and_path_encoded() {
        let settings = HashMap::from([
            ("id".to_string(), " 42 ".to_string()),
            ("odd".to_string(), "a/b?c".to_string()),
            ("blank".to_string(), "  ".to_string()),
        ]);
        assert_eq!(
            fill_token_url("https://h/app/installations/{id}/access_tokens", &settings).unwrap(),
            "https://h/app/installations/42/access_tokens"
        );
        assert_eq!(
            fill_token_url("https://h/{odd}/t", &settings).unwrap(),
            "https://h/a%2Fb%3Fc/t"
        );
        assert!(fill_token_url("https://h/{blank}/t", &settings)
            .unwrap_err()
            .contains("blank setting is empty"));
        assert!(fill_token_url("https://h/{missing}/t", &settings).is_err());
        assert!(fill_token_url("https://h/{id/t", &settings).is_err());
    }

    #[test]
    fn expiry_reads_a_time_or_seconds() {
        let in_an_hour = (chrono::Utc::now() + chrono::Duration::hours(1)).to_rfc3339();
        let secs = expires_in_secs(Some(&serde_json::json!(in_an_hour))).unwrap();
        assert!((3590..=3600).contains(&secs), "{secs}");
        assert_eq!(expires_in_secs(Some(&serde_json::json!(3600))), Some(3600));
        assert_eq!(expires_in_secs(Some(&serde_json::json!("120"))), Some(120));
        assert_eq!(expires_in_secs(Some(&serde_json::json!("soon"))), None);
        assert_eq!(expires_in_secs(None), None);
    }

    /// A manifest with the GitHub App shape, pointed at a local server.
    fn jwt_manifest(port: u16) -> PluginManifest {
        let mut manifest = create_test_manifest(vec!["network:localhost"]);
        manifest.auth.insert(
            host("localhost"),
            PluginAuthConfig::PrivateKeyJwt {
                private_key: "github_app_private_key".to_string(),
                issuer: "github_app_id".to_string(),
                token_url: format!(
                    "http://localhost:{port}/app/installations/{{github_installation_id}}/access_tokens"
                ),
                token_field: "token".to_string(),
                expires_field: "expires_at".to_string(),
            },
        );
        manifest
    }

    fn jwt_creds(pem: &str) -> PluginCredentials {
        PluginCredentials {
            secrets: HashMap::from([("github_app_private_key".to_string(), pem.to_string())]),
            settings: HashMap::from([
                ("github_app_id".to_string(), "12345".to_string()),
                ("github_installation_id".to_string(), "42".to_string()),
            ]),
            cache_scope: "1:test-plugin".to_string(),
        }
    }

    /// Serve `replies` in order, one connection each, and hand back the
    /// request text each connection carried.
    async fn serve_recording(replies: Vec<String>) -> (u16, tokio::task::JoinHandle<Vec<String>>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = tokio::spawn(async move {
            let mut seen = Vec::new();
            for reply in replies {
                let Ok(Ok((mut sock, _))) =
                    tokio::time::timeout(std::time::Duration::from_secs(5), listener.accept())
                        .await
                else {
                    break;
                };
                let mut buf = vec![0u8; 8192];
                let n = sock.read(&mut buf).await.unwrap_or(0);
                seen.push(String::from_utf8_lossy(&buf[..n]).to_string());
                let _ = sock.write_all(reply.as_bytes()).await;
                let _ = sock.flush().await;
            }
            seen
        });
        (port, handle)
    }

    fn http_json(status: &str, body: &str) -> String {
        format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
    }

    fn header_value<'a>(request: &'a str, name: &str) -> Option<&'a str> {
        request.lines().find_map(|line| {
            let (k, v) = line.split_once(':')?;
            k.trim().eq_ignore_ascii_case(name).then(|| v.trim())
        })
    }

    /// The whole exchange: a signed RS256 JWT goes to the filled-in token URL,
    /// the returned token is injected on the request, and a second request
    /// reuses the cached token rather than exchanging again.
    #[tokio::test]
    async fn private_key_jwt_exchanges_injects_and_caches() {
        let _guard = ENV_LOCK.lock().await;
        std::env::set_var("NOSDESK_OUTBOUND_ALLOWED_HOSTS", "localhost");

        let expires_at = (chrono::Utc::now() + chrono::Duration::hours(1)).to_rfc3339();
        let (port, server) = serve_recording(vec![
            http_json(
                "201 Created",
                &format!(r#"{{"token":"ghs_installation","expires_at":"{expires_at}"}}"#),
            ),
            http_json("200 OK", "[]"),
            http_json("200 OK", "[]"),
        ])
        .await;

        let (pkcs1, _, public) = test_rsa_key();
        let manifest = jwt_manifest(port);
        // Pasted into a single-line input: the newlines are gone.
        let creds = jwt_creds(&pkcs1.replace('\n', " "));
        let service = PluginProxyService::new();
        for _ in 0..2 {
            let mut req = proxy_req(&format!(
                "http://localhost:{port}/installation/repositories"
            ));
            // A plugin can't supply its own Authorization for a declared host.
            req.headers = Some(HashMap::from([(
                "Authorization".to_string(),
                "Bearer from-the-plugin".to_string(),
            )]));
            let res = service
                .proxy_request_with("test-plugin", &manifest, &net_perms(&manifest), req, &creds)
                .await;
            assert!(res.is_ok(), "proxied request should succeed: {res:?}");
        }

        std::env::remove_var("NOSDESK_OUTBOUND_ALLOWED_HOSTS");
        let seen = server.await.unwrap();
        assert_eq!(seen.len(), 3, "one exchange, then two API calls");

        let exchange = &seen[0];
        assert!(
            exchange.starts_with("POST /app/installations/42/access_tokens "),
            "{exchange}"
        );
        assert!(header_value(exchange, "user-agent").is_some());
        let jwt = header_value(exchange, "authorization")
            .and_then(|v| v.strip_prefix("Bearer "))
            .expect("the exchange carries the JWT");
        let mut validation = jsonwebtoken::Validation::new(jsonwebtoken::Algorithm::RS256);
        validation.set_required_spec_claims(&["exp", "iat", "iss"]);
        validation.set_issuer(&["12345"]);
        let claims = jsonwebtoken::decode::<serde_json::Value>(
            jwt,
            &jsonwebtoken::DecodingKey::from_rsa_pem(public.as_bytes()).unwrap(),
            &validation,
        )
        .expect("the JWT verifies with the app's public key")
        .claims;
        let iat = claims["iat"].as_i64().unwrap();
        let exp = claims["exp"].as_i64().unwrap();
        assert_eq!(
            exp - iat,
            600,
            "iat is backdated 60 s and exp is 9 min ahead"
        );

        for api_call in &seen[1..] {
            assert_eq!(
                header_value(api_call, "authorization"),
                Some("Bearer ghs_installation"),
                "{api_call}"
            );
        }
    }

    /// A refused exchange is `auth_failed` carrying the endpoint's message, and
    /// the API request is never sent.
    #[tokio::test]
    async fn a_refused_exchange_is_auth_failed_with_the_upstream_message() {
        let _guard = ENV_LOCK.lock().await;
        std::env::set_var("NOSDESK_OUTBOUND_ALLOWED_HOSTS", "localhost");

        let (port, server) = serve_recording(vec![http_json(
            "401 Unauthorized",
            r#"{"message":"A JSON web token could not be decoded","documentation_url":"https://docs.github.com"}"#,
        )])
        .await;
        let (_, pkcs8, _) = test_rsa_key();
        let manifest = jwt_manifest(port);
        let mut creds = jwt_creds(pkcs8);
        creds.cache_scope = "1:refused".to_string();
        let res = PluginProxyService::new()
            .proxy_request_with(
                "test-plugin",
                &manifest,
                &net_perms(&manifest),
                proxy_req(&format!(
                    "http://localhost:{port}/installation/repositories"
                )),
                &creds,
            )
            .await;

        std::env::remove_var("NOSDESK_OUTBOUND_ALLOWED_HOSTS");
        match res {
            Err(PluginProxyError::AuthFailed { message }) => {
                assert_eq!(message, "A JSON web token could not be decoded")
            }
            other => panic!("expected AuthFailed, got {other:?}"),
        }
        assert_eq!(
            server.await.unwrap().len(),
            1,
            "only the exchange was attempted"
        );
    }

    /// Settings problems are `auth_failed` too, before anything is sent.
    #[tokio::test]
    async fn missing_or_malformed_settings_are_auth_failed() {
        let manifest = jwt_manifest(9);
        let service = PluginProxyService::new();
        let perms = net_perms(&manifest);
        let (pkcs1, _, _) = test_rsa_key();
        let url = "http://localhost:9/installation/repositories";

        let mut no_installation = jwt_creds(pkcs1);
        no_installation.settings.remove("github_installation_id");
        let mut no_key = jwt_creds(pkcs1);
        no_key.secrets.clear();
        let not_a_key =
            jwt_creds("-----BEGIN RSA PRIVATE KEY-----\nAAAA\n-----END RSA PRIVATE KEY-----");

        for (creds, want) in [
            (no_installation, "github_installation_id setting is empty"),
            (no_key, "github_app_private_key setting is empty"),
            (not_a_key, "not an RSA private key"),
        ] {
            match service
                .get_auth_for_url("test-plugin", url, &manifest, &perms, &creds)
                .await
            {
                Err(PluginProxyError::AuthFailed { message }) => {
                    assert!(message.contains(want), "expected {want:?}, got {message:?}")
                }
                other => panic!("expected AuthFailed for {want:?}, got {other:?}"),
            }
        }
    }

    #[test]
    fn auth_failed_body_names_the_error() {
        let body: serde_json::Value =
            serde_json::from_str(&auth_failed_body("Bad credentials")).unwrap();
        assert_eq!(
            body,
            serde_json::json!({ "error": "auth_failed", "message": "Bad credentials" })
        );
    }
}
