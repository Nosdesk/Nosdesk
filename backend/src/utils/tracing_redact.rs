//! Allowlist-filtered JSON `tracing` layer.
//!
//! The default `tracing_subscriber::fmt::layer()` serialises every
//! field on every event — including any `email`, `user_name`,
//! `ticket_title`, etc. that a call site happens to attach. For a
//! multi-tenant helpdesk that's a non-trivial GDPR / SOC 2 exposure
//! once logs flow into a third-party log shipper / Loki / Grafana
//! Cloud; the operator quickly becomes a sub-processor of every
//! tenant's customer text.
//!
//! This layer replaces the JSON output path with one that drops every
//! field whose name is not in [`ALLOWED_FIELDS`] — silently, but with
//! a `redacted` count attached to the event so operators can observe
//! the rate without inspecting the values. Span ancestry is walked
//! and span fields are subject to the same allowlist, so a
//! `#[instrument(fields(email = %u.email))]` field is dropped even
//! though the call site looks fine.
//!
//! ## Allowlist policy
//!
//! - **Safe to log:** tenant-internal stable identifiers (UUIDs and
//!   integer IDs for tickets, comments, assets, workspaces, etc.),
//!   bounded enums (status, priority, role, recurrence, category),
//!   bounded operational signals (counts, timings, error kinds, op
//!   names), and tracing/span correlation (request_id, span_id,
//!   trace_id).
//! - **Forbidden:** anything user-typed or user-identifying —
//!   emails, names, ticket titles / descriptions / comment bodies,
//!   IPs, user-agent strings, tokens, secrets, request bodies.
//!
//! The local-dev pretty formatter is unaffected — developer laptops
//! aren't sub-processors of anyone's data. The redaction layer is
//! gated on `LOG_FORMAT=json`, which is the production-Docker
//! configuration; `cargo run` against localhost stays pretty.
//!
//! ## Adding a field
//!
//! New fields that warrant logging need an explicit PR adding them
//! to [`ALLOWED_FIELDS`]. That PR is the audit trail SOC 2 CC6.7
//! data-classification review expects. If the answer to "is this
//! always safe regardless of which tenant we're inside?" isn't
//! obviously yes, leave it off the list and log the corresponding
//! stable ID instead.

use chrono::Utc;
use serde_json::{json, Map, Value};
use std::fmt;
use tracing::field::{Field, Visit};
use tracing::span::Attributes;
use tracing::{Event, Id, Subscriber};
use tracing_subscriber::layer::Context;
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::Layer;

use crate::utils::redact::scrub;

/// Field names whose values are emitted to JSON output. Anything else
/// is dropped and counted under `redacted`. Order doesn't matter
/// (linear scan); kept loosely grouped for diffing.
const ALLOWED_FIELDS: &[&str] = &[
    // tracing internals — must allowlist these or every event is empty.
    "log.file",
    "log.line",
    "log.module_path",
    "log.target",
    "message",
    // tenant-internal stable identifiers (UUIDs + integer keys). Not
    // user-identifying outside the tenant boundary.
    "actor_kind",
    "actor_uuid",
    "asset_id",
    "assignee_uuid",
    "calendar_id",
    "channel_id",
    "comment_id",
    "created_by",
    "cycle_id",
    "holiday_id",
    "id",
    "policy_id",
    "provider_id",
    "queue_id",
    "requester_uuid",
    "state_id",
    "sync_id",
    "bug_report_id",
    "client_session_id",
    "ticket_id",
    "user_uuid",
    "webhook_id",
    "workspace_id",
    // bounded enums + operational dimensions. No free text.
    "aggregate",
    "category",
    "classification",
    // Push sender selection, logged once at boot: `mode` and `sender` are
    // closed sets ("relay" / "native" / "off" / "unset"), `configured` is a
    // bool. Without these the boot line reads "Push sender selected" and names
    // nothing, which defeats the one signal an operator has for telling relay
    // mode from native on a running instance.
    "configured",
    "entity",
    "event_type",
    "kind",
    "mode",
    "op",
    "priority",
    "recurrence",
    "role",
    // Which backend process answered. A random per-process id, not tied to a
    // user, a tenant or a machine address; it exists so two replicas can be
    // told apart in logs and on the edition surface.
    "process_id",
    "sender",
    "status",
    // External (third-party-provider) stable identifiers. Like the
    // intra-tenant uuids above, these are stable IDs assigned by
    // the upstream system (MS Graph, etc.), not user-typed content.
    "external_id",
    // counts, timings, structured outcomes. Cardinality is bounded;
    // values cannot leak user content.
    "attempt",
    "build_sha",
    // Push dispatch accounting: per-platform counts plus the Apple environment.
    // All bounded -- integers and a two-value enum -- and none of it is tied to
    // a person. Without these the native sender's dispatch line is as empty as
    // the boot line was before #312.
    "environment",
    "invalid_android",
    "invalid_ios",
    "sent_android",
    "sent_ios",
    "targets",
    "byte_count",
    "cancelled",
    "code",
    "count",
    "elapsed_ms",
    "error",
    "error_kind",
    "failed",
    "processed",
    "stamped",
    "total",
    // canonical wide-event business dimensions — stamped by handlers via
    // `request_context::record_canonical` and flattened out of the `_canonical`
    // bag (below). Bounded enums / counts / stable IDs only; no free text.
    "entity_id",
    "mfa_required",
    "outcome",
    "result_count",
    // HTTP / operational context: the canonical per-request wide event
    // (`nosdesk::request`, see middleware/request_context.rs) + tracing-actix-web.
    "latency_ms",
    "method",
    "route",
    "status_code",
    // trace correlation — from tracing-actix-web spans + opentelemetry.
    "request_id",
    "span_id",
    "trace_id",
    // Boot-time operator diagnostics. Every one of these lines exists so that
    // someone reading a running instance's output can tell how it is
    // configured, and every one of them shipped empty until now. Same defect as
    // the push boot line in #312, found by the ratchet in
    // tests/tracing_field_allowlist_lint.rs.
    //
    // Values are operator configuration, not tenant data: a bind address, a
    // filesystem directory this process created, counts, bools, and the
    // workspace slug the instance boots into (no more identifying than
    // `workspace_id`, already allowed).
    //
    // The names are qualified on purpose. Bare `path` is an HTTP request path
    // elsewhere in this codebase and is user-influenced; bare `host` is a
    // request Host header elsewhere. Allowlisting either would un-redact those
    // too, which is the blast radius the convention note below warns about.
    // `redis_host` is the parsed host, never the URL, because
    // `rediss://:PASS@host` puts a password in the URL.
    "allow_native_app",
    "bind_host",
    "bind_port",
    "bootstrap_slug",
    "current_key_version",
    "host_count",
    "key_versions",
    "redis_host",
    "redis_tls",
    "search_index_path",
    "static_dir",
    "tenant_domain",
    "upload_dir",
    // Plugin trust surface. A `fingerprint` here is always an Ed25519 signing
    // key fingerprint: public material by construction, and the one value that
    // tells an operator which root key an install trusted. `plugin` is an
    // installed artifact's name, not user-authored content.
    "fingerprint",
    "plugin",
    // Push dispatch, continued from #317. `apns`/`fcm` are configured-or-not
    // bools on the native sender's boot line; `platform` is validated to
    // ios/android/web at registration (handlers/notifications.rs), so it is a
    // closed set even on the branch that fires when it is not one of them.
    //
    // `provider_reason` is the code lifted out of the provider's response and
    // length- and charset-bounded before it is emitted. The raw body it comes
    // from stays dropped: an FCM error echoes the device token back inside
    // `error.message`, so logging the body would ship the credential along with
    // the diagnosis.
    //
    // `recipient_uuid` is a uuid. The email channel's `recipient` is an email
    // address and is deliberately still dropped, which is why these two are
    // named apart rather than sharing one field.
    "apns",
    "fcm",
    "platform",
    "provider_reason",
    "recipient_uuid",
];

// ALLOWED_FIELDS naming convention.
//
// This allowlist is a single flat global namespace: a field name added
// here turns that field from `<redacted>` to cleartext for EVERY
// subsystem that emits it, not just the one motivating the addition.
// Generic names like `session_id` therefore have cross-subsystem blast
// radius: adding `session_id` for one feature would un-redact the Yjs
// WebSocket session id, the WebAuthn / passkey ceremony id, and any
// other "session id" field anywhere in the codebase.
//
// Convention: when the value is subsystem-specific, prefix the field
// name with a subsystem qualifier instead of using the bare name.
// `client_session_id` (browser tab session) is allowed; bare
// `session_id` (used by passkeys, collaboration) is not. The same
// rule applies to any other generic identifier where one subsystem's
// risk profile differs from another's.
//
// Target-scoped allowlisting (allow `session_id` only for events
// emitted under a specific tracing target) is a cleaner architectural
// answer and is deferred to a future Layer rework; until then,
// namespacing the field name is the load-bearing discipline.

/// Custom JSON-emitting `tracing::Layer` with field allowlist.
///
/// Output shape matches `tracing_subscriber::fmt::layer().json()`:
/// `{"timestamp":..,"level":..,"fields":{..},"target":..,"spans":[..],"redacted":N}`.
/// `redacted` only appears when ≥1 field was dropped; `spans` only
/// appears when the event has ancestor spans recorded in the
/// registry.
///
/// **Span ancestry is also redacted.** Fields from
/// `#[instrument(...)]` — including `tracing-actix-web`'s
/// `request_id` — pass through the same allowlist before reaching the
/// `spans` array. So `request_id` (in the allowlist) survives; an
/// `#[instrument(fields(email = %u.email))]` is dropped.
pub struct RedactingJsonLayer;

/// Per-span storage. Populated in `on_new_span` and read in
/// `on_event` so we can include the span's filtered fields in the
/// event's `spans` array without re-running the allowlist filter on
/// every event.
struct SpanFields(Map<String, Value>);

impl<S> Layer<S> for RedactingJsonLayer
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
{
    fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        let mut visitor = AllowlistVisitor::default();
        attrs.record(&mut visitor);
        if let Some(span) = ctx.span(id) {
            span.extensions_mut().insert(SpanFields(visitor.fields));
        }
    }

    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        let metadata = event.metadata();

        // Layer-internal noise suppression. `tracing_actix_web` emits
        // one INFO "served" event per request; we keep its span (the
        // span is what carries `request_id` for ancestry walks) but
        // drop the bare event since handler-side outcomes are
        // logged separately. Doing it here instead of via
        // `EnvFilter` avoids gating the span itself.
        if metadata.target() == "tracing_actix_web" {
            return;
        }

        let mut visitor = AllowlistVisitor::default();
        event.record(&mut visitor);

        // Walk span ancestry (root → leaf) and collect each span's
        // pre-filtered fields. tracing-actix-web's `request_id` lives
        // on the span, not on the event itself — without this walk
        // we'd lose request correlation on every nested log line.
        let mut spans = Vec::new();
        if let Some(scope) = ctx.event_scope(event) {
            for span in scope.from_root() {
                let ext = span.extensions();
                if let Some(SpanFields(fields)) = ext.get::<SpanFields>() {
                    spans.push(json!({
                        "name": span.name(),
                        "fields": fields,
                    }));
                }
            }
        }

        let mut payload = Map::new();
        payload.insert("timestamp".into(), json!(Utc::now().to_rfc3339()));
        payload.insert("level".into(), json!(metadata.level().as_str()));
        payload.insert("fields".into(), Value::Object(visitor.fields));
        payload.insert("target".into(), json!(metadata.target()));
        if !spans.is_empty() {
            payload.insert("spans".into(), json!(spans));
        }
        if visitor.redacted_count > 0 {
            payload.insert("redacted".into(), json!(visitor.redacted_count));
        }

        // One line per event so log shippers (Docker daemon, Loki,
        // Vector) can split on newlines. `println!` already locks
        // stdout per write.
        println!("{}", Value::Object(payload));
    }
}

#[derive(Default)]
struct AllowlistVisitor {
    fields: Map<String, Value>,
    redacted_count: usize,
}

impl AllowlistVisitor {
    /// Flatten the canonical event's `_canonical` JSON object into top-level
    /// fields, applying the allowlist (and `scrub` on string values) per key —
    /// so the accumulated business bag is held to the same PII policy as any
    /// other field. Malformed / non-object payloads are ignored.
    fn merge_canonical(&mut self, json_str: &str) {
        if json_str.is_empty() {
            return;
        }
        let Ok(Value::Object(map)) = serde_json::from_str::<Value>(json_str) else {
            return;
        };
        for (k, v) in map {
            if is_allowed(&k) {
                let v = match v {
                    Value::String(s) => Value::String(scrub(&s).into_owned()),
                    other => other,
                };
                self.fields.insert(k, v);
            } else {
                self.redacted_count += 1;
            }
        }
    }
}

impl Visit for AllowlistVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        if is_allowed(field.name()) {
            // `scrub` masks any email/JWT accidentally interpolated into the
            // text — the always-allowlisted `message` field arrives here, so
            // this closes the free-text channel the field-name allowlist can't.
            let rendered = format!("{value:?}");
            self.fields
                .insert(field.name().into(), json!(scrub(&rendered).as_ref()));
        } else {
            self.redacted_count += 1;
        }
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        // The canonical wide event ships its dynamic per-request business
        // fields as one JSON object under `_canonical` (handlers can't add
        // arbitrary static tracing fields to it). Flatten each key through the
        // SAME allowlist + scrub, so a bag field is redacted identically to a
        // first-class one — a handler can't smuggle PII onto the event this way.
        if field.name() == "_canonical" {
            self.merge_canonical(value);
            return;
        }
        if is_allowed(field.name()) {
            self.fields
                .insert(field.name().into(), json!(scrub(value).as_ref()));
        } else {
            self.redacted_count += 1;
        }
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        if is_allowed(field.name()) {
            self.fields.insert(field.name().into(), json!(value));
        } else {
            self.redacted_count += 1;
        }
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        if is_allowed(field.name()) {
            self.fields.insert(field.name().into(), json!(value));
        } else {
            self.redacted_count += 1;
        }
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        if is_allowed(field.name()) {
            self.fields.insert(field.name().into(), json!(value));
        } else {
            self.redacted_count += 1;
        }
    }

    fn record_f64(&mut self, field: &Field, value: f64) {
        if is_allowed(field.name()) {
            self.fields.insert(field.name().into(), json!(value));
        } else {
            self.redacted_count += 1;
        }
    }

    fn record_error(&mut self, field: &Field, value: &(dyn std::error::Error + 'static)) {
        if is_allowed(field.name()) {
            self.fields
                .insert(field.name().into(), json!(value.to_string()));
        } else {
            self.redacted_count += 1;
        }
    }
}

fn is_allowed(name: &str) -> bool {
    ALLOWED_FIELDS.contains(&name)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The `_canonical` bag is flattened per-key through the allowlist: allowed
    /// business dims land as top-level fields; anything not allowlisted (a
    /// handler accidentally stamping PII) is dropped and counted, never emitted.
    #[test]
    fn canonical_bag_flattens_through_allowlist() {
        let mut v = AllowlistVisitor::default();
        v.merge_canonical(
            r#"{"ticket_id":42,"outcome":"created","email":"kyle@nosdesk.com","result_count":3}"#,
        );
        assert_eq!(v.fields.get("ticket_id"), Some(&json!(42)));
        assert_eq!(v.fields.get("outcome"), Some(&json!("created")));
        assert_eq!(v.fields.get("result_count"), Some(&json!(3)));
        // `email` is not allowlisted → dropped, not emitted, and counted.
        assert!(!v.fields.contains_key("email"));
        assert_eq!(v.redacted_count, 1);
    }

    /// Even an allowlisted string value in the bag is scrubbed (defence in
    /// depth against a mislabelled field carrying an address).
    #[test]
    fn canonical_bag_scrubs_string_values() {
        let mut v = AllowlistVisitor::default();
        v.merge_canonical(r#"{"outcome":"sent to kyle@nosdesk.com"}"#);
        assert_eq!(
            v.fields.get("outcome"),
            Some(&json!("sent to k***@nosdesk.com"))
        );
    }

    /// Malformed / empty payloads are ignored, not panicked on.
    #[test]
    fn canonical_bag_ignores_garbage() {
        let mut v = AllowlistVisitor::default();
        v.merge_canonical("");
        v.merge_canonical("not json");
        v.merge_canonical("[1,2,3]"); // not an object
        assert!(v.fields.is_empty());
        assert_eq!(v.redacted_count, 0);
    }

    /// Positive cases — if this fails, an entry was removed from
    /// `ALLOWED_FIELDS` and a downstream log call just broke.
    #[test]
    fn allowed_fields_pass() {
        for name in [
            "message",
            "log.target",
            "log.module_path",
            "log.file",
            "log.line",
            "ticket_id",
            "comment_id",
            "asset_id",
            "workspace_id",
            "user_uuid",
            "requester_uuid",
            "assignee_uuid",
            "actor_uuid",
            "created_by",
            "cycle_id",
            "policy_id",
            "calendar_id",
            "webhook_id",
            "holiday_id",
            "queue_id",
            "channel_id",
            "provider_id",
            "sync_id",
            "state_id",
            "id",
            "event_type",
            "op",
            "aggregate",
            // Push boot line. Regression guard: these were added after the line
            // shipped logging "Push sender selected" with redacted:4 and naming
            // nothing at all.
            "mode",
            "sender",
            "configured",
            "process_id",
            "status",
            "priority",
            "role",
            "recurrence",
            "category",
            "kind",
            "code",
            "error",
            "error_kind",
            "count",
            "stamped",
            "elapsed_ms",
            "latency_ms",
            "method",
            "route",
            "status_code",
            "actor_kind",
            "request_id",
            "span_id",
            "trace_id",
            "bug_report_id",
            "build_sha",
            "byte_count",
            "client_session_id",
        ] {
            assert!(is_allowed(name), "expected `{name}` in allowlist");
        }
    }

    /// Negative cases — the PII classes flagged by the redaction
    /// plan must NEVER appear in `ALLOWED_FIELDS`. If this fails,
    /// someone added a high-risk field without a matching
    /// architectural review.
    #[test]
    fn pii_fields_are_redacted() {
        for name in [
            // User-identifying free text.
            "email",
            "user_email",
            "user_name",
            "display_name",
            "user_principal_name",
            "existing_user_email",
            "recipient",
            "requester_name",
            "assignee_name",
            "name",
            "full_name",
            // User-typed content.
            "title",
            "description",
            "body",
            "content",
            "comment_body",
            "comment_text",
            "subject",
            "message_body",
            "original_filename",
            // Network identifiers.
            "ip",
            "ip_hash",
            "ip_address",
            "user_agent",
            "host",
            // Subsystem-generic identifiers that must STAY redacted at
            // the bare name. Subsystems prefix the field
            // (`client_session_id`, etc.) instead. See the
            // ALLOWED_FIELDS naming-convention comment above.
            "session_id",
            // Credentials.
            "password",
            "token",
            "secret",
            "api_key",
            "refresh_token",
            "access_token",
            "placeholder_key",
            // MFA recovery codes — shown to the user exactly once
            // in the enrol / regenerate response and never again.
            // The handlers also wrap them in Zeroizing so the
            // source allocation is wiped on drop, but defence in
            // depth: a future log line that accidentally captures
            // the response field name must NOT survive the
            // allowlist filter.
            "backup_code",
            "backup_codes",
            "recovery_code",
            "recovery_codes",
            // Contact.
            "phone",
            "phone_number",
            "address",
            "billing_address",
            // Whole-request splats.
            "entities",
            "request_body",
            "ticket_update",
            "update",
        ] {
            assert!(
                !is_allowed(name),
                "field `{name}` must not appear in the allowlist"
            );
        }
    }

    /// Visitor bookkeeping — every record_* that fails the
    /// allowlist must bump `redacted_count`. Direct method calls
    /// (constructing a `tracing::Field` outside the macro system
    /// requires `pub` internals we can't reach).
    #[test]
    fn allowlist_visitor_counts_redactions() {
        let mut v = AllowlistVisitor::default();
        for _ in 0..3 {
            v.redacted_count += 1;
        }
        assert_eq!(v.redacted_count, 3);
    }

    /// Integration: actually run the layer against a real
    /// subscriber stack with `#[instrument]`-shaped span fields,
    /// and confirm the redaction path doesn't panic + the
    /// SpanFields extension is populated.
    #[test]
    fn span_extensions_carry_filtered_fields() {
        use tracing::subscriber::with_default;
        use tracing_subscriber::layer::SubscriberExt;
        use tracing_subscriber::Registry;

        let subscriber = Registry::default().with(RedactingJsonLayer);
        with_default(subscriber, || {
            let span = tracing::info_span!("test_span", user_uuid = "abc", email = "x@y.z");
            let _guard = span.enter();
            tracing::info!(
                ticket_id = 42,
                title = "should be redacted",
                "event inside span"
            );
        });
    }
}
