//! `POST /api/bug-reports`. User-submitted bug reports from the
//! in-app "Report a problem" modal.
//!
//! Thin: parse and validate the body, normalise free-text and the
//! URL, persist via the repository under the request's workspace
//! actor, emit one tracing line on success. No deduplication, no
//! rate limit (deliberate user action, low volume), no scrubbing
//! beyond URL stripping and string normalisation (the user is
//! authenticated and the text fields are bounded).

use actix_web::{web, HttpMessage, HttpRequest, HttpResponse, Responder};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::extractors::TenantConn;
use crate::handlers::errors;
use crate::middleware::request_context::RequestContext;
use crate::models::NewBugReport;
use crate::repository::bug_reports as repo;

const MAX_DESCRIPTION_LEN: usize = 4000;
const MAX_URL_LEN: usize = 2048;
const MAX_USER_AGENT_LEN: usize = 512;
const MAX_BREADCRUMBS: usize = 10;
const MAX_BREADCRUMB_SUMMARY_LEN: usize = 256;

/// Request body. `#[serde(deny_unknown_fields)]` rejects rogue
/// payloads at the parse step instead of silently storing them.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateBugReportRequest {
    pub session_id: Uuid,
    pub build_sha: String,
    pub description: String,
    pub url: String,
    pub occurred_at: DateTime<Utc>,
    #[serde(default)]
    pub breadcrumbs: Vec<IncomingBreadcrumb>,
    #[serde(default)]
    pub user_agent: Option<String>,
    #[serde(default)]
    pub viewport: Option<IncomingViewport>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct IncomingBreadcrumb {
    /// "route" or "api". Validated post-parse.
    pub category: String,
    pub ts: i64,
    /// Free-form, capped to MAX_BREADCRUMB_SUMMARY_LEN at the
    /// handler. Captured client-side as a path string with no
    /// query/fragment.
    pub summary: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct IncomingViewport {
    pub w: i32,
    pub h: i32,
}

#[derive(Debug, Serialize)]
pub struct CreateBugReportResponse {
    pub id: i64,
}

/// Authenticated POST. The `TenantConn` extractor pulls the
/// workspace-pinned actor off `RequestContext`; if there is no
/// `RequestContext` the extractor returns 401 before the body is
/// even parsed.
pub async fn create_bug_report(
    req: HttpRequest,
    body: web::Json<CreateBugReportRequest>,
    mut tc: TenantConn,
) -> impl Responder {
    let body = body.into_inner();

    let description = match normalise_description(body.description) {
        Ok(d) => d,
        Err(msg) => return errors::bad_request(msg),
    };

    let url = match normalise_url(&body.url) {
        Ok(u) => u,
        Err(msg) => return errors::bad_request(msg),
    };

    if !is_valid_build_sha(&body.build_sha) {
        return errors::bad_request("Invalid build_sha");
    }

    let breadcrumbs_value = match build_breadcrumbs_value(body.breadcrumbs) {
        Ok(v) => v,
        Err(msg) => return errors::bad_request(msg),
    };

    let user_agent = body.user_agent.as_deref().map(normalise_user_agent);
    let viewport = body
        .viewport
        .map(|v| serde_json::json!({ "w": v.w.max(0), "h": v.h.max(0) }));

    let user_uuid = req
        .extensions()
        .get::<RequestContext>()
        .and_then(|c| c.actor.uuid);

    let new_report = NewBugReport {
        session_id: body.session_id,
        user_uuid,
        description,
        url,
        breadcrumbs: breadcrumbs_value,
        build_sha: body.build_sha,
        user_agent,
        viewport,
        occurred_at: body.occurred_at,
    };

    match tc.run(|conn| repo::insert(conn, new_report.clone())) {
        Ok(persisted) => {
            tracing::info!(
                target: "nosdesk::bug_reports",
                bug_report_id = persisted.id,
                client_session_id = %persisted.session_id,
                build_sha = %persisted.build_sha,
                byte_count = persisted.description.len() as i64,
                "[FE] bug report received"
            );
            HttpResponse::Created().json(CreateBugReportResponse { id: persisted.id })
        }
        Err(e) => errors::db_error(&e),
    }
}

fn normalise_description(raw: String) -> Result<String, &'static str> {
    let cleaned = strip_invisible(&raw).trim().to_string();
    if cleaned.is_empty() {
        return Err("Description is required");
    }
    if cleaned.chars().count() > MAX_DESCRIPTION_LEN {
        return Err("Description is too long");
    }
    Ok(cleaned)
}

fn normalise_url(raw: &str) -> Result<String, &'static str> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("url is required");
    }
    let is_safe_scheme = trimmed.starts_with('/')
        || trimmed.starts_with("http://")
        || trimmed.starts_with("https://");
    if !is_safe_scheme {
        return Err("Unsupported url scheme");
    }
    // Strip query + fragment.
    let mut stripped: &str = trimmed;
    if let Some(idx) = stripped.find('?') {
        stripped = &stripped[..idx];
    }
    if let Some(idx) = stripped.find('#') {
        stripped = &stripped[..idx];
    }
    let mut out = strip_invisible(stripped);
    if out.len() > MAX_URL_LEN {
        out.truncate(MAX_URL_LEN);
    }
    Ok(out)
}

fn is_valid_build_sha(s: &str) -> bool {
    if s.is_empty() || s.len() > 64 {
        return false;
    }
    s.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-')
}

fn normalise_user_agent(raw: &str) -> String {
    let mut out: String = strip_invisible(raw)
        .chars()
        .filter(|c| !matches!(*c, '<' | '>' | '&' | '"'))
        .collect();
    if out.chars().count() > MAX_USER_AGENT_LEN {
        out = out.chars().take(MAX_USER_AGENT_LEN).collect();
    }
    out
}

fn build_breadcrumbs_value(
    raw: Vec<IncomingBreadcrumb>,
) -> Result<serde_json::Value, &'static str> {
    if raw.len() > MAX_BREADCRUMBS {
        return Err("breadcrumbs exceeds maximum entries");
    }
    let mut out = Vec::with_capacity(raw.len());
    for crumb in raw {
        let category = match crumb.category.as_str() {
            "route" | "api" => crumb.category,
            _ => return Err("breadcrumb category must be route or api"),
        };
        let summary: String = strip_invisible(&crumb.summary)
            .chars()
            .take(MAX_BREADCRUMB_SUMMARY_LEN)
            .collect();
        out.push(serde_json::json!({
            "category": category,
            "ts": crumb.ts,
            "summary": summary,
        }));
    }
    Ok(serde_json::Value::Array(out))
}

/// Strip NUL bytes and replace lone UTF-16 surrogates so the value
/// survives Postgres jsonb / text storage. Also drops most C0
/// controls (keep `\n` and `\t`).
fn strip_invisible(s: &str) -> String {
    s.chars()
        .filter_map(|c| {
            let code = c as u32;
            if c == '\u{0000}' {
                None
            } else if (0xD800..=0xDFFF).contains(&code) {
                Some('\u{FFFD}')
            } else if (0x01..=0x08).contains(&code)
                || (0x0B..=0x0C).contains(&code)
                || (0x0E..=0x1F).contains(&code)
                || code == 0x7F
            {
                None
            } else {
                Some(c)
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_strips_query_and_fragment() {
        let out = normalise_url("/tickets/1?token=secret#access_token=xyz").unwrap();
        assert_eq!(out, "/tickets/1");
    }

    #[test]
    fn url_rejects_javascript_scheme() {
        assert!(normalise_url("javascript:alert(1)").is_err());
    }

    #[test]
    fn url_accepts_absolute_https() {
        assert_eq!(
            normalise_url("https://example.com/x").unwrap(),
            "https://example.com/x"
        );
    }

    #[test]
    fn description_requires_non_empty() {
        assert!(normalise_description("   ".into()).is_err());
        assert_eq!(normalise_description("  hi  ".into()).unwrap(), "hi");
    }

    #[test]
    fn build_sha_validator_rejects_funny_chars() {
        assert!(is_valid_build_sha("abc123"));
        assert!(is_valid_build_sha("dev-1234"));
        assert!(!is_valid_build_sha(""));
        assert!(!is_valid_build_sha("bad sha"));
        assert!(!is_valid_build_sha("../etc/passwd"));
    }

    #[test]
    fn user_agent_strips_tag_chars_and_caps_length() {
        let raw = "Mozilla/<script>alert(1)</script>".to_string();
        let cleaned = normalise_user_agent(&raw);
        assert!(!cleaned.contains('<'));
        assert!(!cleaned.contains('>'));
    }

    #[test]
    fn breadcrumb_category_must_be_known() {
        let raw = vec![IncomingBreadcrumb {
            category: "click".to_string(),
            ts: 0,
            summary: "x".to_string(),
        }];
        assert!(build_breadcrumbs_value(raw).is_err());
    }

    #[test]
    fn breadcrumbs_cap_enforced() {
        let raw: Vec<IncomingBreadcrumb> = (0..15)
            .map(|i| IncomingBreadcrumb {
                category: "route".to_string(),
                ts: i,
                summary: format!("/r/{i}"),
            })
            .collect();
        assert!(build_breadcrumbs_value(raw).is_err());
    }

    #[test]
    fn strip_invisible_drops_nul_and_control_chars() {
        // Rust `str` cannot legally contain a lone UTF-16 surrogate
        // (it would be invalid UTF-8) so the surrogate branch of
        // strip_invisible is defence-in-depth against `unsafe`
        // callers; only safely-constructable inputs are testable
        // here. NUL and a C0 control are the realistic cases.
        let s = format!("ok{}bell{}", '\u{0000}', '\u{0007}');
        let out = strip_invisible(&s);
        assert!(!out.contains('\u{0000}'));
        assert!(!out.contains('\u{0007}'));
        assert!(out.contains("ok"));
        assert!(out.contains("bell"));
    }
}
