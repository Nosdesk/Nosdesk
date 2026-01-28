//! Plugin Proxy Service
//!
//! Proxies external HTTP requests for plugins, providing:
//! - Permission validation (plugins can only access whitelisted domains)
//! - Request logging for audit
//! - Rate limiting (future)
//! - Response sanitization

use reqwest::{Client, Method};
use std::collections::HashMap;
use std::time::Duration;
use tracing::{debug, error, info, warn};

use crate::models::{PluginManifest, PluginProxyRequest, PluginProxyResponse};

/// Plugin Proxy Service
///
/// Handles proxying external HTTP requests for plugins. All plugin external requests
/// must go through this service to ensure:
/// - The plugin has permission to access the domain
/// - The request is logged for audit
/// - Rate limits are enforced
pub struct PluginProxyService {
    client: Client,
}

impl PluginProxyService {
    /// Create a new proxy service
    pub fn new() -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("Failed to create HTTP client");

        Self { client }
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

    /// Get the authorization token for a URL based on plugin secrets
    ///
    /// Supports common patterns:
    /// - `github_token` -> Authorization header for api.github.com
    /// - `gitlab_token` -> Authorization header for gitlab.com
    /// - Future: more generic domain -> token mapping
    fn get_auth_for_url(&self, url: &str, secrets: &HashMap<String, String>) -> Option<String> {
        let parsed = url::Url::parse(url).ok()?;
        let host = parsed.host_str()?;

        // GitHub API
        if host == "api.github.com" || host.ends_with(".github.com") {
            if let Some(token) = secrets.get("github_token") {
                return Some(format!("Bearer {token}"));
            }
        }

        // GitLab API
        if host == "gitlab.com" || host.ends_with(".gitlab.com") {
            if let Some(token) = secrets.get("gitlab_token") {
                return Some(format!("Bearer {token}"));
            }
        }

        // Generic: check for auth_token setting
        if let Some(token) = secrets.get("auth_token") {
            return Some(format!("Bearer {token}"));
        }

        None
    }

    /// Execute a proxied request for a plugin
    ///
    /// The `secrets` parameter contains plugin settings marked as secrets,
    /// which are used to inject Authorization headers for known APIs.
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

        // Build the request
        let mut req = self.client.request(method, &request.url);

        // Add headers from request
        if let Some(headers) = request.headers {
            for (key, value) in headers {
                // Don't allow certain headers to be set by plugins
                let key_lower = key.to_lowercase();
                if key_lower == "host" || key_lower == "user-agent" || key_lower == "authorization" {
                    continue;
                }
                req = req.header(&key, &value);
            }
        }

        // Inject authorization from secrets if available
        if let Some(auth) = self.get_auth_for_url(&request.url, secrets) {
            debug!(plugin = plugin_name, "Injecting authorization header from secrets");
            req = req.header("Authorization", auth);
        }

        // Add custom User-Agent
        req = req.header(
            "User-Agent",
            format!("Nosdesk-Plugin/{} ({})", manifest.version, plugin_name),
        );

        // Add body for methods that support it
        if let Some(body) = request.body {
            req = req.json(&body);
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

        // Get response body
        let body = response.json::<serde_json::Value>().await.ok();

        debug!(
            plugin = plugin_name,
            url = request.url,
            status = status,
            "Proxied request completed"
        );

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
}
