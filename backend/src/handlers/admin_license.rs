//! Platform-admin surface for the instance licence and push mode
//! (docs/plans/self-hosted-license-activation.md).
//!
//! Mounted under `/api/admin`. Reads work on every deployment; writes are
//! self-hosted only, since a hosted instance is licensed and provisioned by
//! the control plane. Environment variables win over stored values, and a
//! write that the environment would override is refused rather than stored
//! and silently ignored.

use actix_web::{web, HttpRequest, HttpResponse};
use serde::Deserialize;
use std::sync::Arc;
use uuid::Uuid;

use crate::errors::{self, ApiError};
use crate::extractors::PlatformConn;
use crate::license::{self, InstallError, LicenseSource};
use crate::middleware::DeploymentMode;
use crate::repository::{instance_settings, workspaces};
use crate::services::notifications::channels::push::PushSender;
use crate::services::notifications::channels::push_mode::{self, SwitchablePushSender};
use crate::utils::rbac;
use crate::utils::security_events::{record_security_event, SecurityEventInput};

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.route("/admin/license", web::get().to(get_license))
        .route("/admin/license", web::put().to(put_license))
        .route("/admin/license", web::delete().to(delete_license))
        .route("/admin/push-mode", web::put().to(put_push_mode))
        .route("/admin/push-mode/retry", web::post().to(retry_relay));
}

/// The refusal to return on a hosted deployment, or `None` on self-hosted.
fn hosted_refusal() -> Option<HttpResponse> {
    (DeploymentMode::current() != DeploymentMode::SelfHosted).then(|| {
        errors::conflict_with_code(
            "Licensing is managed by Nosdesk on hosted workspaces",
            "not_self_hosted",
        )
    })
}

fn actor_uuid(claims: &crate::models::Claims) -> Option<Uuid> {
    Uuid::parse_str(&claims.sub).ok()
}

/// Everything the Licence & Cloud page shows, in one read.
pub async fn get_license(
    req: HttpRequest,
    mut pc: PlatformConn,
    push: Option<web::Data<Arc<SwitchablePushSender>>>,
) -> Result<HttpResponse, ApiError> {
    rbac::require_platform_admin(&req)?;

    let state = license::state();
    let edition = state.edition();
    let (active, row, instance_id) = pc.run(|c| {
        Ok((
            workspaces::count_active_workspaces(c).unwrap_or(0),
            instance_settings::get(c).ok().flatten(),
            crate::sync::system_meta::instance_id(c).unwrap_or_default(),
        ))
    })?;
    // Stored timestamps describe the stored licence; with env in charge they
    // would describe one that is not live.
    let stored = row.filter(|_| state.source != LicenseSource::Env);

    let push_json = push.map(|p| {
        let resolved = p.resolved();
        serde_json::json!({
            "mode": resolved.mode.as_str(),
            "source": resolved.source.as_str(),
            "env_managed": push_mode::env_value().is_some(),
            "sender": p.name(),
            "configured": p.is_configured(),
            "native_credentials": push_mode::native_credentials_present(),
            "relay": p.relay_status(),
        })
    });

    Ok(HttpResponse::Ok().json(serde_json::json!({
        "self_hosted": DeploymentMode::current() == DeploymentMode::SelfHosted,
        "edition": edition.name(),
        "max_workspaces": edition.max_workspaces(),
        "active_workspaces": active,
        "can_create_workspace": license::workspace_creation_allowed(&edition, active as u64),
        // Not a secret (the relay already receives it), and the offline path
        // needs it: an operator quotes it when asking for a licence by hand.
        "instance_id": instance_id,
        "license": {
            "source": state.source.as_str(),
            "env_managed": license::env_managed(),
            "error": state.error().map(|e| e.kind()),
            "installed_at": stored.as_ref().and_then(|r| r.license_installed_at),
            "last_refresh_at": stored.as_ref().and_then(|r| r.license_last_refresh_at),
            "last_refresh_error": stored.as_ref().and_then(|r| r.license_last_refresh_error.clone()),
            "details": state.info.as_ref().map(|l| serde_json::json!({
                "customer_id": l.customer_id,
                "licensee": l.licensee,
                "license_id": l.license_id,
                "max_workspaces": l.max_workspaces,
                "expires_at": l.expires_at,
                "features": l.features,
            })),
        },
        "push": push_json,
    })))
}

#[derive(Debug, Deserialize)]
pub struct PutLicenseRequest {
    pub key: String,
}

/// Install a pasted licence. Verified against the compiled-in keys before it
/// is stored, so this cannot grant more than a Nosdesk-signed token does.
pub async fn put_license(
    req: HttpRequest,
    mut pc: PlatformConn,
    body: web::Json<PutLicenseRequest>,
) -> Result<HttpResponse, ApiError> {
    let claims = rbac::require_platform_admin(&req)?;
    if let Some(resp) = hosted_refusal() {
        return Ok(resp);
    }
    let key = body.into_inner().key;
    // Generous for a JWT, small enough that nobody posts a file by mistake.
    if key.trim().is_empty() || key.len() > 8192 {
        return Ok(errors::bad_request_with_code(
            "Paste the licence key from your Nosdesk account",
            "license_malformed",
        ));
    }

    let actor = actor_uuid(&claims);
    let result = pc.run(|c| {
        Ok(
            license::install(c, &key, LicenseSource::Pasted, actor).inspect(|info| {
                let _ = record_security_event(
                    c,
                    SecurityEventInput {
                        user_uuid: actor,
                        event_type: "license_installed",
                        severity: "info",
                        details: Some(serde_json::json!({
                            "source": "pasted",
                            "license_id": info.license_id,
                            "licensee": info.licensee,
                        })),
                        request: Some(&req),
                    },
                );
            }),
        )
    })?;

    match result {
        Ok(_) => {
            reprobe_relay(&req).await;
            get_license_after_write(req, pc).await
        }
        Err(InstallError::EnvManaged) => Ok(env_managed_response()),
        Err(InstallError::Invalid(e)) => Ok(errors::bad_request_with_code(
            "That licence key could not be verified",
            &format!("license_{}", e.kind()),
        )),
        Err(InstallError::Storage(e)) => {
            tracing::error!(error = %e, "storing the licence failed");
            Err(ApiError::Internal("Failed to store the licence".into()))
        }
    }
}

/// After a licence change, have a relay sender exchange once so the page shows
/// whether the relay accepts the new licence.
async fn reprobe_relay(req: &HttpRequest) {
    if let Some(push) = req.app_data::<web::Data<Arc<SwitchablePushSender>>>() {
        push.reset_relay().await;
    }
}

fn env_managed_response() -> HttpResponse {
    errors::conflict_with_code(
        "The licence is set by NOSDESK_LICENSE_KEY; change it there",
        "license_env_managed",
    )
}

/// Re-render the page payload after a write, so the client needs no second
/// round trip and cannot race the reload task.
async fn get_license_after_write(
    req: HttpRequest,
    pc: PlatformConn,
) -> Result<HttpResponse, ApiError> {
    let push = req
        .app_data::<web::Data<Arc<SwitchablePushSender>>>()
        .cloned();
    get_license(req, pc, push).await
}

/// Remove the stored licence; the instance returns to Community.
pub async fn delete_license(
    req: HttpRequest,
    mut pc: PlatformConn,
) -> Result<HttpResponse, ApiError> {
    let claims = rbac::require_platform_admin(&req)?;
    if let Some(resp) = hosted_refusal() {
        return Ok(resp);
    }
    let actor = actor_uuid(&claims);
    let previous = license::state().info.as_ref().map(|i| i.license_id.clone());
    let result = pc.run(|c| {
        Ok(license::remove(c).inspect(|_| {
            let _ = record_security_event(
                c,
                SecurityEventInput {
                    user_uuid: actor,
                    event_type: "license_removed",
                    severity: "warning",
                    details: Some(serde_json::json!({ "license_id": previous })),
                    request: Some(&req),
                },
            );
        }))
    })?;
    match result {
        Ok(()) => {
            reprobe_relay(&req).await;
            get_license_after_write(req, pc).await
        }
        Err(InstallError::EnvManaged) => Ok(env_managed_response()),
        Err(e) => {
            tracing::error!(error = %e, "removing the licence failed");
            Err(ApiError::Internal("Failed to remove the licence".into()))
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct PutPushModeRequest {
    /// `default`, `native`, `relay` or `off`.
    pub mode: String,
}

/// Store and apply the push mode. Applied here as well as by the reload task,
/// so the response already reflects the new sender.
pub async fn put_push_mode(
    req: HttpRequest,
    mut pc: PlatformConn,
    push: web::Data<Arc<SwitchablePushSender>>,
    body: web::Json<PutPushModeRequest>,
) -> Result<HttpResponse, ApiError> {
    let claims = rbac::require_platform_admin(&req)?;
    if let Some(resp) = hosted_refusal() {
        return Ok(resp);
    }
    if push_mode::env_value().is_some() {
        return Ok(errors::conflict_with_code(
            "Push mode is set by NOSDESK_PUSH_MODE; change it there",
            "push_mode_env_managed",
        ));
    }
    let Some(mode) = push_mode::PushMode::parse(&body.mode) else {
        return Ok(errors::bad_request_with_code(
            "Unknown push mode",
            "push_mode_invalid",
        ));
    };
    let stored = (mode != push_mode::PushMode::Default).then(|| mode.as_str());
    let resolved = push_mode::resolve(None, stored)
        .map_err(|e| ApiError::Internal(format!("push mode did not resolve: {e}")))?;

    // Build first: a native mode whose credentials are malformed must fail the
    // request, not be stored and then refused by every replica's reload.
    if let Err(e) = push.apply(resolved) {
        tracing::warn!(error = %e, "push mode switch refused");
        return Ok(errors::bad_request_with_code(
            "Push could not be switched to that mode; check the server log",
            "push_mode_unavailable",
        ));
    }

    let actor = actor_uuid(&claims);
    pc.run(|c| {
        instance_settings::set_push_mode(c, stored)?;
        let _ = record_security_event(
            c,
            SecurityEventInput {
                user_uuid: actor,
                event_type: "push_mode_changed",
                severity: "info",
                details: Some(serde_json::json!({ "mode": mode.as_str() })),
                request: Some(&req),
            },
        );
        Ok(())
    })?;
    if mode == push_mode::PushMode::Relay {
        push.reset_relay().await;
    }
    get_license_after_write(req, pc).await
}

/// Forget a held relay refusal so the next push exchanges again. For after
/// accepting the DPA on the dashboard, when waiting out the retry window is
/// needless.
pub async fn retry_relay(
    req: HttpRequest,
    pc: PlatformConn,
    push: web::Data<Arc<SwitchablePushSender>>,
) -> Result<HttpResponse, ApiError> {
    rbac::require_platform_admin(&req)?;
    push.reset_relay().await;
    get_license_after_write(req, pc).await
}
