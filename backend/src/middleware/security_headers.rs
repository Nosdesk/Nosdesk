//! Security response headers middleware.
//!
//! Sets a comprehensive set of security-relevant response headers
//! on every HTTP response: Content-Security-Policy (CSP),
//! X-Content-Type-Options, X-Frame-Options, Referrer-Policy,
//! Permissions-Policy, Strict-Transport-Security (production),
//! Cross-Origin-Opener-Policy, Cross-Origin-Resource-Policy, and
//! environment-aware Cache-Control for static assets.
//!
//! ## Why a typed CSP builder
//!
//! The previous implementation built the CSP header as a single
//! `format!` string. That works but is error-prone — directives
//! and source tokens are stringly-typed, you can typo a directive
//! name or a source keyword and the browser silently drops it.
//! Worse, adding a new directive is a copy-paste of the string,
//! which is how policies grow inconsistencies between dev and
//! production over time.
//!
//! The typed builder below makes invalid policies harder to write:
//! directives are an enum, source tokens are an enum (or explicit
//! Host strings), and the builder enforces the canonical
//! `directive token1 token2 ; ...` shape on render.
//!
//! ## Rollout
//!
//! `CSP_REPORT_ONLY=true` switches enforcement to the
//! `Content-Security-Policy-Report-Only` header so policy changes
//! can be observed without breaking users immediately. Combined
//! with a `report-uri` (not yet wired) this is the safe rollout
//! path for tightening directives.
//!
//! ## What this NOT yet handles
//!
//! - `report-uri` / `report-to`: violation reports aren't yet
//!   collected. Adding a `/api/csp-report` collector is captured
//!   in docs/security-audit-plan.md.
//! - Trusted Types: requires Vue-side TT policy registration
//!   first (every `v-html` path needs to declare TT compatibility).
//!   Phase 4 frontend work per the existing plan.
//! - Per-response nonces: would let us drop `'unsafe-inline'` from
//!   style-src. Requires plumbing a request-scoped nonce into the
//!   SPA bootstrap, which is non-trivial in Vite-built SPAs.

use actix_web::{
    dev::{forward_ready, Service, ServiceRequest, ServiceResponse, Transform},
    http::header,
    Error,
};
use futures::future::LocalBoxFuture;
use std::future::{ready, Ready};

// ---------------------------------------------------------------
// Typed CSP builder
// ---------------------------------------------------------------

/// CSP directive name. Spec reference:
/// https://www.w3.org/TR/CSP3/#csp-directives
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Directive {
    DefaultSrc,
    ScriptSrc,
    StyleSrc,
    ImgSrc,
    FontSrc,
    ConnectSrc,
    MediaSrc,
    WorkerSrc,
    FrameSrc,
    FrameAncestors,
    BaseUri,
    FormAction,
    ObjectSrc,
    ManifestSrc,
}

impl Directive {
    fn name(self) -> &'static str {
        match self {
            Directive::DefaultSrc => "default-src",
            Directive::ScriptSrc => "script-src",
            Directive::StyleSrc => "style-src",
            Directive::ImgSrc => "img-src",
            Directive::FontSrc => "font-src",
            Directive::ConnectSrc => "connect-src",
            Directive::MediaSrc => "media-src",
            Directive::WorkerSrc => "worker-src",
            Directive::FrameSrc => "frame-src",
            Directive::FrameAncestors => "frame-ancestors",
            Directive::BaseUri => "base-uri",
            Directive::FormAction => "form-action",
            Directive::ObjectSrc => "object-src",
            Directive::ManifestSrc => "manifest-src",
        }
    }
}

/// CSP source token. Most directives accept a list of these.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// `'self'` — same-origin.
    SelfOrigin,
    /// `'none'` — explicit empty list.
    None_,
    /// `'unsafe-inline'`. Avoid in script-src; required in
    /// style-src for any inline-style-using framework.
    UnsafeInline,
    /// `'unsafe-eval'`. Vite dev needs this for HMR; production
    /// builds should never include it.
    UnsafeEval,
    /// `data:` URI scheme.
    Data,
    /// `blob:` URI scheme. Needed for File API + Worker URLs.
    Blob,
    /// `ws:` WebSocket (insecure). Dev-only.
    Ws,
    /// `wss:` WebSocket over TLS. Production for SSE / Yjs collab.
    Wss,
    /// Explicit host or scheme-host pattern.
    Host(String),
}

impl Source {
    fn token(&self) -> std::borrow::Cow<'_, str> {
        match self {
            Source::SelfOrigin => "'self'".into(),
            Source::None_ => "'none'".into(),
            Source::UnsafeInline => "'unsafe-inline'".into(),
            Source::UnsafeEval => "'unsafe-eval'".into(),
            Source::Data => "data:".into(),
            Source::Blob => "blob:".into(),
            Source::Ws => "ws:".into(),
            Source::Wss => "wss:".into(),
            Source::Host(h) => h.as_str().into(),
        }
    }
}

/// Builder for a Content-Security-Policy header value. Renders
/// to the canonical `directive tok tok; directive tok; flag`
/// format. Insertion order is preserved for stable diffs.
#[derive(Debug, Default, Clone)]
pub struct Csp {
    directives: Vec<(Directive, Vec<Source>)>,
    /// Flag-style entries (eg. `upgrade-insecure-requests`) that
    /// have no source list.
    flags: Vec<&'static str>,
}

impl Csp {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(mut self, d: Directive, sources: Vec<Source>) -> Self {
        self.directives.push((d, sources));
        self
    }

    /// Add a flag-style directive that takes no sources. Used for
    /// `upgrade-insecure-requests` and `block-all-mixed-content`.
    pub fn flag(mut self, name: &'static str) -> Self {
        self.flags.push(name);
        self
    }

    pub fn render(&self) -> String {
        let mut parts: Vec<String> = Vec::with_capacity(self.directives.len() + self.flags.len());
        for (dir, sources) in &self.directives {
            let toks: Vec<String> = sources.iter().map(|s| s.token().into_owned()).collect();
            if toks.is_empty() {
                // CSP requires at least one source token per directive.
                // An empty list defaults to 'none' which is the safest
                // option; explicit sentinel avoids an invalid header.
                parts.push(format!("{} 'none'", dir.name()));
            } else {
                parts.push(format!("{} {}", dir.name(), toks.join(" ")));
            }
        }
        for f in &self.flags {
            parts.push((*f).to_string());
        }
        parts.join("; ")
    }
}

// ---------------------------------------------------------------
// Environment-specific policies
// ---------------------------------------------------------------

/// Production CSP. Strict by default; the only `unsafe-*` token
/// is `'unsafe-inline'` on style-src, which is structurally
/// required while the SPA emits inline `:style` bindings. Adding
/// a per-response nonce on style-src is the path to dropping
/// even that — tracked in the security audit plan.
fn production_policy(plugin_sandbox_origin: Option<&str>) -> Csp {
    let mut csp = Csp::new()
        .add(Directive::DefaultSrc, vec![Source::SelfOrigin])
        .add(Directive::ScriptSrc, vec![Source::SelfOrigin])
        .add(Directive::WorkerSrc, vec![Source::SelfOrigin, Source::Blob])
        .add(
            Directive::StyleSrc,
            vec![Source::SelfOrigin, Source::UnsafeInline],
        )
        // img-src restricted to same-origin + data URIs (favicons,
        // small SVGs) + blob URIs (drag-drop, paste). Was `https:`
        // in the prior policy, which permitted any HTTPS image —
        // a useful hardening because user-supplied markdown
        // shouldn't be able to embed tracking pixels from
        // arbitrary external hosts.
        .add(
            Directive::ImgSrc,
            vec![Source::SelfOrigin, Source::Data, Source::Blob],
        )
        .add(Directive::FontSrc, vec![Source::SelfOrigin, Source::Data])
        // SSE + y-websocket collab connect over wss in production.
        // Must include wss: explicitly — some browsers don't fold
        // wss into self when the origin is https.
        .add(
            Directive::ConnectSrc,
            vec![Source::SelfOrigin, Source::Wss, Source::Blob],
        )
        .add(Directive::MediaSrc, vec![Source::SelfOrigin, Source::Blob])
        .add(Directive::ManifestSrc, vec![Source::SelfOrigin])
        .add(Directive::FrameAncestors, vec![Source::None_])
        .add(Directive::ObjectSrc, vec![Source::None_])
        .add(Directive::BaseUri, vec![Source::SelfOrigin])
        .add(Directive::FormAction, vec![Source::SelfOrigin])
        // Force any http:// subresource fetch to upgrade to https://
        // before sending. Defence-in-depth against mixed content
        // and old hardcoded http URLs in seeded data.
        .flag("upgrade-insecure-requests");

    // frame-src defaults to 'self' for first-party iframes
    // (DocumentView email rendering, etc) plus an optional
    // sandbox origin for community-tier plugin iframes when
    // configured.
    let mut frame_sources = vec![Source::SelfOrigin];
    if let Some(origin) = plugin_sandbox_origin {
        if !origin.is_empty() {
            frame_sources.push(Source::Host(origin.to_string()));
        }
    }
    csp = csp.add(Directive::FrameSrc, frame_sources);

    csp
}

/// Development CSP. Same shape as production but with HMR
/// concessions: `'unsafe-eval'` for Vite dev's transform pipeline
/// and `ws:` for the HMR socket. Without these, `npm run dev`
/// breaks. Both are scoped to development by construction; a
/// production build going through this branch is a deployment
/// misconfiguration the env check catches.
fn development_policy(plugin_sandbox_origin: Option<&str>) -> Csp {
    let mut csp = Csp::new()
        .add(Directive::DefaultSrc, vec![Source::SelfOrigin])
        .add(
            Directive::ScriptSrc,
            vec![Source::SelfOrigin, Source::UnsafeEval],
        )
        .add(Directive::WorkerSrc, vec![Source::SelfOrigin, Source::Blob])
        .add(
            Directive::StyleSrc,
            vec![Source::SelfOrigin, Source::UnsafeInline],
        )
        .add(
            Directive::ImgSrc,
            vec![Source::SelfOrigin, Source::Data, Source::Blob],
        )
        .add(Directive::FontSrc, vec![Source::SelfOrigin, Source::Data])
        .add(
            Directive::ConnectSrc,
            vec![
                Source::SelfOrigin,
                Source::Ws,
                Source::Wss,
                Source::Blob,
            ],
        )
        .add(Directive::MediaSrc, vec![Source::SelfOrigin, Source::Blob])
        .add(Directive::ManifestSrc, vec![Source::SelfOrigin])
        .add(Directive::FrameAncestors, vec![Source::None_])
        .add(Directive::ObjectSrc, vec![Source::None_])
        .add(Directive::BaseUri, vec![Source::SelfOrigin])
        .add(Directive::FormAction, vec![Source::SelfOrigin]);

    let mut frame_sources = vec![Source::SelfOrigin];
    if let Some(origin) = plugin_sandbox_origin {
        if !origin.is_empty() {
            frame_sources.push(Source::Host(origin.to_string()));
        }
    }
    csp = csp.add(Directive::FrameSrc, frame_sources);

    csp
}

// ---------------------------------------------------------------
// actix-web middleware
// ---------------------------------------------------------------

/// Security headers middleware. Constructed once at server start,
/// applied to every response. Reads environment variables once at
/// transform-creation time so the per-request hot path is just a
/// header insert.
pub struct SecurityHeaders;

impl SecurityHeaders {
    fn build_csp_value() -> (String, bool) {
        let env = std::env::var("ENVIRONMENT")
            .unwrap_or_else(|_| "development".to_string())
            .to_lowercase();

        let plugin_sandbox_origin = std::env::var("PLUGIN_SANDBOX_ORIGIN")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());

        let csp = if env == "production" {
            production_policy(plugin_sandbox_origin.as_deref())
        } else {
            development_policy(plugin_sandbox_origin.as_deref())
        };

        let report_only = std::env::var("CSP_REPORT_ONLY")
            .map(|v| matches!(v.trim().to_lowercase().as_str(), "1" | "true" | "yes"))
            .unwrap_or(false);

        (csp.render(), report_only)
    }

    fn should_enable_hsts() -> bool {
        let env = std::env::var("ENVIRONMENT")
            .unwrap_or_else(|_| "development".to_string())
            .to_lowercase();
        env == "production"
    }
}

impl<S, B> Transform<S, ServiceRequest> for SecurityHeaders
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error>,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type InitError = ();
    type Transform = SecurityHeadersMiddleware<S>;
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        let (csp_value, report_only) = Self::build_csp_value();
        ready(Ok(SecurityHeadersMiddleware {
            service,
            csp_value,
            report_only,
            enable_hsts: Self::should_enable_hsts(),
        }))
    }
}

pub struct SecurityHeadersMiddleware<S> {
    service: S,
    csp_value: String,
    report_only: bool,
    enable_hsts: bool,
}

impl<S, B> Service<ServiceRequest> for SecurityHeadersMiddleware<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error>,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type Future = LocalBoxFuture<'static, Result<Self::Response, Self::Error>>;

    forward_ready!(service);

    fn call(&self, req: ServiceRequest) -> Self::Future {
        let csp_value = self.csp_value.clone();
        let report_only = self.report_only;
        let enable_hsts = self.enable_hsts;
        let path = req.path().to_string();

        let fut = self.service.call(req);

        Box::pin(async move {
            let mut res = fut.await?;
            let headers = res.headers_mut();

            // Cache-Control: only set defaults when the handler
            // didn't. Hashed assets (Vite content hashes) are
            // immutable; everything else gets handler-specified
            // semantics or the actix default.
            if !headers.contains_key(header::CACHE_CONTROL) {
                if path.starts_with("/assets/") || path.starts_with("/pdfjs/") {
                    headers.insert(
                        header::CACHE_CONTROL,
                        "public, max-age=31536000, immutable".parse().unwrap(),
                    );
                }
            }

            // Content-Security-Policy. Skip if a handler set its
            // own (eg. a more-permissive policy for a specific
            // route); the default is restrictive enough that
            // overrides should be deliberate.
            if !headers.contains_key(header::CONTENT_SECURITY_POLICY) {
                let header_name = if report_only {
                    header::CONTENT_SECURITY_POLICY_REPORT_ONLY
                } else {
                    header::CONTENT_SECURITY_POLICY
                };
                if let Ok(value) = csp_value.parse() {
                    headers.insert(header_name, value);
                }
            }

            // X-Frame-Options is legacy compared to
            // frame-ancestors in CSP, but it's still honoured by
            // older browsers and adds zero cost.
            if !headers.contains_key(header::X_FRAME_OPTIONS) {
                headers.insert(header::X_FRAME_OPTIONS, "DENY".parse().unwrap());
            }

            // X-Content-Type-Options: nosniff. Always.
            if !headers.contains_key(header::X_CONTENT_TYPE_OPTIONS) {
                headers.insert(
                    header::X_CONTENT_TYPE_OPTIONS,
                    "nosniff".parse().unwrap(),
                );
            }

            // Referrer-Policy. Same-origin gets the full referrer,
            // cross-origin gets just the origin (no path). Standard
            // OWASP recommendation.
            if !headers.contains_key(header::REFERRER_POLICY) {
                headers.insert(
                    header::REFERRER_POLICY,
                    "strict-origin-when-cross-origin".parse().unwrap(),
                );
            }

            // Permissions-Policy. Lock down powerful features the
            // app doesn't use. Microphone is permitted on self for
            // voice-note attachments. Everything else is closed.
            if !headers.contains_key("Permissions-Policy") {
                headers.insert(
                    "Permissions-Policy".parse().unwrap(),
                    "geolocation=(), microphone=(self), camera=(), \
                     payment=(), usb=(), serial=(), hid=(), bluetooth=(), \
                     midi=(), magnetometer=(), gyroscope=(), accelerometer=(), \
                     interest-cohort=()"
                        .parse()
                        .unwrap(),
                );
            }

            // Cross-Origin-Opener-Policy: isolates the document
            // from cross-origin windows. Mitigates Spectre-class
            // cross-origin leaks.
            if !headers.contains_key("Cross-Origin-Opener-Policy") {
                headers.insert(
                    "Cross-Origin-Opener-Policy".parse().unwrap(),
                    "same-origin".parse().unwrap(),
                );
            }

            // Cross-Origin-Resource-Policy: prevents other origins
            // from loading our responses (no-cors fetch / image
            // embedding). Same-origin is the strict default; we
            // explicitly relax for /uploads/* later if a use case
            // emerges.
            if !headers.contains_key("Cross-Origin-Resource-Policy") {
                headers.insert(
                    "Cross-Origin-Resource-Policy".parse().unwrap(),
                    "same-origin".parse().unwrap(),
                );
            }

            // Strict-Transport-Security: production HTTPS only.
            // 1-year max-age with includeSubDomains. preload not
            // included until ownership of the apex domain is
            // confirmed (hsts preload is one-way for ~6 months).
            if enable_hsts && !headers.contains_key(header::STRICT_TRANSPORT_SECURITY) {
                headers.insert(
                    header::STRICT_TRANSPORT_SECURITY,
                    "max-age=31536000; includeSubDomains".parse().unwrap(),
                );
            }

            // X-XSS-Protection deliberately NOT set. Modern
            // browsers ignore it (Edge, Chrome >78, Firefox have
            // removed XSS auditors entirely), and the legacy
            // implementations had bugs that introduced new XSS
            // vectors. OWASP Secure Headers Project now recommends
            // omitting it.

            Ok(res)
        })
    }
}

// ---------------------------------------------------------------
// Tests
// ---------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_directives(csp: &str) -> std::collections::HashMap<String, Vec<String>> {
        csp.split(';')
            .map(|p| p.trim())
            .filter(|p| !p.is_empty())
            .filter_map(|p| {
                let mut parts = p.split_whitespace();
                let name = parts.next()?.to_string();
                let sources: Vec<String> = parts.map(String::from).collect();
                Some((name, sources))
            })
            .collect()
    }

    fn flags(csp: &str) -> std::collections::HashSet<String> {
        csp.split(';')
            .map(|p| p.trim())
            .filter(|p| !p.is_empty())
            .filter_map(|p| {
                if p.split_whitespace().count() == 1 {
                    Some(p.to_string())
                } else {
                    None
                }
            })
            .collect()
    }

    // ── Production policy ───────────────────────────────────────

    #[test]
    fn production_default_src_is_self() {
        let csp = production_policy(None).render();
        let dirs = parse_directives(&csp);
        assert_eq!(dirs.get("default-src"), Some(&vec!["'self'".to_string()]));
    }

    #[test]
    fn production_script_src_has_no_unsafe_eval() {
        let csp = production_policy(None).render();
        assert!(!csp.contains("'unsafe-eval'"));
    }

    #[test]
    fn production_img_src_excludes_unrestricted_https() {
        let csp = production_policy(None).render();
        let dirs = parse_directives(&csp);
        let img = dirs.get("img-src").unwrap();
        // Tightening from the prior `https:` wildcard. Should
        // only allow self + data + blob; if a future change adds
        // back a bare `https:` token this test fails.
        assert!(img.iter().all(|s| s != "https:"));
        assert!(img.contains(&"'self'".to_string()));
        assert!(img.contains(&"data:".to_string()));
        assert!(img.contains(&"blob:".to_string()));
    }

    #[test]
    fn production_connect_src_includes_wss_for_collab() {
        let csp = production_policy(None).render();
        let dirs = parse_directives(&csp);
        let connect = dirs.get("connect-src").unwrap();
        assert!(
            connect.contains(&"wss:".to_string()),
            "production must include wss: for SSE + y-websocket over HTTPS",
        );
        assert!(
            !connect.contains(&"ws:".to_string()),
            "production must not include insecure ws:",
        );
    }

    #[test]
    fn production_has_upgrade_insecure_requests_flag() {
        let csp = production_policy(None).render();
        assert!(flags(&csp).contains("upgrade-insecure-requests"));
    }

    #[test]
    fn production_frame_ancestors_blocks_clickjacking() {
        let csp = production_policy(None).render();
        let dirs = parse_directives(&csp);
        assert_eq!(
            dirs.get("frame-ancestors"),
            Some(&vec!["'none'".to_string()]),
        );
    }

    #[test]
    fn production_object_src_is_none() {
        let csp = production_policy(None).render();
        let dirs = parse_directives(&csp);
        assert_eq!(dirs.get("object-src"), Some(&vec!["'none'".to_string()]));
    }

    #[test]
    fn production_includes_plugin_sandbox_origin_when_provided() {
        let csp = production_policy(Some("https://sandbox.example.com")).render();
        let dirs = parse_directives(&csp);
        assert!(
            dirs.get("frame-src")
                .unwrap()
                .contains(&"https://sandbox.example.com".to_string()),
        );
    }

    // ── Development policy ──────────────────────────────────────

    #[test]
    fn development_allows_unsafe_eval_for_vite_hmr() {
        let csp = development_policy(None).render();
        let dirs = parse_directives(&csp);
        let scripts = dirs.get("script-src").unwrap();
        assert!(scripts.contains(&"'unsafe-eval'".to_string()));
    }

    #[test]
    fn development_allows_ws_for_vite_hmr() {
        let csp = development_policy(None).render();
        let dirs = parse_directives(&csp);
        let connect = dirs.get("connect-src").unwrap();
        assert!(connect.contains(&"ws:".to_string()));
    }

    // ── Builder shape ───────────────────────────────────────────

    #[test]
    fn empty_directive_sources_render_as_none() {
        let csp = Csp::new().add(Directive::ImgSrc, vec![]).render();
        assert!(csp.contains("img-src 'none'"));
    }

    #[test]
    fn flags_render_after_directives() {
        let csp = Csp::new()
            .add(Directive::DefaultSrc, vec![Source::SelfOrigin])
            .flag("upgrade-insecure-requests")
            .render();
        let semi = csp.find("default-src").unwrap();
        let flag = csp.find("upgrade-insecure-requests").unwrap();
        assert!(flag > semi);
    }

    // ── Wiring ──────────────────────────────────────────────────

    #[test]
    fn build_csp_value_produces_non_empty_header() {
        let (value, _) = SecurityHeaders::build_csp_value();
        assert!(!value.is_empty());
        assert!(value.contains("default-src"));
    }
}
