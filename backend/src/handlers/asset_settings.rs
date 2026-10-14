//! `/api/admin/asset-settings`: workspace-wide asset choices. For now, what
//! requester-role members may see of the workspace's assets.

use actix_web::{web, HttpRequest, HttpResponse};
use serde::{Deserialize, Serialize};

use crate::errors::ApiError;
use crate::extractors::{AuthContext, TenantConn};
use crate::models::{RequesterAssetVisibility, SiteSettings, UpdateSiteSettings, WorkspaceRole};
use crate::utils::rbac::require_workspace_role;

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.route("/admin/asset-settings", web::get().to(get_settings))
        .route("/admin/asset-settings", web::put().to(save_settings));
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct AssetSettings {
    /// What requesters see of the workspace's assets: `all` or `none`.
    pub requester_visibility: String,
}

impl From<&SiteSettings> for AssetSettings {
    fn from(s: &SiteSettings) -> Self {
        Self {
            requester_visibility: s.requester_asset_visibility.clone(),
        }
    }
}

fn validate(s: &AssetSettings) -> Result<(), ApiError> {
    if RequesterAssetVisibility::parse(&s.requester_visibility).is_none() {
        return Err(ApiError::BadRequest(
            "Unknown requester asset visibility".into(),
        ));
    }
    Ok(())
}

fn internal(e: diesel::result::Error, what: &str) -> ApiError {
    tracing::error!(error = ?e, "asset settings: {what} failed");
    ApiError::Internal(format!("Failed to {what}"))
}

pub async fn get_settings(mut tc: TenantConn, req: HttpRequest) -> Result<HttpResponse, ApiError> {
    require_workspace_role(&req, WorkspaceRole::Admin)?;
    let settings = tc
        .run(crate::repository::site_settings::get_site_settings)
        .map_err(|e| internal(e, "load asset settings"))?;
    Ok(HttpResponse::Ok().json(AssetSettings::from(&settings)))
}

/// Requesters' clients pick a change up from the `assets_visible` flag on
/// their next sync poll, and re-read their snapshot to drop or bring back
/// the assets.
pub async fn save_settings(
    mut tc: TenantConn,
    req: HttpRequest,
    auth: AuthContext,
    body: web::Json<AssetSettings>,
) -> Result<HttpResponse, ApiError> {
    require_workspace_role(&req, WorkspaceRole::Admin)?;
    let body = body.into_inner();
    validate(&body)?;
    let update = UpdateSiteSettings {
        requester_asset_visibility: Some(body.requester_visibility),
        updated_by: Some(auth.user_uuid),
        ..Default::default()
    };
    let saved = tc
        .run(|conn| crate::repository::site_settings::update_site_settings(conn, update))
        .map_err(|e| internal(e, "save asset settings"))?;
    Ok(HttpResponse::Ok().json(AssetSettings::from(&saved)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_known_visibilities_are_accepted() {
        let s = |v: &str| AssetSettings {
            requester_visibility: v.into(),
        };
        assert!(validate(&s("all")).is_ok());
        assert!(validate(&s("none")).is_ok());
        assert!(validate(&s("groups")).is_err());
        assert!(validate(&s("")).is_err());
    }
}
