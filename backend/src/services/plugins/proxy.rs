//! Plugin Proxy Service
//!
//! Proxies external HTTP requests for plugins, providing:
//! - Permission validation (plugins can only access whitelisted domains)
//! - Declarative auth injection (manifest-driven, no hardcoded domain checks)
//! - OAuth2 client credentials with token caching
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
        }
    }
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
        let client = crate::utils::safe_http::client(Duration::from_secs(30))
            .expect("Failed to create SSRF-safe HTTP client");

        Self {
            client,
            token_cache: Mutex::new(HashMap::new()),
        }
    }

    /// Check if a plugin has permission to access a URL.
    ///
    /// Both the URL host and the manifest's `network:<pattern>`
    /// permissions go through the same `Host` / `HostPattern`
    /// parsers, so case, normalisation, and wildcard semantics are
    /// shared with the validator. Any URL that doesn't normalise
    /// to a valid named host (IP literals, malformed URLs) is
    /// refused outright.
    fn has_permission(&self, manifest: &PluginManifest, url: &str) -> bool {
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

        manifest
            .permissions
            .iter()
            .filter_map(crate::services::plugins::types::Permission::network_pattern)
            .any(|pattern| pattern.matches(&host))
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
        secrets: &HashMap<String, String>,
    ) -> Result<Option<(String, String)>, PluginProxyError> {
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
                if !self.has_permission(manifest, token_url) {
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
        }
    }

    /// Execute a proxied request for a plugin
    ///
    /// The `secrets` parameter contains plugin settings marked as secrets,
    /// which are used to inject Authorization headers based on the manifest's auth config.
    pub async fn proxy_request(
        &self,
        plugin_name: &str,
        manifest: &PluginManifest,
        request: PluginProxyRequest,
        secrets: &HashMap<String, String>,
    ) -> Result<PluginProxyResponse, PluginProxyError> {
        // Check permission
        if !self.has_permission(manifest, &request.url) {
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

        // Determine which header the auth config will inject (if any)
        let auth_header_name = Self::get_auth_header_name(manifest, &request.url);

        // Build the request
        let mut req = self.client.request(method, &request.url);

        // Add headers from request (strip any that would conflict with declared auth)
        if let Some(headers) = request.headers {
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
            .get_auth_for_url(plugin_name, &request.url, manifest, secrets)
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
        if let Some(body) = request.body {
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
        } else if request.method.eq_ignore_ascii_case("POST")
            || request.method.eq_ignore_ascii_case("PUT")
            || request.method.eq_ignore_ascii_case("PATCH")
        {
            // Some servers require Content-Length even for empty bodies
            req = req.header("Content-Length", "0");
        }

        // The safe_http client refuses internal IPs returned by
        // DNS; this catches the IP-literal case (a manifest
        // declaring `http://127.0.0.1/whatever` doesn't trigger
        // the resolver). The manifest's network allowlist already
        // ran upstream of this call, so reaching this guard is
        // already the "manifest said it was OK" path.
        if let Err(e) = crate::utils::safe_http::reject_unsafe_ip_literal(&request.url) {
            warn!(
                plugin = plugin_name,
                url = request.url,
                error = %e,
                "plugin proxy blocked by SSRF guard"
            );
            return Err(PluginProxyError::Blocked(e));
        }

        // Execute the request
        let response = req.send().await.map_err(|e| {
            error!(
                plugin = plugin_name,
                url = request.url,
                error = %e,
                "Failed to execute proxied request"
            );
            PluginProxyError::Network(e)
        })?;

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
        }
    }

    fn host(s: &str) -> Host {
        Host::parse(s).expect("test host must be valid")
    }

    #[test]
    fn test_exact_domain_permission() {
        let service = PluginProxyService::new();
        let manifest = create_test_manifest(vec!["network:api.example.com"]);

        assert!(service.has_permission(&manifest, "https://api.example.com/v1/data"));
        assert!(!service.has_permission(&manifest, "https://other.example.com/data"));
        assert!(!service.has_permission(&manifest, "https://api.other.com/data"));
    }

    #[test]
    fn test_wildcard_domain_permission() {
        let service = PluginProxyService::new();
        let manifest = create_test_manifest(vec!["network:*.example.com"]);

        assert!(service.has_permission(&manifest, "https://api.example.com/v1/data"));
        assert!(service.has_permission(&manifest, "https://www.example.com/data"));
        assert!(service.has_permission(&manifest, "https://example.com/data"));
        assert!(!service.has_permission(&manifest, "https://api.other.com/data"));
    }

    #[test]
    fn test_wildcard_does_not_match_deeper_subdomain() {
        // Single-label wildcard semantics: `*.example.com` does
        // NOT cover `v1.api.example.com`. This is the correct
        // outcome for the spec we documented; record it.
        let service = PluginProxyService::new();
        let manifest = create_test_manifest(vec!["network:*.example.com"]);
        assert!(!service.has_permission(&manifest, "https://v1.api.example.com/data"));
    }

    #[test]
    fn test_url_with_uppercase_host_normalises() {
        // Browsers and most HTTP clients lowercase the host, but
        // a curl-style direct call could include uppercase. The
        // typed `Host::parse` enforces lowercase normalisation on
        // both sides so the match is consistent.
        let service = PluginProxyService::new();
        let manifest = create_test_manifest(vec!["network:api.example.com"]);
        assert!(service.has_permission(&manifest, "https://API.EXAMPLE.COM/data"));
    }

    #[test]
    fn test_ip_literal_url_is_refused() {
        // The proxy refuses URL hosts that don't normalise to a
        // named host. `Host::parse` rejects IP literals.
        let service = PluginProxyService::new();
        let manifest = create_test_manifest(vec!["network:*.example.com"]);
        assert!(!service.has_permission(&manifest, "https://127.0.0.1/data"));
    }

    #[test]
    fn test_no_permission() {
        let service = PluginProxyService::new();
        let manifest = create_test_manifest(vec!["ticket:read"]);

        assert!(!service.has_permission(&manifest, "https://api.example.com/data"));
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
            .get_auth_for_url("test", "https://api.github.com/repos", &manifest, &secrets)
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
            .get_auth_for_url("test", "https://api.example.com/data", &manifest, &secrets)
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
            .get_auth_for_url("test", "https://api.example.com/data", &manifest, &secrets)
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
            .get_auth_for_url("test", "https://api.example.com/data", &manifest, &secrets)
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
            .get_auth_for_url("test", "https://api.example.com/data", &manifest, &secrets)
            .await;

        assert!(
            matches!(result, Err(PluginProxyError::AuthResolution { .. })),
            "a missing declared secret must fail closed, got: {result:?}"
        );
    }
}
