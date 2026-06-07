//! Resend webhook receiver.
//!
//! Resend signs webhooks with Svix (`svix-id` / `svix-timestamp` /
//! `svix-signature` headers). We verify the HMAC-SHA256 signature against
//! `RESEND_WEBHOOK_SECRET`, then translate the delivery-lifecycle events
//! into `outbound_emails` status + `email_suppressions`, correlated by the
//! provider message id (`email_id`) stored at send time. This is the
//! authoritative delivery / bounce / complaint signal SMTP can't provide.
//!
//! The endpoint is public (Resend is unauthenticated); the signature IS
//! the auth. Unmatched ids and event types we don't act on still return
//! 200 so Resend doesn't retry; only a bad signature returns 401.

use actix_web::{web, HttpRequest, HttpResponse, Responder};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use ring::hmac;
use serde::Deserialize;
use tracing::{error, info, warn};

use crate::models::{email_suppression_reason, NewEmailSuppression};
use crate::repository::{email_suppressions, outbound_emails};
use crate::sync::actor::ActorContext;
use crate::sync::session::{background_run, with_actor_bypass_context};

/// Max clock skew for the svix timestamp. Svix recommends 5 minutes.
const TIMESTAMP_TOLERANCE_SECS: i64 = 300;

#[derive(Debug, Deserialize)]
struct ResendEvent {
    #[serde(rename = "type")]
    event_type: String,
    data: ResendEventData,
}

#[derive(Debug, Deserialize)]
struct ResendEventData {
    /// Resend's own message id; correlates to `outbound_emails.provider_message_id`.
    email_id: Option<String>,
    /// Present on `email.bounced`; the human-readable bounce reason.
    #[serde(default)]
    bounce: Option<ResendBounce>,
}

#[derive(Debug, Deserialize)]
struct ResendBounce {
    message: Option<String>,
}

/// Which lifecycle events we act on. Everything else (sent, opened,
/// clicked, delivery_delayed) is acknowledged but ignored.
enum Kind {
    Delivered,
    Bounced,
    Complained,
}

pub async fn resend_webhook(
    req: HttpRequest,
    body: web::Bytes,
    pool: web::Data<crate::db::Pool>,
) -> impl Responder {
    let secret = match std::env::var("RESEND_WEBHOOK_SECRET") {
        Ok(s) if !s.is_empty() => s,
        _ => {
            warn!("resend webhook called but RESEND_WEBHOOK_SECRET is not set; rejecting");
            return HttpResponse::ServiceUnavailable().finish();
        }
    };

    if !verify_svix_signature(&secret, &req, &body) {
        warn!("resend webhook: signature verification failed");
        return HttpResponse::Unauthorized().finish();
    }

    let event: ResendEvent = match serde_json::from_slice(&body) {
        Ok(e) => e,
        Err(e) => {
            warn!(error = %e, "resend webhook: unparseable payload");
            return HttpResponse::BadRequest().finish();
        }
    };

    let kind = match event.event_type.as_str() {
        "email.delivered" => Kind::Delivered,
        "email.bounced" => Kind::Bounced,
        "email.complained" => Kind::Complained,
        // Sent / delivery_delayed / opened / clicked: ack, no action.
        _ => return HttpResponse::Ok().finish(),
    };

    let Some(email_id) = event.data.email_id else {
        return HttpResponse::Ok().finish();
    };

    // Resolve the row's workspace + recipient so the audited writes
    // (delivery/bounce on outbound_emails, suppression) are scoped to the
    // right workspace, this is a cross-workspace, platform-level handler.
    let lookup = background_run(&pool, "background:resend_webhook_lookup", |conn| {
        outbound_emails::workspace_and_recipient_by_provider(conn, &email_id)
    });
    let (workspace_id, recipient) = match lookup {
        Ok(Some(v)) => v,
        // Untracked / pre-tracking send: ack so Resend stops retrying.
        Ok(None) => return HttpResponse::Ok().finish(),
        Err(e) => {
            error!(error = %e, "resend webhook: row lookup failed");
            return HttpResponse::InternalServerError().finish();
        }
    };

    let diagnostic = event.data.bounce.and_then(|b| b.message);

    let mut conn = match pool.get() {
        Ok(c) => c,
        Err(e) => {
            error!(error = %e, "resend webhook: pool get failed");
            return HttpResponse::InternalServerError().finish();
        }
    };
    let actor = ActorContext::system("resend_webhook").with_workspace(workspace_id);
    let result = with_actor_bypass_context(&mut conn, &actor, |c| {
        match kind {
            Kind::Delivered => {
                outbound_emails::mark_delivered_by_provider(c, &email_id)?;
            }
            Kind::Bounced => {
                outbound_emails::mark_bounced_by_provider(
                    c,
                    &email_id,
                    Some(&recipient),
                    diagnostic.as_deref(),
                )?;
                email_suppressions::upsert(
                    c,
                    NewEmailSuppression {
                        email: recipient.clone(),
                        reason: email_suppression_reason::HARD_BOUNCE.to_string(),
                        bounce_diagnostic: diagnostic.clone(),
                    },
                )?;
            }
            Kind::Complained => {
                email_suppressions::upsert(
                    c,
                    NewEmailSuppression {
                        email: recipient.clone(),
                        reason: email_suppression_reason::COMPLAINT.to_string(),
                        bounce_diagnostic: None,
                    },
                )?;
            }
        }
        Ok::<_, diesel::result::Error>(())
    });

    match result {
        Ok(()) => {
            info!(event = %event.event_type, "resend webhook applied");
            HttpResponse::Ok().finish()
        }
        Err(e) => {
            error!(error = %e, event = %event.event_type, "resend webhook: write failed");
            HttpResponse::InternalServerError().finish()
        }
    }
}

/// Verify the Svix signature. Svix signs `{id}.{timestamp}.{body}` with
/// HMAC-SHA256 under the base64-decoded secret (after the `whsec_`
/// prefix); `svix-signature` is a space-separated list of `v1,<base64sig>`
/// entries, any of which may match.
#[allow(deprecated)]
fn verify_svix_signature(secret: &str, req: &HttpRequest, body: &[u8]) -> bool {
    let header = |name: &str| req.headers().get(name).and_then(|v| v.to_str().ok());
    let (Some(id), Some(ts), Some(sig_header)) = (
        header("svix-id"),
        header("svix-timestamp"),
        header("svix-signature"),
    ) else {
        return false;
    };

    // Reject stale / forward-dated timestamps (replay protection).
    let Ok(ts_num) = ts.parse::<i64>() else {
        return false;
    };
    if (chrono::Utc::now().timestamp() - ts_num).abs() > TIMESTAMP_TOLERANCE_SECS {
        return false;
    }

    let secret_b64 = secret.strip_prefix("whsec_").unwrap_or(secret);
    let Ok(key_bytes) = BASE64.decode(secret_b64) else {
        return false;
    };
    let Ok(body_str) = std::str::from_utf8(body) else {
        return false;
    };

    let signed = format!("{id}.{ts}.{body_str}");
    let key = hmac::Key::new(hmac::HMAC_SHA256, &key_bytes);
    let expected = BASE64.encode(hmac::sign(&key, signed.as_bytes()).as_ref());

    sig_header.split(' ').any(|entry| {
        // Each entry is `v1,<base64sig>`; tolerate a bare signature too.
        let sig = entry.split_once(',').map(|(_, s)| s).unwrap_or(entry);
        ring::constant_time::verify_slices_are_equal(sig.as_bytes(), expected.as_bytes()).is_ok()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // Compute a valid svix signature the way Resend does, for a fixed
    // secret + payload, so the verifier can be exercised without a live
    // request. Mirrors `verify_svix_signature`'s signing.
    fn sign(secret_b64: &str, id: &str, ts: &str, body: &str) -> String {
        let key_bytes = BASE64.decode(secret_b64).unwrap();
        let key = hmac::Key::new(hmac::HMAC_SHA256, &key_bytes);
        let signed = format!("{id}.{ts}.{body}");
        format!(
            "v1,{}",
            BASE64.encode(hmac::sign(&key, signed.as_bytes()).as_ref())
        )
    }

    fn req_with(id: &str, ts: &str, sig: &str) -> HttpRequest {
        actix_web::test::TestRequest::post()
            .insert_header(("svix-id", id))
            .insert_header(("svix-timestamp", ts))
            .insert_header(("svix-signature", sig))
            .to_http_request()
    }

    #[test]
    fn accepts_a_valid_signature() {
        let secret_b64 = BASE64.encode(b"super-secret-key-bytes");
        let secret = format!("whsec_{secret_b64}");
        let body = r#"{"type":"email.delivered","data":{"email_id":"abc"}}"#;
        let ts = chrono::Utc::now().timestamp().to_string();
        let sig = sign(&secret_b64, "msg_1", &ts, body);
        let req = req_with("msg_1", &ts, &sig);
        assert!(verify_svix_signature(&secret, &req, body.as_bytes()));
    }

    #[test]
    fn rejects_a_tampered_body() {
        let secret_b64 = BASE64.encode(b"super-secret-key-bytes");
        let secret = format!("whsec_{secret_b64}");
        let ts = chrono::Utc::now().timestamp().to_string();
        let sig = sign(&secret_b64, "msg_1", &ts, "original");
        let req = req_with("msg_1", &ts, &sig);
        assert!(!verify_svix_signature(&secret, &req, b"tampered"));
    }

    #[test]
    fn rejects_a_stale_timestamp() {
        let secret_b64 = BASE64.encode(b"super-secret-key-bytes");
        let secret = format!("whsec_{secret_b64}");
        let body = "{}";
        let old_ts = (chrono::Utc::now().timestamp() - 4000).to_string();
        let sig = sign(&secret_b64, "msg_1", &old_ts, body);
        let req = req_with("msg_1", &old_ts, &sig);
        assert!(!verify_svix_signature(&secret, &req, body.as_bytes()));
    }

    #[test]
    fn rejects_missing_headers() {
        let secret = "whsec_AAAA";
        let req = actix_web::test::TestRequest::post().to_http_request();
        assert!(!verify_svix_signature(secret, &req, b"{}"));
    }
}
