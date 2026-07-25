//! Per-request observability + actor context.
//!
//! Bundles two concerns that always travel together:
//! 1. The `tracing-actix-web` request id (the public correlation key
//!    surfaced on every log line via the root span).
//! 2. The [`ActorContext`] (who's making the change, drives the audit
//!    triggers via Postgres GUCs through `sync::session::set_actor`).
//!
//! Auth middlewares insert a [`RequestContext`] into request
//! extensions after validating credentials; handlers pull it via
//! `web::ReqData<RequestContext>` or `req.extensions().get()`.
//!
//! For routes without auth (login, public health probes), no
//! RequestContext exists. The `tracing-actix-web` span is still
//! present, so the request_id remains on every log line emitted
//! during the request.

use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

use actix_web::body::MessageBody;
use actix_web::dev::{ServiceRequest, ServiceResponse};
use actix_web::{Error, HttpMessage, HttpRequest};
use serde_json::{Map, Value};
use tracing::Span;
use tracing_actix_web::{root_span, DefaultRootSpanBuilder, RequestId, RootSpanBuilder};
use uuid::Uuid;

use crate::models::Claims;
use crate::sync::actor::{ActorContext, ActorKind};

/// Request-start instant, stashed on request extensions in
/// [`NosdeskRootSpanBuilder::on_request_start`] so the canonical wide event can
/// report `latency_ms` at request end.
#[derive(Clone, Copy)]
struct RequestStart(Instant);

/// Request-scoped bag of high-cardinality **business** dimensions that handlers
/// accumulate through the request — the missing half of the wide-events model.
///
/// `tracing` requires every event/span field declared up front, so a handler
/// can't attach `ticket_id`/`outcome`/… to the canonical event directly. Instead
/// handlers stamp this bag via [`record_canonical`]; at request end
/// [`emit_canonical_event`] serialises it into the single `_canonical` field,
/// and the redaction layer flattens each key back to a top-level field *through
/// the allowlist* (`utils::tracing_redact`) — so one row carries HTTP dims +
/// actor + whatever business outcome the handler recorded, and bag fields are
/// held to the same PII policy as first-class ones.
///
/// `Arc<Mutex<…>>` so it's `Clone` (extractable via `web::ReqData`) and can be
/// stamped from anywhere holding the request, including across `.await`s.
#[derive(Clone, Default)]
pub struct CanonicalContext(Arc<Mutex<Map<String, Value>>>);

impl CanonicalContext {
    /// Stamp one business dimension onto the canonical event. Last write wins.
    /// The key must be on the `tracing_redact` allowlist or it's dropped at
    /// emit — keep these to bounded enums / counts / stable IDs, never free text.
    pub fn record(&self, key: &str, value: impl Into<Value>) {
        if let Ok(mut map) = self.0.lock() {
            map.insert(key.to_string(), value.into());
        }
    }

    /// Serialise the bag for the `_canonical` field, or `None` when empty (so a
    /// request that stamped nothing doesn't carry an empty field).
    fn snapshot_json(&self) -> Option<String> {
        let map = self.0.lock().ok()?;
        if map.is_empty() {
            return None;
        }
        serde_json::to_string(&*map).ok()
    }
}

/// Stamp a business dimension onto the request's canonical wide event. No-op if
/// the request has no `CanonicalContext` (only happens outside the normal
/// middleware stack, e.g. some unit tests). Ergonomic front door for handlers:
/// `record_canonical(&req, "ticket_id", ticket.id);`
pub fn record_canonical(req: &HttpRequest, key: &str, value: impl Into<Value>) {
    if let Some(cc) = req.extensions().get::<CanonicalContext>() {
        cc.record(key, value);
    }
}

/// Per-request context: who's acting, with what correlation id.
///
/// The `correlation_id` mirrors the tracing-actix-web request id, so a
/// grep across logs and persisted audit rows resolves through the same
/// key.
#[derive(Debug, Clone)]
pub struct RequestContext {
    pub correlation_id: Uuid,
    pub actor: ActorContext,
}

impl RequestContext {
    pub fn new(correlation_id: Uuid, actor: ActorContext) -> Self {
        Self {
            correlation_id,
            actor,
        }
    }
}

/// W3C Trace Context for **cross-service** correlation (observability Phase 1 —
/// `docs/decisions/observability-tracing-2026-07-15.md` in the control-plane
/// repo). `trace_id` is the id shared across every service that touches one
/// logical operation; `span_id` is this service's leg of it. Both are stamped
/// on the canonical wide event (already allowlisted), so a provisioning / OIDC
/// flow that crosses the control plane and this product can be stitched in Loki
/// with `| json | trace_id="…"` — no Tempo, no OTel SDK yet.
///
/// If an inbound request carries a `traceparent` header (the control plane
/// injects one on its calls here) we adopt its `trace_id` and mint a fresh
/// `span_id`; otherwise we originate a new trace.
#[derive(Clone)]
pub struct TraceContext {
    pub trace_id: String,
    pub span_id: String,
}

fn to_hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        })
}

impl TraceContext {
    /// 16-byte trace id + 8-byte span id, hex-encoded (W3C sizes), from UUIDv4
    /// randomness (getrandom-backed) — no extra RNG dep.
    fn generate() -> Self {
        Self {
            trace_id: to_hex(Uuid::new_v4().as_bytes()),
            span_id: to_hex(&Uuid::new_v4().as_bytes()[..8]),
        }
    }

    /// Parse a W3C `traceparent` (`version-traceid-spanid-flags`), keep its
    /// trace id, and mint a fresh span id for our leg. Returns `None` for any
    /// malformed / all-zero / non-v00 header (fall back to a new trace).
    fn from_traceparent(header: &str) -> Option<Self> {
        let parts: Vec<&str> = header.split('-').collect();
        if parts.len() != 4 || parts[0] != "00" {
            return None;
        }
        let trace_id = parts[1];
        let is_hex = |s: &str| s.bytes().all(|b| b.is_ascii_hexdigit());
        if trace_id.len() != 32 || !is_hex(trace_id) || trace_id.bytes().all(|b| b == b'0') {
            return None;
        }
        Some(Self {
            trace_id: trace_id.to_ascii_lowercase(),
            span_id: to_hex(&Uuid::new_v4().as_bytes()[..8]),
        })
    }

    fn extract_or_generate(req: &ServiceRequest) -> Self {
        req.headers()
            .get("traceparent")
            .and_then(|v| v.to_str().ok())
            .and_then(Self::from_traceparent)
            .unwrap_or_else(Self::generate)
    }

    /// The `traceparent` header value to propagate onward (flags `01` = sampled).
    pub fn traceparent(&self) -> String {
        format!("00-{}-{}-01", self.trace_id, self.span_id)
    }
}

/// Custom `RootSpanBuilder` that (a) pre-declares `user_uuid`,
/// `actor_kind`, and `workspace_id` as empty fields on the root span so
/// auth middlewares can record them post-hoc (via [`record_user_on_span`] /
/// [`populate`]) and every handler log line inherits them by ancestry, and
/// (b) emits ONE canonical wide event per request at completion — the
/// loggingsucks.com "canonical log line": a single flat, high-dimensionality,
/// PII-free summary (`request_id`, `method`, `route`, `status_code`,
/// `latency_ms`, `workspace_id`, `user_uuid`, `actor_kind`). `tracing`
/// requires every span field declared up front, hence [`tracing::field::Empty`].
pub struct NosdeskRootSpanBuilder;

impl RootSpanBuilder for NosdeskRootSpanBuilder {
    fn on_request_start(request: &ServiceRequest) -> Span {
        {
            let trace = TraceContext::extract_or_generate(request);
            let mut ext = request.extensions_mut();
            ext.insert(RequestStart(Instant::now()));
            // Empty business-dimension bag; handlers fill it via record_canonical.
            ext.insert(CanonicalContext::default());
            // Cross-service trace context (adopted from an inbound traceparent
            // or freshly originated). Stamped on the canonical event below.
            ext.insert(trace);
        }
        root_span!(
            request,
            user_uuid = tracing::field::Empty,
            actor_kind = tracing::field::Empty,
            workspace_id = tracing::field::Empty,
        )
    }

    fn on_request_end<B: MessageBody>(span: Span, outcome: &Result<ServiceResponse<B>, Error>) {
        DefaultRootSpanBuilder::on_request_end(span, outcome);
        emit_canonical_event(outcome);
    }
}

/// Requests at/above this latency are always kept by the sampler — slow
/// requests are the interesting ones (tail sampling, loggingsucks.com).
const SLOW_REQUEST_MS: u64 = 1_000;

/// Fraction of successful, fast requests whose canonical event is emitted.
/// `LOG_SAMPLE_RATE` in `[0.0, 1.0]`; default `1.0` — keep everything, so this
/// is an inert lever until an operator dials it down for log-volume cost.
/// Parsed once (env doesn't change at runtime).
fn sample_rate() -> f64 {
    static RATE: OnceLock<f64> = OnceLock::new();
    *RATE.get_or_init(|| {
        std::env::var("LOG_SAMPLE_RATE")
            .ok()
            .and_then(|s| s.parse::<f64>().ok())
            .map(|r| r.clamp(0.0, 1.0))
            .unwrap_or(1.0)
    })
}

/// Tail-sampling decision: always keep errors (`status >= 400`) and slow
/// requests; sample the fast-success remainder. Reads the process [`sample_rate`].
fn keep_canonical(status: u16, latency_ms: u64, request_id: &str) -> bool {
    keep_canonical_at(status, latency_ms, request_id, sample_rate())
}

/// Testable core of [`keep_canonical`] with the rate passed in. Deterministic
/// on `request_id` (FNV-1a → uniform bucket in `[0,10000)`) so a request is
/// wholly kept or dropped, and a given id samples identically every time.
fn keep_canonical_at(status: u16, latency_ms: u64, request_id: &str, rate: f64) -> bool {
    if status >= 400 || latency_ms >= SLOW_REQUEST_MS {
        return true;
    }
    if rate >= 1.0 {
        return true;
    }
    if rate <= 0.0 {
        return false;
    }
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in request_id.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    (h % 10_000) < (rate * 10_000.0) as u64
}

/// Emit the canonical per-request wide event. One flat line, self-contained
/// (no span nesting), so a Loki/LogQL `| json | workspace_id="7" | status_code>=500`
/// query works directly. Static per-process context (region, version, machine
/// id) is deliberately NOT duplicated here: it belongs as Loki *labels* set by
/// the log shipper from the `FLY_*` / `NOSDESK_VERSION` env, not in every line.
/// Health/readiness probes are skipped — they fire every ~30s per machine and
/// carry no signal.
fn emit_canonical_event<B>(outcome: &Result<ServiceResponse<B>, Error>) {
    // Only the Ok path carries the request + response we need. A raw `Err`
    // (service returned before response conversion — rare) is already covered by
    // the handler's own error log line and the root span.
    let Ok(resp) = outcome else {
        return;
    };
    let req = resp.request();
    let route = req
        .match_pattern()
        .unwrap_or_else(|| req.path().to_string());
    if route == "/health" || route == "/readiness" {
        return;
    }
    let status = resp.status().as_u16();
    let method = req.method().as_str();
    // Stable error taxonomy for `?`-propagated ApiErrors, stashed on the
    // response by `ApiError::error_response`. Stamped into the bag below so it
    // rides `_canonical` (present only on errors, never a noisy "" on 2xx).
    let error_kind = resp
        .response()
        .extensions()
        .get::<crate::handlers::errors::ErrorKind>()
        .map(|k| k.0);

    // Extract owned values so the extensions borrow is released before the
    // event macro. Absent-auth requests get stable sentinels (workspace 0,
    // empty user, "anonymous") so the field set is uniform across every event.
    let (request_id, workspace_id, user_uuid, actor_kind, latency_ms, canonical, trace_id, span_id) = {
        let ext = req.extensions();
        let request_id = ext
            .get::<RequestId>()
            .map(|r| (**r).to_string())
            .unwrap_or_default();
        let ctx = ext.get::<RequestContext>();
        let workspace_id = ctx.and_then(|c| c.actor.workspace_id).unwrap_or(0) as i64;
        let user_uuid = ctx
            .and_then(|c| c.actor.uuid)
            .map(|u| u.to_string())
            .unwrap_or_default();
        let actor_kind = ctx.map(|c| c.actor.kind.as_str()).unwrap_or("anonymous");
        let latency_ms = ext
            .get::<RequestStart>()
            .map(|s| s.0.elapsed().as_millis() as u64)
            .unwrap_or(0);
        // Fold the error taxonomy into the business bag, then serialise. The
        // redaction layer flattens the bag (per-key, through the allowlist)
        // back into top-level fields on this one event.
        if let (Some(ek), Some(cc)) = (error_kind, ext.get::<CanonicalContext>()) {
            cc.record("error_kind", ek);
        }
        let canonical = ext
            .get::<CanonicalContext>()
            .and_then(|c| c.snapshot_json());
        // Cross-service trace ids (always present — adopted or generated at
        // request start). Empty strings only if the context somehow wasn't set.
        let (trace_id, span_id) = ext
            .get::<TraceContext>()
            .map(|t| (t.trace_id.clone(), t.span_id.clone()))
            .unwrap_or_default();
        (
            request_id,
            workspace_id,
            user_uuid,
            actor_kind,
            latency_ms,
            canonical,
            trace_id,
            span_id,
        )
    };

    // Tail sampling: always keep errors + slow requests; sample the rest.
    if !keep_canonical(status, latency_ms, &request_id) {
        return;
    }

    tracing::info!(
        target: "nosdesk::request",
        request_id = %request_id,
        trace_id = %trace_id,
        span_id = %span_id,
        method = method,
        route = %route,
        status_code = status,
        latency_ms = latency_ms,
        workspace_id = workspace_id,
        user_uuid = %user_uuid,
        actor_kind = actor_kind,
        _canonical = canonical.as_deref().unwrap_or(""),
        "http_request"
    );
}

/// Record user identity on the request's root span. Called by auth
/// middlewares after Claims are extracted, so log lines emitted by
/// the handler carry user attribution alongside the auto-populated
/// HTTP fields.
pub fn record_user_on_span(uuid: &str, kind: &'static str) {
    let span = Span::current();
    span.record("user_uuid", uuid);
    span.record("actor_kind", kind);
}

/// Build a [`RequestContext`] from validated claims and the
/// tracing-actix-web request id, then stash it in extensions and record
/// the user on the current root span. Called by every auth middleware
/// at the moment it accepts a credential, so attribution is uniform
/// regardless of how the request authenticated (cookie, Bearer token,
/// or future SSO flows).
///
/// If the request id isn't on extensions yet (no `TracingLogger`
/// upstream — only happens in unit tests), a fresh v7 uuid stands in.
/// If `claims.sub` isn't a uuid (legacy / malformed token), the actor
/// is recorded as anonymous-but-correlated so audit triggers still see
/// the correlation id.
pub fn populate(req: &ServiceRequest, claims: &Claims) {
    let correlation_id = req
        .extensions()
        .get::<RequestId>()
        .map(|rid| **rid)
        .unwrap_or_else(Uuid::now_v7);
    let user_uuid = Uuid::parse_str(&claims.sub).ok();
    let mut actor = if let Some(uuid) = user_uuid {
        ActorContext::user(uuid, Some(correlation_id))
    } else {
        ActorContext {
            kind: ActorKind::User,
            uuid: None,
            reference: None,
            correlation_id: Some(correlation_id),
            client_tx_id: None,
            workspace_id: None,
        }
    };
    // The WorkspaceContextMiddleware runs ahead of this and
    // attaches a WorkspaceContext to the request extensions
    // (self-hosted always; hosted when subdomain resolves). Pin
    // the actor to that workspace so the `app.workspace_id` GUC
    // gets set inside `with_actor_context`. None for apex /
    // unrecognised-subdomain paths — those routes shouldn't
    // touch tenant tables.
    if let Some(ws) = req
        .extensions()
        .get::<crate::extractors::WorkspaceContext>()
    {
        actor = actor.with_workspace(ws.workspace_id);
        // Surface the tenant on the root span so every handler log line for
        // this request inherits it by ancestry (and the canonical wide event
        // reports it from the actor).
        Span::current().record("workspace_id", ws.workspace_id);
    }
    // Plugin-initiated writes (from the sandbox host API) carry
    // `X-Nosdesk-Plugin: <uuid>`. Record it as the actor reference so the audit /
    // sync trail attributes the write to the plugin. The actor stays
    // `kind = user` (authz is unchanged — the plugin acts as the user, bounded by
    // the user's own perms + RLS); this only annotates *who initiated* it.
    // Best-effort: a malformed or absent header simply leaves the reference unset.
    if let Some(plugin_uuid) = req
        .headers()
        .get("X-Nosdesk-Plugin")
        .and_then(|h| h.to_str().ok())
        .and_then(|s| Uuid::parse_str(s).ok())
    {
        actor.reference = Some(format!("plugin:{plugin_uuid}"));
    }
    record_user_on_span(&claims.sub, ActorKind::User.as_str());
    req.extensions_mut()
        .insert(RequestContext::new(correlation_id, actor));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    use actix_web::body::BoxBody;
    use actix_web::test::TestRequest;
    use actix_web::HttpResponse;
    use tracing::field::{Field, Visit};
    use tracing::{Event, Subscriber};
    use tracing_subscriber::layer::{Context, SubscriberExt};
    use tracing_subscriber::registry::LookupSpan;
    use tracing_subscriber::Layer;

    #[derive(Debug, Default)]
    struct CapturedEvent {
        target: String,
        message: String,
        fields: HashMap<String, String>,
    }

    #[derive(Clone, Default)]
    struct CaptureLayer(Arc<Mutex<Vec<CapturedEvent>>>);

    struct FieldGrab<'a>(&'a mut CapturedEvent);
    impl Visit for FieldGrab<'_> {
        fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
            let rendered = format!("{value:?}");
            if field.name() == "message" {
                self.0.message = rendered;
            } else {
                self.0.fields.insert(field.name().to_string(), rendered);
            }
        }
        fn record_str(&mut self, field: &Field, value: &str) {
            self.0
                .fields
                .insert(field.name().to_string(), value.to_string());
        }
        fn record_u64(&mut self, field: &Field, value: u64) {
            self.0
                .fields
                .insert(field.name().to_string(), value.to_string());
        }
        fn record_i64(&mut self, field: &Field, value: i64) {
            self.0
                .fields
                .insert(field.name().to_string(), value.to_string());
        }
    }

    impl<S: Subscriber + for<'a> LookupSpan<'a>> Layer<S> for CaptureLayer {
        fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
            let mut captured = CapturedEvent {
                target: event.metadata().target().to_string(),
                ..Default::default()
            };
            event.record(&mut FieldGrab(&mut captured));
            self.0.lock().unwrap().push(captured);
        }
    }

    fn run_capturing(f: impl FnOnce()) -> Vec<CapturedEvent> {
        // Serialize all capture-based tests: `with_default` sets a *thread-local*
        // subscriber, but cargo runs tests in parallel, and an emit on a
        // work-stealing thread can fall back to another test's dispatcher —
        // occasionally leaking a `nosdesk::request` event across captures. One
        // process-wide lock removes the race deterministically.
        static CAPTURE_LOCK: Mutex<()> = Mutex::new(());
        let _guard = CAPTURE_LOCK.lock().unwrap_or_else(|p| p.into_inner());

        let layer = CaptureLayer::default();
        let events = layer.0.clone();
        let subscriber = tracing_subscriber::registry().with(layer);
        tracing::subscriber::with_default(subscriber, f);
        Arc::try_unwrap(events).unwrap().into_inner().unwrap()
    }

    #[test]
    fn canonical_event_reports_the_request_dimensions() {
        let events = run_capturing(|| {
            let req = TestRequest::get().uri("/api/tickets/42").to_srv_request();
            req.extensions_mut().insert(RequestStart(Instant::now()));
            let actor = ActorContext::user(Uuid::from_u128(0xA11CE), Some(Uuid::from_u128(0xC0DE)))
                .with_workspace(7);
            req.extensions_mut()
                .insert(RequestContext::new(Uuid::from_u128(0xC0DE), actor));
            let resp = req.into_response(HttpResponse::Ok().finish());
            let outcome: Result<ServiceResponse<BoxBody>, Error> = Ok(resp);
            emit_canonical_event(&outcome);
        });

        let ev = events
            .iter()
            .find(|e| e.target == "nosdesk::request")
            .expect("canonical nosdesk::request event should fire");
        assert_eq!(ev.message, "http_request");
        assert_eq!(ev.fields.get("method").map(String::as_str), Some("GET"));
        assert_eq!(
            ev.fields.get("route").map(String::as_str),
            Some("/api/tickets/42")
        );
        assert_eq!(
            ev.fields.get("status_code").map(String::as_str),
            Some("200")
        );
        assert_eq!(ev.fields.get("workspace_id").map(String::as_str), Some("7"));
        assert_eq!(
            ev.fields.get("actor_kind").map(String::as_str),
            Some("user")
        );
        // High-cardinality dimensions present (values non-deterministic/PII-safe).
        assert!(ev.fields.contains_key("user_uuid"), "user_uuid present");
        assert!(ev.fields.contains_key("latency_ms"), "latency_ms present");
    }

    #[test]
    fn canonical_event_carries_stamped_business_fields() {
        let events = run_capturing(|| {
            let req = TestRequest::post().uri("/api/tickets").to_srv_request();
            req.extensions_mut().insert(RequestStart(Instant::now()));
            let cc = CanonicalContext::default();
            cc.record("ticket_id", 42);
            cc.record("outcome", "created");
            req.extensions_mut().insert(cc);
            let resp = req.into_response(HttpResponse::Ok().finish());
            let outcome: Result<ServiceResponse<BoxBody>, Error> = Ok(resp);
            emit_canonical_event(&outcome);
        });

        let ev = events
            .iter()
            .find(|e| e.target == "nosdesk::request")
            .expect("canonical event should fire");
        // The bag is serialised into `_canonical`; the redaction layer flattens
        // it per-key at emit (covered by tracing_redact tests). Here we prove the
        // handler-stamped dims reach the event.
        let canonical = ev.fields.get("_canonical").expect("_canonical present");
        assert!(canonical.contains("\"ticket_id\":42"), "got {canonical}");
        assert!(
            canonical.contains("\"outcome\":\"created\""),
            "got {canonical}"
        );
    }

    #[test]
    fn record_canonical_stamps_via_request_extensions() {
        let req = TestRequest::default().to_http_request();
        let cc = CanonicalContext::default();
        req.extensions_mut().insert(cc.clone());
        record_canonical(&req, "outcome", "ok");
        record_canonical(&req, "result_count", 5);
        let json = cc.snapshot_json().expect("bag non-empty");
        assert!(json.contains("\"outcome\":\"ok\""), "got {json}");
        assert!(json.contains("\"result_count\":5"), "got {json}");
    }

    #[test]
    fn snapshot_json_is_none_when_empty() {
        assert!(CanonicalContext::default().snapshot_json().is_none());
    }

    #[test]
    fn trace_context_generate_has_w3c_sizes() {
        let t = TraceContext::generate();
        assert_eq!(t.trace_id.len(), 32);
        assert_eq!(t.span_id.len(), 16);
        assert!(t.trace_id.bytes().all(|b| b.is_ascii_hexdigit()));
        assert_eq!(
            t.traceparent(),
            format!("00-{}-{}-01", t.trace_id, t.span_id)
        );
    }

    #[test]
    fn trace_context_adopts_inbound_traceparent_trace_id_and_mints_span() {
        let tp = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";
        let t = TraceContext::from_traceparent(tp).expect("valid traceparent");
        assert_eq!(t.trace_id, "4bf92f3577b34da6a3ce929d0e0e4736");
        // Fresh span id for our leg — NOT the inbound parent span.
        assert_ne!(t.span_id, "00f067aa0ba902b7");
        assert_eq!(t.span_id.len(), 16);
    }

    #[test]
    fn trace_context_rejects_malformed_traceparents() {
        for bad in [
            "",
            "garbage",
            "01-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01", // wrong version
            "00-tooShort-00f067aa0ba902b7-01",
            "00-00000000000000000000000000000000-00f067aa0ba902b7-01", // all-zero trace id
            "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7",    // missing flags
        ] {
            assert!(
                TraceContext::from_traceparent(bad).is_none(),
                "should reject: {bad}"
            );
        }
    }

    #[test]
    fn canonical_event_carries_trace_id() {
        let events = run_capturing(|| {
            let req = TestRequest::get()
                .uri("/api/tickets/42")
                .insert_header((
                    "traceparent",
                    "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
                ))
                .to_srv_request();
            let trace = TraceContext::extract_or_generate(&req);
            req.extensions_mut().insert(RequestStart(Instant::now()));
            req.extensions_mut().insert(trace);
            let outcome: Result<ServiceResponse<BoxBody>, Error> =
                Ok(req.into_response(HttpResponse::Ok().finish()));
            emit_canonical_event(&outcome);
        });
        let ev = events
            .iter()
            .find(|e| e.target == "nosdesk::request")
            .expect("canonical event should fire");
        assert_eq!(
            ev.fields.get("trace_id").map(String::as_str),
            Some("4bf92f3577b34da6a3ce929d0e0e4736")
        );
        assert!(ev.fields.contains_key("span_id"), "span_id present");
    }

    #[test]
    fn sampler_always_keeps_errors_and_slow_even_at_zero_rate() {
        // Errors kept regardless of rate.
        assert!(keep_canonical_at(500, 5, "req-a", 0.0));
        assert!(keep_canonical_at(404, 5, "req-a", 0.0));
        // Slow requests kept regardless of rate.
        assert!(keep_canonical_at(200, SLOW_REQUEST_MS, "req-a", 0.0));
    }

    #[test]
    fn sampler_rate_bounds_are_keep_all_and_drop_all() {
        // rate 1.0 keeps every fast success; rate 0.0 drops them.
        assert!(keep_canonical_at(200, 5, "req-a", 1.0));
        assert!(!keep_canonical_at(200, 5, "req-a", 0.0));
    }

    #[test]
    fn sampler_is_deterministic_and_roughly_proportional() {
        // Same id + rate → same decision every call.
        let d = keep_canonical_at(200, 5, "stable-id", 0.5);
        assert_eq!(d, keep_canonical_at(200, 5, "stable-id", 0.5));
        // ~50% of distinct ids kept at rate 0.5 (loose bounds — just proves the
        // hash spreads and the threshold bites).
        let kept = (0..2000)
            .filter(|i| keep_canonical_at(200, 5, &format!("id-{i}"), 0.5))
            .count();
        assert!((700..=1300).contains(&kept), "kept {kept}/2000 at rate 0.5");
    }

    #[test]
    fn canonical_event_reports_error_kind_from_response_ext() {
        use crate::handlers::errors::ErrorKind;
        let events = run_capturing(|| {
            let req = TestRequest::get().uri("/api/tickets/42").to_srv_request();
            req.extensions_mut().insert(RequestStart(Instant::now()));
            req.extensions_mut().insert(CanonicalContext::default());
            let mut resp = HttpResponse::NotFound().finish();
            resp.extensions_mut().insert(ErrorKind("not_found"));
            let outcome: Result<ServiceResponse<BoxBody>, Error> = Ok(req.into_response(resp));
            emit_canonical_event(&outcome);
        });
        let ev = events
            .iter()
            .find(|e| e.target == "nosdesk::request")
            .expect("canonical event should fire");
        let canonical = ev.fields.get("_canonical").expect("_canonical present");
        assert!(
            canonical.contains("\"error_kind\":\"not_found\""),
            "got {canonical}"
        );
    }

    #[test]
    fn canonical_event_skips_health_probes() {
        let events = run_capturing(|| {
            let req = TestRequest::get().uri("/health").to_srv_request();
            req.extensions_mut().insert(RequestStart(Instant::now()));
            let resp = req.into_response(HttpResponse::Ok().finish());
            let outcome: Result<ServiceResponse<BoxBody>, Error> = Ok(resp);
            emit_canonical_event(&outcome);
        });
        assert!(
            !events.iter().any(|e| e.target == "nosdesk::request"),
            "health probe must not emit a canonical event"
        );
    }
}
