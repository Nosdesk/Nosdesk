//! Browser error reports: `POST /api/client-errors`.
//!
//! The web apps post the errors nothing else caught (uncaught exceptions,
//! unhandled rejections, Vue render errors, and API 5xx the browser saw) to
//! their own server, which writes one log line per report. Nothing leaves the
//! instance: on the hosted service the lines reach the log shipper like any
//! other; on a self-hosted install they stay in the container log.
//!
//! Public and unauthenticated, like `/api/csp-report`: sign-in pages fail too,
//! and `sendBeacon` carries no CSRF header. It only logs, so there is nothing
//! to forge. Its own rate-limit bucket (`clienterr:{ip}`) keeps a looping page
//! away from the public and auth quotas. Always 204, so a client never retries.
//!
//! Every field is reduced to a bounded shape before it is logged, which keeps
//! the redacting JSON layer's allowlist safe by construction: `kind` and the
//! surface are enums, `route` is a router pattern, frames keep only asset file
//! names and positions, and the free-text message goes through the layer's
//! scrubber. The raw stack and the user agent are never logged.
//!
//! `NOSDESK_CLIENT_ERROR_REPORTS=off` makes the endpoint a no-op.

use std::sync::LazyLock;

use actix_web::{web, HttpMessage, HttpRequest, HttpResponse};
use regex::Regex;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::extractors::WorkspaceContext;
use crate::utils::utf8_trunc::{byte_prefix, char_prefix, strip_line_breaks_for_log_field};

/// Reports read from one request; the rest are ignored.
const MAX_REPORTS: usize = 10;
const MAX_MESSAGE_CHARS: usize = 300;
const MAX_FRAMES: usize = 5;
const MAX_ROUTE_CHARS: usize = 120;
/// Stack text parsed per report. Frames past this are not worth the work.
const MAX_STACK_BYTES: usize = 8 * 1024;

/// V8: `    at fn (https://host/assets/index-abc.js:1:44)` or, for an anonymous
/// function, `    at https://host/assets/index-abc.js:3:69`.
static V8_FRAME: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^\s*at (?:.*?\()?(?P<url>[A-Za-z][A-Za-z0-9+.\-]*://[^\s()]+|/[^\s()]+):(?P<line>\d{1,7}):(?P<col>\d{1,7})\)?\s*$",
    )
    .expect("valid V8 frame regex")
});

/// JavaScriptCore and SpiderMonkey: `fn@https://host/assets/index-abc.js:1:57`,
/// or `@https://…` for an anonymous function.
static AT_FRAME: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^[^@]*@(?P<url>[A-Za-z][A-Za-z0-9+.\-]*://\S+?|/\S+?):(?P<line>\d{1,7}):(?P<col>\d{1,7})$",
    )
    .expect("valid @ frame regex")
});

/// A bundle or source file name: what a frame keeps of its URL.
static ASSET_NAME: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[A-Za-z0-9_.\-]{1,100}\.(?:m?js|ts|tsx|vue)$").expect("valid asset name regex")
});

/// A vue-router path pattern, such as `/tickets/:id(\d+)`.
static ROUTE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^/[A-Za-z0-9/:_.\-*()?+\\]*$").expect("valid route regex"));

static BUILD_SHA: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?:[0-9a-f]{7,40}|dev)$").expect("valid build sha regex"));

fn enabled() -> bool {
    !matches!(
        std::env::var("NOSDESK_CLIENT_ERROR_REPORTS").as_deref(),
        Ok("off" | "0" | "false" | "no")
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Error,
    UnhandledRejection,
    Vue,
    Http,
}

impl Kind {
    fn parse(s: &str) -> Option<Self> {
        match s {
            "error" => Some(Self::Error),
            "unhandled_rejection" => Some(Self::UnhandledRejection),
            "vue" => Some(Self::Vue),
            "http" => Some(Self::Http),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::UnhandledRejection => "unhandled_rejection",
            Self::Vue => "vue",
            Self::Http => "http",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Surface {
    Agent,
    Portal,
    Teams,
    Widget,
}

impl Surface {
    fn parse(s: &str) -> Option<Self> {
        match s {
            "agent" => Some(Self::Agent),
            "portal" => Some(Self::Portal),
            "teams" => Some(Self::Teams),
            "widget" => Some(Self::Widget),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Agent => "agent",
            Self::Portal => "portal",
            Self::Teams => "teams",
            Self::Widget => "widget",
        }
    }
}

#[derive(Debug, Default, Deserialize)]
struct Batch {
    #[serde(default)]
    reports: Vec<serde_json::Value>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct RawReport {
    kind: String,
    surface: String,
    message: String,
    stack: String,
    route: String,
    build_sha: String,
    status_code: Option<u16>,
    count: Option<u32>,
}

/// One report, reduced to what may be logged.
#[derive(Debug, PartialEq)]
struct Report {
    kind: Kind,
    surface: Surface,
    message: String,
    frames: String,
    route: String,
    build_sha: String,
    status_code: Option<u16>,
    count: u32,
    fingerprint: String,
}

impl Report {
    /// `None` drops the report: an unknown kind or surface, or an http report
    /// without a 5xx status.
    fn from_raw(raw: RawReport) -> Option<Self> {
        let kind = Kind::parse(&raw.kind)?;
        let surface = Surface::parse(&raw.surface)?;
        let status_code = match kind {
            Kind::Http => Some(raw.status_code.filter(|s| (500..=599).contains(s))?),
            _ => None,
        };
        let message = char_prefix(
            strip_line_breaks_for_log_field(&raw.message).trim(),
            MAX_MESSAGE_CHARS,
        );
        let message = if message.is_empty() {
            "(no message)".to_string()
        } else {
            message
        };
        let frames = frames(&raw.stack);
        let route = if raw.route.chars().count() <= MAX_ROUTE_CHARS && ROUTE.is_match(&raw.route) {
            raw.route
        } else {
            String::new()
        };
        let build_sha = if BUILD_SHA.is_match(&raw.build_sha) {
            raw.build_sha
        } else {
            "unknown".to_string()
        };
        let count = raw.count.unwrap_or(1).clamp(1, 1000);
        let fingerprint = fingerprint(kind, &message, frames.split(" | ").next().unwrap_or(""));
        Some(Self {
            kind,
            surface,
            message,
            frames,
            route,
            build_sha,
            status_code,
            count,
            fingerprint,
        })
    }
}

/// The top frames as `file:line:col`, joined by ` | `. Only lines that parse as
/// a frame count, so the message line V8 puts first, `eval` frames and anything
/// else in the stack are discarded; of a URL only the file name survives.
fn frames(stack: &str) -> String {
    let stack = byte_prefix(stack, MAX_STACK_BYTES);
    let mut out: Vec<String> = Vec::new();
    for line in stack.lines() {
        if out.len() == MAX_FRAMES {
            break;
        }
        let Some(caps) = V8_FRAME
            .captures(line)
            .or_else(|| AT_FRAME.captures(line.trim()))
        else {
            continue;
        };
        let url = &caps["url"];
        let path = url.split(['?', '#']).next().unwrap_or("");
        let name = path.rsplit('/').next().unwrap_or("");
        if !ASSET_NAME.is_match(name) {
            continue;
        }
        out.push(format!("{name}:{}:{}", &caps["line"], &caps["col"]));
    }
    out.join(" | ")
}

fn fingerprint(kind: Kind, message: &str, top_frame: &str) -> String {
    let digest = Sha256::digest(format!("{}\n{message}\n{top_frame}", kind.as_str()));
    hex::encode(&digest[..8])
}

fn parse_batch(body: &[u8]) -> Vec<Report> {
    let Ok(batch) = serde_json::from_slice::<Batch>(body) else {
        return Vec::new();
    };
    batch
        .reports
        .into_iter()
        .take(MAX_REPORTS)
        .filter_map(|v| serde_json::from_value::<RawReport>(v).ok())
        .filter_map(Report::from_raw)
        .collect()
}

/// One line per report. `status_code` rides only on http reports and
/// `workspace_id` only when the request resolved a workspace, so neither is
/// ever a placeholder.
fn log_report(r: &Report, workspace_id: Option<i32>) {
    let (kind, surface) = (r.kind.as_str(), r.surface.as_str());
    match (r.status_code, workspace_id) {
        (Some(status_code), Some(workspace_id)) => tracing::warn!(
            target: "client_error",
            kind,
            client_error_surface = surface,
            route = %r.route,
            build_sha = %r.build_sha,
            client_error_frames = %r.frames,
            client_error_fingerprint = %r.fingerprint,
            count = r.count,
            status_code,
            workspace_id,
            "{}",
            r.message
        ),
        (Some(status_code), None) => tracing::warn!(
            target: "client_error",
            kind,
            client_error_surface = surface,
            route = %r.route,
            build_sha = %r.build_sha,
            client_error_frames = %r.frames,
            client_error_fingerprint = %r.fingerprint,
            count = r.count,
            status_code,
            "{}",
            r.message
        ),
        (None, Some(workspace_id)) => tracing::warn!(
            target: "client_error",
            kind,
            client_error_surface = surface,
            route = %r.route,
            build_sha = %r.build_sha,
            client_error_frames = %r.frames,
            client_error_fingerprint = %r.fingerprint,
            count = r.count,
            workspace_id,
            "{}",
            r.message
        ),
        (None, None) => tracing::warn!(
            target: "client_error",
            kind,
            client_error_surface = surface,
            route = %r.route,
            build_sha = %r.build_sha,
            client_error_frames = %r.frames,
            client_error_fingerprint = %r.fingerprint,
            count = r.count,
            "{}",
            r.message
        ),
    }
}

/// `POST /api/client-errors`. Body: `{ "reports": [...] }`, sent as
/// `text/plain` so a beacon needs no preflight; the content type is ignored.
pub async fn report(req: HttpRequest, body: web::Bytes) -> HttpResponse {
    if enabled() {
        let workspace_id = req
            .extensions()
            .get::<WorkspaceContext>()
            .map(|w| w.workspace_id);
        for r in parse_batch(&body) {
            log_report(&r, workspace_id);
        }
    }
    HttpResponse::NoContent().finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // Captured from Chromium and WebKit throwing inside a served asset.
    const V8_STACK: &str = "TypeError: Cannot read properties of undefined (reading 'title')\n    at b (https://app.example.test/assets/index-BqX7d2Kf.js:1:44)\n    at a (https://app.example.test/assets/index-BqX7d2Kf.js:1:21)\n    at window.syncErr (https://app.example.test/assets/index-BqX7d2Kf.js:2:27)\n    at eval (eval at evaluate (:311:30), <anonymous>:5:29)";
    const V8_ASYNC_STACK: &str = "TypeError: Cannot read properties of null (reading 'x')\n    at https://app.example.test/assets/index-BqX7d2Kf.js:3:69\n    at window.asyncErr (https://app.example.test/assets/index-BqX7d2Kf.js:3:72)";
    const JSC_STACK: &str = "b@https://app.example.test/assets/index-BqX7d2Kf.js:1:57\na@https://app.example.test/assets/index-BqX7d2Kf.js:1:22\n@https://app.example.test/assets/index-BqX7d2Kf.js:2:28\n@";

    fn raw(v: serde_json::Value) -> RawReport {
        serde_json::from_value(v).expect("raw report")
    }

    #[test]
    fn v8_frames_keep_file_line_col_and_drop_the_rest() {
        assert_eq!(
            frames(V8_STACK),
            "index-BqX7d2Kf.js:1:44 | index-BqX7d2Kf.js:1:21 | index-BqX7d2Kf.js:2:27"
        );
        assert_eq!(
            frames(V8_ASYNC_STACK),
            "index-BqX7d2Kf.js:3:69 | index-BqX7d2Kf.js:3:72"
        );
    }

    #[test]
    fn webkit_frames_parse_including_anonymous_ones() {
        assert_eq!(
            frames(JSC_STACK),
            "index-BqX7d2Kf.js:1:57 | index-BqX7d2Kf.js:1:22 | index-BqX7d2Kf.js:2:28"
        );
    }

    #[test]
    fn frames_strip_queries_and_cap_at_five() {
        let dev = "    at setup (http://localhost:5173/src/views/TicketView.vue?t=1719:10:5)";
        assert_eq!(frames(dev), "TicketView.vue:10:5");
        let many = (1..=8)
            .map(|n| format!("    at f (https://h/assets/a.js:{n}:1)"))
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(frames(&many).split(" | ").count(), MAX_FRAMES);
    }

    #[test]
    fn a_frame_that_is_not_an_asset_is_dropped() {
        let stack = "    at x (https://h/api/tickets/secret-token:1:2)\n    at y (https://h/assets/ok.js:3:4)";
        assert_eq!(frames(stack), "ok.js:3:4");
    }

    #[test]
    fn a_plain_string_rejection_keeps_its_message_and_has_no_frames() {
        let r = Report::from_raw(raw(json!({
            "kind": "unhandled_rejection", "surface": "portal", "message": "plain string reason"
        })))
        .expect("kept");
        assert_eq!(r.message, "plain string reason");
        assert_eq!(r.frames, "");
        assert_eq!(r.status_code, None);
    }

    #[test]
    fn unknown_kind_or_surface_is_dropped() {
        assert!(Report::from_raw(raw(json!({"kind": "boom", "surface": "agent"}))).is_none());
        assert!(Report::from_raw(raw(json!({"kind": "error", "surface": "kiosk"}))).is_none());
    }

    #[test]
    fn http_reports_need_a_5xx_status_and_others_drop_theirs() {
        assert!(Report::from_raw(raw(json!({"kind": "http", "surface": "agent"}))).is_none());
        assert!(Report::from_raw(raw(
            json!({"kind": "http", "surface": "agent", "status_code": 404})
        ))
        .is_none());
        let http = Report::from_raw(raw(
            json!({"kind": "http", "surface": "agent", "status_code": 502}),
        ))
        .expect("kept");
        assert_eq!(http.status_code, Some(502));
        let err = Report::from_raw(raw(
            json!({"kind": "error", "surface": "agent", "status_code": 502}),
        ))
        .expect("kept");
        assert_eq!(err.status_code, None);
    }

    #[test]
    fn route_and_build_sha_must_match_their_shapes() {
        let ok = Report::from_raw(raw(json!({
            "kind": "vue", "surface": "agent", "route": "/tickets/:id(\\d+)", "build_sha": "a1b2c3d"
        })))
        .expect("kept");
        assert_eq!(ok.route, "/tickets/:id(\\d+)");
        assert_eq!(ok.build_sha, "a1b2c3d");
        let bad = Report::from_raw(raw(json!({
            "kind": "vue", "surface": "agent", "route": "/login?token=abc def", "build_sha": "<script>"
        })))
        .expect("kept");
        assert_eq!(bad.route, "");
        assert_eq!(bad.build_sha, "unknown");
    }

    #[test]
    fn message_is_one_line_and_bounded() {
        let long = format!("line one\nline two {}", "x".repeat(1000));
        let r = Report::from_raw(raw(
            json!({"kind": "error", "surface": "agent", "message": long}),
        ))
        .expect("kept");
        assert!(!r.message.contains('\n'));
        assert_eq!(r.message.chars().count(), MAX_MESSAGE_CHARS);
        let empty =
            Report::from_raw(raw(json!({"kind": "error", "surface": "agent"}))).expect("kept");
        assert_eq!(empty.message, "(no message)");
    }

    #[test]
    fn fingerprint_groups_the_same_error_and_splits_different_ones() {
        let a = Report::from_raw(raw(json!({
            "kind": "error", "surface": "agent", "message": "m", "stack": V8_STACK
        })))
        .expect("kept");
        let b = Report::from_raw(raw(json!({
            "kind": "error", "surface": "portal", "message": "m", "stack": V8_STACK, "count": 3
        })))
        .expect("kept");
        let c = Report::from_raw(raw(json!({
            "kind": "error", "surface": "agent", "message": "other", "stack": V8_STACK
        })))
        .expect("kept");
        assert_eq!(a.fingerprint, b.fingerprint);
        assert_ne!(a.fingerprint, c.fingerprint);
        assert_eq!(a.fingerprint.len(), 16);
        assert_eq!(b.count, 3);
    }

    #[test]
    fn batches_are_capped_and_bad_input_yields_nothing() {
        let reports: Vec<_> = (0..25)
            .map(|_| json!({"kind": "error", "surface": "agent", "message": "m"}))
            .collect();
        let body = serde_json::to_vec(&json!({ "reports": reports })).unwrap();
        assert_eq!(parse_batch(&body).len(), MAX_REPORTS);
        assert!(parse_batch(b"not json").is_empty());
        assert!(parse_batch(b"{\"reports\": 5}").is_empty());
        let mixed = serde_json::to_vec(&json!({
            "reports": [{"kind": "error", "surface": "agent"}, "junk", {"count": "x"}]
        }))
        .unwrap();
        assert_eq!(parse_batch(&mixed).len(), 1);
    }

    #[actix_web::test]
    async fn the_endpoint_always_answers_204() {
        use actix_web::{test, App};
        let app =
            test::init_service(App::new().route("/api/client-errors", web::post().to(report)))
                .await;
        for body in [
            b"{\"reports\":[{\"kind\":\"error\",\"surface\":\"agent\",\"message\":\"m\"}]}"
                .to_vec(),
            b"garbage".to_vec(),
            Vec::new(),
        ] {
            let req = test::TestRequest::post()
                .uri("/api/client-errors")
                .insert_header(("content-type", "text/plain"))
                .set_payload(body)
                .to_request();
            let resp = test::call_service(&app, req).await;
            assert_eq!(resp.status(), 204);
        }
    }
}
