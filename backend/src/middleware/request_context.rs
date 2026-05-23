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

use actix_web::body::MessageBody;
use actix_web::dev::{ServiceRequest, ServiceResponse};
use actix_web::{Error, HttpMessage};
use tracing::Span;
use tracing_actix_web::{root_span, DefaultRootSpanBuilder, RequestId, RootSpanBuilder};
use uuid::Uuid;

use crate::models::Claims;
use crate::sync::actor::{ActorContext, ActorKind};

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

/// Custom `RootSpanBuilder` that pre-declares `user_uuid` and
/// `actor_kind` as empty fields on the root span. `tracing` requires
/// every field to be declared up front; [`tracing::field::Empty`]
/// reserves the slot so auth middlewares can record values post-hoc
/// via [`record_user_on_span`].
pub struct NosdeskRootSpanBuilder;

impl RootSpanBuilder for NosdeskRootSpanBuilder {
    fn on_request_start(request: &ServiceRequest) -> Span {
        root_span!(
            request,
            user_uuid = tracing::field::Empty,
            actor_kind = tracing::field::Empty,
        )
    }

    fn on_request_end<B: MessageBody>(span: Span, outcome: &Result<ServiceResponse<B>, Error>) {
        DefaultRootSpanBuilder::on_request_end(span, outcome);
    }
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
    }
    record_user_on_span(&claims.sub, ActorKind::User.as_str());
    req.extensions_mut()
        .insert(RequestContext::new(correlation_id, actor));
}
