//! Plugin Proxy Service
//!
//! Proxies external HTTP requests for plugins, providing:
//! - Permission validation (plugins can only access whitelisted domains)
//! - Declarative auth injection (manifest-driven, no hardcoded domain checks)
//! - OAuth2 client credentials with token caching
//! - Request logging for audit
//! - Rate limiting (future)
//! - Response sanitization

use base64::Engine;
use reqwest::{Client, Method};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tracing::{debug, error, info, warn};

use crate::models::{PluginAuthConfig, PluginManifest, PluginProxyRequest, PluginProxyResponse};

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
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("Failed to create HTTP client");

        Self {
            client,
            token_cache: Mutex::new(HashMap::new()),
        }
    }

    /// Check if a plugin has permission to access a URL
    ///
    /// Permissions are in the format "external:<domain>" where domain can be:
    /// - Exact match: "external:api.example.com"
    /// - Wildcard: "external:*.example.com"
    fn has_permission(&self, manifest: &PluginManifest, url: &str) -> bool {
        let parsed = match url::Url::parse(url) {
            Ok(u) => u,
            Err(_) => return false,
        };

        let host = match parsed.host_str() {
            Some(h) => h,
            None => return false,
        };

        for permission in &manifest.permissions {
            if let Some(domain) = permission.strip_prefix("external:") {
                // Check for wildcard match
                if domain.starts_with("*.") {
                    let suffix = &domain[1..]; // Gets ".example.com"
                    if host.ends_with(suffix) || host == &domain[2..] {
                        return true;
                    }
                } else if domain == host {
                    return true;
                }
            }
        }

        false
    }

    /// Get the auth header name that the manifest's auth config would inject for a given URL.
    /// Used to strip plugin-supplied headers that would conflict with declared auth.
    fn get_auth_header_name(manifest: &PluginManifest, url: &str) -> Option<String> {
        let parsed = url::Url::parse(url).ok()?;
        let host = parsed.host_str()?;

        let auth_config = manifest.auth.iter().find(|(domain, _)| {
            if domain.starts_with("*.") {
                let suffix = &domain[1..];
                host.ends_with(suffix) || host == &domain[2..]
            } else {
                *domain == host
            }
        })?.1;

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
    ) -> Option<(String, String)> {
        let parsed = url::Url::parse(url).ok()?;
        let host = parsed.host_str()?;

        // Find matching auth config from manifest
        let auth_config = manifest.auth.iter().find(|(domain, _)| {
            if domain.starts_with("*.") {
                let suffix = &domain[1..];
                host.ends_with(suffix) || host == &domain[2..]
            } else {
                *domain == host
            }
        })?.1;

        match auth_config {
            PluginAuthConfig::Bearer { secret } => {
                let token = secrets.get(secret)?;
                Some(("Authorization".into(), format!("Bearer {token}")))
            }
            PluginAuthConfig::Basic { username_secret, password_secret } => {
                let user = secrets.get(username_secret)?;
                let pass = secrets.get(password_secret)?;
                let encoded = base64::engine::general_purpose::STANDARD
                    .encode(format!("{user}:{pass}"));
                Some(("Authorization".into(), format!("Basic {encoded}")))
            }
            PluginAuthConfig::ApiKey { header, secret } => {
                let value = secrets.get(secret)?;
                Some((header.clone(), value.clone()))
            }
            PluginAuthConfig::Oauth2ClientCredentials {
                token_url, client_id_secret, client_secret_secret,
            } => {
                let client_id = secrets.get(client_id_secret)?;
                let client_secret = secrets.get(client_secret_secret)?;

                // Check token cache
                let cache_key = format!("{plugin_name}:{host}");
                if let Ok(cache) = self.token_cache.lock() {
                    if let Some(cached) = cache.get(&cache_key) {
                        if cached.expires_at > Instant::now() {
                            return Some(("Authorization".into(), cached.token.clone()));
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
                ).ok()?;

                let resp = self.client
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
                    })
                    .ok()?;

                let resp_status = resp.status();
                let resp_text = resp.text().await.map_err(|e| {
                    error!(
                        plugin = plugin_name,
                        error = %e,
                        "Failed to read OAuth2 token response body"
                    );
                }).ok()?;

                if !resp_status.is_success() {
                    error!(
                        plugin = plugin_name,
                        status = %resp_status,
                        body_preview = %resp_text.chars().take(500).collect::<String>(),
                        "OAuth2 token endpoint returned error"
                    );
                    return None;
                }

                let json: serde_json::Value = serde_json::from_str(&resp_text).map_err(|e| {
                    error!(
                        plugin = plugin_name,
                        error = %e,
                        body_preview = %resp_text.chars().take(500).collect::<String>(),
                        "Failed to parse OAuth2 token response as JSON"
                    );
                }).ok()?;

                let access_token = json.get("access_token")?.as_str()?;
                let bearer = format!("Bearer {access_token}");

                // Cache with TTL (use expires_in from response, default 50 min)
                let ttl = json.get("expires_in")
                    .and_then(|v| v.as_u64())
                    .map(|s| s.saturating_sub(60)) // 1 min buffer
                    .unwrap_or(3000);

                if let Ok(mut cache) = self.token_cache.lock() {
                    cache.insert(cache_key, CachedToken {
                        token: bearer.clone(),
                        expires_at: Instant::now() + Duration::from_secs(ttl),
                    });
                }

                Some(("Authorization".into(), bearer))
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
    ) -> Result<PluginProxyResponse, String> {
        // Check permission
        if !self.has_permission(manifest, &request.url) {
            warn!(
                plugin = plugin_name,
                url = request.url,
                "Plugin denied access to URL - no matching external permission"
            );
            return Err(format!(
                "Plugin '{}' does not have permission to access '{}'",
                plugin_name, request.url
            ));
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
            _ => return Err(format!("Unsupported HTTP method: {}", request.method)),
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

        // Inject authorization from manifest auth config
        if let Some((header_name, header_value)) = self
            .get_auth_for_url(plugin_name, &request.url, manifest, secrets)
            .await
        {
            debug!(plugin = plugin_name, "Injecting auth header from manifest config");
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

        // Execute the request
        let response = req.send().await.map_err(|e| {
            error!(
                plugin = plugin_name,
                url = request.url,
                error = %e,
                "Failed to execute proxied request"
            );
            format!("Request failed: {e}")
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
                .or_else(|| Some(serde_json::Value::String(text)))
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

    fn create_test_manifest(permissions: Vec<String>) -> PluginManifest {
        PluginManifest {
            name: "test-plugin".to_string(),
            display_name: "Test Plugin".to_string(),
            version: "1.0.0".to_string(),
            description: None,
            icon: None,
            repository: None,
            homepage: None,
            author: None,
            permissions,
            components: HashMap::new(),
            events: vec![],
            settings: vec![],
            collections: HashMap::new(),
            auth: HashMap::new(),
        }
    }

    #[test]
    fn test_exact_domain_permission() {
        let service = PluginProxyService::new();
        let manifest = create_test_manifest(vec!["external:api.example.com".to_string()]);

        assert!(service.has_permission(&manifest, "https://api.example.com/v1/data"));
        assert!(!service.has_permission(&manifest, "https://other.example.com/data"));
        assert!(!service.has_permission(&manifest, "https://api.other.com/data"));
    }

    #[test]
    fn test_wildcard_domain_permission() {
        let service = PluginProxyService::new();
        let manifest = create_test_manifest(vec!["external:*.example.com".to_string()]);

        assert!(service.has_permission(&manifest, "https://api.example.com/v1/data"));
        assert!(service.has_permission(&manifest, "https://www.example.com/data"));
        assert!(service.has_permission(&manifest, "https://example.com/data"));
        assert!(!service.has_permission(&manifest, "https://api.other.com/data"));
    }

    #[test]
    fn test_no_permission() {
        let service = PluginProxyService::new();
        let manifest = create_test_manifest(vec!["tickets:read".to_string()]);

        assert!(!service.has_permission(&manifest, "https://api.example.com/data"));
    }

    #[test]
    fn test_auth_header_name_bearer() {
        let mut auth = HashMap::new();
        auth.insert(
            "api.example.com".to_string(),
            PluginAuthConfig::Bearer { secret: "token".to_string() },
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
        let mut auth = HashMap::new();
        auth.insert(
            "api.example.com".to_string(),
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
        let mut auth = HashMap::new();
        auth.insert(
            "api.github.com".to_string(),
            PluginAuthConfig::Bearer { secret: "github_token".to_string() },
        );
        let mut manifest = create_test_manifest(vec![]);
        manifest.auth = auth;

        let mut secrets = HashMap::new();
        secrets.insert("github_token".to_string(), "ghp_test123".to_string());

        let result = service
            .get_auth_for_url("test", "https://api.github.com/repos", &manifest, &secrets)
            .await;

        assert_eq!(
            result,
            Some(("Authorization".to_string(), "Bearer ghp_test123".to_string()))
        );
    }

    #[tokio::test]
    async fn test_basic_auth() {
        let service = PluginProxyService::new();
        let mut auth = HashMap::new();
        auth.insert(
            "api.example.com".to_string(),
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
            .await;

        let expected_encoded = base64::engine::general_purpose::STANDARD.encode("admin:secret");
        assert_eq!(
            result,
            Some(("Authorization".to_string(), format!("Basic {expected_encoded}")))
        );
    }

    #[tokio::test]
    async fn test_api_key_auth() {
        let service = PluginProxyService::new();
        let mut auth = HashMap::new();
        auth.insert(
            "api.example.com".to_string(),
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
            .await;

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
            .await;

        assert_eq!(result, None);
    }

    #[tokio::test]
    async fn test_wildcard_auth_match() {
        let service = PluginProxyService::new();
        let mut auth = HashMap::new();
        auth.insert(
            "*.example.com".to_string(),
            PluginAuthConfig::Bearer { secret: "token".to_string() },
        );
        let mut manifest = create_test_manifest(vec![]);
        manifest.auth = auth;

        let mut secrets = HashMap::new();
        secrets.insert("token".to_string(), "mytoken".to_string());

        let result = service
            .get_auth_for_url("test", "https://api.example.com/data", &manifest, &secrets)
            .await;

        assert_eq!(
            result,
            Some(("Authorization".to_string(), "Bearer mytoken".to_string()))
        );
    }
}
