//! Plugin sandbox origin (build-order step 1).
//!
//! Serves the sandboxed-iframe runtime + token-authorized bundle bytes, plus the
//! authenticated endpoint that mints a bundle token. Isolation is the opaque
//! iframe (`sandbox="allow-scripts"`), so these routes work served from the app's
//! own origin (`/__plugin-sandbox/*`, the self-host/dev default) OR from a
//! separate `NOSDESK_SANDBOX_ORIGIN` (defense in depth). See
//! `docs/plans/plugin-sandbox-step1.md`.

use actix_web::{web, HttpMessage, HttpRequest, HttpResponse, Responder};
use serde::Deserialize;
use tracing::{error, warn};
use uuid::Uuid;

use crate::db::Pool;
use crate::extractors::TenantConn;
use crate::handlers::errors;
use crate::middleware::RequestContext;
use crate::models::Claims;
use crate::repository::plugins as plugin_repo;
use crate::services::plugins::bundle_token;
use crate::sync::session::{self, BackgroundRunError};

const RUNTIME_HTML: &str = include_str!("plugin_sandbox/runtime.html");
const RUNTIME_JS: &str = include_str!("plugin_sandbox/runtime.js");

/// Where the parent app lives (for CSP `frame-ancestors`).
fn app_origin() -> String {
    std::env::var("FRONTEND_URL")
        .unwrap_or_else(|_| "http://localhost:3000".to_string())
        .trim_end_matches('/')
        .to_string()
}

/// Where the runtime is served: the separate sandbox origin if configured, else
/// the app origin (same-origin default).
fn runtime_origin() -> String {
    std::env::var("NOSDESK_SANDBOX_ORIGIN")
        .ok()
        .map(|s| s.trim_end_matches('/').to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(app_origin)
}

/// GET /api/plugins/{uuid}/bundle-token — authenticated. Mints a short-lived,
/// workspace + plugin + bundle-hash scoped token the sandbox iframe uses to
/// fetch this plugin's bundle cross-origin (it can't send the session cookie).
pub async fn mint_bundle_token(
    req: HttpRequest,
    mut tc: TenantConn,
    path: web::Path<Uuid>,
) -> impl Responder {
    if req.extensions().get::<Claims>().is_none() {
        return errors::unauthorized("Authentication required");
    }
    let plugin_uuid = path.into_inner();
    let Some(workspace_id) = req
        .extensions()
        .get::<RequestContext>()
        .and_then(|c| c.actor.workspace_id)
    else {
        return errors::unauthorized("No active workspace");
    };

    let plugin = match tc.run(|conn| plugin_repo::get_plugin_by_uuid(conn, plugin_uuid)) {
        Ok(p) => p,
        Err(diesel::result::Error::NotFound) => return errors::not_found_msg("Plugin not found"),
        Err(e) => {
            error!(error = %e, %plugin_uuid, "failed to look up plugin for bundle token");
            return errors::internal("Failed to look up plugin");
        }
    };
    if !plugin.is_active() {
        return errors::forbidden("Plugin is not active");
    }
    let Some(hash) = plugin.bundle_hash.as_deref() else {
        return errors::not_found_msg("Plugin has no bundle");
    };

    match bundle_token::mint(workspace_id, plugin_uuid, hash) {
        Ok(token) => {
            let runtime_url = format!(
                "{}/__plugin-sandbox/runtime.html?t={}",
                runtime_origin(),
                token
            );
            HttpResponse::Ok().json(serde_json::json!({
                "token": token,
                "runtime_url": runtime_url,
                "expires_in": bundle_token::ttl_secs(),
            }))
        }
        Err(e) => {
            error!(error = %e, %plugin_uuid, "failed to mint bundle token");
            errors::internal("Failed to mint bundle token")
        }
    }
}

/// GET /__plugin-sandbox/runtime.html — the sandbox shell (no auth; the bundle
/// fetch it triggers is token-gated).
pub async fn serve_runtime_html() -> impl Responder {
    let csp = format!(
        "default-src 'none'; script-src {ro}; style-src 'unsafe-inline'; \
         connect-src 'none'; frame-ancestors {ao};",
        ro = runtime_origin(),
        ao = app_origin(),
    );
    HttpResponse::Ok()
        .content_type("text/html; charset=utf-8")
        .insert_header(("Content-Security-Policy", csp))
        .insert_header(("X-Content-Type-Options", "nosniff"))
        .insert_header(("Cross-Origin-Resource-Policy", "cross-origin"))
        .body(RUNTIME_HTML)
}

/// GET /__plugin-sandbox/runtime.js — the iframe-side runtime. Cross-origin
/// module fetch from the opaque document, so it needs CORS.
pub async fn serve_runtime_js() -> impl Responder {
    HttpResponse::Ok()
        .content_type("text/javascript; charset=utf-8")
        .insert_header(("Access-Control-Allow-Origin", "*"))
        .insert_header(("X-Content-Type-Options", "nosniff"))
        .insert_header(("Cross-Origin-Resource-Policy", "cross-origin"))
        .body(RUNTIME_JS)
}

#[derive(Deserialize)]
pub struct BundleQuery {
    t: String,
}

/// GET /__plugin-sandbox/bundle?t=<token> — token-authorized bundle bytes,
/// read under the token's workspace pin (RLS), with a bundle-hash match so a
/// stale token can't serve a rotated bundle.
pub async fn serve_bundle(pool: web::Data<Pool>, q: web::Query<BundleQuery>) -> impl Responder {
    let claims = match bundle_token::verify(&q.t) {
        Ok(c) => c,
        Err(e) => {
            warn!(error = %e, "rejected plugin bundle token");
            return errors::forbidden("Invalid or expired token");
        }
    };
    let plugin_uuid = claims.plugin_uuid;

    let plugin = match session::run_in_workspace(
        &pool,
        "plugin_sandbox:serve_bundle",
        claims.workspace_id,
        |conn| plugin_repo::get_plugin_by_uuid(conn, plugin_uuid),
    ) {
        Ok(p) => p,
        Err(BackgroundRunError::Db(diesel::result::Error::NotFound)) => {
            return errors::not_found_msg("Plugin not found");
        }
        Err(e) => {
            error!(error = %e, %plugin_uuid, "failed to read plugin bundle");
            return errors::internal("Failed to read plugin bundle");
        }
    };

    if !plugin.is_active() {
        return errors::forbidden("Plugin is not active");
    }
    // A rotated/reinstalled bundle mints a new hash; a token pinned to the old
    // one must not serve the new bytes.
    if plugin.bundle_hash.as_deref() != Some(claims.bundle_hash.as_str()) {
        return errors::not_found_msg("Bundle version no longer available");
    }
    let Some(bytes) = plugin.bundle_js else {
        return errors::not_found_msg("Plugin bundle not found");
    };

    HttpResponse::Ok()
        .content_type("text/javascript; charset=utf-8")
        .insert_header(("Access-Control-Allow-Origin", "*"))
        .insert_header(("X-Content-Type-Options", "nosniff"))
        .insert_header(("Cross-Origin-Resource-Policy", "cross-origin"))
        .insert_header(("Cache-Control", "private, max-age=60"))
        .body(bytes)
}

/// Mounts the unauthenticated sandbox routes at `/__plugin-sandbox/*`. Register
/// at the top level (outside the auth/tenant middleware): the runtime is static
/// and the bundle is token-gated.
pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/__plugin-sandbox")
            .route("/runtime.html", web::get().to(serve_runtime_html))
            .route("/runtime.js", web::get().to(serve_runtime_js))
            .route("/bundle", web::get().to(serve_bundle)),
    );
}
