//! Public one-click email unsubscribe (B2 / RFC 8058).
//!
//! Linked from the `List-Unsubscribe` header on notification mail. No auth: the
//! signed token in the query names the user, so the endpoint is safe to expose
//! publicly. POST is the RFC 8058 one-click form (the mail client submits
//! `List-Unsubscribe=One-Click` automatically); GET is the human following the
//! link in a browser. Both turn off the email channel for every notification
//! type for that user; transactional mail (password reset, invitation) is
//! unaffected.

use actix_web::{web, HttpResponse, Responder};
use serde::Deserialize;

use crate::services::notifications::NotificationService;
use crate::utils::unsubscribe_token;

#[derive(Deserialize)]
pub struct UnsubscribeQuery {
    pub token: String,
}

/// Verify the token and apply the opt-out, or return the response to send.
async fn apply(
    token: &str,
    notification_service: &web::Data<NotificationService>,
) -> Result<(), HttpResponse> {
    // Don't distinguish "bad signature" from "unknown user": a public endpoint
    // shouldn't confirm whether a given token or user is valid.
    let user_uuid = unsubscribe_token::verify(token).ok_or_else(|| {
        HttpResponse::BadRequest().body("This unsubscribe link is invalid or has expired.")
    })?;
    notification_service
        .preferences()
        .disable_all_email(&user_uuid)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "unsubscribe: failed to disable email notifications");
            HttpResponse::InternalServerError().body("Could not process the unsubscribe request.")
        })?;
    Ok(())
}

/// RFC 8058 one-click POST. The mail client sends this when the user taps its
/// built-in unsubscribe affordance; the request body is ignored.
pub async fn one_click(
    query: web::Query<UnsubscribeQuery>,
    notification_service: web::Data<NotificationService>,
) -> impl Responder {
    match apply(&query.token, &notification_service).await {
        Ok(()) => HttpResponse::Ok().finish(),
        Err(resp) => resp,
    }
}

/// Human-facing GET: someone followed the link in their mail client. Same
/// effect, with a plain confirmation page.
pub async fn landing(
    query: web::Query<UnsubscribeQuery>,
    notification_service: web::Data<NotificationService>,
) -> impl Responder {
    match apply(&query.token, &notification_service).await {
        Ok(()) => HttpResponse::Ok()
            .content_type("text/html; charset=utf-8")
            .body(CONFIRM_HTML),
        Err(resp) => resp,
    }
}

const CONFIRM_HTML: &str = "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">\
<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
<title>Unsubscribed</title></head>\
<body style=\"font-family:system-ui,sans-serif;max-width:32rem;margin:4rem auto;padding:0 1rem\">\
<h1>You're unsubscribed</h1>\
<p>You will no longer receive email notifications. You can re-enable them anytime \
in your notification settings.</p></body></html>";
