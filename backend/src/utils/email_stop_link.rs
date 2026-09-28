//! Signed "stop these emails" links for mail the workspace sends to an address
//! nobody has confirmed yet (the anonymous request form's confirmation). The
//! token names the workspace and the address; following it adds the address to
//! that workspace's suppression list, so a form can't be used to keep mailing
//! someone who asked it to stop.
//!
//! Stateless and non-expiring, like the notification unsubscribe token: the
//! signature (a key derived from `JWT_SECRET` under its own label) is what makes
//! it unforgeable.

use base64::Engine as _;
use ring::hmac;

const KEY_LABEL: &[u8] = b"nosdesk-email-stop-link-v1";

fn key() -> Option<hmac::Key> {
    let secret = std::env::var("JWT_SECRET").ok().filter(|s| !s.is_empty())?;
    let k = hmac::Key::new(hmac::HMAC_SHA256, secret.as_bytes());
    Some(hmac::Key::new(
        hmac::HMAC_SHA256,
        hmac::sign(&k, KEY_LABEL).as_ref(),
    ))
}

fn body_of(workspace_id: i32, email: &str) -> String {
    format!("{workspace_id}:{}", email.trim().to_lowercase())
}

/// `<base64url(email)>.<base64url(hmac)>`.
pub fn sign_with(key: &hmac::Key, workspace_id: i32, email: &str) -> String {
    let b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD;
    let email = email.trim().to_lowercase();
    let tag = hmac::sign(key, body_of(workspace_id, &email).as_bytes());
    format!("{}.{}", b64.encode(&email), b64.encode(tag.as_ref()))
}

/// The address `token` names in `workspace_id`, if the signature matches.
pub fn verify_with(key: &hmac::Key, workspace_id: i32, token: &str) -> Option<String> {
    let b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD;
    let (email, sig) = token.split_once('.')?;
    let email = String::from_utf8(b64.decode(email).ok()?).ok()?;
    let sig = b64.decode(sig).ok()?;
    hmac::verify(key, body_of(workspace_id, &email).as_bytes(), &sig).ok()?;
    Some(email)
}

pub fn verify(workspace_id: i32, token: &str) -> Option<String> {
    verify_with(&key()?, workspace_id, token)
}

/// The full link for an email: the workspace's portal origin plus the stop page.
pub fn url(conn: &mut crate::db::DbConnection, workspace_id: i32, email: &str) -> Option<String> {
    let origin = crate::utils::portal_ticket_link::portal_origin(conn, workspace_id)?;
    let token = sign_with(&key()?, workspace_id, email);
    Some(format!("{origin}/api/public/email/stop?t={token}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stop_link_names_one_address_in_one_workspace() {
        let key = hmac::Key::new(hmac::HMAC_SHA256, b"test");
        let token = sign_with(&key, 3, "Sam@Acme.test");
        assert_eq!(
            verify_with(&key, 3, &token).as_deref(),
            Some("sam@acme.test")
        );
        assert_eq!(verify_with(&key, 4, &token), None, "another workspace");
        let other = sign_with(&key, 3, "kim@acme.test");
        let spliced = format!(
            "{}.{}",
            other.split_once('.').unwrap().0,
            token.split_once('.').unwrap().1
        );
        assert_eq!(verify_with(&key, 3, &spliced), None, "another address");
        assert_eq!(verify_with(&key, 3, "garbage"), None);
    }
}
