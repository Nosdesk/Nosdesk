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
use crate::services::plugins::types::{Permission, ResourceKind};
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

/// Build the sandbox document CSP. The base is immutable: `default-src 'none'`,
/// `script-src <runtime>` (no remote code), `connect-src 'none'` (data egress
/// stays on the `api.fetch` proxy). The only thing a plugin can widen, via its
/// consented `resource:*` grants, is the passive-resource directives
/// (`img-src`/`font-src`/`media-src` and external `style-src`), always forced to
/// HTTPS. A plugin with no such grants gets exactly the base policy.
fn build_sandbox_csp(resource_perms: &[Permission]) -> String {
    let ro = runtime_origin();
    let ao = app_origin();

    // Consented external hosts grouped by the directive they widen. style-src is
    // seeded with 'unsafe-inline' (always allowed for inline styles); the passive
    // directives start empty and are omitted when a plugin declares none of them.
    let mut style = vec!["'unsafe-inline'".to_string()];
    let mut img: Vec<String> = Vec::new();
    let mut font: Vec<String> = Vec::new();
    let mut media: Vec<String> = Vec::new();
    for (kind, pattern) in resource_perms
        .iter()
        .filter_map(Permission::resource_pattern)
    {
        let src = format!("https://{}", pattern.as_string());
        match kind {
            ResourceKind::Style => style.push(src),
            ResourceKind::Img => img.push(src),
            ResourceKind::Font => font.push(src),
            ResourceKind::Media => media.push(src),
        }
    }

    let mut csp = format!("default-src 'none'; script-src {ro}; connect-src 'none'; ");
    csp.push_str(&format!("style-src {}; ", style.join(" ")));
    for (directive, hosts) in [
        ("img-src", &img),
        ("font-src", &font),
        ("media-src", &media),
    ] {
        if !hosts.is_empty() {
            csp.push_str(&format!("{directive} {}; ", hosts.join(" ")));
        }
    }
    csp.push_str(&format!("frame-ancestors {ao};"));
    csp
}

/// GET /__plugin-sandbox/runtime.html?t=<token> — the sandbox shell (no auth; the
/// bundle fetch it triggers is token-gated). The CSP is per-plugin: the token
/// identifies the plugin, and its consented `resource:*` grants widen the passive
/// resource directives. Fails closed to the base CSP if the token is
/// missing/invalid or the plugin can't be loaded.
pub async fn serve_runtime_html(
    pool: web::Data<Pool>,
    q: web::Query<RuntimeQuery>,
) -> impl Responder {
    let resource_perms = plugin_resource_permissions(&pool, q.t.as_deref().unwrap_or("")).await;
    let csp = build_sandbox_csp(&resource_perms);
    HttpResponse::Ok()
        .content_type("text/html; charset=utf-8")
        .insert_header(("Content-Security-Policy", csp))
        .insert_header(("X-Content-Type-Options", "nosniff"))
        .insert_header(("Cross-Origin-Resource-Policy", "cross-origin"))
        // The CSP varies per plugin, so this response must never be shared.
        .insert_header(("Cache-Control", "private, no-store"))
        .body(RUNTIME_HTML)
}

/// Resolve a bundle token to the plugin's consented `resource:*` permissions.
/// Returns an empty set (base CSP) on any failure: a missing/invalid token, an
/// inactive/unknown plugin, or a permission string that doesn't parse. Never
/// widens the CSP on a bad input (fail closed).
async fn plugin_resource_permissions(pool: &Pool, token: &str) -> Vec<Permission> {
    let Ok(claims) = bundle_token::verify(token) else {
        return Vec::new();
    };
    let plugin = match session::run_in_workspace(
        pool,
        "plugin_sandbox:runtime_csp",
        claims.workspace_id,
        |conn| plugin_repo::get_plugin_by_uuid(conn, claims.plugin_uuid),
    ) {
        Ok(p) if p.is_active() => p,
        _ => return Vec::new(),
    };
    plugin
        .effective_permission_set()
        .iter()
        .filter_map(|s| Permission::parse(s).ok())
        .filter(|p| p.resource_pattern().is_some())
        .collect()
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

/// runtime.html query. The token is optional here (a tokenless request still
/// serves the shell under the base CSP); when present it selects the plugin
/// whose consented `resource:*` grants widen the CSP.
#[derive(Deserialize)]
pub struct RuntimeQuery {
    t: Option<String>,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn perm(s: &str) -> Permission {
        Permission::parse(s).unwrap()
    }

    #[test]
    fn base_csp_has_no_passive_directives() {
        let csp = build_sandbox_csp(&[]);
        assert!(csp.contains("default-src 'none'"));
        assert!(csp.contains("connect-src 'none'"));
        assert!(csp.contains("style-src 'unsafe-inline';"));
        assert!(!csp.contains("img-src"));
        assert!(!csp.contains("font-src"));
        assert!(!csp.contains("media-src"));
    }

    #[test]
    fn resource_grants_widen_only_their_directive_and_force_https() {
        let csp = build_sandbox_csp(&[
            perm("resource:img:*.tile.openstreetmap.org"),
            perm("resource:img:cdn.example.com"),
            perm("resource:font:fonts.example.com"),
        ]);
        assert!(csp.contains("img-src https://*.tile.openstreetmap.org https://cdn.example.com;"));
        assert!(csp.contains("font-src https://fonts.example.com;"));
        assert!(!csp.contains("media-src"));
        // The immutable base is never weakened by a resource grant.
        assert!(csp.contains("connect-src 'none'"));
        assert!(!csp.contains("script-src 'none'"));
    }

    #[test]
    fn external_style_appends_to_unsafe_inline() {
        let csp = build_sandbox_csp(&[perm("resource:style:cdn.example.com")]);
        assert!(csp.contains("style-src 'unsafe-inline' https://cdn.example.com;"));
    }

    #[test]
    fn non_resource_permissions_are_ignored() {
        // A caller may hand the whole effective set; only resource:* count.
        let csp = build_sandbox_csp(&[perm("ticket:read"), perm("network:api.github.com")]);
        assert!(!csp.contains("img-src"));
        assert!(!csp.contains("api.github.com"));
    }
}
