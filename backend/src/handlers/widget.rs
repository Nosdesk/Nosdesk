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
    cfg.route("/widget", web::get().to(embed_info))
        .route("/widget/session", web::post().to(exchange_session));
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
    let secret = row.and_then(|r| settings::secret(r).ok().flatten());
    json!({
        "enabled": row.is_some_and(|r| r.enabled),
        "allowed_origins": row.map(|r| r.origins()).unwrap_or_default(),
        "has_secret": secret.is_some(),
        // What a site puts in its tokens' `kid` header and `aud` claim.
        "secret_kid": secret.as_deref().map(settings::kid),
        "token_audience": origin.clone(),
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
            let row = settings::save(conn, body.enabled, &origins)?;
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

/// `GET /api/portal/auth/widget`: whether the widget is on. (What visitors
/// who aren't signed in get is the guest access settings' call.)
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
        .insert_header(("Cache-Control", "no-cache"))
        // Sites that set COEP can frame it (what may frame it is the CSP's call).
        .insert_header(("Cross-Origin-Resource-Policy", "cross-origin"));
    if !origins.is_empty() {
        res.insert_header((
            "Content-Security-Policy",
            crate::middleware::security_headers::embed_csp(&origins),
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

/// Longest a visitor token may live (`exp - iat`). Short, because the host
/// page asks for a fresh one whenever it's needed.
const MAX_TOKEN_SECONDS: i64 = 600;

/// What a site's server signs for a visitor.
#[derive(Debug, Deserialize)]
pub struct VisitorClaims {
    /// The site's own id for the person. Required: identity is keyed on it, so
    /// a changed email still reaches the same person and nobody else.
    pub sub: String,
    pub email: String,
    #[serde(default)]
    pub name: Option<String>,
    /// The site has confirmed the person owns this address. Required to join
    /// an account that already exists here.
    #[serde(default)]
    pub email_verified: Option<bool>,
    pub iat: i64,
    pub exp: i64,
}

/// Verify a visitor token: HS256 under one of `secrets` (the one its `kid`
/// names, if it names one), for `audience` (this help portal's origin),
/// unexpired with a minute of leeway, at most [`MAX_TOKEN_SECONDS`] long, with
/// a `sub` and an email.
pub fn verify_visitor_token(
    token: &str,
    secrets: &[String],
    audience: &str,
) -> Result<VisitorClaims, &'static str> {
    use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};
    let header = decode_header(token).map_err(|_| "malformed")?;
    if header.alg != Algorithm::HS256 {
        return Err("wrong algorithm");
    }
    let candidates: Vec<&String> = match header.kid.as_deref() {
        Some(kid) => secrets.iter().filter(|s| settings::kid(s) == kid).collect(),
        None => secrets.iter().collect(),
    };
    if candidates.is_empty() {
        return Err("unknown key");
    }
    let mut validation = Validation::new(Algorithm::HS256);
    validation.leeway = 60;
    validation.set_required_spec_claims(&["exp", "iat", "aud", "sub"]);
    validation.set_audience(&[audience]);
    let claims = candidates
        .iter()
        .find_map(|secret| {
            decode::<VisitorClaims>(
                token,
                &DecodingKey::from_secret(secret.as_bytes()),
                &validation,
            )
            .ok()
        })
        .ok_or("invalid token")?
        .claims;
    if claims.exp - claims.iat > MAX_TOKEN_SECONDS {
        return Err("token lives too long");
    }
    if claims.iat > chrono::Utc::now().timestamp() + 60 {
        return Err("token issued in the future");
    }
    let sub = claims.sub.trim();
    if sub.is_empty() || sub.len() > 255 {
        return Err("no usable sub");
    }
    let email = claims.email.trim();
    if email.len() > 254 || !email.contains('@') || email.contains(char::is_whitespace) {
        return Err("no usable email");
    }
    Ok(claims)
}

/// Whether `origin` is one of `allowed` (exactly, or under a leftmost
/// wildcard with the same scheme and port).
pub fn origin_allowed(origin: &str, allowed: &[String]) -> bool {
    let Ok(origin) = normalize_origin(origin) else {
        return false;
    };
    allowed.iter().any(|a| {
        if *a == origin {
            return true;
        }
        let Some((scheme, suffix)) = a.split_once("://*.") else {
            return false;
        };
        origin
            .strip_prefix(&format!("{scheme}://"))
            .is_some_and(|host| host.ends_with(&format!(".{suffix}")))
    })
}

#[derive(Debug, Deserialize)]
pub struct SessionRequest {
    pub token: String,
    /// The host page's origin, as the iframe saw it on the loader's message.
    pub parent_origin: String,
}

/// `POST /api/portal/auth/widget/session`: trade a visitor token the site's
/// server signed for a portal access token (returned in the body; the iframe
/// holds it in memory, since third-party cookies don't work). Each token works
/// once. No refresh: when the access token expires the iframe asks the host
/// page for a new visitor token.
pub async fn exchange_session(
    req: HttpRequest,
    pool: web::Data<Pool>,
    body: web::Json<SessionRequest>,
) -> HttpResponse {
    let refused = |why: &str| {
        tracing::info!("widget: visitor token refused ({why})");
        crate::errors::unauthorized("That sign-in didn't work")
    };
    let Some(ws) = req.extensions().get::<WorkspaceContext>().cloned() else {
        return crate::errors::not_found("No help portal here");
    };
    let workspace_id = ws.workspace_id;
    let Some((row, audience)) = crate::sync::session::run_in_workspace(
        &pool,
        "background:widget_session_settings",
        workspace_id,
        |conn| {
            let row = settings::get(conn)?;
            let audience = crate::utils::portal_ticket_link::portal_origin(conn, workspace_id);
            Ok((row, audience))
        },
    )
    .ok()
    .and_then(|(row, audience)| Some((row.filter(|r| r.enabled)?, audience?))) else {
        return refused("widget off");
    };
    if !origin_allowed(&body.parent_origin, &row.origins()) {
        return refused("site not listed");
    }
    let secrets = match settings::signing_secrets(&row) {
        Ok(s) if !s.is_empty() => s,
        _ => return refused("no secret"),
    };
    let claims = match verify_visitor_token(&body.token, &secrets, &audience) {
        Ok(c) => c,
        Err(why) => return refused(why),
    };
    // Each token works once, for as long as it could otherwise be replayed.
    let replay_key = format!(
        "widget_token_used:{}",
        ring::digest::digest(&ring::digest::SHA256, body.token.as_bytes())
            .as_ref()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    );
    let ttl = (claims.exp - chrono::Utc::now().timestamp() + 60).max(1) as u64;
    match crate::utils::rate_limit::RateLimiter::claim_once(
        &crate::utils::rate_limit::get_redis_url(),
        &replay_key,
        ttl,
    )
    .await
    {
        Ok(true) => {}
        Ok(false) => return refused("token already used"),
        Err(e) => tracing::warn!(error = %e, "widget: replay store unavailable; allowing"),
    }

    let email = claims.email.trim().to_lowercase();
    let name = claims
        .name
        .clone()
        .map(|n| n.trim().chars().take(120).collect::<String>())
        .filter(|n| !n.is_empty())
        .or_else(|| crate::utils::name_from_email(&email))
        .unwrap_or_else(|| email.clone());
    let visitor = crate::repository::widget_visitors::Visitor {
        sub: claims.sub.trim(),
        email: &email,
        name: &name,
        email_verified: claims.email_verified == Some(true),
    };
    let resolved = crate::sync::session::run_in_workspace(
        &pool,
        "background:widget_session_person",
        workspace_id,
        |conn| crate::repository::widget_visitors::resolve(conn, workspace_id, &visitor),
    );
    let user_uuid = match resolved {
        Ok(Ok(u)) => u,
        Ok(Err(refusal)) => return refused(refusal.as_str()),
        Err(e) => {
            tracing::error!(error = ?e, "widget: resolving the visitor failed");
            return crate::errors::internal("Couldn't sign you in");
        }
    };
    crate::handlers::portal::embedded_sign_in(
        &req,
        &pool,
        crate::repository::active_sessions::EmbedHost::Widget,
        user_uuid,
        ws.workspace_uuid,
    )
}

#[cfg(test)]
mod tests {
    use super::normalize_origin as n;
    use super::{origin_allowed, verify_visitor_token};

    const AUD: &str = "https://help.acme.test";

    fn sign_kid(claims: serde_json::Value, secret: &str, kid: Option<String>) -> String {
        let mut header = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::HS256);
        header.kid = kid;
        jsonwebtoken::encode(
            &header,
            &claims,
            &jsonwebtoken::EncodingKey::from_secret(secret.as_bytes()),
        )
        .unwrap()
    }

    fn sign(claims: serde_json::Value, secret: &str) -> String {
        sign_kid(claims, secret, None)
    }

    fn claims(extra: serde_json::Value) -> serde_json::Value {
        let now = chrono::Utc::now().timestamp();
        let mut c = serde_json::json!({
            "sub": "acme-42", "email": "sam@acme.test", "aud": AUD, "iat": now, "exp": now + 300
        });
        for (k, v) in extra.as_object().unwrap() {
            c[k] = v.clone();
        }
        c
    }

    #[test]
    fn visitor_tokens_are_short_lived_signed_and_carry_a_sub_and_email() {
        let now = chrono::Utc::now().timestamp();
        let s = vec!["s".to_string()];
        assert_eq!(
            verify_visitor_token(&sign(claims(serde_json::json!({})), "s"), &s, AUD)
                .unwrap()
                .sub,
            "acme-42"
        );
        assert!(
            verify_visitor_token(&sign(claims(serde_json::json!({})), "other"), &s, AUD).is_err(),
            "wrong secret"
        );
        assert!(
            verify_visitor_token(
                &sign(claims(serde_json::json!({"exp": now + 3600})), "s"),
                &s,
                AUD
            )
            .is_err(),
            "too long"
        );
        assert!(
            verify_visitor_token(
                &sign(
                    claims(serde_json::json!({"iat": now - 900, "exp": now - 300})),
                    "s"
                ),
                &s,
                AUD
            )
            .is_err(),
            "expired"
        );
        assert!(
            verify_visitor_token(
                &sign(claims(serde_json::json!({"aud": "https://evil.test"})), "s"),
                &s,
                AUD
            )
            .is_err(),
            "another portal"
        );
        let mut no_sub = claims(serde_json::json!({}));
        no_sub.as_object_mut().unwrap().remove("sub");
        assert!(
            verify_visitor_token(&sign(no_sub, "s"), &s, AUD).is_err(),
            "sub required"
        );
        let mut no_aud = claims(serde_json::json!({}));
        no_aud.as_object_mut().unwrap().remove("aud");
        assert!(
            verify_visitor_token(&sign(no_aud, "s"), &s, AUD).is_err(),
            "aud required"
        );
        assert!(verify_visitor_token(
            &sign(claims(serde_json::json!({"email": "nobody"})), "s"),
            &s,
            AUD
        )
        .is_err());
        let hs512 = jsonwebtoken::encode(
            &jsonwebtoken::Header::new(jsonwebtoken::Algorithm::HS512),
            &claims(serde_json::json!({})),
            &jsonwebtoken::EncodingKey::from_secret(b"s"),
        )
        .unwrap();
        assert!(verify_visitor_token(&hs512, &s, AUD).is_err(), "HS256 only");
    }

    #[test]
    fn the_kid_picks_the_secret_and_the_previous_one_still_works() {
        let secrets = vec!["new".to_string(), "old".to_string()];
        let kid = |s: &str| Some(crate::repository::workspace_widget_settings::kid(s));
        let old = sign_kid(claims(serde_json::json!({})), "old", kid("old"));
        assert!(
            verify_visitor_token(&old, &secrets, AUD).is_ok(),
            "previous secret, named"
        );
        let unnamed = sign(claims(serde_json::json!({})), "old");
        assert!(
            verify_visitor_token(&unnamed, &secrets, AUD).is_ok(),
            "previous secret, no kid"
        );
        let mislabelled = sign_kid(claims(serde_json::json!({})), "old", kid("new"));
        assert!(verify_visitor_token(&mislabelled, &secrets, AUD).is_err());
        let unknown = sign_kid(
            claims(serde_json::json!({})),
            "old",
            Some("feedface".into()),
        );
        assert!(verify_visitor_token(&unknown, &secrets, AUD).is_err());
    }

    #[test]
    fn only_listed_sites_may_ask() {
        let allowed = vec![
            "https://www.acme.com".to_string(),
            "https://*.acme.org".to_string(),
        ];
        assert!(origin_allowed("https://www.acme.com", &allowed));
        assert!(origin_allowed("https://help.acme.org", &allowed));
        assert!(origin_allowed("https://a.b.acme.org", &allowed));
        assert!(
            !origin_allowed("https://acme.org", &allowed),
            "wildcard needs a subdomain"
        );
        assert!(!origin_allowed("https://evilacme.org", &allowed));
        assert!(
            !origin_allowed("http://help.acme.org", &allowed),
            "scheme must match"
        );
        assert!(!origin_allowed("https://acme.com", &allowed));
        assert!(!origin_allowed("null", &allowed));
    }

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
