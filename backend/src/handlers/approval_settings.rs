//! `/api/admin/approval-settings`: how approvals behave in this workspace.
//! Which request types need approval, and from whom, is set per type on the
//! categories page; these are the workspace-wide choices around it.

use actix_web::{web, HttpRequest, HttpResponse};
use serde::{Deserialize, Serialize};

use crate::errors::ApiError;
use crate::extractors::{AuthContext, TenantConn};
use crate::models::{SiteSettings, UpdateSiteSettings, WorkspaceRole};
use crate::utils::rbac::require_workspace_role;

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.route("/admin/approval-settings", web::get().to(get_settings))
        .route("/admin/approval-settings", web::put().to(save_settings));
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct ApprovalSettings {
    /// `badge`: waiting tickets stay in the queues, marked. `held`: they're
    /// kept out of the working queues until approved.
    pub waiting_display: String,
    /// Who may skip a pending approval: `nobody`, `admins` or `agents`.
    pub skip_by: String,
    /// Approve automatically after this many days without an answer.
    pub auto_approve_days: Option<i32>,
}

impl From<&SiteSettings> for ApprovalSettings {
    fn from(s: &SiteSettings) -> Self {
        Self {
            waiting_display: s.approval_waiting_display.clone(),
            skip_by: s.approval_skip_by.clone(),
            auto_approve_days: s.approval_auto_approve_days,
        }
    }
}

fn validate(s: &ApprovalSettings) -> Result<(), ApiError> {
    if !matches!(s.waiting_display.as_str(), "badge" | "held") {
        return Err(ApiError::BadRequest("Unknown waiting display".into()));
    }
    if !matches!(s.skip_by.as_str(), "nobody" | "admins" | "agents") {
        return Err(ApiError::BadRequest("Unknown skip permission".into()));
    }
    if s.auto_approve_days.is_some_and(|d| !(1..=90).contains(&d)) {
        return Err(ApiError::BadRequest(
            "Automatic approval can wait 1 to 90 days".into(),
        ));
    }
    Ok(())
}

fn internal(e: diesel::result::Error, what: &str) -> ApiError {
    tracing::error!(error = ?e, "approval settings: {what} failed");
    ApiError::Internal(format!("Failed to {what}"))
}

pub async fn get_settings(mut tc: TenantConn, req: HttpRequest) -> Result<HttpResponse, ApiError> {
    require_workspace_role(&req, WorkspaceRole::Admin)?;
    let settings = tc
        .run(crate::repository::site_settings::get_site_settings)
        .map_err(|e| internal(e, "load approval settings"))?;
    Ok(HttpResponse::Ok().json(ApprovalSettings::from(&settings)))
}

pub async fn save_settings(
    mut tc: TenantConn,
    req: HttpRequest,
    auth: AuthContext,
    body: web::Json<ApprovalSettings>,
) -> Result<HttpResponse, ApiError> {
    require_workspace_role(&req, WorkspaceRole::Admin)?;
    let body = body.into_inner();
    validate(&body)?;
    let update = UpdateSiteSettings {
        approval_waiting_display: Some(body.waiting_display),
        approval_skip_by: Some(body.skip_by),
        approval_auto_approve_days: Some(body.auto_approve_days),
        updated_by: Some(auth.user_uuid),
        ..Default::default()
    };
    let saved = tc
        .run(|conn| crate::repository::site_settings::update_site_settings(conn, update))
        .map_err(|e| internal(e, "save approval settings"))?;
    Ok(HttpResponse::Ok().json(ApprovalSettings::from(&saved)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(display: &str, skip: &str, days: Option<i32>) -> ApprovalSettings {
        ApprovalSettings {
            waiting_display: display.into(),
            skip_by: skip.into(),
            auto_approve_days: days,
        }
    }

    #[test]
    fn only_known_choices_and_a_bounded_timeout_are_accepted() {
        assert!(validate(&s("badge", "admins", None)).is_ok());
        assert!(validate(&s("held", "agents", Some(3))).is_ok());
        assert!(validate(&s("hidden", "admins", None)).is_err());
        assert!(validate(&s("badge", "everyone", None)).is_err());
        assert!(validate(&s("badge", "nobody", Some(0))).is_err());
        assert!(validate(&s("badge", "nobody", Some(91))).is_err());
    }
}
