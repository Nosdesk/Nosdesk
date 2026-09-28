//! The Microsoft Teams personal tab: the requester portal inside Teams (and
//! Outlook and the Microsoft 365 app), signed in without a prompt.
//!
//! It uses the workspace's own Entra app, the one set up for requester SSO.
//! Teams gives the tab an access token for that app (nested app
//! authentication, no secret involved); `POST /api/portal/auth/teams/session`
//! checks it (`utils::entra_token`) and trades it for a short portal session,
//! like the help widget. The person is their Entra tenant + object id, shared
//! with requester SSO (`repository::requester_identities`).
//!
//! `{portal}/teams` serves the portal in embed mode, framable only by the
//! Microsoft 365 hosts, and only while the tab is on.
//!
//! Admin: `/api/admin/teams` (status, setup values, on/off) and
//! `/api/admin/teams/package` (the app package the customer's Teams admin
//! uploads).

use std::io::Write as _;

use actix_web::{web, HttpMessage, HttpRequest, HttpResponse};
use serde::Deserialize;
use serde_json::json;

use crate::db::Pool;
use crate::errors::ApiError;
use crate::extractors::{TenantConn, WorkspaceContext};
use crate::models::{WorkspaceIdentityProvider, WorkspaceRole};
use crate::repository::workspace_identity_providers as providers;
use crate::utils::rbac::require_workspace_role;

/// The package's version. Bump it whenever the manifest changes, so Teams
/// takes an uploaded package as an update of the same app.
pub const PACKAGE_VERSION: &str = "1.0.0";
const MANIFEST_VERSION: &str = "1.19";
const COLOR_ICON: &[u8] = include_bytes!("teams/color.png");
const OUTLINE_ICON: &[u8] = include_bytes!("teams/outline.png");

/// Public, mounted at `/api/portal/auth/teams`.
pub fn portal_auth_config(cfg: &mut web::ServiceConfig) {
    cfg.route("", web::get().to(tab_info))
        .route("/session", web::post().to(exchange_session));
}

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.route("/admin/teams", web::get().to(get_status))
        .route("/admin/teams", web::put().to(save_status))
        .route("/admin/teams/package", web::get().to(download_package));
}

/// The provider, when it can serve the tab: Entra, with the tab turned on.
fn tab_provider(pool: &Pool, workspace_id: i32) -> Option<WorkspaceIdentityProvider> {
    crate::sync::session::run_in_workspace(
        pool,
        "background:teams_provider",
        workspace_id,
        |conn| providers::get(conn),
    )
    .ok()
    .flatten()
    .filter(|p| p.kind == "entra" && p.teams_enabled)
}

/// The scope the tab asks Teams for.
fn scope(client_id: &str) -> String {
    format!("api://{client_id}/{}", crate::utils::entra_token::SCOPE)
}

/// `GET /api/portal/auth/teams`: whether the tab is on, and what it needs to
/// ask Teams for a token (both public: they're in the app package too).
pub async fn tab_info(req: HttpRequest, pool: web::Data<Pool>) -> HttpResponse {
    let provider = req
        .extensions()
        .get::<WorkspaceContext>()
        .map(|ws| ws.workspace_id)
        .and_then(|id| tab_provider(&pool, id));
    match provider {
        Some(p) => HttpResponse::Ok().json(json!({
            "enabled": true,
            "client_id": p.client_id,
            "scope": scope(&p.client_id),
        })),
        None => HttpResponse::Ok().json(json!({ "enabled": false })),
    }
}

#[derive(Debug, Deserialize)]
pub struct SessionRequest {
    /// The Entra access token Teams gave the tab.
    pub token: String,
}

/// `POST /api/portal/auth/teams/session`: a Teams token for a portal session.
pub async fn exchange_session(
    req: HttpRequest,
    pool: web::Data<Pool>,
    body: web::Json<SessionRequest>,
) -> HttpResponse {
    let refused = |why: &str| {
        tracing::info!("teams: token refused ({why})");
        crate::errors::unauthorized("That sign-in didn't work")
    };
    let Some(ws) = req.extensions().get::<WorkspaceContext>().cloned() else {
        return crate::errors::not_found("No help portal here");
    };
    let workspace_id = ws.workspace_id;
    let Some(provider) = tab_provider(&pool, workspace_id) else {
        return refused("tab off");
    };
    if body.token.len() > 16 * 1024 {
        return refused("oversized");
    }
    let user = match crate::utils::entra_token::verify(
        &body.token,
        &provider.issuer_url,
        &provider.client_id,
    )
    .await
    {
        Ok(u) => u,
        Err(why) => return refused(why),
    };

    let keys = [crate::repository::requester_identities::entra_key(
        &user.tenant_id,
        &user.object_id,
    )];
    let resolved = crate::sync::session::run_in_workspace(
        &pool,
        "background:teams_person",
        workspace_id,
        |conn| {
            crate::repository::requester_identities::resolve(
                conn,
                workspace_id,
                &provider.domains(),
                &crate::repository::requester_identities::Assertion {
                    keys: &keys,
                    email: user.email.as_deref(),
                    email_verified: user.email_verified,
                    name: user.name.as_deref(),
                    via: "teams",
                },
            )
        },
    );
    let user_uuid = match resolved {
        Ok(Ok(u)) => u,
        Ok(Err(_)) => {
            tracing::info!("teams: token refused (no email at a listed domain)");
            // The tab says so: it's for the admin to fix, not a retry.
            return crate::errors::unauthorized_with_code(
                "Your account isn't set up for this help portal",
                "teams_domain",
            );
        }
        Err(e) => {
            tracing::error!(error = ?e, "teams: resolving the person failed");
            return crate::errors::internal("Couldn't sign you in");
        }
    };
    crate::handlers::portal::embedded_sign_in(
        &req,
        &pool,
        crate::repository::active_sessions::EmbedHost::Teams,
        user_uuid,
        ws.workspace_uuid,
    )
}

/// `GET /teams` (and `/portal/teams`): the portal shell for the tab. Framable
/// by the Microsoft 365 hosts only while the tab is on; otherwise it keeps the
/// app's `frame-ancestors 'none'`.
pub async fn serve_shell(req: HttpRequest, pool: web::Data<Pool>) -> HttpResponse {
    let Some(ws) = req.extensions().get::<WorkspaceContext>().cloned() else {
        return crate::errors::not_found("No help portal here");
    };
    let Ok(shell) = tokio::fs::read("./public/portal.html").await else {
        return crate::errors::not_found("The help portal isn't built");
    };
    let mut res = HttpResponse::Ok();
    res.content_type("text/html; charset=utf-8")
        .insert_header(("Cache-Control", "no-cache"));
    if tab_provider(&pool, ws.workspace_id).is_some() {
        let hosts: Vec<String> = crate::middleware::security_headers::TEAMS_FRAME_ANCESTORS
            .iter()
            .map(|h| h.to_string())
            .collect();
        res.insert_header((
            "Content-Security-Policy",
            crate::middleware::security_headers::embed_csp(&hosts),
        ));
    }
    res.body(shell)
}

fn internal(e: impl std::fmt::Debug, what: &str) -> ApiError {
    tracing::error!(error = ?e, "teams: {what} failed");
    ApiError::Internal(format!("Failed to {what}"))
}

/// Everything the admin page and the package need.
struct Setup {
    provider: Option<WorkspaceIdentityProvider>,
    origin: Option<String>,
    app_name: String,
}

fn load(tc: &mut TenantConn, workspace_id: i32) -> Result<Setup, ApiError> {
    tc.run(|conn| {
        let provider = providers::get(conn)?;
        let origin = crate::utils::portal_ticket_link::portal_origin(conn, workspace_id);
        let app_name = crate::repository::site_settings::get_site_settings(conn)?.app_name;
        Ok::<_, diesel::result::Error>(Setup {
            provider,
            origin,
            app_name,
        })
    })
    .map_err(|e| internal(e, "load the Teams settings"))
}

fn tab_url(origin: &str) -> String {
    format!("{origin}{}", crate::handlers::portal::portal_path("/teams"))
}

fn host_of(origin: &str) -> &str {
    origin.split_once("://").map(|(_, h)| h).unwrap_or(origin)
}

fn status_view(setup: &Setup) -> serde_json::Value {
    let entra = setup.provider.as_ref().filter(|p| p.kind == "entra");
    json!({
        // The tab needs requester sign-in set up with Microsoft Entra ID.
        "available": entra.is_some() && setup.origin.is_some(),
        "provider_kind": setup.provider.as_ref().map(|p| p.kind.clone()),
        "enabled": entra.is_some_and(|p| p.teams_enabled),
        "client_id": entra.map(|p| p.client_id.clone()),
        "allowed_domains": entra.map(|p| p.domains()).unwrap_or_default(),
        "package_version": PACKAGE_VERSION,
        // What the admin sets on their Entra app registration.
        "setup": setup.origin.as_deref().zip(entra).map(|(origin, p)| json!({
            "redirect_uri": format!("brk-multihub://{}", host_of(origin)),
            "application_id_uri": format!("api://{}", p.client_id),
            "scope": crate::utils::entra_token::SCOPE,
            "tab_url": tab_url(origin),
        })),
    })
}

/// `GET /api/admin/teams`.
pub async fn get_status(
    mut tc: TenantConn,
    req: HttpRequest,
    ws: WorkspaceContext,
) -> Result<HttpResponse, ApiError> {
    require_workspace_role(&req, WorkspaceRole::Admin)?;
    let setup = load(&mut tc, ws.workspace_id)?;
    Ok(HttpResponse::Ok().json(status_view(&setup)))
}

#[derive(Debug, Deserialize)]
pub struct SaveRequest {
    pub enabled: bool,
}

/// `PUT /api/admin/teams`: turn the tab on or off.
pub async fn save_status(
    mut tc: TenantConn,
    req: HttpRequest,
    ws: WorkspaceContext,
    body: web::Json<SaveRequest>,
) -> Result<HttpResponse, ApiError> {
    require_workspace_role(&req, WorkspaceRole::Admin)?;
    let setup = load(&mut tc, ws.workspace_id)?;
    if body.enabled && setup.provider.as_ref().is_none_or(|p| p.kind != "entra") {
        return Err(ApiError::BadRequest(
            "Set up requester sign-in with Microsoft Entra ID first".into(),
        ));
    }
    tc.run(|conn| providers::set_teams_enabled(conn, body.enabled))
        .map_err(|e| internal(e, "save the Teams settings"))?;
    let setup = load(&mut tc, ws.workspace_id)?;
    Ok(HttpResponse::Ok().json(status_view(&setup)))
}

/// Keep `s` within `max` characters (the manifest's limits).
fn clip(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

/// The Teams app manifest for this workspace.
pub fn manifest(
    provider: &WorkspaceIdentityProvider,
    origin: &str,
    app_name: &str,
) -> serde_json::Value {
    let name = if app_name.trim().is_empty() {
        "Help"
    } else {
        app_name.trim()
    };
    let portal_home = format!("{origin}{}", crate::handlers::portal::portal_path("/"));
    json!({
        "$schema": format!(
            "https://developer.microsoft.com/json-schemas/teams/v{MANIFEST_VERSION}/MicrosoftTeams.schema.json"
        ),
        "manifestVersion": MANIFEST_VERSION,
        "version": PACKAGE_VERSION,
        "id": provider.teams_app_id,
        "developer": {
            "name": clip(name, 32),
            "websiteUrl": portal_home,
            "privacyUrl": portal_home,
            "termsOfUseUrl": portal_home,
        },
        "name": { "short": clip(name, 30), "full": clip(&format!("{name} help"), 100) },
        "description": {
            "short": "Ask for help and follow your requests.",
            "full": "Raise a request, follow its progress, reply, and approve requests \
                     waiting for you, without leaving Microsoft Teams.",
        },
        "icons": { "color": "color.png", "outline": "outline.png" },
        "accentColor": "#FFFFFF",
        "staticTabs": [{
            "entityId": "help",
            "name": "Help",
            "contentUrl": tab_url(origin),
            "websiteUrl": portal_home,
            "scopes": ["personal"],
        }],
        "permissions": ["identity"],
        "validDomains": [host_of(origin)],
        "showLoadingIndicator": true,
        // Declares the app Teams signs people in to (and that admins consent
        // for in the Teams admin center).
        "webApplicationInfo": { "id": provider.client_id },
    })
}

/// `GET /api/admin/teams/package`: the zip the customer's Teams admin uploads
/// (Teams admin center, Manage apps, Upload new app).
pub async fn download_package(
    mut tc: TenantConn,
    req: HttpRequest,
    ws: WorkspaceContext,
) -> Result<HttpResponse, ApiError> {
    require_workspace_role(&req, WorkspaceRole::Admin)?;
    let setup = load(&mut tc, ws.workspace_id)?;
    let (Some(provider), Some(origin)) = (
        setup.provider.as_ref().filter(|p| p.kind == "entra"),
        setup.origin.as_deref(),
    ) else {
        return Err(ApiError::BadRequest(
            "Set up requester sign-in with Microsoft Entra ID first".into(),
        ));
    };
    let manifest = serde_json::to_vec_pretty(&manifest(provider, origin, &setup.app_name))
        .map_err(|e| internal(e, "build the Teams package"))?;
    let zip = (|| -> zip::result::ZipResult<Vec<u8>> {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        for (name, bytes) in [
            ("manifest.json", manifest.as_slice()),
            ("color.png", COLOR_ICON),
            ("outline.png", OUTLINE_ICON),
        ] {
            zip.start_file(name, options)?;
            zip.write_all(bytes)?;
        }
        Ok(zip.finish()?.into_inner())
    })()
    .map_err(|e| internal(e, "build the Teams package"))?;
    Ok(HttpResponse::Ok()
        .content_type("application/zip")
        .insert_header((
            "Content-Disposition",
            "attachment; filename=\"teams-app.zip\"",
        ))
        .insert_header(("Cache-Control", "no-store"))
        .body(zip))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::entra_token::test_support as entra;

    fn entra_provider(
        conn: &mut crate::db::DbConnection,
        domains: &[String],
    ) -> WorkspaceIdentityProvider {
        providers::save(
            conn,
            1,
            &providers::ProviderInput {
                kind: "entra",
                display_name: "Microsoft",
                issuer_url: &entra::issuer(),
                client_id: entra::CLIENT,
                client_secret: None,
                allowed_domains: domains,
                enabled: false,
            },
        )
        .unwrap()
    }

    #[test]
    fn the_manifest_names_the_workspace_app_and_its_portal_only() {
        let mut conn = crate::test_helpers::setup_test_connection();
        let provider = entra_provider(&mut conn, &[]);
        let m = manifest(&provider, "https://help.acme.test", "Acme IT");
        assert_eq!(m["id"], json!(provider.teams_app_id));
        assert_eq!(m["webApplicationInfo"]["id"], entra::CLIENT);
        assert_eq!(m["validDomains"], json!(["help.acme.test"]));
        let tab = &m["staticTabs"][0];
        assert!(tab["contentUrl"]
            .as_str()
            .unwrap()
            .starts_with("https://help.acme.test/"));
        assert!(tab["contentUrl"].as_str().unwrap().ends_with("/teams"));
        assert_eq!(m["name"]["short"], "Acme IT");
        assert_eq!(m["version"], PACKAGE_VERSION);
        let long = manifest(&provider, "https://help.acme.test", &"x".repeat(80));
        assert_eq!(long["name"]["short"].as_str().unwrap().len(), 30);
    }

    async fn exchange(pool: &Pool, token: String) -> HttpResponse {
        let req = actix_web::test::TestRequest::post().to_http_request();
        let ws = crate::repository::workspaces::find_by_id(&mut pool.get().unwrap(), 1)
            .unwrap()
            .unwrap();
        req.extensions_mut().insert(WorkspaceContext {
            workspace_id: ws.id,
            workspace_uuid: ws.uuid,
            slug: ws.slug,
            name: ws.name,
            custom_domain: ws.custom_domain,
            organisation_id: ws.organisation_id,
        });
        exchange_session(
            req,
            web::Data::new(pool.clone()),
            web::Json(SessionRequest { token }),
        )
        .await
    }

    #[actix_web::test]
    async fn a_teams_token_becomes_a_portal_session_only_while_the_tab_is_on() {
        if std::env::var("JWT_SECRET").is_err() {
            std::env::set_var("JWT_SECRET", "test-teams-tab-secret-at-least-32-bytes!");
        }
        let pool = crate::test_helpers::setup_test_pool();
        {
            let mut conn = pool.get().unwrap();
            entra_provider(&mut conn, &["acme.test".to_string()]);
        }
        crate::oidc::prime_discovery_for_test(&entra::issuer(), entra::jwks());
        let token = entra::sign(&entra::claims(), "k1");

        let off = exchange(&pool, token.clone()).await;
        assert_eq!(off.status(), 401, "the tab is off");

        providers::set_teams_enabled(&mut pool.get().unwrap(), true).unwrap();
        let on = exchange(&pool, token.clone()).await;
        assert_eq!(on.status(), 200);
        let body = actix_web::body::to_bytes(on.into_body()).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(body["access_token"].as_str().is_some_and(|t| !t.is_empty()));

        // The person is keyed on tenant + object id.
        let keyed = crate::repository::user_auth_identities::find_user_by_scoped_identity(
            1,
            &format!("entra:{}", entra::TID),
            entra::OID,
            &mut pool.get().unwrap(),
        )
        .unwrap();
        assert!(keyed.is_some());

        // Someone at a domain the workspace didn't list is told so.
        let mut stranger = entra::claims();
        stranger["oid"] = json!("bbbbbbbb-bbbb-cccc-dddd-eeeeeeeeeeee");
        stranger["email"] = json!("eve@elsewhere.test");
        let refused = exchange(&pool, entra::sign(&stranger, "k1")).await;
        assert_eq!(refused.status(), 401);
        let body = actix_web::body::to_bytes(refused.into_body())
            .await
            .unwrap();
        assert!(String::from_utf8_lossy(&body).contains("teams_domain"));

        // A forged token is just refused.
        let mut forged = token.clone();
        forged.push('x');
        assert_eq!(exchange(&pool, forged).await.status(), 401);
    }
}
