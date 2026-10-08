//! Authenticated asset proxy for the webview.
//!
//! The webview can't load workspace-scoped files directly: a relative URL
//! resolves against the `tauri://localhost` origin, and a direct `<img>` /
//! `<audio>` / `<video>` load carries no bearer (the auth interceptor only
//! applies to the native HTTP plugin, not webview resource loads). So file URLs
//! are rewritten to the `nosdesk-asset` scheme and this Rust handler proxies
//! each request to the real API with the session bearer, forwarding `Range` so
//! media seeks natively. The body is a full buffer per response (wry has no
//! streaming body type); seeking works because the webview pulls successive
//! bounded ranges. The backend resolves the workspace from the resource, so the
//! proxied request only needs the bearer.

use std::borrow::Cow;
use std::sync::{Arc, RwLock};

use tauri::http::{header, Request, Response, StatusCode};
use tauri::{Manager, Runtime, UriSchemeContext, UriSchemeResponder};

/// Custom scheme the webview uses for authenticated file URLs. iOS sees
/// `nosdesk-asset://localhost/<path>`; Android rewrites it to
/// `http://nosdesk-asset.localhost/<path>`. Either way the request reaches this
/// handler and `uri().path_and_query()` carries the API path.
pub const SCHEME: &str = "nosdesk-asset";

/// Session the handler reads on each request, updated from JS
/// (`set_asset_proxy_session`) on login / refresh / logout.
#[derive(Clone)]
pub struct AssetProxy {
    session: Arc<RwLock<Session>>,
    client: reqwest::Client,
}

#[derive(Default)]
struct Session {
    token: Option<String>,
    base_url: Option<String>,
}

impl AssetProxy {
    pub fn new() -> Self {
        Self {
            session: Arc::new(RwLock::new(Session::default())),
            client: reqwest::Client::new(),
        }
    }

    fn set(&self, token: Option<String>, base_url: Option<String>) {
        let mut s = self.session.write().unwrap();
        s.token = token;
        // Keep the last base if the caller didn't supply one (token-only refresh).
        if base_url.is_some() {
            s.base_url = base_url;
        }
    }

    fn read(&self) -> (Option<String>, Option<String>) {
        let s = self.session.read().unwrap();
        (s.token.clone(), s.base_url.clone())
    }
}

impl Default for AssetProxy {
    fn default() -> Self {
        Self::new()
    }
}

/// JS sets the bearer + API base so the handler can authenticate proxied fetches.
/// Called from the transport on `setSession`, token refresh, and logout (null).
#[tauri::command]
pub fn set_asset_proxy_session(
    proxy: tauri::State<'_, AssetProxy>,
    token: Option<String>,
    base_url: Option<String>,
) {
    proxy.set(token, base_url);
}

fn respond_status(responder: UriSchemeResponder, status: StatusCode) {
    if let Ok(resp) = Response::builder()
        .status(status)
        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
        .body(Cow::<[u8]>::Owned(Vec::new()))
    {
        responder.respond(resp);
    }
}

/// The request the proxy makes for one webview load.
#[derive(Debug, PartialEq)]
struct Upstream {
    url: String,
    headers: Vec<(header::HeaderName, String)>,
}

/// The upstream GET for `path`: the API base plus the path, with the bearer
/// when signed in and the webview's `Range` when it sent one. `None` only when
/// no server is configured yet. Signed out, the request still goes, without a
/// bearer: public files (the workspace's logo on the sign-in and invitation
/// pages) load, and the server answers 401 for anything else.
fn upstream(
    token: Option<&str>,
    base_url: Option<&str>,
    path: &str,
    range: Option<&str>,
) -> Option<Upstream> {
    let base_url = base_url?;
    let mut headers = Vec::new();
    if let Some(token) = token {
        headers.push((header::AUTHORIZATION, format!("Bearer {token}")));
    }
    if let Some(range) = range {
        headers.push((header::RANGE, range.to_owned()));
    }
    Some(Upstream {
        url: format!("{}{}", base_url.trim_end_matches('/'), path),
        headers,
    })
}

/// Proxy handler for the `nosdesk-asset` scheme.
pub fn handle<R: Runtime>(
    ctx: UriSchemeContext<'_, R>,
    request: Request<Vec<u8>>,
    responder: UriSchemeResponder,
) {
    let proxy = ctx.app_handle().state::<AssetProxy>().inner().clone();
    let (token, base_url) = proxy.read();

    // The scheme path carries the full API path (e.g. `/api/files/tickets/1/x`).
    let path = request
        .uri()
        .path_and_query()
        .map(|pq| pq.as_str().to_string())
        .unwrap_or_default();
    let range = request
        .headers()
        .get(header::RANGE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);

    let Some(upstream) = upstream(
        token.as_deref(),
        base_url.as_deref(),
        &path,
        range.as_deref(),
    ) else {
        // No server configured yet.
        return respond_status(responder, StatusCode::UNAUTHORIZED);
    };

    let client = proxy.client.clone();
    tauri::async_runtime::spawn(async move {
        let mut req = client.get(&upstream.url);
        for (name, value) in upstream.headers {
            req = req.header(name, value);
        }

        let upstream = match req.send().await {
            Ok(resp) => resp,
            Err(_) => return respond_status(responder, StatusCode::BAD_GATEWAY),
        };
        let status = upstream.status();
        let headers = upstream.headers().clone();
        let body = match upstream.bytes().await {
            Ok(b) => b.to_vec(),
            Err(_) => return respond_status(responder, StatusCode::BAD_GATEWAY),
        };

        let mut builder = Response::builder().status(status.as_u16());
        // Carry through the headers the media element + cache need.
        for name in [
            header::CONTENT_TYPE,
            header::CONTENT_RANGE,
            header::ACCEPT_RANGES,
            header::CONTENT_LENGTH,
            header::CACHE_CONTROL,
        ] {
            if let Some(value) = headers.get(&name) {
                builder = builder.header(name, value);
            }
        }
        // The webview `fetch()`es audio (the player reads the bytes for its
        // waveform) from the tauri://localhost origin, so the proxied response
        // needs CORS. Plain `<img>`/`<audio>` resource loads don't, but fetch
        // enforces it, which is why audio failed while images worked.
        builder = builder
            .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
            .header(
                header::ACCESS_CONTROL_EXPOSE_HEADERS,
                "Content-Range, Accept-Ranges, Content-Length",
            );

        match builder.body(Cow::<[u8]>::Owned(body)) {
            Ok(resp) => responder.respond(resp),
            Err(_) => respond_status(responder, StatusCode::INTERNAL_SERVER_ERROR),
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signed_out_a_load_goes_without_a_bearer() {
        let up = upstream(
            None,
            Some("https://help.example/"),
            "/uploads/branding/logo.png",
            None,
        )
        .expect("forwarded");
        assert_eq!(up.url, "https://help.example/uploads/branding/logo.png");
        assert!(up.headers.is_empty());
    }

    #[test]
    fn signed_in_a_load_carries_the_bearer_and_range() {
        let up = upstream(
            Some("tok"),
            Some("https://help.example"),
            "/api/files/tickets/1/a.mp4",
            Some("bytes=0-1023"),
        )
        .expect("forwarded");
        assert_eq!(up.url, "https://help.example/api/files/tickets/1/a.mp4");
        assert_eq!(
            up.headers,
            vec![
                (header::AUTHORIZATION, "Bearer tok".to_string()),
                (header::RANGE, "bytes=0-1023".to_string()),
            ]
        );
    }

    #[test]
    fn without_a_server_nothing_is_forwarded() {
        assert_eq!(upstream(Some("tok"), None, "/api/files/x", None), None);
    }
}
