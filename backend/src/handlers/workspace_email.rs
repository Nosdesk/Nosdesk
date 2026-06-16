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

    // RSA keygen is CPU-bound, so run the whole upsert + provision on a
    // blocking thread, atomically in one workspace-pinned transaction.
    let pool = pool.get_ref().clone();
    let provision = tokio::task::spawn_blocking(move || {
        run_in_workspace(&pool, "dkim-provision", workspace_id, |conn| {
            ws_settings::upsert(conn, fields)?;
            ws_settings::provision_dkim(conn, workspace_id, &domain)
                .map_err(|e| diesel::result::Error::QueryBuilderError(e.to_string().into()))
        })
    })
    .await;

    match provision {
        Ok(Ok(record)) => HttpResponse::Ok().json(serde_json::json!({
            "dkim_record": { "name": record.name, "txt_value": record.txt_value },
            "verification_status": workspace_email_verification_status::PENDING,
        })),
        Ok(Err(e)) => errors::internal(format!("provision DKIM: {e}")),
        Err(e) => errors::internal(format!("provision task: {e}")),
    }
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
    match dkim_verification::verify_dkim_domain(pool.get_ref(), workspace_id).await {
        Ok(status) => HttpResponse::Ok().json(serde_json::json!({ "verification_status": status })),
        Err(dkim_verification::VerifyError::NotProvisioned) => {
            errors::bad_request("no verified domain configured")
        }
        Err(e) => errors::internal(format!("verify domain: {e}")),
    }
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
    match tc.run(|conn| ws_settings::reset_to_fallback(conn, workspace_id)) {
        Ok(()) => HttpResponse::Ok().json(serde_json::json!({ "status": "reset" })),
        Err(e) => errors::internal(format!("reset: {e}")),
    }
}
