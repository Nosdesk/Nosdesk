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
pub async fn get_public_config() -> impl Responder {
    let workspace_routing = if crate::middleware::workspace_context::selection_resolution_enabled()
    {
        "path"
    } else {
        "host"
    };
    HttpResponse::Ok().json(json!({ "workspace_routing": workspace_routing }))
}
