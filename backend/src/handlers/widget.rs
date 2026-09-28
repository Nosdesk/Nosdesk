//! The embeddable help widget.
//!
//! A site adds `<script src="{portal}/widget.js" async>`; the loader draws a
//! launcher and an iframe of `{portal}/widget`, which the portal serves in
//! embed mode. Only the sites an admin lists may frame it: the page's CSP
//! `frame-ancestors` is built from that list (the app-wide policy otherwise,
//! so nothing else loosens), and with no list it keeps `frame-ancestors 'none'`
//! and `X-Frame-Options: DENY`.
//!
//! Admin: `/api/admin/widget` (settings) and `/api/admin/widget/secret` (a new
//! signing secret, shown once).

use actix_web::{web, HttpMessage, HttpRequest, HttpResponse};
use serde::Deserialize;
use serde_json::json;

use crate::db::Pool;
use crate::errors::ApiError;
use crate::extractors::{TenantConn, WorkspaceContext};
use crate::models::WorkspaceRole;
use crate::repository::workspace_widget_settings as settings;
use crate::utils::rbac::require_workspace_role;

/// Most sites a workspace can list.
const MAX_ORIGINS: usize = 20;

/// What the embedded page needs before anyone signs in (public, in
/// `/api/portal/auth`).
pub fn portal_auth_config(cfg: &mut web::ServiceConfig) {
    cfg.route("/widget", web::get().to(embed_info));
}

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.route("/admin/widget", web::get().to(get_settings))
        .route("/admin/widget", web::put().to(save_settings))
        .route("/admin/widget/secret", web::post().to(rotate_secret));
}

/// A site allowed to embed the widget, normalised: `scheme://host[:port]`,
/// lower-case, no path. `https` only (plain `http` just for localhost); a
/// wildcard only as the whole leftmost label (`https://*.acme.com`), never on
/// its own. Anything looser would be a clickjacking allowlist.
pub fn normalize_origin(raw: &str) -> Result<String, String> {
    let s = raw.trim().trim_end_matches('/').to_lowercase();
    let bad = || {
        format!(
            "{} isn't a site address like https://help.example.com",
            raw.trim()
        )
    };
    let (scheme, rest) = s.split_once("://").ok_or_else(bad)?;
    if rest.is_empty() || rest.contains(['/', '?', '#', '@', ' ']) {
        return Err(bad());
    }
    let (host, port) = match rest.rsplit_once(':') {
        Some((h, p)) if !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()) => (h, Some(p)),
        Some(_) => return Err(bad()),
        None => (rest, None),
    };
    let local = matches!(host, "localhost" | "127.0.0.1");
    match scheme {
        "https" => {}
        "http" if local => {}
        "http" => return Err(format!("{} must use https", raw.trim())),
        _ => return Err(bad()),
    }
    let labels: Vec<&str> = host.split('.').collect();
    let valid_label = |l: &str| {
        !l.is_empty()
            && l.len() <= 63
            && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
            && !l.starts_with('-')
            && !l.ends_with('-')
    };
    let (first, tail) = labels.split_first().ok_or_else(bad)?;
    let wildcard = *first == "*";
    if wildcard && tail.len() < 2 {
        // `*.com` would let any site in.
        return Err(format!("{} is too broad", raw.trim()));
    }
    if !(wildcard || valid_label(first)) || !tail.iter().all(|l| valid_label(l)) {
        return Err(bad());
    }
    if !local && labels.len() < 2 {
        return Err(bad());
    }
    Ok(match port {
        Some(p) => format!("{scheme}://{host}:{p}"),
        None => format!("{scheme}://{host}"),
    })
}

#[derive(Debug, Deserialize)]
pub struct SaveRequest {
    pub enabled: bool,
    pub allowed_origins: Vec<String>,
    pub allow_anonymous: bool,
}

fn internal(e: impl std::fmt::Debug, what: &str) -> ApiError {
    tracing::error!(error = ?e, "widget: {what} failed");
    ApiError::Internal(format!("Failed to {what}"))
}

/// Where the site's script tag points, and the page the iframe loads.
fn script_url(origin: &str) -> String {
    format!(
        "{origin}{}",
        crate::handlers::portal::portal_path("/widget.js")
    )
}

fn view(
    row: Option<&crate::models::WorkspaceWidgetSettings>,
    origin: Option<String>,
) -> serde_json::Value {
    json!({
        "enabled": row.is_some_and(|r| r.enabled),
        "allowed_origins": row.map(|r| r.origins()).unwrap_or_default(),
        "allow_anonymous": row.is_none_or(|r| r.allow_anonymous),
        "has_secret": row.is_some_and(|r| r.encrypted_secret.is_some()),
        "script_url": origin.map(|o| script_url(&o)),
    })
}

pub async fn get_settings(
    mut tc: TenantConn,
    req: HttpRequest,
    ws: WorkspaceContext,
) -> Result<HttpResponse, ApiError> {
    require_workspace_role(&req, WorkspaceRole::Admin)?;
    let workspace_id = ws.workspace_id;
    let (row, origin) = tc
        .run(|conn| {
            let row = settings::get(conn)?;
            let origin = crate::utils::portal_ticket_link::portal_origin(conn, workspace_id);
            Ok::<_, diesel::result::Error>((row, origin))
        })
        .map_err(|e| internal(e, "load widget settings"))?;
    Ok(HttpResponse::Ok().json(view(row.as_ref(), origin)))
}

pub async fn save_settings(
    mut tc: TenantConn,
    req: HttpRequest,
    ws: WorkspaceContext,
    body: web::Json<SaveRequest>,
) -> Result<HttpResponse, ApiError> {
    require_workspace_role(&req, WorkspaceRole::Admin)?;
    let body = body.into_inner();
    let mut origins: Vec<String> = Vec::new();
    for raw in body.allowed_origins.iter().filter(|o| !o.trim().is_empty()) {
        let origin = normalize_origin(raw).map_err(ApiError::BadRequest)?;
        if !origins.contains(&origin) {
            origins.push(origin);
        }
    }
    if origins.len() > MAX_ORIGINS {
        return Err(ApiError::BadRequest(format!(
            "List at most {MAX_ORIGINS} sites"
        )));
    }
    if body.enabled && origins.is_empty() {
        return Err(ApiError::BadRequest(
            "List at least one site that will show the widget".into(),
        ));
    }
    let workspace_id = ws.workspace_id;
    let (row, origin) = tc
        .run(|conn| {
            let row = settings::save(conn, body.enabled, &origins, body.allow_anonymous)?;
            let origin = crate::utils::portal_ticket_link::portal_origin(conn, workspace_id);
            Ok::<_, diesel::result::Error>((row, origin))
        })
        .map_err(|e| internal(e, "save widget settings"))?;
    Ok(HttpResponse::Ok().json(view(Some(&row), origin)))
}

/// `POST /api/admin/widget/secret`: a new signing secret, returned once.
pub async fn rotate_secret(
    mut tc: TenantConn,
    req: HttpRequest,
    ws: WorkspaceContext,
) -> Result<HttpResponse, ApiError> {
    require_workspace_role(&req, WorkspaceRole::Admin)?;
    let workspace_id = ws.workspace_id;
    let secret = tc
        .run(|conn| Ok::<_, diesel::result::Error>(settings::rotate_secret(conn, workspace_id)))
        .map_err(|e| internal(e, "generate the widget secret"))?
        .map_err(|e| internal(e, "seal the widget secret"))?;
    Ok(HttpResponse::Ok().json(json!({ "secret": secret })))
}

/// The origins allowed to frame this workspace's widget right now; empty when
/// the widget is off.
fn frame_origins(pool: &Pool, workspace_id: i32) -> Vec<String> {
    crate::sync::session::run_in_workspace(pool, "background:widget_frame", workspace_id, |conn| {
        settings::get(conn)
    })
    .ok()
    .flatten()
    .filter(|s| s.enabled)
    .map(|s| s.origins())
    .unwrap_or_default()
}

/// `GET /api/portal/auth/widget`: whether the widget is on and whether
/// visitors without a signed identity get the help centre and request form.
pub async fn embed_info(req: HttpRequest, pool: web::Data<Pool>) -> HttpResponse {
    let row = req
        .extensions()
        .get::<WorkspaceContext>()
        .map(|ws| ws.workspace_id)
        .and_then(|id| {
            crate::sync::session::run_in_workspace(&pool, "background:widget_info", id, |conn| {
                settings::get(conn)
            })
            .ok()
            .flatten()
        });
    HttpResponse::Ok().json(json!({
        "enabled": row.as_ref().is_some_and(|r| r.enabled),
        "allow_anonymous": row.as_ref().is_none_or(|r| r.allow_anonymous),
    }))
}

/// `GET /widget` (hosted) and `/portal/widget` (self-hosted): the portal shell
/// in embed mode, frameable by the listed sites only.
pub async fn serve_shell(req: HttpRequest, pool: web::Data<Pool>) -> HttpResponse {
    let Some(ws) = req.extensions().get::<WorkspaceContext>().cloned() else {
        return crate::errors::not_found("No help portal here");
    };
    let Ok(shell) = tokio::fs::read("./public/portal.html").await else {
        return crate::errors::not_found("The help portal isn't built");
    };
    let origins = frame_origins(&pool, ws.workspace_id);
    let mut res = HttpResponse::Ok();
    res.content_type("text/html; charset=utf-8")
        .insert_header(("Cache-Control", "no-cache"));
    if !origins.is_empty() {
        res.insert_header((
            "Content-Security-Policy",
            crate::middleware::security_headers::widget_csp(&origins),
        ));
    }
    res.body(shell)
}

/// `GET /widget.js` (and `/portal/widget.js`): the loader. A short cache so a
/// fix reaches embedding sites within minutes.
pub async fn serve_loader() -> HttpResponse {
    match tokio::fs::read("./public/widget.js").await {
        Ok(js) => HttpResponse::Ok()
            .content_type("text/javascript; charset=utf-8")
            .insert_header(("Cache-Control", "public, max-age=300"))
            // Other sites load this script; what they may frame is decided by
            // the widget page's own CSP.
            .insert_header(("Cross-Origin-Resource-Policy", "cross-origin"))
            .body(js),
        Err(_) => crate::errors::not_found("The widget isn't built"),
    }
}

#[cfg(test)]
mod tests {
    use super::normalize_origin as n;

    #[test]
    fn origins_are_strict_and_normalised() {
        assert_eq!(
            n("https://Help.Acme.com/").unwrap(),
            "https://help.acme.com"
        );
        assert_eq!(n("https://*.acme.com").unwrap(), "https://*.acme.com");
        assert_eq!(n("https://acme.com:8443").unwrap(), "https://acme.com:8443");
        assert_eq!(n("http://localhost:3000").unwrap(), "http://localhost:3000");
        for bad in [
            "*",
            "https://*",
            "*.acme.com",
            "https://*.com",
            "https://a.*.acme.com",
            "http://acme.com",
            "https://acme.com/help",
            "https://acme.com?x=1",
            "https://user@acme.com",
            "javascript://acme.com",
            "https://acme",
            "acme.com",
            "https://acme.com:",
        ] {
            assert!(n(bad).is_err(), "{bad} should be refused");
        }
    }
}
