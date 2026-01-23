use actix_web::{web, HttpResponse, Responder};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

// App state for tracking server start time
pub struct SystemState {
    pub start_time: Instant,
}

impl Default for SystemState {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemState {
    pub fn new() -> Self {
        Self {
            start_time: Instant::now(),
        }
    }
}

#[derive(Serialize)]
pub struct SystemInfoResponse {
    pub version: String,
    pub environment: String,
    pub uptime_seconds: u64,
    pub uptime_formatted: String,
}

#[derive(Serialize)]
pub struct UpdateCheckResponse {
    pub update_available: bool,
    pub current_version: String,
    pub latest_version: Option<String>,
    pub release_url: Option<String>,
}

#[derive(Deserialize)]
struct GitHubRelease {
    tag_name: String,
    html_url: String,
}

// Get current version from Cargo.toml
fn get_current_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

// Format uptime duration into human-readable string
fn format_uptime(duration: Duration) -> String {
    let total_seconds = duration.as_secs();
    let days = total_seconds / 86400;
    let hours = (total_seconds % 86400) / 3600;
    let minutes = (total_seconds % 3600) / 60;
    let seconds = total_seconds % 60;

    if days > 0 {
        format!("{days}d {hours}h {minutes}m")
    } else if hours > 0 {
        format!("{hours}h {minutes}m {seconds}s")
    } else if minutes > 0 {
        format!("{minutes}m {seconds}s")
    } else {
        format!("{seconds}s")
    }
}

// Compare version strings (simple semver comparison)
fn is_newer_version(current: &str, latest: &str) -> bool {
    let current_parts: Vec<u32> = current
        .trim_start_matches('v')
        .split('.')
        .filter_map(|s| s.parse().ok())
        .collect();

    let latest_parts: Vec<u32> = latest
        .trim_start_matches('v')
        .split('.')
        .filter_map(|s| s.parse().ok())
        .collect();

    for i in 0..3 {
        let current_part = current_parts.get(i).unwrap_or(&0);
        let latest_part = latest_parts.get(i).unwrap_or(&0);

        if latest_part > current_part {
            return true;
        } else if latest_part < current_part {
            return false;
        }
    }

    false
}

// Check GitHub for latest release with a short timeout
async fn check_for_updates() -> Option<(String, String)> {
    let client = reqwest::Client::builder()
        .user_agent("Nosdesk-Update-Checker")
        .timeout(Duration::from_secs(3)) // Short timeout to not block UI
        .build()
        .ok()?;

    let response = client
        .get("https://api.github.com/repos/kylephillipsau/nosdesk/releases/latest")
        .send()
        .await
        .ok()?;

    if !response.status().is_success() {
        return None;
    }

    let release: GitHubRelease = response.json().await.ok()?;
    Some((release.tag_name, release.html_url))
}

// GET /api/admin/system/info
pub async fn get_system_info(
    system_state: web::Data<SystemState>,
) -> impl Responder {
    let current_version = get_current_version();
    let uptime = system_state.start_time.elapsed();

    // Get environment
    let environment = std::env::var("RUST_ENV")
        .or_else(|_| std::env::var("APP_ENV"))
        .unwrap_or_else(|_| "development".to_string());

    let response = SystemInfoResponse {
        version: current_version,
        environment,
        uptime_seconds: uptime.as_secs(),
        uptime_formatted: format_uptime(uptime),
    };

    HttpResponse::Ok().json(response)
}

// GET /api/admin/system/updates
pub async fn check_system_updates() -> impl Responder {
    let current_version = get_current_version();

    let (update_available, latest_version, release_url) = match check_for_updates().await {
        Some((latest, url)) => {
            let is_update = is_newer_version(&current_version, &latest);
            (is_update, Some(latest), Some(url))
        }
        None => (false, None, None),
    };

    let response = UpdateCheckResponse {
        update_available,
        current_version,
        latest_version,
        release_url,
    };

    HttpResponse::Ok().json(response)
}
