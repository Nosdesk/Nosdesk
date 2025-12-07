use actix_web::{web, HttpResponse, Responder};
use serde::{Deserialize, Serialize};

/// Log entry from frontend
#[derive(Debug, Deserialize)]
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
        // Color-code based on log level
        let level_prefix = match log.level.as_str() {
            "error" => "\x1b[31m[FE:ERROR]\x1b[0m",  // Red
            "warn" => "\x1b[33m[FE:WARN]\x1b[0m",    // Yellow
            "info" => "\x1b[32m[FE:INFO]\x1b[0m",    // Green
            "debug" => "\x1b[36m[FE:DEBUG]\x1b[0m",  // Cyan
            _ => "[FE:LOG]",
        };

        // Print the main message
        println!("{} {} - {}", level_prefix, log.timestamp, log.message);

        // Print additional data if present
        if let Some(data) = &log.data {
            if !data.is_empty() && data != "undefined" {
                // Truncate very long data
                let truncated = if data.len() > 500 {
                    format!("{}... [truncated]", &data[..500])
                } else {
                    data.clone()
                };
                println!("    └─ Data: {}", truncated);
            }
        }
    }

    HttpResponse::Ok().json(FrontendLogsResponse {
        received: logs.len(),
    })
}
