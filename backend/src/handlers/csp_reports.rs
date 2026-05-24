//! Handlers for CSP violation reports.
//!
//! Two endpoints:
//!
//!   POST /api/csp-report (public, unauthenticated) — receives
//!     reports submitted by the browser via the `report-uri`
//!     directive. Browsers do not send credentials with these,
//!     so the handler runs without auth.
//!
//!   GET /api/admin/csp-reports (admin only) — lists the most
//!     recent aggregated violations for triage in the admin UI.
//!
//! Two report shapes are accepted on the public endpoint:
//!
//!   1. Legacy `application/csp-report` (Firefox, older Chrome):
//!        { "csp-report": { "document-uri": "...", ... } }
//!
//!   2. Modern `application/reports+json` (Reporting API):
//!        [ { "type": "csp-violation", "body": { ... }, ... } ]
//!
//! Field names also differ between shapes (`blocked-uri` vs
//! `blockedURL` etc). Normalisation collapses both to a single
//! internal `ParsedReport` shape before hashing and persisting.

use crate::extractors::{PlatformConn, TenantConn};
#[allow(unused_imports)]
use crate::handlers; // keep helpers reachable for tests
use crate::handlers::errors;
use crate::models::{Claims, NewCspReport};
use crate::repository::csp_reports as repo;
use crate::utils::rbac;
use actix_web::{web, HttpMessage, HttpRequest, HttpResponse, Responder};
use serde::Deserialize;
use serde_json::Value;
use tracing::warn;

const MAX_FIELD_LEN: usize = 4096;
const MAX_DIRECTIVE_LEN: usize = 64;
const MAX_DISPOSITION_LEN: usize = 16;

/// Internal canonical report shape. Both legacy and modern report
/// payloads are normalised into this before hashing / persisting.
#[derive(Debug, Default)]
struct ParsedReport {
    document_uri: String,
    referrer: Option<String>,
    blocked_uri: Option<String>,
    effective_directive: String,
    violated_directive: Option<String>,
    original_policy: Option<String>,
    disposition: String,
    source_file: Option<String>,
    line_number: Option<i32>,
    column_number: Option<i32>,
}

#[derive(Debug, Deserialize)]
struct LegacyEnvelope {
    #[serde(rename = "csp-report")]
    csp_report: Value,
}

/// Parse a single legacy-shape report body. Lenient: missing
/// fields default to empty strings rather than rejecting the
/// whole report — partial information is still useful and
/// browsers don't always populate every field.
fn parse_legacy(body: &Value) -> Option<ParsedReport> {
    let obj = body.as_object()?;
    let s = |key: &str| -> Option<String> {
        obj.get(key)
            .and_then(|v| v.as_str())
            .map(|s| truncate(s, MAX_FIELD_LEN))
    };
    let i = |key: &str| -> Option<i32> {
        obj.get(key)
            .and_then(|v| v.as_i64())
            .and_then(|n| i32::try_from(n).ok())
    };

    let document_uri = s("document-uri").unwrap_or_default();
    let effective_directive = s("effective-directive")
        .or_else(|| s("violated-directive"))
        .unwrap_or_default();
    let disposition = s("disposition").unwrap_or_else(|| "enforce".to_string());

    Some(ParsedReport {
        document_uri,
        referrer: s("referrer").filter(|v| !v.is_empty()),
        blocked_uri: s("blocked-uri").filter(|v| !v.is_empty()),
        effective_directive: truncate(&effective_directive, MAX_DIRECTIVE_LEN),
        violated_directive: s("violated-directive")
            .filter(|v| !v.is_empty())
            .map(|v| truncate(&v, MAX_DIRECTIVE_LEN)),
        original_policy: s("original-policy"),
        disposition: truncate(&disposition, MAX_DISPOSITION_LEN),
        source_file: s("source-file").filter(|v| !v.is_empty()),
        line_number: i("line-number"),
        column_number: i("column-number"),
    })
}

/// Parse a single modern-shape (Reporting API) report body. The
/// outer envelope is an array of `{ type, body, age, url, ... }`;
/// the body field names are camelCase rather than dash-case.
fn parse_modern(body: &Value) -> Option<ParsedReport> {
    let obj = body.as_object()?;
    let s = |key: &str| -> Option<String> {
        obj.get(key)
            .and_then(|v| v.as_str())
            .map(|s| truncate(s, MAX_FIELD_LEN))
    };
    let i = |key: &str| -> Option<i32> {
        obj.get(key)
            .and_then(|v| v.as_i64())
            .and_then(|n| i32::try_from(n).ok())
    };

    let document_uri = s("documentURL").unwrap_or_default();
    let effective_directive = s("effectiveDirective")
        .or_else(|| s("violatedDirective"))
        .unwrap_or_default();
    let disposition = s("disposition").unwrap_or_else(|| "enforce".to_string());

    Some(ParsedReport {
        document_uri,
        referrer: s("referrer").filter(|v| !v.is_empty()),
        blocked_uri: s("blockedURL").filter(|v| !v.is_empty()),
        effective_directive: truncate(&effective_directive, MAX_DIRECTIVE_LEN),
        violated_directive: s("violatedDirective")
            .filter(|v| !v.is_empty())
            .map(|v| truncate(&v, MAX_DIRECTIVE_LEN)),
        original_policy: s("originalPolicy"),
        disposition: truncate(&disposition, MAX_DISPOSITION_LEN),
        source_file: s("sourceFile").filter(|v| !v.is_empty()),
        line_number: i("lineNumber"),
        column_number: i("columnNumber"),
    })
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        // Truncate on a char boundary to avoid producing invalid
        // UTF-8 when the cut falls inside a multi-byte sequence.
        let mut end = max;
        while !s.is_char_boundary(end) && end > 0 {
            end -= 1;
        }
        s[..end].to_string()
    }
}

/// Public report intake. Accepts both legacy and modern shapes,
/// always returns 204 No Content. Never leaks anything about
/// internal state to the browser — the only signal a misconfigured
/// reporter gets is the HTTP status, which is always success.
pub async fn report_violation(
    req: HttpRequest,
    body: web::Bytes,
    pc: PlatformConn,
) -> impl Responder {
    let content_type = req
        .headers()
        .get(actix_web::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    // User agent + authenticated user (if any) for context.
    // Reports usually arrive without credentials but the cookie
    // auth middleware may still have populated Claims when the
    // browser does include them.
    let user_agent = req
        .headers()
        .get(actix_web::http::header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .map(|s| truncate(s, MAX_FIELD_LEN));

    let user_uuid = req
        .extensions()
        .get::<Claims>()
        .and_then(|c| uuid::Uuid::parse_str(&c.sub).ok());

    // Parse JSON. Browsers send JSON regardless of content-type
    // declared; the type tells us which schema to expect.
    let json: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => {
            // Don't 4xx — browsers retry on errors and we don't
            // want to flood our logs. Just drop and 204.
            warn!(error = %e, "Failed to parse CSP report body");
            return HttpResponse::NoContent().finish();
        }
    };

    let mut parsed_reports: Vec<ParsedReport> = Vec::new();

    if content_type.contains("application/reports+json") || json.is_array() {
        // Modern Reporting API: array of {type, body, ...}
        if let Some(arr) = json.as_array() {
            for entry in arr {
                let entry_type = entry.get("type").and_then(|v| v.as_str()).unwrap_or("");
                if entry_type != "csp-violation" {
                    continue;
                }
                if let Some(body_val) = entry.get("body") {
                    if let Some(report) = parse_modern(body_val) {
                        parsed_reports.push(report);
                    }
                }
            }
        }
    } else {
        // Legacy CSP report: { "csp-report": {...} }
        if let Ok(envelope) = serde_json::from_value::<LegacyEnvelope>(json.clone()) {
            if let Some(report) = parse_legacy(&envelope.csp_report) {
                parsed_reports.push(report);
            }
        } else if let Some(report) = parse_legacy(&json) {
            // Some browsers post the body directly without the
            // envelope. Tolerate.
            parsed_reports.push(report);
        }
    }

    if parsed_reports.is_empty() {
        return HttpResponse::NoContent().finish();
    }

    // Persist via PlatformConn — the endpoint is public
    // (browsers don't send cookies on CSP reports by default), so
    // TenantConn isn't reachable. PlatformConn elevates to the
    // nosdesk_admin BYPASSRLS role for the txn. csp_reports is
    // RLS-enabled, so a tenant-scoped write would fail without
    // bypass. The report shape (per-document_uri + dedup_hash) is
    // intrinsically per-workspace once 3f.2 lands, but for now
    // the table NOT-NULLs workspace_id with a default that reads
    // from the actor's GUC, so we override the inherited
    // "platform:fallback:<route>" actor with a stable
    // "handler:csp_report" actor pinned to workspace_id=1.
    // TODO Phase 3f.2: read workspace from
    // WorkspaceContextMiddleware (which DOES run for the apex
    // path in hosted mode and attaches a WorkspaceContext) so
    // CSP reports get scoped to the originating workspace.
    let mut pc = pc.with_actor(
        crate::sync::actor::ActorContext::system("handler:csp_report").with_workspace(1),
    );
    let _ = pc.run(|conn| {
        for r in parsed_reports {
            if r.effective_directive.is_empty() {
                continue;
            }

            let dedup = repo::dedup_hash(
                &r.effective_directive,
                r.blocked_uri.as_deref(),
                r.source_file.as_deref(),
                r.line_number,
            );

            let new_report = NewCspReport {
                dedup_hash: dedup,
                effective_directive: r.effective_directive,
                blocked_uri: r.blocked_uri,
                source_file: r.source_file,
                line_number: r.line_number,
                column_number: r.column_number,
                document_uri: r.document_uri,
                referrer: r.referrer,
                violated_directive: r.violated_directive,
                original_policy: r.original_policy,
                disposition: r.disposition,
                user_agent: user_agent.clone(),
                user_uuid,
            };

            if let Err(e) = repo::upsert(conn, new_report) {
                warn!(error = ?e, "Failed to upsert CSP report");
            }
        }
        Ok::<_, diesel::result::Error>(())
    });

    HttpResponse::NoContent().finish()
}

/// Admin: list recent aggregated violations.
pub async fn list_violations(req: HttpRequest, mut tc: TenantConn) -> impl Responder {
    if let Err(resp) = rbac::require_admin(&req) {
        return resp;
    }

    match tc.run(|conn| repo::list_recent(conn, 200)) {
        Ok(rows) => HttpResponse::Ok().json(rows),
        Err(e) => {
            warn!(error = ?e, "Failed to list CSP reports");
            errors::internal("Failed to list CSP reports")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn truncates_on_char_boundary() {
        // A 3-byte char (em dash) at the cut boundary should not
        // produce invalid UTF-8.
        let input = "abc—def";
        // Cut at byte 4 (mid em-dash); should back up to byte 3.
        let out = truncate(input, 4);
        assert!(out.is_char_boundary(out.len()));
        assert!(out.starts_with("abc"));
    }

    #[test]
    fn parses_legacy_csp_report() {
        let body = json!({
            "document-uri": "https://example.com/page",
            "referrer": "https://example.com/",
            "blocked-uri": "https://evil.example.com/x.js",
            "effective-directive": "script-src",
            "violated-directive": "script-src 'self'",
            "original-policy": "default-src 'self'; script-src 'self'",
            "disposition": "enforce",
            "source-file": "https://example.com/app.js",
            "line-number": 42,
            "column-number": 7
        });
        let r = parse_legacy(&body).expect("should parse");
        assert_eq!(r.document_uri, "https://example.com/page");
        assert_eq!(r.effective_directive, "script-src");
        assert_eq!(
            r.blocked_uri.as_deref(),
            Some("https://evil.example.com/x.js")
        );
        assert_eq!(r.line_number, Some(42));
        assert_eq!(r.disposition, "enforce");
    }

    #[test]
    fn parses_modern_reports_api() {
        let body = json!({
            "documentURL": "https://example.com/page",
            "blockedURL": "https://evil.example.com/x.js",
            "effectiveDirective": "script-src-elem",
            "violatedDirective": "script-src-elem",
            "originalPolicy": "default-src 'self'",
            "disposition": "report",
            "sourceFile": "https://example.com/app.js",
            "lineNumber": 99,
            "columnNumber": 3
        });
        let r = parse_modern(&body).expect("should parse");
        assert_eq!(r.document_uri, "https://example.com/page");
        assert_eq!(r.effective_directive, "script-src-elem");
        assert_eq!(
            r.blocked_uri.as_deref(),
            Some("https://evil.example.com/x.js")
        );
        assert_eq!(r.line_number, Some(99));
        assert_eq!(r.disposition, "report");
    }

    #[test]
    fn legacy_falls_back_to_violated_directive_when_effective_missing() {
        let body = json!({
            "document-uri": "https://example.com/",
            "violated-directive": "script-src 'self'",
            "blocked-uri": "https://evil.example.com/x.js"
        });
        let r = parse_legacy(&body).expect("should parse");
        // Older Firefox didn't send `effective-directive`; we
        // should fall back to `violated-directive` so the report
        // isn't silently dropped.
        assert_eq!(r.effective_directive, "script-src 'self'");
    }

    #[test]
    fn empty_blocked_uri_normalizes_to_none() {
        let body = json!({
            "document-uri": "https://example.com/",
            "effective-directive": "script-src",
            "blocked-uri": ""
        });
        let r = parse_legacy(&body).expect("should parse");
        // Empty string is treated as None so the dedup hash
        // collapses with reports that omit the field entirely.
        assert!(r.blocked_uri.is_none());
    }
}
