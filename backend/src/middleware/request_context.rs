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

use std::time::Instant;

use actix_web::body::MessageBody;
use actix_web::dev::{ServiceRequest, ServiceResponse};
use actix_web::{Error, HttpMessage};
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
        request
            .extensions_mut()
            .insert(RequestStart(Instant::now()));
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

    // Extract owned values so the extensions borrow is released before the
    // event macro. Absent-auth requests get stable sentinels (workspace 0,
    // empty user, "anonymous") so the field set is uniform across every event.
    let (request_id, workspace_id, user_uuid, actor_kind, latency_ms) = {
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
        (request_id, workspace_id, user_uuid, actor_kind, latency_ms)
    };

    tracing::info!(
        target: "nosdesk::request",
        request_id = %request_id,
        method = method,
        route = %route,
        status_code = status,
        latency_ms = latency_ms,
        workspace_id = workspace_id,
        user_uuid = %user_uuid,
        actor_kind = actor_kind,
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
