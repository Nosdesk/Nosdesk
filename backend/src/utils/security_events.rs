//! Shared helpers for writing rows into `security_events`.
//!
//! Before this module existed the same `#[derive(Insertable)] struct
//! NewSecurityEvent { … }` was inlined in four different handlers (auth,
//! password_reset, invitation, guest). They all built essentially the same
//! payload with slight variations in how they sourced the IP and user-agent
//! from the request. This module consolidates the struct *and* the
//! IP/user-agent extraction so each call site shrinks to a single
//! `record_security_event(...)` call.
//!
//! Failures to insert are the caller's concern — we return the
//! `QueryResult<usize>` so the caller can `.ok()` or `.log()` as fits.

use actix_web::HttpRequest;
use chrono::Utc;
use diesel::prelude::*;
use diesel::QueryResult;
use ipnetwork::IpNetwork;
use uuid::Uuid;

use crate::db::DbConnection;

#[derive(diesel::Insertable)]
#[diesel(table_name = crate::schema::security_events)]
pub struct NewSecurityEvent {
    pub user_uuid: Option<Uuid>,
    pub event_type: String,
    pub ip_address: Option<IpNetwork>,
    pub user_agent: Option<String>,
    pub location: Option<String>,
    pub details: Option<serde_json::Value>,
    pub severity: String,
    pub created_at: chrono::NaiveDateTime,
    pub session_id: Option<i32>,
}

/// Input shape for [`record_security_event`]. Keeps the call site short
/// while still allowing every field that made sense before — the only
/// thing we derive automatically is `created_at` (always "now").
pub struct SecurityEventInput<'a> {
    /// `None` for events not tied to a known account — e.g. a failed
    /// login against an email that doesn't resolve to a user (C/W2).
    /// Put the attempted identifier in `details` for those rows.
    pub user_uuid: Option<Uuid>,
    pub event_type: &'a str,
    /// Conventional values: `"info"`, `"warning"`, `"error"`.
    pub severity: &'a str,
    pub details: Option<serde_json::Value>,
    /// When supplied, IP and User-Agent are pulled from headers automatically.
    /// Handlers that don't have the request in scope can pass `None`.
    pub request: Option<&'a HttpRequest>,
    pub session_id: Option<i32>,
}

/// Pull the client IP out of a request via the trusted-proxy-aware
/// helper, so security-event rows show the real client (not the
/// reverse proxy) when `TRUSTED_PROXIES` is configured, and the
/// peer address when it isn't.
fn request_ip(req: &HttpRequest) -> Option<IpNetwork> {
    crate::utils::client_ip::from_http_request(req).and_then(|ip| ip.to_string().parse().ok())
}

fn request_user_agent(req: &HttpRequest) -> Option<String> {
    req.headers()
        .get("user-agent")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.to_string())
}

/// Insert a row into `security_events`. Returns the number of inserted
/// rows so callers can decide how to react to a zero result, but in
/// practice every caller either ignores the result or warn-logs on error.
pub fn record_security_event(
    conn: &mut DbConnection,
    event: SecurityEventInput<'_>,
) -> QueryResult<usize> {
    use crate::schema::security_events;

    let (ip_address, user_agent) = match event.request {
        Some(req) => (request_ip(req), request_user_agent(req)),
        None => (None, None),
    };

    let row = NewSecurityEvent {
        user_uuid: event.user_uuid,
        event_type: event.event_type.to_string(),
        ip_address,
        user_agent,
        location: None,
        details: event.details,
        severity: event.severity.to_string(),
        created_at: Utc::now().naive_utc(),
        session_id: event.session_id,
    };

    diesel::insert_into(security_events::table)
        .values(&row)
        .execute(conn)
}

/// Delete security_events rows older than `older_than_days`. Operational
/// observability prefers a long window (logins / MFA toggles a year ago
/// are still useful for compliance) but unbounded growth eventually
/// hurts indexes; the scheduler calls this daily with the configured
/// SECURITY_EVENT_RETENTION_DAYS retention window.
pub fn prune_older_than(
    conn: &mut diesel::pg::PgConnection,
    older_than_days: i32,
) -> diesel::QueryResult<usize> {
    use crate::schema::security_events::dsl::*;
    use diesel::dsl::sql;
    use diesel::sql_types::Timestamptz;

    let cutoff = sql::<Timestamptz>(&format!("NOW() - INTERVAL '{older_than_days} days'"));
    diesel::delete(security_events.filter(created_at.lt(cutoff))).execute(conn)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::{setup_test_connection, TestFixtures};

    #[test]
    fn record_security_event_inserts_row_without_request() {
        use crate::schema::security_events::dsl as se;

        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "Audit", "user");

        let inserted = record_security_event(
            &mut conn,
            SecurityEventInput {
                user_uuid: Some(user.uuid),
                event_type: "test_event",
                severity: "info",
                details: Some(serde_json::json!({ "k": "v" })),
                request: None,
                session_id: None,
            },
        )
        .expect("insert succeeds");
        assert_eq!(inserted, 1);

        let rows: Vec<(String, Option<String>, Option<IpNetwork>)> = se::security_events
            .filter(se::user_uuid.eq(user.uuid))
            .select((se::event_type, se::user_agent, se::ip_address))
            .load(&mut conn)
            .expect("load");

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0, "test_event");
        // Without a request in scope, IP and UA must be null.
        assert!(rows[0].1.is_none());
        assert!(rows[0].2.is_none());
    }

    #[test]
    fn records_anonymous_event_with_null_user_uuid() {
        // W2: a failed login against an unknown email has no
        // user_uuid. The nullable column + None input must round-trip,
        // carrying the attempted identifier in `details`.
        use crate::schema::security_events::dsl as se;

        let mut conn = setup_test_connection();

        let inserted = record_security_event(
            &mut conn,
            SecurityEventInput {
                user_uuid: None,
                event_type: "login_failed",
                severity: "info",
                details: Some(serde_json::json!({
                    "attempted_email": "nobody@example.com",
                    "reason": "invalid_credentials",
                })),
                request: None,
                session_id: None,
            },
        )
        .expect("anonymous insert succeeds");
        assert_eq!(inserted, 1);

        let rows: Vec<(Option<Uuid>, String)> = se::security_events
            .filter(se::event_type.eq("login_failed"))
            .select((se::user_uuid, se::event_type))
            .load(&mut conn)
            .expect("load");
        assert_eq!(rows.len(), 1);
        assert!(rows[0].0.is_none(), "anonymous event has NULL user_uuid");
    }
}
