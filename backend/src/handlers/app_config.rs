//! Public, instance-level frontend bootstrap config.
//!
//! Served unauthenticated and fetched once at app startup so the SPA can pick
//! its routing topology before it builds the router. This is instance-global
//! (identical for every workspace); per-workspace config lives behind auth.

use actix_web::{HttpResponse, Responder};
use serde_json::json;

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
    HttpResponse::Ok().json(json!({
        "workspace_routing": workspace_routing,
        "deployment_mode": deployment_mode,
        "inbound_forwarding_enabled": inbound_forwarding_enabled,
    }))
}
