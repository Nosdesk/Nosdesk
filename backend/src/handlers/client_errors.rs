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
//! to forge. Its own rate-limit bucket (`clienterr:{client}`) keeps a looping page
//! away from the public and auth quotas. Always 204, so a client never retries.
//!
//! Every field is reduced to a bounded shape before it is logged, which keeps
//! the redacting JSON layer's allowlist safe by construction: `kind` and the
//! surface are enums, `route` is a router pattern with any literal ID
//! replaced, frames keep only asset file names and positions, and an http
//! report's message is rebuilt here from its status, method and grouped path.
//! Any other message is cleaned here, whatever the log format: control and
//! invisible formatting characters become spaces, emails are masked, JWTs,
//! UUIDs and long tokens are replaced and query strings that carry a value are
//! cut. What can still get through is plain text an error message happened to
//! quote, such as a name. The raw stack and the user agent are never logged.
//!
//! Volume: at most ten reports a request, the per-IP bucket, and a ceiling of
//! [`MAX_LINES_PER_MINUTE`] lines a process writes a minute however many
//! addresses send them.
//!
//! `NOSDESK_CLIENT_ERROR_REPORTS=off` makes the endpoint a no-op.

use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use actix_web::{web, HttpMessage, HttpRequest, HttpResponse};
use regex::Regex;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::extractors::WorkspaceContext;
use crate::utils::redact::scrub;
use crate::utils::utf8_trunc::{byte_prefix, char_prefix};

/// The request body cap, enforced by the payload extractor before any parsing:
/// a larger body is refused with 413 and never read past the cap.
pub const MAX_BODY_BYTES: usize = 32 * 1024;
/// Reports read from one request; the rest are ignored.
const MAX_REPORTS: usize = 10;
const MAX_MESSAGE_CHARS: usize = 300;
const MAX_FRAMES: usize = 5;
const MAX_ROUTE_CHARS: usize = 120;
/// Stack text parsed per report. Frames past this are not worth the work.
const MAX_STACK_BYTES: usize = 8 * 1024;
/// Message text looked at before it is cleaned and cut to [`MAX_MESSAGE_CHARS`].
const MAX_MESSAGE_INPUT_BYTES: usize = 2 * 1024;
/// Report lines one process writes a minute, across every sender. A flood
/// from many addresses stops here; the per-IP bucket can't see it.
const MAX_LINES_PER_MINUTE: u32 = 600;

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

/// The message the web apps build for an http report: `502 GET /api/tickets/:id`.
static HTTP_MESSAGE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\d{3} (?P<method>[A-Z]{3,7}) (?P<path>/\S{0,500})$")
        .expect("valid http message regex")
});

/// A path segment kept as is: lowercase words of up to 16 letters joined by
/// `-` or `_`, as every literal API and app route segment is, or an API
/// version. A long unbroken run of letters is more likely a token than a word.
static WORD_SEGMENT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:[a-z]{1,16}(?:[-_][a-z]{1,16}){0,5}|v[0-9]{1,2})$")
        .expect("valid segment regex")
});

/// A route parameter: `:id`, `:id(\d+)`, `:workspace?`, `:pathMatch(.*)*`.
static ROUTE_PARAM: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^:[A-Za-z_][A-Za-z0-9_]{0,39}(?:\([^/]{0,40}\))?[?*+]?$")
        .expect("valid route param regex")
});

static UUID_TEXT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\b")
        .expect("valid uuid regex")
});

/// A run long enough to be a token. Replaced only when it mixes letters and
/// digits, so long identifiers in a message survive.
static TOKEN_TEXT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[A-Za-z0-9_\-]{24,}").expect("valid token regex"));

/// A query string or fragment that carries a value: `?token=…`, `#access_token=…`.
static QUERY_TEXT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"[?#][^\s'"<>()\[\]{}]*=[^\s'"<>()\[\]{}]*"#).expect("valid query regex")
});

fn enabled() -> bool {
    enabled_from(
        std::env::var("NOSDESK_CLIENT_ERROR_REPORTS")
            .ok()
            .as_deref(),
    )
}

fn enabled_from(value: Option<&str>) -> bool {
    !value.is_some_and(|v| {
        matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "off" | "0" | "false" | "no"
        )
    })
}

/// Characters that never reach a log line: controls (newlines, ESC and the
/// rest of an ANSI sequence's lead, C1 including NEL) and the invisible
/// formatting characters that can reorder or hide text in a viewer (bidi
/// overrides and isolates, zero-width characters, line and paragraph
/// separators, the BOM).
fn is_unsafe_char(c: char) -> bool {
    c.is_control()
        || matches!(
            c,
            '\u{200B}'..='\u{200F}' | '\u{2028}'..='\u{202E}' | '\u{2060}'..='\u{206F}' | '\u{FEFF}'
        )
}

/// The free-text message as it may be logged. See the module docs.
fn clean_message(raw: &str) -> String {
    let text: String = byte_prefix(raw, MAX_MESSAGE_INPUT_BYTES)
        .chars()
        .map(|c| if is_unsafe_char(c) { ' ' } else { c })
        .collect();
    // An address in a URL is percent-encoded; decode the `@` so it is masked.
    let text = text.replace("%40", "@");
    let text = QUERY_TEXT.replace_all(&text, "?[query]");
    let text = scrub(&text);
    let text = UUID_TEXT.replace_all(&text, ":uuid");
    let text = TOKEN_TEXT.replace_all(&text, |caps: &regex::Captures| {
        let run = &caps[0];
        if run.bytes().any(|b| b.is_ascii_digit()) && run.bytes().any(|b| b.is_ascii_alphabetic()) {
            ":token".to_string()
        } else {
            run.to_string()
        }
    });
    char_prefix(text.trim(), MAX_MESSAGE_CHARS)
}

fn is_uuid(s: &str) -> bool {
    s.len() == 36 && UUID_TEXT.is_match(s)
}

/// A path with every segment that is not a lowercase word replaced, the way
/// the web apps group API paths (`apiPath` in `utils/errorTracking.ts`).
/// Placeholders they already put in stay. `route` mode keeps a router
/// pattern's parameters and 4xx/5xx page names, and its `?` modifiers.
fn group_path(path: &str, route: bool) -> String {
    let path = if route {
        path
    } else {
        path.split(['?', '#']).next().unwrap_or("")
    };
    let segments: Vec<&str> = path.split('/').collect();
    segments
        .iter()
        .enumerate()
        .map(|(i, segment)| {
            let previous = i.checked_sub(1).map(|p| segments[p]);
            group_segment(segment, previous, route)
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn group_segment<'a>(segment: &'a str, previous: Option<&str>, route: bool) -> &'a str {
    let placeholder = matches!(segment, ":id" | ":uuid" | ":token" | ":param" | ":slug");
    let status_page = segment.len() == 3
        && matches!(segment.as_bytes()[0], b'4' | b'5')
        && segment.bytes().all(|b| b.is_ascii_digit());
    if segment.is_empty()
        || placeholder
        || (route && (ROUTE_PARAM.is_match(segment) || status_page))
    {
        segment
    } else if previous == Some("slug") {
        ":slug"
    } else if WORD_SEGMENT.is_match(segment) {
        segment
    } else if is_uuid(segment) {
        ":uuid"
    } else if segment.bytes().all(|b| b.is_ascii_digit()) {
        ":id"
    } else if segment.chars().count() >= 20 {
        ":token"
    } else {
        ":param"
    }
}

/// An http report's message, rebuilt from parts that can be checked: the
/// validated status, the method, and the path grouped again here.
fn http_message(status_code: u16, raw: &str) -> String {
    match HTTP_MESSAGE.captures(raw.trim()) {
        Some(caps) => format!(
            "{status_code} {} {}",
            &caps["method"],
            group_path(&caps["path"], false)
        ),
        None => format!("{status_code} (unknown request)"),
    }
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
        let message = match status_code {
            Some(status_code) => http_message(status_code, &raw.message),
            None => clean_message(&raw.message),
        };
        let message = if message.is_empty() {
            "(no message)".to_string()
        } else {
            message
        };
        let frames = frames(&raw.stack);
        let route = if raw.route.chars().count() <= MAX_ROUTE_CHARS && ROUTE.is_match(&raw.route) {
            group_path(&raw.route, true)
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

/// The process-wide ceiling on report lines: a fixed one-minute window.
struct LineBudget {
    window_start: Option<Instant>,
    used: u32,
}

#[derive(Debug, PartialEq, Eq)]
enum Take {
    Log,
    /// The first report over the ceiling this window, which says so once.
    FirstDropped,
    Dropped,
}

impl LineBudget {
    const fn new() -> Self {
        Self {
            window_start: None,
            used: 0,
        }
    }

    fn take(&mut self, now: Instant, max: u32) -> Take {
        let fresh = self
            .window_start
            .is_none_or(|start| now.duration_since(start) >= Duration::from_secs(60));
        if fresh {
            self.window_start = Some(now);
            self.used = 0;
        }
        self.used = self.used.saturating_add(1);
        match self.used {
            n if n <= max => Take::Log,
            n if n == max + 1 => Take::FirstDropped,
            _ => Take::Dropped,
        }
    }
}

static LINE_BUDGET: Mutex<LineBudget> = Mutex::new(LineBudget::new());

/// The rate-limit bucket: `clienterr:` and the client the other limits count
/// (`X-Forwarded-For` only from a trusted proxy, IPv6 by its /64), as the
/// pseudonym [`crate::utils::client_ip::limit_bucket`] gives it.
pub fn rate_limit_key(req: &actix_web::dev::ServiceRequest) -> Option<String> {
    crate::utils::client_ip::from_service_request(req)
        .map(|ip| format!("clienterr:{}", crate::utils::client_ip::limit_bucket(ip)))
}

/// `POST /api/client-errors`. Body: `{ "reports": [...] }`, sent as
/// `text/plain` so a beacon needs no preflight; the content type is ignored.
pub async fn report(req: HttpRequest, body: web::Bytes) -> HttpResponse {
    handle(&req, &body, enabled());
    HttpResponse::NoContent().finish()
}

fn handle(req: &HttpRequest, body: &[u8], enabled: bool) {
    if !enabled {
        return;
    }
    let workspace_id = req
        .extensions()
        .get::<WorkspaceContext>()
        .map(|w| w.workspace_id);
    for r in parse_batch(body) {
        let take = LINE_BUDGET
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take(Instant::now(), MAX_LINES_PER_MINUTE);
        match take {
            Take::Log => log_report(&r, workspace_id),
            Take::FirstDropped => {
                tracing::warn!(
                    target: "client_error",
                    count = MAX_LINES_PER_MINUTE,
                    "Client error reports hit the per-minute ceiling; the rest of this minute's are dropped"
                );
                return;
            }
            Take::Dropped => return,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // Captured from Chromium and WebKit throwing inside a served asset.
    const V8_STACK: &str = "TypeError: Cannot read properties of undefined (reading 'title')\n    at b (https://app.example.test/assets/index-BqX7d2Kf.js:1:44)\n    at a (https://app.example.test/assets/index-BqX7d2Kf.js:1:21)\n    at window.syncErr (https://app.example.test/assets/index-BqX7d2Kf.js:2:27)\n    at eval (eval at evaluate (:311:30), <anonymous>:5:29)";
    const V8_ASYNC_STACK: &str = "TypeError: Cannot read properties of null (reading 'x')\n    at https://app.example.test/assets/index-BqX7d2Kf.js:3:69\n    at window.asyncErr (https://app.example.test/assets/index-BqX7d2Kf.js:3:72)";
    const JSC_STACK: &str = "b@https://app.example.test/assets/index-BqX7d2Kf.js:1:57\na@https://app.example.test/assets/index-BqX7d2Kf.js:1:22\n@https://app.example.test/assets/index-BqX7d2Kf.js:2:28\n@";

    /// Held by every test that reaches a logging callsite. `tracing` caches a
    /// callsite's interest process-wide, so a test logging with no subscriber
    /// on another thread can mark it uninteresting while a capture runs.
    static LOGGING: std::sync::Mutex<()> = std::sync::Mutex::new(());

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
    fn message_loses_control_and_invisible_formatting_characters() {
        let hostile =
            "a\x1b[31mred\x1b[0m\tb\u{0085}c\u{2028}d\u{202E}e\u{200B}f\u{FEFF}g\x00h\x7f";
        let cleaned = clean_message(hostile);
        assert!(
            !cleaned.chars().any(is_unsafe_char),
            "unsafe character survived: {cleaned:?}"
        );
        assert_eq!(cleaned, "a [31mred [0m b c d e f g h");
    }

    #[test]
    fn message_masks_emails_tokens_ids_and_query_values() {
        let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxIn0.c2lnbmF0dXJlLXZhbHVl";
        let cases = [
            (
                "User kyle@example.com not found",
                "User k***@example.com not found",
            ),
            (
                "GET /api/admin/email-suppressions/kyle%40example.com failed",
                "GET /api/admin/email-suppressions/k***@example.com failed",
            ),
            (&*format!("bad token {jwt}"), "bad token [REDACTED_JWT]"),
            (
                "ticket 0b8e6c1e-8a5a-4f5c-9d61-0a1b2c3d4e5f gone",
                "ticket :uuid gone",
            ),
            (
                "invite nsk_live_4f9a8b7c6d5e4f3a2b1c0d9e8f expired",
                "invite :token expired",
            ),
            (
                "fetch https://h.test/reset?token=abc123&x=1#frag failed",
                "fetch https://h.test/reset?[query] failed",
            ),
            ("#access_token=secretvalue in hash", "?[query] in hash"),
        ];
        for (input, expected) in cases {
            assert_eq!(clean_message(input), expected, "input: {input}");
        }
        // Ordinary error text is untouched, long identifiers included.
        for keep in [
            "TypeError: Cannot read properties of undefined (reading 'title')",
            "useWorkspaceMembersStoreWithAVeryLongName is not a function",
            "Why did this fail?",
        ] {
            assert_eq!(clean_message(keep), keep);
        }
    }

    #[test]
    fn message_input_is_bounded_before_it_is_cleaned() {
        let huge = format!("{}kyle@example.com", "x ".repeat(MAX_MESSAGE_INPUT_BYTES));
        let cleaned = clean_message(&huge);
        assert!(!cleaned.contains("kyle@"));
        assert!(cleaned.chars().count() <= MAX_MESSAGE_CHARS);
    }

    #[test]
    fn http_messages_are_rebuilt_from_a_grouped_path() {
        let r = |message: &str| {
            Report::from_raw(raw(json!({
                "kind": "http", "surface": "agent", "status_code": 502, "message": message
            })))
            .expect("kept")
            .message
        };
        // What the web apps send passes through unchanged.
        assert_eq!(
            r("502 GET /api/tickets/:id/comments"),
            "502 GET /api/tickets/:id/comments"
        );
        // A client that sent a raw path still gets it grouped.
        assert_eq!(
            r("502 DELETE /api/admin/email-suppressions/kyle@example.com"),
            "502 DELETE /api/admin/email-suppressions/:param"
        );
        assert_eq!(
            r("502 GET /api/documentation/pages/slug/payroll-review/with-children"),
            "502 GET /api/documentation/pages/slug/:slug/with-children"
        );
        assert_eq!(
            r("502 GET /api/users/0b8e6c1e-8a5a-4f5c-9d61-0a1b2c3d4e5f?q=kyle"),
            "502 GET /api/users/:uuid"
        );
        // The status is the validated field, not the message's.
        assert_eq!(r("200 GET /api/tickets"), "502 GET /api/tickets");
        // Anything else is not logged as text at all.
        assert_eq!(r("free text kyle@example.com"), "502 (unknown request)");
        assert_eq!(r("502 GET /api/x\u{1b}[2J"), "502 GET /api/:param");
    }

    #[test]
    fn api_paths_keep_words_and_replace_everything_else() {
        assert_eq!(
            group_path("/api/workspaces/abc-123/members/42", false),
            "/api/workspaces/:param/members/:id"
        );
        assert_eq!(
            group_path("/api/v1/canned-response-starters", false),
            "/api/v1/canned-response-starters"
        );
        assert_eq!(
            group_path("/api/auth/invitation/AbCdEfGhIjKlMnOpQrStUvWx", false),
            "/api/auth/invitation/:token"
        );
        assert_eq!(group_path("/api/Tickets", false), "/api/:param");
        // An all-letter token is not a word.
        assert_eq!(
            group_path("/api/auth/invitation/abcdefghijklmnopqrstuvwxyz", false),
            "/api/auth/invitation/:token"
        );
        // Route parameter syntax is not a word outside route mode.
        assert_eq!(group_path("/api/:workspace", false), "/api/:param");
    }

    #[test]
    fn routes_keep_parameters_and_lose_literal_ids() {
        let route = |r: &str| {
            Report::from_raw(raw(json!({"kind": "vue", "surface": "agent", "route": r})))
                .expect("kept")
                .route
        };
        assert_eq!(
            route("/:workspace?/tickets/:id(\\d+)"),
            "/:workspace?/tickets/:id(\\d+)"
        );
        assert_eq!(route("/:pathMatch(.*)*"), "/:pathMatch(.*)*");
        assert_eq!(route("/404"), "/404");
        assert_eq!(route("/acme-corp/tickets/123"), "/acme-corp/tickets/:id");
        assert_eq!(
            route("/tickets/0b8e6c1e-8a5a-4f5c-9d61-0a1b2c3d4e5f"),
            "/tickets/:uuid"
        );
        assert_eq!(route("/reset/AbCdEfGhIjKlMnOpQrStUv12"), "/reset/:token");
    }

    #[test]
    fn the_kill_switch_reads_any_case() {
        assert!(enabled_from(None));
        assert!(enabled_from(Some("")));
        assert!(enabled_from(Some("on")));
        for off in ["off", "OFF", " Off ", "0", "false", "FALSE", "no"] {
            assert!(!enabled_from(Some(off)), "{off:?} should turn reports off");
        }
    }

    #[test]
    fn the_line_budget_resets_each_minute_and_says_once_when_it_drops() {
        let mut budget = LineBudget::new();
        let t0 = Instant::now();
        assert_eq!(budget.take(t0, 2), Take::Log);
        assert_eq!(budget.take(t0, 2), Take::Log);
        assert_eq!(budget.take(t0, 2), Take::FirstDropped);
        assert_eq!(budget.take(t0 + Duration::from_secs(59), 2), Take::Dropped);
        assert_eq!(budget.take(t0 + Duration::from_secs(60), 2), Take::Log);
    }

    mod logging {
        use super::*;
        use std::collections::BTreeMap;
        use std::sync::{Arc, Mutex as StdMutex};
        use tracing::field::{Field, Visit};
        use tracing::{Event, Subscriber};
        use tracing_subscriber::layer::{Context, SubscriberExt};
        use tracing_subscriber::Layer;

        #[derive(Clone, Default)]
        struct Capture(Arc<StdMutex<Vec<BTreeMap<String, String>>>>);

        struct Fields<'a>(&'a mut BTreeMap<String, String>);

        impl Visit for Fields<'_> {
            fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
                self.0.insert(field.name().into(), format!("{value:?}"));
            }
            fn record_str(&mut self, field: &Field, value: &str) {
                self.0.insert(field.name().into(), value.into());
            }
        }

        impl<S: Subscriber> Layer<S> for Capture {
            fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
                if event.metadata().target() != "client_error" {
                    return;
                }
                let mut fields = BTreeMap::new();
                event.record(&mut Fields(&mut fields));
                self.0.lock().unwrap().push(fields);
            }
        }

        fn lines(req: &HttpRequest, body: &[u8], enabled: bool) -> Vec<BTreeMap<String, String>> {
            let _guard = LOGGING.lock().unwrap_or_else(|p| p.into_inner());
            let capture = Capture::default();
            let subscriber = tracing_subscriber::registry().with(capture.clone());
            tracing::subscriber::with_default(subscriber, || {
                tracing::callsite::rebuild_interest_cache();
                handle(req, body, enabled)
            });
            let out = capture.0.lock().unwrap().clone();
            out
        }

        fn body() -> Vec<u8> {
            serde_json::to_vec(&json!({"reports": [
                {
                    "kind": "error", "surface": "agent",
                    "message": "Failed for kyle@example.com\n\u{1b}[2Jinjected",
                    "stack": V8_STACK, "route": "/tickets/:id", "build_sha": "a1b2c3d",
                    "user_agent": "Mozilla/5.0 secret", "email": "kyle@example.com"
                },
                {"kind": "http", "surface": "portal", "status_code": 503,
                 "message": "503 POST /api/portal/tickets/42"}
            ]}))
            .unwrap()
        }

        #[test]
        fn each_report_is_one_reduced_line() {
            let req = actix_web::test::TestRequest::post().to_http_request();
            let got = lines(&req, &body(), true);
            assert_eq!(got.len(), 2, "{got:?}");
            let first = &got[0];
            let keys: Vec<&str> = first.keys().map(String::as_str).collect();
            assert_eq!(
                keys,
                [
                    "build_sha",
                    "client_error_fingerprint",
                    "client_error_frames",
                    "client_error_surface",
                    "count",
                    "kind",
                    "message",
                    "route"
                ]
            );
            assert_eq!(first["message"], "Failed for k***@example.com  [2Jinjected");
            assert_eq!(first["route"], "/tickets/:id");
            assert!(!format!("{got:?}").contains("Mozilla"));
            assert!(!format!("{got:?}").contains("kyle@"));
            assert_eq!(got[1]["message"], "503 POST /api/portal/tickets/:id");
            assert_eq!(got[1]["status_code"], "503");
        }

        #[test]
        fn workspace_id_comes_only_from_the_resolved_host() {
            let req = actix_web::test::TestRequest::post()
                .insert_header(("X-Nosdesk-Workspace", "other"))
                .to_http_request();
            assert!(lines(&req, &body(), true)
                .iter()
                .all(|l| !l.contains_key("workspace_id")));
            req.extensions_mut().insert(WorkspaceContext {
                workspace_id: 7,
                workspace_uuid: uuid::Uuid::nil(),
                slug: "acme".into(),
                name: "Acme".into(),
                custom_domain: None,
                organisation_id: None,
            });
            let got = lines(&req, &body(), true);
            assert!(got
                .iter()
                .all(|l| l.get("workspace_id").map(String::as_str) == Some("7")));
        }

        #[test]
        fn the_kill_switch_logs_nothing() {
            let req = actix_web::test::TestRequest::post().to_http_request();
            assert!(lines(&req, &body(), false).is_empty());
        }
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

    /// The route as startup registers it: the body cap and the handler.
    fn app() -> actix_web::App<
        impl actix_web::dev::ServiceFactory<
            actix_web::dev::ServiceRequest,
            Config = (),
            Response = actix_web::dev::ServiceResponse,
            Error = actix_web::Error,
            InitError = (),
        >,
    > {
        actix_web::App::new().service(
            web::resource("/api/client-errors")
                .app_data(web::PayloadConfig::new(MAX_BODY_BYTES))
                .route(web::post().to(report)),
        )
    }

    // A sync test around `block_on`, so the logging lock is never held across
    // an `.await`.
    #[test]
    fn the_endpoint_answers_204_to_anything_within_the_cap() {
        use actix_web::test;
        let _guard = LOGGING.lock().unwrap_or_else(|p| p.into_inner());
        actix_web::rt::System::new().block_on(async {
            let app = test::init_service(app()).await;
            for body in [
                b"{\"reports\":[{\"kind\":\"error\",\"surface\":\"agent\",\"message\":\"m\"}]}"
                    .to_vec(),
                b"garbage".to_vec(),
                // Nesting far past serde_json's recursion limit: refused, not a
                // stack overflow.
                format!(
                    "{{\"reports\":[{}{}]}}",
                    "[".repeat(15_000),
                    "]".repeat(15_000)
                )
                .into_bytes(),
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
        });
    }

    #[actix_web::test]
    async fn a_body_over_the_cap_is_refused_unread() {
        use actix_web::test;
        let app = test::init_service(app()).await;
        let req = test::TestRequest::post()
            .uri("/api/client-errors")
            .insert_header(("content-type", "text/plain"))
            .set_payload(vec![b' '; MAX_BODY_BYTES + 1])
            .to_request();
        let resp = test::call_service(&app, req).await;
        assert_eq!(resp.status(), 413);
    }

    #[test]
    fn the_rate_limit_bucket_is_the_peer_not_a_forwarded_header() {
        use crate::utils::client_ip::limit_bucket;
        use actix_web::test::TestRequest;
        let req = TestRequest::post()
            .peer_addr("203.0.113.7:5000".parse().unwrap())
            .insert_header(("X-Forwarded-For", "198.51.100.1"))
            .to_srv_request();
        let peer = format!("clienterr:{}", limit_bucket("203.0.113.7".parse().unwrap()));
        assert_eq!(rate_limit_key(&req).as_deref(), Some(peer.as_str()));
        let v6 = TestRequest::post()
            .peer_addr("[2001:db8:1:2:3:4:5:6]:5000".parse().unwrap())
            .to_srv_request();
        let same_64 = format!(
            "clienterr:{}",
            limit_bucket("2001:db8:1:2::9".parse().unwrap())
        );
        assert_eq!(rate_limit_key(&v6).as_deref(), Some(same_64.as_str()));
    }
}
