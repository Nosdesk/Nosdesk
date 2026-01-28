use actix_web::{web, HttpResponse, Responder};
use serde::{Deserialize, Serialize};
use tracing::{debug, error, info, warn};

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

/// Receive frontend console logs and print them to backend stdout
/// This allows viewing frontend logs via `docker compose logs backend`
pub async fn receive_frontend_logs(
    body: web::Json<FrontendLogsRequest>,
) -> impl Responder {
    let logs = &body.logs;

    for log in logs {
        // Truncate additional data if present
        let data_str = log.data.as_ref().and_then(|data| {
            if data.is_empty() || data == "undefined" {
                None
            } else if data.len() > 500 {
                Some(format!("{}... [truncated]", &data[..500]))
            } else {
                Some(data.clone())
            }
        });

        // Log using appropriate tracing level based on frontend log level
        match log.level.as_str() {
            "error" => {
                error!(
                    target: "frontend",
                    timestamp = %log.timestamp,
                    url = %log.url,
                    data = ?data_str,
                    "[FE] {}", log.message
                );
            }
            "warn" => {
                warn!(
                    target: "frontend",
                    timestamp = %log.timestamp,
                    url = %log.url,
                    data = ?data_str,
                    "[FE] {}", log.message
                );
            }
            "info" => {
                info!(
                    target: "frontend",
                    timestamp = %log.timestamp,
                    url = %log.url,
                    data = ?data_str,
                    "[FE] {}", log.message
                );
            }
            "debug" | _ => {
                debug!(
                    target: "frontend",
                    timestamp = %log.timestamp,
                    url = %log.url,
                    data = ?data_str,
                    "[FE] {}", log.message
                );
            }
        }
    }

    HttpResponse::Ok().json(FrontendLogsResponse {
        received: logs.len(),
    })
}
