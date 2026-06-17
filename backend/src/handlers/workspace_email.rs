//! Admin endpoints for a workspace's verified sending domain.
//!
//! Workspace-admin gated. The flow: PUT a From identity, the backend provisions
//! a per-domain DKIM keypair and returns the DNS record to publish; the admin
//! publishes it and hits verify; once verified, outbound mail sends from the
//! workspace's domain (DKIM-signed via the instance relay) and a test-send is
//! allowed. DELETE reverts to the instance fallback identity.

use std::sync::Arc;

use actix_web::{web, HttpRequest, HttpResponse, Responder};
use serde::{Deserialize, Serialize};

use crate::db::Pool;
use crate::extractors::TenantConn;
use crate::handlers::errors;
use crate::models::{
    workspace_email_sending_mode, workspace_email_verification_status, Claims,
    UpsertWorkspaceEmailSettings, WorkspaceEmailSettings, WorkspaceRole,
};
use crate::repository::{user_helpers, workspace_email_settings as ws_settings};
use crate::services::dkim_verification;
use crate::services::outbound_email::OutboundEmailResolver;
use crate::services::ses_identity;
use crate::sync::session::run_in_workspace;
use crate::utils::rbac;

fn require_admin(req: &HttpRequest) -> Result<Claims, HttpResponse> {
    rbac::require_workspace_role(req, WorkspaceRole::Admin)
}

#[derive(Serialize)]
struct DkimRecordDto {
    name: String,
    txt_value: String,
}

#[derive(Serialize)]
struct OutboundSettingsResponse {
    sending_mode: String,
    from_name: String,
    from_email: String,
    sending_domain: Option<String>,
    verification_status: String,
    verified_at: Option<chrono::NaiveDateTime>,
    /// The DKIM record to publish (verified_domain mode only).
    dkim_record: Option<DkimRecordDto>,
}

impl OutboundSettingsResponse {
    fn from_row(row: &WorkspaceEmailSettings, record: Option<ws_settings::DkimDnsRecord>) -> Self {
        Self {
            sending_mode: row.sending_mode.clone(),
            from_name: row.from_name.clone(),
            from_email: row.from_email.clone(),
            sending_domain: row.sending_domain.clone(),
            verification_status: row.verification_status.clone(),
            verified_at: row.verified_at,
            dkim_record: record.map(|r| DkimRecordDto {
                name: r.name,
                txt_value: r.txt_value,
            }),
        }
    }

    fn unconfigured() -> Self {
        Self {
            sending_mode: workspace_email_sending_mode::FALLBACK.to_string(),
            from_name: String::new(),
            from_email: String::new(),
            sending_domain: None,
            verification_status: workspace_email_verification_status::UNVERIFIED.to_string(),
            verified_at: None,
            dkim_record: None,
        }
    }
}

/// GET /admin/email/outbound
pub async fn get_outbound(mut tc: TenantConn, req: HttpRequest) -> impl Responder {
    if let Err(resp) = require_admin(&req) {
        return resp;
    }
    let loaded = tc.run(|conn| {
        let row = ws_settings::get(conn)?;
        let record = match &row {
            Some(r) => ws_settings::dns_record_for(r)
                .map_err(|e| diesel::result::Error::QueryBuilderError(e.to_string().into()))?,
            None => None,
        };
        Ok::<_, diesel::result::Error>((row, record))
    });
    match loaded {
        Ok((Some(row), record)) => {
            HttpResponse::Ok().json(OutboundSettingsResponse::from_row(&row, record))
        }
        Ok((None, _)) => HttpResponse::Ok().json(OutboundSettingsResponse::unconfigured()),
        Err(e) => errors::internal(format!("load outbound settings: {e}")),
    }
}

#[derive(Deserialize)]
pub struct SetDomainRequest {
    from_name: String,
    from_email: String,
}

/// Register the workspace's verified sending domain with SES (BYODKIM, our key),
/// idempotently. No-op when SES identity management is unconfigured (self-host)
/// or the workspace has no verified-domain key. Reads + decrypts the key under a
/// workspace-pinned blocking connection, then makes the async SES call.
///
/// Both `set_domain` (initial setup) and `verify_domain` (before flipping to
/// verified) call this, so "verified" can never diverge from "registered in
/// SES": if `set_domain`'s registration fails after the key is stored, the next
/// verify re-ensures it before the status can advance.
async fn ensure_ses_registration(pool: &Pool, workspace_id: i32) -> Result<(), String> {
    let ses = match ses_identity::SesIdentityManager::from_env() {
        Ok(Some(s)) => s,
        Ok(None) => return Ok(()), // self-host / no SES identity management
        Err(e) => return Err(e.to_string()),
    };

    let pool = pool.clone();
    let material = tokio::task::spawn_blocking(move || {
        run_in_workspace(&pool, "ses-ensure", workspace_id, |conn| {
            let Some(row) = ws_settings::get(conn)? else {
                return Ok(None);
            };
            if row.sending_mode != workspace_email_sending_mode::VERIFIED_DOMAIN {
                return Ok(None);
            }
            let (Some(domain), Some(selector)) =
                (row.sending_domain.clone(), row.dkim_selector.clone())
            else {
                return Ok(None);
            };
            match ws_settings::decrypt_dkim_key(&row) {
                Ok(Some(pem)) => Ok(Some((domain, selector, pem))),
                Ok(None) => Ok(None),
                Err(e) => Err(diesel::result::Error::QueryBuilderError(
                    e.to_string().into(),
                )),
            }
        })
    })
    .await
    .map_err(|e| format!("SES ensure task: {e}"))?
    .map_err(|e| format!("SES ensure read: {e}"))?;

    let Some((domain, selector, pem)) = material else {
        return Ok(());
    };
    ses.register_sending_domain(&domain, &selector, &pem)
        .await
        .map_err(|e| e.to_string())
}

/// PUT /admin/email/outbound/domain — set the verified-domain identity and
/// provision a DKIM keypair. The sending domain is the From address's domain.
/// Returns the DNS record to publish.
pub async fn set_domain(
    tc: TenantConn,
    req: HttpRequest,
    pool: web::Data<Pool>,
    body: web::Json<SetDomainRequest>,
) -> impl Responder {
    if let Err(resp) = require_admin(&req) {
        return resp;
    }
    let Some(workspace_id) = tc.workspace_id() else {
        return errors::bad_request("no workspace context");
    };

    let from_email = body.from_email.trim().to_string();
    let parts: Vec<&str> = from_email.split('@').collect();
    if parts.len() != 2 || parts[0].is_empty() || parts[1].is_empty() || !parts[1].contains('.') {
        return errors::bad_request("from_email is not a valid email address");
    }
    let domain = parts[1].to_ascii_lowercase();

    let fields = UpsertWorkspaceEmailSettings {
        enabled: true,
        from_name: body.from_name.trim().to_string(),
        from_email: from_email.clone(),
        smtp_host: String::new(),
        smtp_port: 587,
        smtp_security: "starttls".to_string(),
        smtp_username: String::new(),
        sending_mode: workspace_email_sending_mode::VERIFIED_DOMAIN.to_string(),
    };

    // RSA keygen is CPU-bound, so run the upsert + provision on a blocking
    // thread, atomically in one workspace-pinned transaction.
    let pool_for_provision = pool.get_ref().clone();
    let domain_for_provision = domain.clone();
    let provision = tokio::task::spawn_blocking(move || {
        run_in_workspace(
            &pool_for_provision,
            "dkim-provision",
            workspace_id,
            |conn| {
                ws_settings::upsert(conn, fields)?;
                ws_settings::provision_dkim(conn, workspace_id, &domain_for_provision)
                    .map_err(|e| diesel::result::Error::QueryBuilderError(e.to_string().into()))
            },
        )
    })
    .await;

    let record = match provision {
        Ok(Ok(r)) => r,
        Ok(Err(e)) => return errors::internal(format!("provision DKIM: {e}")),
        Err(e) => return errors::internal(format!("provision task: {e}")),
    };

    // Hosted: authorise the From domain in SES so sends from it aren't rejected.
    // Self-host leaves SES unconfigured and this is a no-op. Fatal on failure so
    // the admin retries rather than publishing a record that can't send; the key
    // is already stored, and verify_domain re-ensures registration before the
    // status can advance.
    if let Err(e) = ensure_ses_registration(pool.get_ref(), workspace_id).await {
        return errors::internal(format!("register {domain} with SES: {e}"));
    }

    HttpResponse::Ok().json(serde_json::json!({
        "dkim_record": { "name": record.name, "txt_value": record.txt_value },
        "verification_status": workspace_email_verification_status::PENDING,
    }))
}

/// POST /admin/email/outbound/verify — check the published DKIM record.
pub async fn verify_domain(
    tc: TenantConn,
    req: HttpRequest,
    pool: web::Data<Pool>,
) -> impl Responder {
    if let Err(resp) = require_admin(&req) {
        return resp;
    }
    let Some(workspace_id) = tc.workspace_id() else {
        return errors::bad_request("no workspace context");
    };

    // Re-ensure SES knows this domain before the DNS check can flip us to
    // verified, so a verified status always implies the domain is registered for
    // sending (closes the gap where set_domain stored the key but its SES call
    // failed). Idempotent; no-op off-SES.
    if let Err(e) = ensure_ses_registration(pool.get_ref(), workspace_id).await {
        return errors::internal(format!("ensure SES registration: {e}"));
    }

    match dkim_verification::verify_dkim_domain(pool.get_ref(), workspace_id).await {
        Ok(status) => HttpResponse::Ok().json(serde_json::json!({ "verification_status": status })),
        Err(dkim_verification::VerifyError::NotProvisioned) => {
            errors::bad_request("no verified domain configured")
        }
        Err(e) => errors::internal(format!("verify domain: {e}")),
    }
}

/// GET /admin/email/outbound/dns-check — live SPF/DKIM/DMARC/MX readout for the
/// workspace's sending domain, so the admin can self-diagnose deliverability.
/// Read-only; does not change verification status.
pub async fn dns_check(mut tc: TenantConn, req: HttpRequest) -> impl Responder {
    if let Err(resp) = require_admin(&req) {
        return resp;
    }

    let loaded = tc.run(|conn| {
        let row = ws_settings::get(conn)?;
        let record = match &row {
            Some(r) => ws_settings::dns_record_for(r)
                .map_err(|e| diesel::result::Error::QueryBuilderError(e.to_string().into()))?,
            None => None,
        };
        Ok::<_, diesel::result::Error>((row, record))
    });
    let (row, record) = match loaded {
        Ok(v) => v,
        Err(e) => return errors::internal(format!("load sending domain: {e}")),
    };

    let (Some(row), Some(record)) = (row, record) else {
        return errors::bad_request("no sending domain configured");
    };
    let Some(domain) = row.sending_domain else {
        return errors::bad_request("no sending domain configured");
    };

    let report = crate::services::dns_diagnostics::check_email_auth(
        &domain,
        &record.name,
        &record.public_b64,
    )
    .await;
    HttpResponse::Ok().json(report)
}

/// POST /admin/email/outbound/test — send a test email from the verified
/// identity to the requesting admin's own address.
pub async fn test_send(
    mut tc: TenantConn,
    req: HttpRequest,
    resolver: web::Data<Arc<OutboundEmailResolver>>,
) -> impl Responder {
    let claims = match require_admin(&req) {
        Ok(c) => c,
        Err(resp) => return resp,
    };
    let Some(workspace_id) = tc.workspace_id() else {
        return errors::bad_request("no workspace context");
    };
    let user_uuid = match uuid::Uuid::parse_str(&claims.sub) {
        Ok(u) => u,
        Err(_) => return errors::bad_request("invalid user id"),
    };

    let prep = tc.run(|conn| {
        let row = ws_settings::get(conn)?;
        let email = user_helpers::get_primary_email(&user_uuid, conn);
        Ok::<_, diesel::result::Error>((row, email))
    });
    let (row, recipient) = match prep {
        Ok(v) => v,
        Err(e) => return errors::internal(format!("test prep: {e}")),
    };
    let Some(recipient) = recipient else {
        return errors::bad_request("your account has no email address");
    };

    let verified = matches!(&row, Some(r)
        if r.sending_mode == workspace_email_sending_mode::VERIFIED_DOMAIN
            && r.verification_status == workspace_email_verification_status::VERIFIED);
    if !verified {
        return errors::bad_request("verify your sending domain before sending a test");
    }

    let svc = match resolver.resolve_owned(workspace_id) {
        Ok(s) => s,
        Err(e) => return errors::internal(format!("resolve sender: {e}")),
    };
    let branding = crate::utils::email::EmailBranding::default();
    match svc.send_test_email(&recipient, &branding).await {
        Ok(_) => HttpResponse::Ok().json(serde_json::json!({ "status": "sent", "to": recipient })),
        Err(e) => errors::internal(format!("send test: {e}")),
    }
}

/// DELETE /admin/email/outbound — revert to the instance fallback identity.
pub async fn reset(mut tc: TenantConn, req: HttpRequest) -> impl Responder {
    if let Err(resp) = require_admin(&req) {
        return resp;
    }
    let Some(workspace_id) = tc.workspace_id() else {
        return errors::bad_request("no workspace context");
    };

    // Read the domain before clearing it, so we can deregister the SES identity.
    let cleared = tc.run(|conn| {
        let domain = ws_settings::get(conn)?.and_then(|r| r.sending_domain);
        ws_settings::reset_to_fallback(conn, workspace_id)?;
        Ok::<_, diesel::result::Error>(domain)
    });
    let domain = match cleared {
        Ok(d) => d,
        Err(e) => return errors::internal(format!("reset: {e}")),
    };

    // Best-effort SES cleanup: the workspace is already back on fallback, so a
    // lingering identity is unused. Don't fail the reset on an SES hiccup.
    if let Some(domain) = domain {
        match ses_identity::SesIdentityManager::from_env() {
            Ok(Some(ses)) => {
                if let Err(e) = ses.deregister_sending_domain(&domain).await {
                    tracing::warn!("SES deregister for {domain} failed (left for cleanup): {e}");
                }
            }
            Ok(None) => {}
            Err(e) => tracing::warn!("SES config while deregistering {domain}: {e}"),
        }
    }

    HttpResponse::Ok().json(serde_json::json!({ "status": "reset" }))
}
