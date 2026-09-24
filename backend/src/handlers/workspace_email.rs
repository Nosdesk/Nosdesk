//! Admin endpoints for how a workspace sends mail: the server default, a
//! verified sending domain, or the workspace's own SMTP server.
//!
//! Workspace-admin gated. Verified domain: PUT a From identity, the backend
//! provisions a per-domain DKIM keypair and returns the DNS record to publish;
//! the admin publishes it and hits verify. Own SMTP server: PUT the relay
//! settings (checked for a port/security pair that cannot connect), test them
//! before saving, remove the stored password. DELETE reverts to the server
//! default. Tests always go to the requesting admin's own address.

use std::sync::Arc;

use actix_web::{web, HttpRequest, HttpResponse};
use serde::{Deserialize, Serialize};

use crate::db::Pool;
use crate::errors::ApiError;
use crate::extractors::TenantConn;
use crate::models::{
    workspace_email_sending_mode, workspace_email_verification_status, Claims,
    UpsertWorkspaceEmailSettings, WorkspaceEmailSettings, WorkspaceRole,
};
use crate::repository::{user_helpers, workspace_email_settings as ws_settings};
use crate::services::dkim_verification;
use crate::services::outbound_email::OutboundEmailResolver;
use crate::services::ses_identity;
use crate::sync::session::run_in_workspace;
use crate::utils::email::{
    check_port_security, EmailConfig, EmailService, SmtpCoherence, SmtpError, SmtpSecurity,
};
use crate::utils::rbac;

fn require_admin(req: &HttpRequest) -> Result<Claims, ApiError> {
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
    /// The workspace's own SMTP server, kept across mode changes so switching
    /// away and back does not lose it. Never includes the password.
    smtp_host: String,
    smtp_port: i32,
    smtp_security: String,
    smtp_username: String,
    password_configured: bool,
    /// Whether the saved port and security suit each other.
    port_security: PortSecurityDto,
}

#[derive(Serialize)]
struct PortSecurityDto {
    /// `ok`, `warn` or `error`.
    level: &'static str,
    message: Option<String>,
}

fn port_security_of(port: i32, security: &str) -> PortSecurityDto {
    let Some(security) = parse_security(security) else {
        return PortSecurityDto {
            level: "error",
            message: Some("Unknown security mode".into()),
        };
    };
    let Ok(port) = u16::try_from(port) else {
        return PortSecurityDto {
            level: "error",
            message: Some("Port must be between 1 and 65535".into()),
        };
    };
    match check_port_security(port, security) {
        SmtpCoherence::Ok => PortSecurityDto {
            level: "ok",
            message: None,
        },
        SmtpCoherence::Warn(m) => PortSecurityDto {
            level: "warn",
            message: Some(m),
        },
        SmtpCoherence::Error(m) => PortSecurityDto {
            level: "error",
            message: Some(m),
        },
    }
}

fn parse_security(s: &str) -> Option<SmtpSecurity> {
    match s {
        "tls" => Some(SmtpSecurity::Tls),
        "starttls" => Some(SmtpSecurity::StartTls),
        "plaintext" => Some(SmtpSecurity::Plaintext),
        _ => None,
    }
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
            smtp_host: row.smtp_host.clone(),
            smtp_port: row.smtp_port,
            smtp_security: row.smtp_security.clone(),
            smtp_username: row.smtp_username.clone(),
            password_configured: row.encrypted_smtp_password.is_some(),
            port_security: port_security_of(row.smtp_port, &row.smtp_security),
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
            smtp_host: String::new(),
            smtp_port: 587,
            smtp_security: "starttls".into(),
            smtp_username: String::new(),
            password_configured: false,
            port_security: port_security_of(587, "starttls"),
        }
    }
}

/// GET /admin/email/outbound
pub async fn get_outbound(mut tc: TenantConn, req: HttpRequest) -> Result<HttpResponse, ApiError> {
    require_admin(&req)?;
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
            Ok(HttpResponse::Ok().json(OutboundSettingsResponse::from_row(&row, record)))
        }
        Ok((None, _)) => Ok(HttpResponse::Ok().json(OutboundSettingsResponse::unconfigured())),
        Err(e) => Err(ApiError::Internal(format!("load outbound settings: {e}"))),
    }
}

#[derive(Deserialize)]
pub struct SetModeRequest {
    mode: String,
}

/// PUT /admin/email/outbound/mode — send with an identity that is already
/// saved: the server default, the set-up domain, or the saved SMTP server.
/// Nothing is cleared, so switching back is free. Removing a domain is
/// `DELETE /admin/email/outbound`.
pub async fn set_mode(
    mut tc: TenantConn,
    req: HttpRequest,
    body: web::Json<SetModeRequest>,
) -> Result<HttpResponse, ApiError> {
    require_admin(&req)?;
    let Some(workspace_id) = tc.workspace_id() else {
        return Err(ApiError::BadRequest("no workspace context".into()));
    };
    let mode = body.into_inner().mode;
    let loaded = tc.run(|conn| ws_settings::get(conn));
    let row = match loaded {
        Ok(r) => r,
        Err(e) => return Err(ApiError::Internal(format!("load outbound settings: {e}"))),
    };

    match mode.as_str() {
        workspace_email_sending_mode::FALLBACK => {
            // Nothing saved yet means the workspace already uses the default.
            if row.is_none() {
                return Ok(HttpResponse::Ok().json(OutboundSettingsResponse::unconfigured()));
            }
        }
        workspace_email_sending_mode::VERIFIED_DOMAIN => {
            let Some(domain) = row.as_ref().and_then(|r| r.sending_domain.clone()) else {
                return Ok(bad("Set up a sending domain first.", "MODE_NOT_SET_UP"));
            };
            // The From address is shared by every identity; a domain can only
            // sign mail sent from itself.
            let from_domain = row.as_ref().and_then(|r| {
                r.from_email
                    .rsplit_once('@')
                    .map(|(_, d)| d.to_ascii_lowercase())
            });
            if from_domain.as_deref() != Some(domain.as_str()) {
                return Ok(bad(
                    "The From address is no longer on the set-up domain. Set the domain up again.",
                    "MODE_DOMAIN_MISMATCH",
                ));
            }
        }
        workspace_email_sending_mode::SMTP_RELAY => {
            if row.as_ref().is_none_or(|r| r.smtp_host.trim().is_empty()) {
                return Ok(bad("Save an SMTP server first.", "MODE_NOT_SET_UP"));
            }
        }
        _ => return Ok(bad("Unknown sending mode.", "MODE_INVALID")),
    }

    let saved = tc.run(|conn| {
        ws_settings::set_sending_mode(conn, workspace_id, &mode)?;
        let row = ws_settings::get(conn)?;
        let record = match &row {
            Some(r) => ws_settings::dns_record_for(r)
                .map_err(|e| diesel::result::Error::QueryBuilderError(e.to_string().into()))?,
            None => None,
        };
        Ok::<_, diesel::result::Error>((row, record))
    });
    match saved {
        Ok((Some(row), record)) => {
            Ok(HttpResponse::Ok().json(OutboundSettingsResponse::from_row(&row, record)))
        }
        Ok((None, _)) => Ok(HttpResponse::Ok().json(OutboundSettingsResponse::unconfigured())),
        Err(e) => Err(ApiError::Internal(format!("switch sending mode: {e}"))),
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
async fn ensure_ses_registration(
    pool: &Pool,
    workspace_id: i32,
    update_existing: bool,
) -> Result<(), String> {
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
    ses.register_sending_domain(&domain, &selector, &pem, update_existing)
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
) -> Result<HttpResponse, ApiError> {
    require_admin(&req)?;
    let Some(workspace_id) = tc.workspace_id() else {
        return Err(ApiError::BadRequest("no workspace context".into()));
    };

    let from_email = body.from_email.trim().to_string();
    let parts: Vec<&str> = from_email.split('@').collect();
    if parts.len() != 2 || parts[0].is_empty() || parts[1].is_empty() || !parts[1].contains('.') {
        return Err(ApiError::BadRequest(
            "from_email is not a valid email address".into(),
        ));
    }
    let domain = parts[1].to_ascii_lowercase();

    // Platform-owned namespaces can't be verified as a tenant sending domain:
    // DNS would fail the DKIM check anyway (the records live in our zone),
    // but reject early and explicitly — a tenant must never even attempt to
    // claim `<other-slug>.<tenant_domain>` or the token inbound host.
    let platform_owned = [
        crate::utils::tenant_origin::tenant_domain(),
        std::env::var("NOSDESK_INBOUND_DOMAIN")
            .ok()
            .filter(|d| !d.is_empty()),
    ];
    for owned in platform_owned.iter().flatten() {
        let owned = owned.to_ascii_lowercase();
        if domain == owned || domain.ends_with(&format!(".{owned}")) {
            return Err(ApiError::BadRequest(
                "This domain is managed by the platform and cannot be used as a sending domain"
                    .into(),
            ));
        }
    }

    let from_name = body.from_name.trim().to_string();
    let from_email_for_row = from_email.clone();

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
                // Keep a saved SMTP server across the switch, so choosing
                // "your domain" and later going back does not lose it.
                let existing = ws_settings::get(conn)?;
                let fields = UpsertWorkspaceEmailSettings {
                    enabled: true,
                    from_name,
                    from_email: from_email_for_row,
                    smtp_host: existing
                        .as_ref()
                        .map(|r| r.smtp_host.clone())
                        .unwrap_or_default(),
                    smtp_port: existing.as_ref().map(|r| r.smtp_port).unwrap_or(587),
                    smtp_security: existing
                        .as_ref()
                        .map(|r| r.smtp_security.clone())
                        .unwrap_or_else(|| "starttls".to_string()),
                    smtp_username: existing
                        .as_ref()
                        .map(|r| r.smtp_username.clone())
                        .unwrap_or_default(),
                    sending_mode: workspace_email_sending_mode::VERIFIED_DOMAIN.to_string(),
                };
                ws_settings::upsert(conn, fields)?;
                ws_settings::provision_dkim(conn, workspace_id, &domain_for_provision)
                    .map_err(|e| diesel::result::Error::QueryBuilderError(e.to_string().into()))
            },
        )
    })
    .await;

    let record = match provision {
        Ok(Ok(r)) => r,
        Ok(Err(e)) => return Err(ApiError::Internal(format!("provision DKIM: {e}"))),
        Err(e) => return Err(ApiError::Internal(format!("provision task: {e}"))),
    };

    // Hosted: authorise the From domain in SES so sends from it aren't rejected.
    // Self-host leaves SES unconfigured and this is a no-op. Fatal on failure so
    // the admin retries rather than publishing a record that can't send; the key
    // is already stored, and verify_domain re-ensures registration before the
    // status can advance.
    // set_domain just (re)generated the DKIM key, so push it (update_existing).
    if let Err(e) = ensure_ses_registration(pool.get_ref(), workspace_id, true).await {
        return Err(ApiError::Internal(format!(
            "register {domain} with SES: {e}"
        )));
    }

    Ok(HttpResponse::Ok().json(serde_json::json!({
        "dkim_record": { "name": record.name, "txt_value": record.txt_value },
        "verification_status": workspace_email_verification_status::PENDING,
    })))
}

/// POST /admin/email/outbound/verify — check the published DKIM record.
pub async fn verify_domain(
    tc: TenantConn,
    req: HttpRequest,
    pool: web::Data<Pool>,
) -> Result<HttpResponse, ApiError> {
    require_admin(&req)?;
    let Some(workspace_id) = tc.workspace_id() else {
        return Err(ApiError::BadRequest("no workspace context".into()));
    };

    // Re-ensure SES knows this domain before the DNS check can flip us to
    // verified, so a verified status always implies the domain is registered for
    // sending (closes the gap where set_domain stored the key but its SES call
    // failed). Idempotent; no-op off-SES.
    // Verify only ensures the identity EXISTS (creates if missing); it must not
    // re-apply an unchanged key, or every customer "Verify" click makes SES email
    // a "DKIM setup successful" notice. The key is only changed via set_domain.
    if let Err(e) = ensure_ses_registration(pool.get_ref(), workspace_id, false).await {
        return Err(ApiError::Internal(format!("ensure SES registration: {e}")));
    }

    match dkim_verification::verify_dkim_domain(pool.get_ref(), workspace_id).await {
        Ok(status) => {
            Ok(HttpResponse::Ok().json(serde_json::json!({ "verification_status": status })))
        }
        Err(dkim_verification::VerifyError::NotProvisioned) => {
            Err(ApiError::BadRequest("no verified domain configured".into()))
        }
        Err(e) => Err(ApiError::Internal(format!("verify domain: {e}"))),
    }
}

/// GET /admin/email/outbound/dns-check — live SPF/DKIM/DMARC/MX readout for the
/// workspace's sending domain, so the admin can self-diagnose deliverability.
/// Read-only; does not change verification status.
pub async fn dns_check(mut tc: TenantConn, req: HttpRequest) -> Result<HttpResponse, ApiError> {
    require_admin(&req)?;

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
        Err(e) => return Err(ApiError::Internal(format!("load sending domain: {e}"))),
    };

    let Some(row) = row else {
        return Err(ApiError::BadRequest("no sending domain configured".into()));
    };
    // Own SMTP server: check the From domain. The provider signs (or not), so
    // there is no key of ours to look for.
    if row.sending_mode == workspace_email_sending_mode::SMTP_RELAY {
        let Some((_, domain)) = row.from_email.rsplit_once('@') else {
            return Err(ApiError::BadRequest("no From address configured".into()));
        };
        let report =
            crate::services::dns_diagnostics::check_email_auth(&domain.to_ascii_lowercase(), None)
                .await;
        return Ok(HttpResponse::Ok().json(report));
    }
    let (Some(domain), Some(record)) = (row.sending_domain, record) else {
        return Err(ApiError::BadRequest("no sending domain configured".into()));
    };

    let report = crate::services::dns_diagnostics::check_email_auth(
        &domain,
        Some((&record.name, &record.public_b64)),
    )
    .await;
    Ok(HttpResponse::Ok().json(report))
}

// ---------------------------------------------------------------------------
// Own SMTP server (`smtp_relay`)
// ---------------------------------------------------------------------------

/// The relay form, saved or tested. `password` is write-only: omitted or
/// blank keeps the stored one.
#[derive(Deserialize)]
pub struct RelayRequest {
    from_name: String,
    from_email: String,
    smtp_host: String,
    smtp_port: i32,
    smtp_security: String,
    #[serde(default)]
    smtp_username: String,
    #[serde(default)]
    password: Option<String>,
}

/// A relay form that passed validation.
struct ValidRelay {
    from_name: String,
    from_email: String,
    host: String,
    port: u16,
    security: SmtpSecurity,
    security_raw: String,
    username: String,
    password: Option<String>,
}

fn bad(message: &str, code: &str) -> HttpResponse {
    crate::errors::bad_request_with_code(message, code)
}

/// Why a relay form was refused: the sentence and the code the form maps to
/// the field at fault.
struct RelayInvalid {
    message: String,
    code: &'static str,
}

impl RelayInvalid {
    fn new(message: impl Into<String>, code: &'static str) -> Self {
        Self {
            message: message.into(),
            code,
        }
    }

    fn response(self) -> HttpResponse {
        bad(&self.message, self.code)
    }
}

/// Check a relay form.
fn validate_relay(body: RelayRequest) -> Result<ValidRelay, RelayInvalid> {
    let host = body.smtp_host.trim().to_ascii_lowercase();
    // A bare hostname or IP: no scheme, path, port or spaces.
    let host_ok = !host.is_empty()
        && host.len() <= 253
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':' | '[' | ']'))
        && !host.contains("://");
    if !host_ok {
        return Err(RelayInvalid::new(
            "Enter the SMTP server's host name, like smtp.example.com",
            "RELAY_HOST_INVALID",
        ));
    }
    let Ok(port) = u16::try_from(body.smtp_port) else {
        return Err(RelayInvalid::new(
            "Port must be between 1 and 65535",
            "RELAY_PORT_INVALID",
        ));
    };
    if port == 0 {
        return Err(RelayInvalid::new(
            "Port must be between 1 and 65535",
            "RELAY_PORT_INVALID",
        ));
    }
    let Some(security) = parse_security(&body.smtp_security) else {
        return Err(RelayInvalid::new(
            "Unknown security mode",
            "RELAY_SECURITY_INVALID",
        ));
    };
    if let SmtpCoherence::Error(message) = check_port_security(port, security) {
        return Err(RelayInvalid::new(message, "SMTP_CONFIG_MISMATCH"));
    }
    let from_email = body.from_email.trim().to_string();
    let valid_from = from_email
        .parse::<lettre::Address>()
        .is_ok_and(|a| a.domain().contains('.'));
    if !valid_from {
        return Err(RelayInvalid::new(
            "Enter a valid From address",
            "FROM_EMAIL_INVALID",
        ));
    }
    Ok(ValidRelay {
        from_name: body.from_name.trim().to_string(),
        from_email,
        host,
        port,
        security,
        security_raw: body.smtp_security,
        username: body.smtp_username.trim().to_string(),
        password: body.password.filter(|p| !p.is_empty()),
    })
}

/// PUT /admin/email/outbound/relay — save the workspace's own SMTP server and
/// send through it from now on.
pub async fn put_relay(
    mut tc: TenantConn,
    req: HttpRequest,
    body: web::Json<RelayRequest>,
) -> Result<HttpResponse, ApiError> {
    require_admin(&req)?;
    let Some(workspace_id) = tc.workspace_id() else {
        return Err(ApiError::BadRequest("no workspace context".into()));
    };
    let relay = match validate_relay(body.into_inner()) {
        Ok(r) => r,
        Err(invalid) => return Ok(invalid.response()),
    };
    let fields = UpsertWorkspaceEmailSettings {
        enabled: true,
        from_name: relay.from_name,
        from_email: relay.from_email,
        smtp_host: relay.host,
        smtp_port: i32::from(relay.port),
        smtp_security: relay.security_raw,
        smtp_username: relay.username,
        sending_mode: workspace_email_sending_mode::SMTP_RELAY.to_string(),
    };
    let password = relay.password;
    let saved = tc.run(|conn| {
        ws_settings::upsert(conn, fields)?;
        if let Some(pw) = &password {
            ws_settings::set_password(conn, workspace_id, pw)
                .map_err(|e| diesel::result::Error::QueryBuilderError(e.to_string().into()))?;
        }
        ws_settings::get(conn)
    });
    match saved {
        Ok(Some(row)) => {
            Ok(HttpResponse::Ok().json(OutboundSettingsResponse::from_row(&row, None)))
        }
        Ok(None) => Err(ApiError::Internal("relay saved but not readable".into())),
        Err(e) => Err(ApiError::Internal(format!("save relay: {e}"))),
    }
}

/// DELETE /admin/email/outbound/relay/password — forget the stored password.
/// The relay stays; with no password it sends unauthenticated.
pub async fn delete_relay_password(
    mut tc: TenantConn,
    req: HttpRequest,
) -> Result<HttpResponse, ApiError> {
    require_admin(&req)?;
    let result = tc.run(|conn| {
        ws_settings::clear_password(conn)?;
        ws_settings::get(conn)
    });
    match result {
        Ok(Some(row)) => {
            Ok(HttpResponse::Ok().json(OutboundSettingsResponse::from_row(&row, None)))
        }
        Ok(None) => Ok(HttpResponse::Ok().json(OutboundSettingsResponse::unconfigured())),
        Err(e) => Err(ApiError::Internal(format!("clear relay password: {e}"))),
    }
}

/// How long a test waits per SMTP step: long enough for a slow provider,
/// short enough that the form answers.
const TEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
/// Tests per admin per window. They send real mail, if only to the admin.
const TEST_LIMIT: u32 = 10;
const TEST_WINDOW_SECS: u64 = 600;

/// The result of a test send, for the form to show. `code` is a closed set
/// the UI maps to a sentence and a fix; `detail` is the server's own words.
#[derive(Serialize)]
struct TestResult {
    ok: bool,
    /// Where the test went (the admin's own address).
    to: String,
    code: Option<&'static str>,
    detail: Option<String>,
}

/// Which step of sending failed, from the transport's typed error.
fn classify(e: &SmtpError) -> &'static str {
    use crate::utils::egress::EgressError;
    match e {
        SmtpError::NotConfigured => "incomplete",
        SmtpError::Build(_) => "invalid",
        SmtpError::Egress(EgressError::DnsLookup { .. } | EgressError::NoAddresses { .. }) => "dns",
        SmtpError::Egress(_) => "egress_blocked",
        SmtpError::Smtp {
            code: Some(530 | 534 | 535 | 538),
            ..
        } => "auth",
        SmtpError::Smtp { code: Some(_), .. } => "rejected",
        SmtpError::Smtp { message, .. } => {
            let m = message.to_ascii_lowercase();
            if m.contains("timed out") || m.contains("timeout") {
                "timeout"
            } else if m.contains("tls") || m.contains("certificate") || m.contains("handshake") {
                "tls"
            } else if m.contains("authentication") || m.contains("credentials") {
                "auth"
            } else {
                "connect"
            }
        }
    }
}

async fn send_test(svc: &EmailService, to: String) -> TestResult {
    let branding = crate::utils::email::EmailBranding::default();
    match svc.send_test(&to, &branding).await {
        Ok(()) => TestResult {
            ok: true,
            to,
            code: None,
            detail: None,
        },
        Err(e) => {
            let code = classify(&e);
            tracing::info!(error_kind = code, "email test send failed");
            TestResult {
                ok: false,
                to,
                code: Some(code),
                detail: Some(e.to_string()),
            }
        }
    }
}

/// Throttle tests per admin. Fails open on a Redis error: a test mails only
/// the caller, so availability wins.
async fn test_allowed(user: &uuid::Uuid) -> bool {
    let key = format!("smtp_test:{user}");
    let redis = crate::utils::rate_limit::get_redis_url();
    match crate::utils::rate_limit::RateLimiter::check_rate_limit(
        &redis,
        &key,
        TEST_LIMIT,
        TEST_WINDOW_SECS,
    )
    .await
    {
        Ok(allowed) => allowed,
        Err(e) => {
            tracing::warn!(error = %e, "smtp test rate limit unavailable; allowing");
            true
        }
    }
}

fn caller_uuid(claims: &Claims) -> Result<uuid::Uuid, ApiError> {
    uuid::Uuid::parse_str(&claims.sub).map_err(|_| ApiError::BadRequest("invalid user id".into()))
}

/// POST /admin/email/outbound/relay/test — try an SMTP server before saving
/// it, by sending a test to the requesting admin.
///
/// A blank password reuses the stored one only when the host and username are
/// the saved ones. Otherwise any workspace admin could point the host at a
/// server they control and have this send them the stored password.
pub async fn test_relay(
    mut tc: TenantConn,
    req: HttpRequest,
    body: web::Json<RelayRequest>,
) -> Result<HttpResponse, ApiError> {
    let claims = require_admin(&req)?;
    let user = caller_uuid(&claims)?;
    let relay = match validate_relay(body.into_inner()) {
        Ok(r) => r,
        Err(invalid) => return Ok(invalid.response()),
    };
    if !test_allowed(&user).await {
        return Ok(crate::errors::with_fields(
            actix_web::http::StatusCode::TOO_MANY_REQUESTS,
            "RATE_LIMITED",
            "Too many tests. Wait a few minutes and try again.",
            serde_json::json!({}),
        ));
    }

    let host = relay.host.clone();
    let username = relay.username.clone();
    let loaded = tc.run(move |conn| {
        let row = ws_settings::get(conn)?;
        let recipient = user_helpers::get_primary_email(&user, conn);
        let stored = match &row {
            Some(r) if r.smtp_host == host && r.smtp_username == username => {
                ws_settings::decrypt_password(r)
                    .map_err(|e| diesel::result::Error::QueryBuilderError(e.to_string().into()))?
            }
            _ => None,
        };
        Ok::<_, diesel::result::Error>((recipient, stored))
    });
    let (recipient, stored_password) = match loaded {
        Ok(v) => v,
        Err(e) => return Err(ApiError::Internal(format!("relay test prep: {e}"))),
    };
    let Some(recipient) = recipient else {
        return Err(ApiError::BadRequest(
            "your account has no email address".into(),
        ));
    };
    let password = match (relay.password, stored_password) {
        (Some(pw), _) => pw,
        (None, Some(stored)) => stored,
        (None, None) if !relay.username.is_empty() => {
            return Ok(bad(
                "Enter the password to test this server",
                "RELAY_PASSWORD_REQUIRED",
            ));
        }
        (None, None) => String::new(),
    };

    let config = EmailConfig {
        smtp_host: relay.host,
        smtp_port: relay.port,
        smtp_username: relay.username,
        smtp_password: password,
        from_name: relay.from_name,
        from_email: relay.from_email,
        enabled: true,
        security: relay.security,
    };
    let svc = EmailService::new_untrusted_relay_with_timeout(config, TEST_TIMEOUT);
    Ok(HttpResponse::Ok().json(send_test(&svc, recipient).await))
}

/// POST /admin/email/outbound/test — send a test through whatever this
/// workspace sends with right now (its own server, its verified domain, or
/// the server default) to the requesting admin's own address.
pub async fn test_send(
    mut tc: TenantConn,
    req: HttpRequest,
    resolver: web::Data<Arc<OutboundEmailResolver>>,
) -> Result<HttpResponse, ApiError> {
    let claims = require_admin(&req)?;
    let Some(workspace_id) = tc.workspace_id() else {
        return Err(ApiError::BadRequest("no workspace context".into()));
    };
    let user = caller_uuid(&claims)?;
    if !test_allowed(&user).await {
        return Ok(crate::errors::with_fields(
            actix_web::http::StatusCode::TOO_MANY_REQUESTS,
            "RATE_LIMITED",
            "Too many tests. Wait a few minutes and try again.",
            serde_json::json!({}),
        ));
    }
    let recipient = match tc
        .run(|conn| Ok::<_, diesel::result::Error>(user_helpers::get_primary_email(&user, conn)))
    {
        Ok(Some(r)) => r,
        Ok(None) => {
            return Err(ApiError::BadRequest(
                "your account has no email address".into(),
            ))
        }
        Err(e) => return Err(ApiError::Internal(format!("test prep: {e}"))),
    };
    let svc = match resolver.resolve_owned(workspace_id) {
        Ok(s) => s,
        Err(e) => {
            return Ok(HttpResponse::Ok().json(TestResult {
                ok: false,
                to: recipient,
                code: Some("incomplete"),
                detail: Some(e.to_string()),
            }))
        }
    };
    Ok(HttpResponse::Ok().json(send_test(&svc, recipient).await))
}

/// DELETE /admin/email/outbound — revert to the instance fallback identity.
pub async fn reset(mut tc: TenantConn, req: HttpRequest) -> Result<HttpResponse, ApiError> {
    require_admin(&req)?;
    let Some(workspace_id) = tc.workspace_id() else {
        return Err(ApiError::BadRequest("no workspace context".into()));
    };

    // Read the domain before clearing it, so we can deregister the SES identity.
    let cleared = tc.run(|conn| {
        let domain = ws_settings::get(conn)?.and_then(|r| r.sending_domain);
        ws_settings::reset_to_fallback(conn, workspace_id)?;
        Ok::<_, diesel::result::Error>(domain)
    });
    let domain = match cleared {
        Ok(d) => d,
        Err(e) => return Err(ApiError::Internal(format!("reset: {e}"))),
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

    Ok(HttpResponse::Ok().json(serde_json::json!({ "status": "reset" })))
}
