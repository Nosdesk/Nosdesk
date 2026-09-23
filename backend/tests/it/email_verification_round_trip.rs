//! The confirmation round trip: mint, compose, redeem.
//!
//! Each piece of this flow is unit-tested, but the thing that actually has to
//! hold is that the token minted for a row is the token the email carries and
//! the token redemption accepts. That seam is where a flow like this breaks
//! while every part of it passes, so this test walks it end to end, pulling the
//! token out of the composed email body rather than from the code that made it.
//!
//! SMTP is not involved: `EmailService` is a disabled stub, so this exercises
//! composition and redemption, not delivery.

#![allow(clippy::expect_used)]

use backend::models::NewUserEmail;
use backend::utils::email::EmailBranding;
use backend::utils::email::{EmailConfig, EmailService, SmtpSecurity};
use backend::utils::reset_tokens::{ResetTokenUtils, TokenType};

fn email_service_stub() -> EmailService {
    EmailService::new(EmailConfig {
        smtp_host: String::new(),
        smtp_port: 587,
        smtp_username: String::new(),
        smtp_password: String::new(),
        from_name: String::new(),
        from_email: String::new(),
        enabled: false,
        security: SmtpSecurity::StartTls,
    })
}

fn branding() -> EmailBranding {
    EmailBranding {
        app_name: "Nosdesk".to_string(),
        logo_url: None,
        primary_color: "#000000".to_string(),
        base_url: "https://help.example.com".to_string(),
        ..Default::default()
    }
}

/// Pull the token back out of the link exactly as a mail client would: by
/// reading it off the page, not by remembering what we generated.
fn token_from_body(body: &str) -> String {
    let at = body
        .find("/verify-email?token=")
        .expect("body carries the link");
    body[at + "/verify-email?token=".len()..]
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric())
        .collect()
}

#[test]
fn a_confirmation_link_verifies_the_address_once() {
    crate::common::ensure_test_keyring();
    let db = crate::common::TestDb::new();
    let mut conn = db.conn();

    let user = crate::common::insert_user(&mut conn, "Round Trip");
    let created = backend::repository::user_emails::add_email(
        &mut conn,
        &NewUserEmail {
            user_uuid: user.uuid,
            email: "second@example.com".to_string(),
            email_type: "personal".to_string(),
            is_primary: false,
            is_verified: false,
            source: Some("manual".to_string()),
        },
    )
    .expect("add email");
    assert!(!created.is_verified, "a new address starts unproven");

    // Mint exactly as the send path does, including the metadata that names
    // which row this proves.
    let issued = ResetTokenUtils::create_reset_token(user.uuid, TokenType::EmailVerification);
    backend::repository::reset_tokens::create_reset_token(
        &mut conn,
        &issued.token_hash,
        user.uuid,
        TokenType::EmailVerification.as_str(),
        None,
        None,
        issued.expires_at,
        Some(serde_json::json!({
            "user_email_id": created.id,
            "address": created.email,
        })),
    )
    .expect("mint token");

    // Compose the email and recover the token from the link it contains. If the
    // link were built from a different value, or the token were mangled in the
    // template, this is where it would show.
    let svc = email_service_stub();
    let (_subject, html, body_text) = svc.compose_email_verification(
        &user.name,
        &created.email,
        &issued.raw_token,
        &branding(),
        &"en-US".parse().expect("locale"),
    );
    let emailed_token = token_from_body(&body_text);
    assert_eq!(
        emailed_token, issued.raw_token,
        "the link must carry the token that was minted"
    );

    // The HTML body is the one most people actually click, and it is built by a
    // different path from the text body (the template's CTA href rather than a
    // Fluent string), so checking only the text version would leave the link
    // most recipients use untested.
    assert_eq!(
        token_from_body(&html),
        issued.raw_token,
        "the HTML button must point at the same token as the text link"
    );
    assert!(
        !html.contains("&amp;token=") && html.matches("/verify-email?token=").count() >= 1,
        "the href must not be double-escaped into a dead link"
    );

    // Redeem it the way the public handler does.
    let (claimed_user, metadata) =
        backend::repository::reset_tokens::validate_and_consume_token_with_metadata(
            &mut conn,
            &emailed_token,
            TokenType::EmailVerification.as_str(),
        )
        .expect("a fresh token must redeem");
    assert_eq!(claimed_user, user.uuid);

    let meta = metadata.expect("metadata present");
    let email_id = meta["user_email_id"].as_i64().expect("row id") as i32;
    let address = meta["address"].as_str().expect("address");

    assert_eq!(
        backend::repository::user_emails::mark_verified_if_matches(
            &mut conn,
            email_id,
            claimed_user,
            address
        )
        .expect("verify"),
        1
    );

    let after = backend::repository::user_emails::get_user_emails_by_uuid(&mut conn, &user.uuid)
        .expect("reload")
        .into_iter()
        .find(|e| e.id == created.id)
        .expect("row present");
    assert!(after.is_verified, "the address is confirmed");

    // Single use. A forwarded mailbox is not a second chance.
    assert!(
        backend::repository::reset_tokens::validate_and_consume_token_with_metadata(
            &mut conn,
            &emailed_token,
            TokenType::EmailVerification.as_str(),
        )
        .is_err(),
        "a confirmation link must not redeem twice"
    );
}
