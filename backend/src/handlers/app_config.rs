//! Public, instance-level frontend bootstrap config.
//!
//! Served unauthenticated and fetched once at app startup so the SPA can pick
//! its routing topology before it builds the router. This is instance-global
//! (identical for every workspace); per-workspace config lives behind auth.

use actix_web::{HttpResponse, Responder};
use serde_json::json;

/// The oldest mobile app this server works with. Raise it when a server change
/// breaks an app release still in the stores.
const MIN_APP_VERSION: &str = "1.1.0";

/// GET /api/config — instance-level config the frontend needs before auth.
///
/// `workspace_routing` tells the SPA where the workspace lives in the URL:
/// `"path"` for the single-origin agent app (slug in the path, Model C) and
/// `"host"` for the subdomain / self-hosted model. Derived from the same
/// selection-resolution switch the backend auth gate reads, so the client's
/// routing and the server's workspace resolution never disagree.
///
/// `deployment_mode` (`"hosted"` | `"self_hosted"`) lets the SPA render
/// deployment-aware admin UI, e.g. hiding the platform SMTP relay panel on
/// hosted (it's Nosdesk-managed infra, not a tenant concern). Not sensitive:
/// hosted vs self-host is observable from the surface anyway.
///
/// `ticket_numbers` says tickets carry a per-workspace `number` that URLs
/// use. The mobile app can reach servers older than that, where a URL's
/// ticket number is its id.
///
/// `server_version` and `min_app_version` let the store app refuse a server
/// it can't work with, and tell an outdated app to update. The app is versioned
/// apart from the server and talks to servers of many versions.
pub async fn get_public_config() -> impl Responder {
    let workspace_routing = if crate::middleware::workspace_context::selection_resolution_enabled()
    {
        "path"
    } else {
        "host"
    };
    let deployment_mode = match crate::middleware::DeploymentMode::current() {
        crate::middleware::DeploymentMode::Hosted => "hosted",
        crate::middleware::DeploymentMode::SelfHosted => "self_hosted",
    };
    // Forwarding-based inbound email is available only when the instance has an
    // inbound domain configured (the hosted SES-receiving path); the admin UI
    // uses this to show or hide the forwarding channel type.
    let inbound_forwarding_enabled = std::env::var("NOSDESK_INBOUND_DOMAIN")
        .map(|s| !s.is_empty())
        .unwrap_or(false);
    // The control-plane dashboard base URL (hosted mode), where members are
    // added and managed (Workspaces -> Staff). Empty when unset; the SPA renders
    // the in-app "add member" hand-off link only when this is present.
    let control_plane_url = std::env::var("CONTROL_PLANE_URL").unwrap_or_default();
    // The signed-in person's own Nosdesk account settings (hosted), where staff
    // change their name, avatar, email addresses and sign-in.
    let account_url = crate::utils::nosdesk_account::account_settings_url().unwrap_or_default();
    HttpResponse::Ok().json(json!({
        "workspace_routing": workspace_routing,
        "deployment_mode": deployment_mode,
        "inbound_forwarding_enabled": inbound_forwarding_enabled,
        "control_plane_url": control_plane_url,
        "account_url": account_url,
        "ticket_numbers": true,
        "server_version": env!("CARGO_PKG_VERSION"),
        "min_app_version": MIN_APP_VERSION,
    }))
}

#[cfg(test)]
mod tests {
    use actix_web::{test, web, App};

    #[actix_web::test]
    async fn reports_the_server_version_and_the_oldest_app_it_accepts() {
        let app = test::init_service(
            App::new().route("/api/config", web::get().to(super::get_public_config)),
        )
        .await;
        let body: serde_json::Value = test::call_and_read_body_json(
            &app,
            test::TestRequest::get().uri("/api/config").to_request(),
        )
        .await;
        assert_eq!(body["server_version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(body["min_app_version"], "1.1.0");
    }
}
