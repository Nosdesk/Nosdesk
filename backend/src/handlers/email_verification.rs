//! Redeem an address-confirmation link.
//!
//! Public and unauthenticated on purpose: the link is clicked from a mail
//! client, which may not be the browser holding the session, and requiring a
//! login first would mean a user who cannot receive mail at their signed-in
//! address can never confirm a second one. Possession of the token is the
//! proof; nothing here reveals anything to someone who does not hold one.

use actix_web::{web, HttpRequest, HttpResponse, Responder, ResponseError};
use serde::Deserialize;
use serde_json::json;
use tracing::{info, warn};

use crate::handlers::errors;
use crate::handlers::helpers;
use crate::utils::reset_tokens::TokenType;

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.route("/verify-email", web::post().to(verify_email));
}

#[derive(Debug, Deserialize)]
pub struct VerifyEmailRequest {
    pub token: String,
}

/// `POST /api/public/verify-email`
///
/// Claiming the token and verifying the row are separate steps, and the claim
/// comes first: it is a single atomic UPDATE guarded on `is_used = false`, so
/// two clicks on the same link cannot both proceed.
pub async fn verify_email(
    db_pool: web::Data<crate::db::Pool>,
    _req: HttpRequest,
    body: web::Json<VerifyEmailRequest>,
) -> impl Responder {
    let mut conn = match helpers::db_conn(&db_pool) {
        Ok(c) => c,
        Err(e) => return e.error_response(),
    };

    let (user_uuid, metadata) =
        match crate::repository::reset_tokens::validate_and_consume_token_with_metadata(
            &mut conn,
            &body.token,
            TokenType::EmailVerification.as_str(),
        ) {
            Ok(claimed) => claimed,
            Err(_) => {
                // Expired, already used, wrong type, or never existed. One message
                // for all of them: telling them apart tells a token-guesser which
                // of their guesses was once real.
                return errors::bad_request("This confirmation link is invalid or has expired");
            }
        };

    let Some(email_id) = metadata
        .as_ref()
        .and_then(|m| m.get("user_email_id"))
        .and_then(serde_json::Value::as_i64)
    else {
        warn!(%user_uuid, "email verification: token carries no user_email_id");
        return errors::bad_request("This confirmation link is invalid or has expired");
    };
    let Some(address) = metadata
        .as_ref()
        .and_then(|m| m.get("address"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
    else {
        warn!(%user_uuid, "email verification: token carries no address");
        return errors::bad_request("This confirmation link is invalid or has expired");
    };

    match crate::repository::user_emails::mark_verified_if_matches(
        &mut conn,
        email_id as i32,
        user_uuid,
        &address,
    ) {
        Ok(1) => {
            info!(%user_uuid, email_id, "Email address confirmed");
            HttpResponse::Ok().json(json!({ "status": "verified", "email": address }))
        }
        // The row was deleted, reassigned, or now holds a different address.
        // The token was still spent, which is correct: it was valid once and a
        // link is not a standing permission to verify whatever that row later
        // becomes.
        Ok(_) => {
            warn!(%user_uuid, email_id, "email verification: row no longer matches the token");
            errors::bad_request("That address is no longer on this account")
        }
        Err(e) => {
            warn!(%user_uuid, email_id, error = ?e, "email verification: update failed");
            errors::internal("Could not confirm this address")
        }
    }
}
