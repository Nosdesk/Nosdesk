//! Signed, stateless tokens for one-click email unsubscribe (B2 / RFC 8058).
//!
//! A notification email's `List-Unsubscribe` URL carries a token naming the
//! recipient. The token is HMAC-signed with the server's `JWT_SECRET` so the
//! no-auth unsubscribe endpoint can trust it without a session or DB lookup, and
//! a third party can't unsubscribe an arbitrary user by guessing a uuid. The
//! token is not secret (it travels in mail headers); the signature is what makes
//! it unforgeable.

use ring::hmac;
use uuid::Uuid;

/// Hex HMAC-SHA256 of `body` under `secret`.
fn hmac_hex(secret: &[u8], body: &str) -> String {
    let key = hmac::Key::new(hmac::HMAC_SHA256, secret);
    let sig = hmac::sign(&key, body.as_bytes());
    let mut hex = String::with_capacity(sig.as_ref().len() * 2);
    for b in sig.as_ref() {
        hex.push_str(&format!("{b:02x}"));
    }
    hex
}

/// Build an unsubscribe token for `user_uuid` under `secret`. Format is
/// `<uuid-simple>.<hmac-hex>`, URL-safe as-is (hex + a dot).
pub fn sign_with(secret: &[u8], user_uuid: &Uuid) -> String {
    let body = user_uuid.as_simple().to_string();
    let sig = hmac_hex(secret, &body);
    format!("{body}.{sig}")
}

/// Verify `token` under `secret` and recover the user it unsubscribes, or
/// `None` if the signature doesn't match (forged / wrong secret / malformed).
pub fn verify_with(secret: &[u8], token: &str) -> Option<Uuid> {
    let (body, sig) = token.split_once('.')?;
    let expected = hmac_hex(secret, body);
    if !constant_time_eq::constant_time_eq(expected.as_bytes(), sig.as_bytes()) {
        return None;
    }
    Uuid::parse_str(body).ok()
}

/// The server's token-signing secret (`JWT_SECRET`), or `None` when unset.
fn secret() -> Option<String> {
    std::env::var("JWT_SECRET").ok().filter(|s| !s.is_empty())
}

/// [`sign_with`] keyed by the server's `JWT_SECRET`. `None` only when that's
/// unset (never in a configured deployment).
pub fn sign(user_uuid: &Uuid) -> Option<String> {
    secret().map(|s| sign_with(s.as_bytes(), user_uuid))
}

/// [`verify_with`] keyed by the server's `JWT_SECRET`.
pub fn verify(token: &str) -> Option<Uuid> {
    verify_with(secret()?.as_bytes(), token)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &[u8] = b"unsub-test-secret";

    #[test]
    fn round_trips_user() {
        let u = Uuid::now_v7();
        let token = sign_with(SECRET, &u);
        assert_eq!(verify_with(SECRET, &token), Some(u));
    }

    #[test]
    fn rejects_tampered_signature() {
        let u = Uuid::now_v7();
        let token = sign_with(SECRET, &u);
        let mut bytes: Vec<char> = token.chars().collect();
        let last = bytes.len() - 1;
        bytes[last] = if bytes[last] == 'a' { 'b' } else { 'a' };
        let tampered: String = bytes.into_iter().collect();
        assert_eq!(verify_with(SECRET, &tampered), None);
    }

    #[test]
    fn rejects_swapped_user() {
        // A body swapped to a different uuid no longer matches the signature.
        let token = sign_with(SECRET, &Uuid::now_v7());
        let other = Uuid::now_v7().as_simple().to_string();
        let sig = token.split_once('.').unwrap().1;
        assert_eq!(verify_with(SECRET, &format!("{other}.{sig}")), None);
    }

    #[test]
    fn rejects_wrong_secret_and_malformed() {
        let token = sign_with(SECRET, &Uuid::now_v7());
        assert_eq!(verify_with(b"different", &token), None);
        assert_eq!(verify_with(SECRET, "no-dot"), None);
        assert_eq!(verify_with(SECRET, "not-a-uuid.deadbeef"), None);
    }
}
