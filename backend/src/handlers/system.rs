use actix_web::{web, HttpRequest, HttpResponse, Responder};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

/// Where the upgrade notes for Nosdesk 1.1 (PostgreSQL 18) live.
pub const UPGRADE_GUIDE_URL: &str =
    "https://nosdesk.com/docs/operations/installation#upgrading-postgresql";

/// The PostgreSQL major version Nosdesk 1.1 needs.
pub const NEXT_RELEASE_POSTGRES_MAJOR: u32 = 18;

// App state for tracking server start time
pub struct SystemState {
    pub start_time: Instant,
    /// The database server's major version, read once at startup.
    pub postgres_major: Option<u32>,
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
            postgres_major: None,
        }
    }

    pub fn with_postgres_major(postgres_major: Option<u32>) -> Self {
        Self {
            postgres_major,
            ..Self::new()
        }
    }
}

/// The database server's major version (`server_version_num` / 10000).
pub fn postgres_major(conn: &mut crate::db::DbConnection) -> Option<u32> {
    use diesel::RunQueryDsl;
    #[derive(diesel::QueryableByName)]
    struct Version {
        #[diesel(sql_type = diesel::sql_types::Integer)]
        v: i32,
    }
    diesel::sql_query("SELECT current_setting('server_version_num')::int AS v")
        .get_result::<Version>(conn)
        .ok()
        .and_then(|row| u32::try_from(row.v / 10_000).ok())
}

/// Whether the database must be upgraded before Nosdesk 1.1.
pub fn postgres_upgrade_needed(postgres_major: Option<u32>) -> bool {
    postgres_major.is_some_and(|major| major < NEXT_RELEASE_POSTGRES_MAJOR)
}

#[derive(Serialize)]
pub struct SystemInfoResponse {
    pub version: String,
    pub environment: String,
    pub uptime_seconds: u64,
    pub uptime_formatted: String,
    /// The database server's major version, when it could be read.
    pub postgres_major: Option<u32>,
    /// True when the database is older than Nosdesk 1.1 needs.
    pub postgres_upgrade_needed: bool,
    /// The upgrade notes to read first.
    pub upgrade_guide_url: &'static str,
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

// Release version for the System Information card. Release images bake the
// real tag (e.g. "0.1.0-rc.5") via the NOSDESK_VERSION build-arg; local/dev
// builds fall back to the crate version. CARGO_PKG_VERSION isn't bumped
// per-rc, so without the injected value the card would always read "0.1.0".
fn get_current_version() -> String {
    option_env!("NOSDESK_VERSION")
        .filter(|v| !v.is_empty())
        .unwrap_or(env!("CARGO_PKG_VERSION"))
        .to_string()
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

// True when `latest` is a strictly newer release than `current`, using real
// semver ordering so prereleases sort correctly (0.1.0-rc.5 < 0.1.0, and
// rc.4 < rc.5). The previous hand-rolled parse split on '.' and dropped the
// `-rc.N` segment, which could falsely flag an update. Parse failure on
// either side ⇒ no update (conservative).
fn is_newer_version(current: &str, latest: &str) -> bool {
    let parse = |s: &str| semver::Version::parse(s.trim().trim_start_matches('v')).ok();
    match (parse(current), parse(latest)) {
        (Some(c), Some(l)) => l > c,
        _ => false,
    }
}

// Check GitHub for latest release with a short timeout
async fn check_for_updates() -> Option<(String, String)> {
    let client = reqwest::Client::builder()
        .user_agent("Nosdesk-Update-Checker")
        .timeout(Duration::from_secs(3)) // Short timeout to not block UI
        .build()
        .ok()?;

    let response = client
        .get("https://api.github.com/repos/Nosdesk/Nosdesk/releases/latest")
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
    req: HttpRequest,
    system_state: web::Data<SystemState>,
) -> impl Responder {
    // Version / environment / uptime is operator info: useful for
    // fingerprinting against known CVEs, so gate it to admins. See
    // security-audit-2026-06.
    if let Err(resp) =
        crate::utils::rbac::require_workspace_role(&req, crate::models::WorkspaceRole::Admin)
    {
        return resp;
    }
    let current_version = get_current_version();
    let uptime = system_state.start_time.elapsed();

    // Read the same ENVIRONMENT var the rest of the app uses (compose, main.rs,
    // cookies, security headers). The old RUST_ENV/APP_ENV names were never set
    // anywhere, so this card always showed "development" — even in production.
    let environment = std::env::var("ENVIRONMENT").unwrap_or_else(|_| "development".to_string());

    let response = SystemInfoResponse {
        version: current_version,
        environment,
        uptime_seconds: uptime.as_secs(),
        uptime_formatted: format_uptime(uptime),
        postgres_major: system_state.postgres_major,
        postgres_upgrade_needed: postgres_upgrade_needed(system_state.postgres_major),
        upgrade_guide_url: UPGRADE_GUIDE_URL,
    };

    HttpResponse::Ok().json(response)
}

// GET /api/admin/system/updates
pub async fn check_system_updates(req: HttpRequest) -> impl Responder {
    // Admin-only: this triggers an outbound GitHub request and reveals
    // the running version. See security-audit-2026-06.
    if let Err(resp) =
        crate::utils::rbac::require_workspace_role(&req, crate::models::WorkspaceRole::Admin)
    {
        return resp;
    }
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

#[cfg(test)]
mod postgres_notice_tests {
    use super::*;

    #[test]
    fn only_a_server_older_than_18_needs_upgrading() {
        assert!(postgres_upgrade_needed(Some(17)));
        assert!(!postgres_upgrade_needed(Some(18)));
        assert!(!postgres_upgrade_needed(None));
    }
}
