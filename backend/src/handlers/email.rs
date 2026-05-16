use actix_web::{web, HttpMessage, HttpRequest, HttpResponse, Responder};
use serde::Deserialize;
use serde_json::json;

use crate::db::Pool;
use crate::handlers::errors;
use crate::handlers::helpers;
use crate::utils::email::{EmailConfig, EmailService};
use crate::utils::email_branding::get_email_branding;

/// Test email request
#[derive(Deserialize)]
pub struct TestEmailRequest {
    pub to: String,
}

/// Get email configuration status (admin only, read-only)
pub async fn get_email_config(db_pool: web::Data<Pool>, req: HttpRequest) -> impl Responder {
    // Get database connection
    let _conn = match helpers::db_conn(&db_pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    // Extract claims from cookie auth middleware
    let claims = match req.extensions().get::<crate::models::Claims>() {
        Some(claims) => claims.clone(),
        None => return errors::unauthorized("Authentication required"),
    };

    // Check if the user is an admin
    if claims.role != "admin" {
        return errors::forbidden("Only administrators can view email configuration");
    }

    // Load email configuration from environment
    match EmailConfig::from_env() {
        Ok(config) => {
            // Return configuration (without password)
            HttpResponse::Ok().json(json!({
                "smtp_host": config.smtp_host,
                "smtp_port": config.smtp_port,
                "smtp_username": config.smtp_username,
                "smtp_password_configured": !config.smtp_password.is_empty(),
                "from_name": config.from_name,
                "from_email": config.from_email,
                "enabled": config.enabled,
                "is_configured": config.is_configured()
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
    db_pool: web::Data<Pool>,
    req: HttpRequest,
    request: web::Json<TestEmailRequest>,
) -> impl Responder {
    // Get database connection
    let mut conn = match helpers::db_conn(&db_pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    // Extract claims from cookie auth middleware
    let claims = match req.extensions().get::<crate::models::Claims>() {
        Some(claims) => claims.clone(),
        None => return errors::unauthorized("Authentication required"),
    };

    // Check if the user is an admin
    if claims.role != "admin" {
        return errors::forbidden("Only administrators can send test emails");
    }

    // Create email service
    let email_service = match EmailService::from_env() {
        Ok(service) => service,
        Err(e) => return errors::bad_request(format!("Email is not configured: {}", e)),
    };

    // Get branding for test email
    let base_url =
        std::env::var("FRONTEND_URL").unwrap_or_else(|_| "http://localhost:3000".to_string());
    let branding = get_email_branding(&mut conn, &base_url);

    // Send test email
    match email_service.send_test_email(&request.to, &branding).await {
        Ok(_) => HttpResponse::Ok().json(json!({
            "status": "success",
            "message": format!("Test email sent successfully to {}", request.to)
        })),
        Err(e) => errors::internal(format!("Failed to send test email: {}", e)),
    }
}
