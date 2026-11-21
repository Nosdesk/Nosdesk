//! `/api/admin/asset-settings`: workspace-wide asset choices: what
//! requester-role members may see of the workspace's assets, and the
//! workspace currency.

use actix_web::{web, HttpRequest, HttpResponse};
use serde::{Deserialize, Serialize};

use crate::errors::ApiError;
use crate::extractors::{AuthContext, TenantConn};
use crate::models::{RequesterAssetVisibility, SiteSettings, UpdateSiteSettings, WorkspaceRole};
use crate::utils::money;
use crate::utils::rbac::require_workspace_role;

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.route("/admin/asset-settings", web::get().to(get_settings))
        .route("/admin/asset-settings", web::put().to(save_settings));
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct AssetSettings {
    /// What requesters see of the workspace's assets: `all` or `none`.
    pub requester_visibility: String,
    /// The workspace currency, an ISO 4217 code money fields offer by
    /// default; `None` until an admin sets one.
    pub currency: Option<String>,
    /// The currency the default locale suggests while `currency` is unset.
    pub suggested_currency: Option<String>,
    /// The codes `currency` may be set to.
    pub currencies: Vec<String>,
}

impl From<&SiteSettings> for AssetSettings {
    fn from(s: &SiteSettings) -> Self {
        Self {
            requester_visibility: s.requester_asset_visibility.clone(),
            currency: s.currency_code.clone(),
            suggested_currency: money::suggested_currency(&s.default_locale).map(str::to_string),
            currencies: money::current_currencies()
                .map(|c| c.code.to_string())
                .collect(),
        }
    }
}

/// A change to the settings: a field left out keeps its value, and
/// `"currency": null` clears the workspace currency.
#[derive(Debug, Default, Deserialize)]
pub struct AssetSettingsUpdate {
    #[serde(default)]
    pub requester_visibility: Option<String>,
    #[serde(default, deserialize_with = "crate::utils::double_option::deserialize")]
    pub currency: Option<Option<String>>,
}

/// The update as columns, or why it's refused. A currency code is read
/// without regard to case and must be one a workspace can pick.
fn validate(s: AssetSettingsUpdate) -> Result<(Option<String>, Option<Option<String>>), ApiError> {
    if let Some(v) = &s.requester_visibility {
        if RequesterAssetVisibility::parse(v).is_none() {
            return Err(ApiError::BadRequest(
                "Unknown requester asset visibility".into(),
            ));
        }
    }
    let currency = match s.currency {
        Some(Some(code)) => {
            let code = code.trim().to_ascii_uppercase();
            match money::currency(&code) {
                Some(c) if !c.withdrawn => Some(Some(code)),
                _ => {
                    return Err(ApiError::BadRequest(format!(
                        "'{code}' is not a currency a workspace can use; use a current ISO 4217 code such as AUD"
                    )))
                }
            }
        }
        other => other,
    };
    Ok((s.requester_visibility, currency))
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

/// Requesters' clients pick a visibility change up from the
/// `assets_visible` flag on their next sync poll, and re-read their snapshot
/// to drop or bring back the assets.
pub async fn save_settings(
    mut tc: TenantConn,
    req: HttpRequest,
    auth: AuthContext,
    body: web::Json<AssetSettingsUpdate>,
) -> Result<HttpResponse, ApiError> {
    require_workspace_role(&req, WorkspaceRole::Admin)?;
    let (requester_asset_visibility, currency_code) = validate(body.into_inner())?;
    let update = UpdateSiteSettings {
        requester_asset_visibility,
        currency_code,
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

    fn update(json: serde_json::Value) -> AssetSettingsUpdate {
        serde_json::from_value(json).expect("update parses")
    }

    #[test]
    fn only_known_visibilities_are_accepted() {
        let v = |v: &str| validate(update(serde_json::json!({ "requester_visibility": v })));
        assert!(v("all").is_ok());
        assert!(v("none").is_ok());
        assert!(v("groups").is_err());
        assert!(v("").is_err());
    }

    #[test]
    fn currencies_are_current_iso_codes_and_left_out_fields_stay() {
        let c = |json| validate(update(json)).map(|(_, currency)| currency);
        assert_eq!(
            c(serde_json::json!({ "currency": "aud" })).unwrap(),
            Some(Some("AUD".to_string()))
        );
        assert_eq!(
            c(serde_json::json!({ "currency": null })).unwrap(),
            Some(None)
        );
        assert_eq!(c(serde_json::json!({})).unwrap(), None);
        assert!(c(serde_json::json!({ "currency": "ZZZ" })).is_err());
        assert!(c(serde_json::json!({ "currency": "HRK" })).is_err());
        assert!(c(serde_json::json!({ "currency": "" })).is_err());
        assert_eq!(
            validate(update(serde_json::json!({ "currency": "EUR" })))
                .unwrap()
                .0,
            None
        );
    }
}
