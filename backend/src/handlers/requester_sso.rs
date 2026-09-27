//! `/api/admin/requester-sso`: a workspace's own sign-in provider for the
//! requester portal (Microsoft Entra ID, Google Workspace, or on self-hosted any
//! OpenID Connect provider). Staff sign-in is configured separately.
//!
//! Trust rule: the provider is the workspace's own, single-tenant, so an email
//! it asserts is trusted only at the domains the admin lists, never a free-mail
//! domain, and never when the provider says the address is unverified. Accounts
//! are keyed on the provider's issuer and subject, not the email.

use actix_web::{web, HttpRequest, HttpResponse};
use serde::Deserialize;
use serde_json::json;

use crate::errors::ApiError;
use crate::extractors::{TenantConn, WorkspaceContext};
use crate::models::{IdentityProviderView, WorkspaceRole};
use crate::repository::workspace_identity_providers::{self as providers, ProviderInput};
use crate::utils::rbac::require_workspace_role;

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.route("/admin/requester-sso", web::get().to(get_provider))
        .route("/admin/requester-sso", web::put().to(save_provider))
        .route("/admin/requester-sso", web::delete().to(delete_provider));
}

/// Where the provider sends people back: the admin registers this URL with it.
pub const CALLBACK_PATH: &str = "/api/portal/auth/sso/callback";

#[derive(Debug, Deserialize)]
pub struct SaveProviderRequest {
    /// `entra`, `google` or (self-hosted only) `oidc`.
    pub kind: String,
    pub display_name: Option<String>,
    /// Entra: the directory (tenant) id or a verified domain.
    pub tenant_id: Option<String>,
    /// Generic OIDC (self-hosted): the issuer URL.
    pub issuer_url: Option<String>,
    pub client_id: String,
    /// Omit to keep the stored secret.
    pub client_secret: Option<String>,
    pub allowed_domains: Vec<String>,
    pub enabled: bool,
}

/// The issuer for a preset, built server-side so a hosted tenant admin never
/// points the server at an arbitrary URL.
fn issuer_for(body: &SaveProviderRequest) -> Result<String, ApiError> {
    match body.kind.as_str() {
        "entra" => {
            let tenant = body
                .tenant_id
                .as_deref()
                .map(str::trim)
                .filter(|t| !t.is_empty())
                .ok_or_else(|| {
                    ApiError::BadRequest("Enter your Microsoft Entra tenant ID".into())
                })?;
            // The multi-tenant endpoints would accept accounts from any tenant.
            if matches!(
                tenant.to_lowercase().as_str(),
                "common" | "organizations" | "consumers"
            ) {
                return Err(ApiError::BadRequest(
                    "Use your own tenant ID, not a multi-tenant endpoint".into(),
                ));
            }
            if !tenant
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.')
            {
                return Err(ApiError::BadRequest(
                    "That tenant ID doesn't look right".into(),
                ));
            }
            Ok(format!("https://login.microsoftonline.com/{tenant}/v2.0"))
        }
        "google" => Ok("https://accounts.google.com".into()),
        "oidc" if !crate::middleware::workspace_context::is_hosted() => body
            .issuer_url
            .as_deref()
            .map(|u| u.trim().trim_end_matches('/').to_string())
            .filter(|u| u.starts_with("https://") || u.starts_with("http://"))
            .ok_or_else(|| ApiError::BadRequest("Enter the provider's issuer URL".into())),
        _ => Err(ApiError::BadRequest("Unsupported provider".into())),
    }
}

/// Lowercased, de-duplicated domains; free-mail domains are refused (they'd let
/// anyone with a personal account in).
fn clean_domains(domains: &[String]) -> Result<Vec<String>, ApiError> {
    let mut out: Vec<String> = Vec::new();
    for raw in domains {
        let d = raw.trim().trim_start_matches('@').to_lowercase();
        if d.is_empty() {
            continue;
        }
        let valid = d.contains('.')
            && d.len() <= 253
            && d.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.');
        if !valid {
            return Err(ApiError::BadRequest(format!("{d} isn't a domain")));
        }
        if crate::utils::free_mail::is_free_mail(&d) {
            return Err(ApiError::BadRequest(format!(
                "{d} is a personal email provider; list your organisation's own domains"
            )));
        }
        if !out.contains(&d) {
            out.push(d);
        }
    }
    if out.is_empty() {
        return Err(ApiError::BadRequest(
            "List at least one email domain people sign in with".into(),
        ));
    }
    Ok(out)
}

fn default_name(kind: &str) -> &'static str {
    match kind {
        "entra" => "Microsoft",
        "google" => "Google",
        _ => "Single sign-on",
    }
}

fn internal(e: impl std::fmt::Debug, what: &str) -> ApiError {
    tracing::error!(error = ?e, "requester sso: {what} failed");
    ApiError::Internal(format!("Failed to {what}"))
}

pub async fn get_provider(
    mut tc: TenantConn,
    req: HttpRequest,
    ws: WorkspaceContext,
) -> Result<HttpResponse, ApiError> {
    require_workspace_role(&req, WorkspaceRole::Admin)?;
    let workspace_id = ws.workspace_id;
    let (provider, origin) = tc
        .run(|conn| {
            let provider = providers::get(conn)?;
            let origin = crate::utils::portal_ticket_link::portal_origin(conn, workspace_id);
            Ok::<_, diesel::result::Error>((provider, origin))
        })
        .map_err(|e| internal(e, "load the sign-in provider"))?;
    Ok(HttpResponse::Ok().json(json!({
        "provider": provider.as_ref().map(IdentityProviderView::from),
        "redirect_uri": origin.map(|o| format!("{o}{CALLBACK_PATH}")),
        "generic_oidc_allowed": !crate::middleware::workspace_context::is_hosted(),
    })))
}

pub async fn save_provider(
    mut tc: TenantConn,
    req: HttpRequest,
    ws: WorkspaceContext,
    body: web::Json<SaveProviderRequest>,
) -> Result<HttpResponse, ApiError> {
    require_workspace_role(&req, WorkspaceRole::Admin)?;
    let body = body.into_inner();
    let issuer = issuer_for(&body)?;
    let domains = clean_domains(&body.allowed_domains)?;
    let client_id = body.client_id.trim().to_string();
    if client_id.is_empty() {
        return Err(ApiError::BadRequest("Enter the client ID".into()));
    }
    let secret = body
        .client_secret
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let display_name = body
        .display_name
        .as_deref()
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .unwrap_or(default_name(&body.kind))
        .chars()
        .take(80)
        .collect::<String>();
    // Turning it on means requesters will be sent there: make sure it answers.
    if body.enabled {
        crate::oidc::check_discovery(&issuer)
            .await
            .map_err(|e| ApiError::BadRequest(format!("The provider didn't respond: {e}")))?;
    }
    let workspace_id = ws.workspace_id;
    let saved = tc
        .run(|conn| {
            if secret.is_none()
                && providers::get(conn)?.is_none_or(|p| p.encrypted_client_secret.is_none())
            {
                return Ok(Err(ApiError::BadRequest("Enter the client secret".into())));
            }
            let row = providers::save(
                conn,
                workspace_id,
                &ProviderInput {
                    kind: &body.kind,
                    display_name: &display_name,
                    issuer_url: &issuer,
                    client_id: &client_id,
                    client_secret: secret.as_deref(),
                    allowed_domains: &domains,
                    enabled: body.enabled,
                },
            )
            .map_err(|e| match e {
                crate::repository::channels::CredentialError::Db(e) => e,
                other => {
                    tracing::error!(error = ?other, "requester sso: sealing the secret failed");
                    diesel::result::Error::RollbackTransaction
                }
            })?;
            Ok::<_, diesel::result::Error>(Ok(row))
        })
        .map_err(|e| internal(e, "save the sign-in provider"))??;
    Ok(HttpResponse::Ok().json(json!({ "provider": IdentityProviderView::from(&saved) })))
}

pub async fn delete_provider(
    mut tc: TenantConn,
    req: HttpRequest,
) -> Result<HttpResponse, ApiError> {
    require_workspace_role(&req, WorkspaceRole::Admin)?;
    tc.run(providers::delete)
        .map_err(|e| internal(e, "remove the sign-in provider"))?;
    Ok(HttpResponse::NoContent().finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(kind: &str, tenant: Option<&str>) -> SaveProviderRequest {
        SaveProviderRequest {
            kind: kind.into(),
            display_name: None,
            tenant_id: tenant.map(str::to_string),
            issuer_url: None,
            client_id: "c".into(),
            client_secret: None,
            allowed_domains: vec![],
            enabled: false,
        }
    }

    #[test]
    fn issuers_are_built_from_presets_and_refuse_multi_tenant_endpoints() {
        assert_eq!(
            issuer_for(&req("entra", Some("acme.onmicrosoft.com"))).unwrap(),
            "https://login.microsoftonline.com/acme.onmicrosoft.com/v2.0"
        );
        assert!(issuer_for(&req("entra", Some("common"))).is_err());
        assert!(issuer_for(&req("entra", Some("organizations"))).is_err());
        assert!(issuer_for(&req("entra", Some("x/../evil"))).is_err());
        assert!(issuer_for(&req("entra", None)).is_err());
        assert_eq!(
            issuer_for(&req("google", None)).unwrap(),
            "https://accounts.google.com"
        );
    }

    #[test]
    fn domains_are_cleaned_and_free_mail_refused() {
        assert_eq!(
            clean_domains(&["@Acme.com ".into(), "acme.com".into(), "it.acme.com".into()]).unwrap(),
            vec!["acme.com".to_string(), "it.acme.com".to_string()]
        );
        assert!(clean_domains(&["gmail.com".into()]).is_err());
        assert!(clean_domains(&["not a domain".into()]).is_err());
        assert!(clean_domains(&[]).is_err());
    }
}
