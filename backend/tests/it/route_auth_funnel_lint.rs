//! Lint: every `/api` route tree either authenticates through one of the four
//! known funnels, or is listed here as public with a reason.
//!
//! ## Why this shape
//!
//! A per-handler authorization lint was prototyped first and rejected. Handlers
//! in this codebase gate through at least seven different mechanisms
//! (extractors, `require_workspace_role`, `is_platform_admin`, `admin_user_conn`,
//! repository predicates that take the subject as an argument, signed-URL
//! verification, and self-subject handlers that only ever touch `claims.sub`),
//! and the vocabulary is open-ended: every future gate helper would be a silent
//! false positive until someone remembered to add it. Measured over the 529
//! routed handlers, the residue after five vocabulary corrections was ~31
//! handlers that are all public by design, which would have meant ~31
//! exemption markers whose reason was uniformly "this route is public".
//!
//! The property below is the one that actually failed in practice, and its
//! vocabulary is closed. `/api/collaboration` was registered as its own
//! top-level scope carrying only an auth wrap, with no scope enforcement, and
//! nothing caught it because nothing was checking the shape of the tree. What
//! goes wrong is not "a handler forgot a check", it is "a new top-level scope
//! was added and quietly reached the internet".
//!
//! ## What is matched
//!
//! Inside `configure_app`, every `web::scope("/api…")` / `web::resource("/api…")`
//! and every `App`-level `.route("/api/…")`. A tree passes if its chained
//! expression wraps one of `AUTH_FUNNELS`. Otherwise it must appear in
//! `PUBLIC_BY_DESIGN`.
//!
//! The list is checked in both directions: an entry that has since gained a
//! funnel is also an error, so it cannot rot into a blanket amnesty.

use std::fs;
use std::path::PathBuf;

use regex::Regex;

/// The middlewares that establish an authenticated principal. Adding a fifth
/// is exactly the kind of change that should be looked at deliberately, which
/// is why this list is short and hand-maintained rather than inferred.
const AUTH_FUNNELS: &[&str] = &[
    // Cookie or Bearer or `nsk_` API token -> api_token::authenticate.
    "dual_auth_middleware",
    // Cookie session only -> the same funnel with API tokens refused.
    "cookie_auth_middleware",
    // Customer-portal session; inserts PortalContext, not Claims.
    "portal_auth_middleware",
    // Control-plane EdDSA platform token.
    "platform_auth_middleware",
];

/// Route trees that are reachable without authenticating, and why. A path here
/// is a deliberate decision, not a backlog item.
const PUBLIC_BY_DESIGN: &[(&str, &str)] = &[
    (
        "/api/public",
        "guest ticket submission; feature-flagged per handler and rate-limited",
    ),
    (
        "/api/auth",
        "sign-in, setup and password reset: the routes that mint a session",
    ),
    (
        "/api/portal/auth",
        "customer-portal magic-link sign-in; the refresh route checks its own realm",
    ),
    (
        "/api/csp-report",
        "browsers POST CSP violations without credentials",
    ),
    (
        "/api/inbound/email",
        "SES/SNS server-to-server; the handler verifies the SNS signature and topic ARN",
    ),
    (
        "/api/debug/frontend-logs",
        "unauthenticated client log intake; rate-limited",
    ),
    (
        "/api/branding",
        "login-screen branding, needed before a session exists",
    ),
    (
        "/api/config",
        "public instance config, read by the client before sign-in",
    ),
    (
        "/api/collaboration",
        "the WebSocket upgrade authenticates itself inside ws_handler (a wrap would \
         intercept the upgrade); the REST sub-scope wraps dual_auth inside \
         collaboration::config, which this scanner cannot see from here",
    ),
    (
        "/api/events/stream",
        "SSE cannot send headers, so the connection token rides in the query string \
         and the handler validates it",
    ),
    (
        "/api/events/status",
        "SSE liveness probe alongside the stream above",
    ),
];

/// The chained expression starting at `start`, ending where it closes the
/// `.service(` / `cfg.` call that contains it.
fn expression_at(src: &str, start: usize) -> &str {
    let bytes = src.as_bytes();
    let mut depth: i32 = 0;
    let mut i = start;
    while i < bytes.len() {
        match bytes[i] {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth < 0 {
                    return &src[start..i];
                }
            }
            _ => {}
        }
        i += 1;
    }
    &src[start..]
}

#[test]
fn every_api_route_tree_authenticates_or_is_listed_public() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let src = fs::read_to_string(manifest.join("src/startup.rs")).expect("read startup.rs");
    let body_start = src.find("pub fn configure_app").expect("configure_app");
    let body = &src[body_start..];

    // `web::scope("/api…")`, `web::resource("/api…")`, and App-level
    // `.route("/api/…")`. The last has no chained wrap by construction, so it
    // is always required to be listed.
    let tree_re = Regex::new(r#"web::(?:scope|resource)\("(/api[^"]*)"\)"#).expect("tree regex");
    let route_re = Regex::new(r#"\.route\("(/api/[^"]*)""#).expect("route regex");

    let mut unlisted: Vec<String> = Vec::new();
    let mut found: Vec<String> = Vec::new();
    // Byte spans of every scope/resource expression, so a `.route` can be told
    // apart by whether it sits inside one. Without this the App-level check is
    // vacuous: `/api` is itself a scope, so every `/api/...` literal would look
    // nested.
    let mut spans: Vec<(usize, usize)> = Vec::new();

    for caps in tree_re.captures_iter(body) {
        let path = caps.get(1).expect("path").as_str().to_string();
        let start = caps.get(0).expect("m").start();
        let expr = expression_at(body, start);
        spans.push((start, start + expr.len()));
        let authed = AUTH_FUNNELS.iter().any(|f| expr.contains(f));
        found.push(path.clone());
        if !authed && !PUBLIC_BY_DESIGN.iter().any(|(p, _)| *p == path) {
            unlisted.push(format!(
                "{path}  (web::scope / web::resource, no auth funnel)"
            ));
        }
        if authed {
            if let Some((p, _)) = PUBLIC_BY_DESIGN.iter().find(|(p, _)| *p == path) {
                unlisted.push(format!(
                    "{p}  (listed public, but now wraps an auth funnel: remove the entry)"
                ));
            }
        }
    }

    // App-level routes: nothing wraps these, so each must be listed. A route
    // inside a scope's span is covered by that scope's funnel instead.
    for caps in route_re.captures_iter(body) {
        let m = caps.get(0).expect("m");
        if spans.iter().any(|(s, e)| m.start() > *s && m.start() < *e) {
            continue;
        }
        let path = caps.get(1).expect("path").as_str();
        if !PUBLIC_BY_DESIGN.iter().any(|(p, _)| path.starts_with(p)) {
            unlisted.push(format!("{path}  (App-level .route, no auth funnel)"));
        }
    }

    assert!(
        !PUBLIC_BY_DESIGN.is_empty() && !found.is_empty(),
        "the scanner found no route trees; it has drifted from startup.rs"
    );

    unlisted.sort();
    unlisted.dedup();
    assert!(
        unlisted.is_empty(),
        "\nThese /api route trees are reachable without authenticating, and are not \
         listed as public:\n\n  {}\n\n\
         Either wrap the scope in one of: {}\n\
         or add it to PUBLIC_BY_DESIGN in this file with the reason it is public.\n",
        unlisted.join("\n  "),
        AUTH_FUNNELS.join(", "),
    );
}
