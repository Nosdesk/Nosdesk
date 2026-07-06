use actix_web::{web, HttpMessage, HttpRequest, HttpResponse, Responder};
use serde::Deserialize;
use serde_json::json;

use crate::extractors::TenantConn;
use crate::handlers::errors;
use crate::utils::email::EmailService;
use crate::utils::email_branding::get_email_branding;

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.route(
        "/admin/email/config",
        web::get().to(crate::handlers::email::get_email_config),
    )
    .route(
        "/admin/email/test",
        web::post().to(crate::handlers::email::send_test_email),
    )
    // Per-workspace verified sending domain (DKIM)
    .route(
        "/admin/email/outbound",
        web::get().to(crate::handlers::workspace_email::get_outbound),
    )
    .route(
        "/admin/email/outbound",
        web::delete().to(crate::handlers::workspace_email::reset),
    )
    .route(
        "/admin/email/outbound/domain",
        web::put().to(crate::handlers::workspace_email::set_domain),
    )
    .route(
        "/admin/email/outbound/verify",
        web::post().to(crate::handlers::workspace_email::verify_domain),
    )
    .route(
        "/admin/email/outbound/dns-check",
        web::get().to(crate::handlers::workspace_email::dns_check),
    )
    .route(
        "/admin/email/outbound/test",
        web::post().to(crate::handlers::workspace_email::test_send),
    );
}

/// Test email request
#[derive(Deserialize)]
pub struct TestEmailRequest {
    pub to: String,
}

/// Get email configuration status (admin only, read-only)
pub async fn get_email_config(
    mut tc: TenantConn,
    req: HttpRequest,
    resolver: web::Data<std::sync::Arc<crate::services::outbound_email::OutboundEmailResolver>>,
) -> impl Responder {
    // Extract claims from cookie auth middleware
    let claims = match req.extensions().get::<crate::models::Claims>() {
        Some(claims) => claims.clone(),
        None => return errors::unauthorized("Authentication required"),
    };

    if !crate::utils::rbac::is_platform_admin(&claims) {
        return errors::forbidden("Only administrators can view email configuration");
    }

    // SMTP transport. EmailService::from_env reports is_configured for the
    // active transport.
    match EmailService::from_env() {
        Ok(service) => {
            let config = service.config();
            // Hosted: outbound is delivered by Nosdesk-managed infra. The
            // platform SMTP relay (host + username) is operator/infra config, and
            // the username is an SES IAM credential, so it must not surface in the
            // product admin UI. Report the workspace's EFFECTIVE identity — the
            // resolver's answer, the same one the queue worker sends with —
            // not the platform relay's From (which tenant mail never uses).
            let managed = crate::middleware::DeploymentMode::current()
                == crate::middleware::DeploymentMode::Hosted;
            if managed {
                let workspace_id = tc.workspace_id();
                // Mode label: what kind of identity the workspace resolves to.
                let mode = workspace_id
                    .and_then(|id| {
                        tc.run(|conn| {
                            crate::repository::workspace_email_settings::get_for_workspace(conn, id)
                        })
                        .ok()
                        .flatten()
                    })
                    .filter(|r| r.enabled)
                    .and_then(|r| match r.sending_mode.as_str() {
                        crate::models::workspace_email_sending_mode::SMTP_RELAY
                            if !r.smtp_host.trim().is_empty() =>
                        {
                            Some("smtp_relay")
                        }
                        crate::models::workspace_email_sending_mode::VERIFIED_DOMAIN
                            if r.verification_status
                                == crate::models::workspace_email_verification_status::VERIFIED =>
                        {
                            Some("verified_domain")
                        }
                        _ => None,
                    })
                    .unwrap_or(if crate::utils::tenant_origin::tenant_domain().is_some() {
                        "managed"
                    } else {
                        "platform"
                    });
                // Effective From: the resolver's answer for this workspace.
                let effective = workspace_id.and_then(|id| resolver.resolve_owned(id).ok());
                let (from_name, from_email, configured) = match &effective {
                    Some(svc) => (
                        svc.config().from_name.clone(),
                        svc.config().from_email.clone(),
                        true,
                    ),
                    // Unresolvable (deferring) workspace: no sending identity.
                    None => (String::new(), String::new(), false),
                };
                return HttpResponse::Ok().json(json!({
                    "managed": true,
                    "mode": mode,
                    "provider": service.provider_name(),
                    "from_name": from_name,
                    "from_email": from_email,
                    "enabled": config.enabled,
                    "is_configured": service.is_configured() && configured,
                }));
            }
            // Self-host: the operator configured this relay; show what it points
            // at (host/port/from) so they can verify it. Never echo `smtp_username`
            // back, it's a credential identifier and adds no value over the
            // configured/host fields.
            HttpResponse::Ok().json(json!({
                "managed": false,
                "provider": service.provider_name(),
                "from_name": config.from_name,
                "from_email": config.from_email,
                "enabled": config.enabled,
                "is_configured": service.is_configured(),
                "smtp_host": config.smtp_host,
                "smtp_port": config.smtp_port,
                "smtp_password_configured": !config.smtp_password.is_empty(),
            }))
        }
        Err(e) => HttpResponse::Ok().json(json!({
            "enabled": false,
            "is_configured": false,
            "error": e
        })),
    }
}

/// Send a test email (admin only)
pub async fn send_test_email(
    mut tc: TenantConn,
    req: HttpRequest,
    request: web::Json<TestEmailRequest>,
) -> impl Responder {
    // Extract claims from cookie auth middleware
    let claims = match req.extensions().get::<crate::models::Claims>() {
        Some(claims) => claims.clone(),
        None => return errors::unauthorized("Authentication required"),
    };

    if !crate::utils::rbac::is_platform_admin(&claims) {
        return errors::forbidden("Only administrators can send test emails");
    }

    // Create email service
    let email_service = match EmailService::from_env() {
        Ok(service) => service,
        Err(e) => return errors::bad_request(format!("Email is not configured: {}", e)),
    };

    // Get branding for test email. site_settings is workspace-scoped,
    // so the lookup rides on TenantConn's RLS-primed transaction. The link host
    // is this workspace's canonical origin (the test send targets the current
    // workspace), then FRONTEND_URL, then a local default.
    let ws_origin = req
        .extensions()
        .get::<crate::extractors::WorkspaceContext>()
        .and_then(|ws| ws.canonical_origin());
    let base_url = crate::utils::tenant_origin::email_link_base(ws_origin)
        .unwrap_or_else(|| "http://localhost:3000".to_string());
    let branding =
        match tc.run(|conn| Ok::<_, diesel::result::Error>(get_email_branding(conn, &base_url))) {
            Ok(b) => b,
            Err(e) => return errors::internal(format!("Failed to load email branding: {}", e)),
        };

    // Send test email
    match email_service.send_test_email(&request.to, &branding).await {
        Ok(_) => HttpResponse::Ok().json(json!({
            "status": "success",
            "message": format!("Test email sent successfully to {}", request.to)
        })),
        Err(e) => errors::internal(format!("Failed to send test email: {}", e)),
    }
}
