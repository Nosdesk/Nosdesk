use actix_web::{web, HttpResponse, Responder};
use serde::{Deserialize, Serialize};
use tracing::{debug, error, info, warn};

use crate::utils::utf8_trunc::{
    byte_prefix_with_ellipsis, char_prefix, strip_line_breaks_for_log_field,
};

/// Enable capture of forwarded browser-console logs (`/api/debug/frontend-logs`).
/// Debug builds expose this by default. Release/production requires
/// `NOSDESK_ALLOW_FRONTEND_DEBUG_LOGS=1` — keep off unless you explicitly need
/// container log correlation for troubleshooting.
#[must_use]
#[inline]
pub fn frontend_logs_endpoint_enabled() -> bool {
    cfg!(debug_assertions)
        || matches!(
            std::env::var("NOSDESK_ALLOW_FRONTEND_DEBUG_LOGS").as_deref(),
            Ok("1" | "true" | "yes")
        )
}

/// Maximum log records accepted per request (cheap DoS fence).
pub const FRONTEND_LOGS_MAX_ENTRIES: usize = 100;

fn sanitize_logged_field(input: &str, max_chars: usize) -> String {
    let collapsed = strip_line_breaks_for_log_field(input);
    char_prefix(&collapsed, max_chars)
}

/// Log entry from frontend
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct LogEntry {
    pub level: String,
    pub message: String,
    pub data: Option<String>,
    pub timestamp: String,
    pub url: String,
    #[serde(rename = "userAgent")]
    pub user_agent: String,
}

/// Request body for frontend logs
#[derive(Debug, Deserialize)]
pub struct FrontendLogsRequest {
    pub logs: Vec<LogEntry>,
}

/// Response for frontend logs endpoint
#[derive(Debug, Serialize)]
pub struct FrontendLogsResponse {
    pub received: usize,
}

/// Receive frontend console logs and print them to backend stdout.
/// Intended for Docker dev (`compose logs backend`); gated in release unless
/// `NOSDESK_ALLOW_FRONTEND_DEBUG_LOGS=1`.
pub async fn receive_frontend_logs(body: web::Json<FrontendLogsRequest>) -> impl Responder {
    if !frontend_logs_endpoint_enabled() {
        return HttpResponse::NotFound().finish();
    }

    let logs = &body.logs;
    let received = logs.len().min(FRONTEND_LOGS_MAX_ENTRIES);

    for log in logs.iter().take(FRONTEND_LOGS_MAX_ENTRIES) {
        let timestamp = sanitize_logged_field(&log.timestamp, 64);
        let url = sanitize_logged_field(&log.url, 2048);
        let message = sanitize_logged_field(&log.message, 8192);

        let data_str = log.data.as_ref().and_then(|data| {
            if data.is_empty() || data == "undefined" {
                None
            } else {
                let d = strip_line_breaks_for_log_field(data);
                Some(if d.len() > 500 {
                    byte_prefix_with_ellipsis(&d, 500)
                } else {
                    d
                })
            }
        });

        let level_key = sanitize_logged_field(&log.level, 24).to_ascii_lowercase();

        match level_key.as_str() {
            "error" => {
                error!(
                    target: "frontend",
                    timestamp = %timestamp,
                    url = %url,
                    data = ?data_str,
                    "[FE] {}",
                    message
                );
            }
            "warn" | "warning" => {
                warn!(
                    target: "frontend",
                    timestamp = %timestamp,
                    url = %url,
                    data = ?data_str,
                    "[FE] {}",
                    message
                );
            }
            "info" => {
                info!(
                    target: "frontend",
                    timestamp = %timestamp,
                    url = %url,
                    data = ?data_str,
                    "[FE] {}",
                    message
                );
            }
            _ => {
                debug!(
                    target: "frontend",
                    timestamp = %timestamp,
                    url = %url,
                    data = ?data_str,
                    "[FE] {}",
                    message
                );
            }
        }
    }

    HttpResponse::Ok().json(FrontendLogsResponse { received })
}
