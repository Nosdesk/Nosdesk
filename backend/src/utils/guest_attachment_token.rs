//! Signed claim tokens binding a guest's temp upload to the guest who made it.
//!
//! `POST /api/public/files/temp` stores a file and returns the `attachments`
//! row it created. That row's id is a sequential integer, and the claim on
//! submit checked only that the id was unclaimed, unowned and recent. Nothing
//! tied an id to whoever uploaded it, so the id was a guessable bearer
//! capability: an unauthenticated actor could submit a ticket naming ids just
//! below the current sequence and reparent any still-pending upload into their
//! own ticket. The victim's own submission then silently dropped the
//! attachment.
//!
//! The fix is to make the capability unguessable rather than to guess harder at
//! ownership. A guest has no session or account to bind to, so there is nothing
//! stable to record on the row; what the uploader does have is the response to
//! their own upload. Signing that response turns "knows an integer" into "holds
//! a token we issued", with no schema change and no state to expire.
//!
//! The workspace is signed over and re-supplied at verification rather than
//! read out of the token, so a token minted for one tenant cannot be replayed
//! against another even if the id happens to exist there. The 60-minute TTL
//! stays where it was, on `attachments.created_at`, so there is one source of
//! truth for how long a pending upload lives.

use std::sync::OnceLock;

use ring::hmac;

/// Domain-separation label for the claim-token key. Bumping the suffix
/// invalidates every outstanding token.
const CLAIM_KEY_LABEL: &[u8] = b"nosdesk-guest-attachment-claim-v1";

/// A dedicated key DERIVED from `JWT_SECRET` via `HMAC-SHA256(JWT_SECRET,
/// label)`, matching `plugins::bundle_token`. A session token cannot be
/// verified as a claim token or vice versa, and leaking this key does not
/// reveal `JWT_SECRET`. Derived rather than configured, so operators gain no
/// new setting.
fn claim_key() -> Option<&'static Vec<u8>> {
    static KEY: OnceLock<Option<Vec<u8>>> = OnceLock::new();
    KEY.get_or_init(|| {
        let secret = std::env::var("JWT_SECRET").ok().filter(|s| !s.is_empty())?;
        let k = hmac::Key::new(hmac::HMAC_SHA256, secret.as_bytes());
        Some(hmac::sign(&k, CLAIM_KEY_LABEL).as_ref().to_vec())
    })
    .as_ref()
}

fn hmac_hex(secret: &[u8], body: &str) -> String {
    let key = hmac::Key::new(hmac::HMAC_SHA256, secret);
    let sig = hmac::sign(&key, body.as_bytes());
    let mut hex = String::with_capacity(sig.as_ref().len() * 2);
    for b in sig.as_ref() {
        hex.push_str(&format!("{b:02x}"));
    }
    hex
}

/// The signed body. The workspace is included so a token is worthless in any
/// other tenant.
fn body_of(workspace_id: i32, attachment_id: i32) -> String {
    format!("{workspace_id}.{attachment_id}")
}

/// Build a claim token for `attachment_id` in `workspace_id` under `secret`.
/// Format is `<attachment-id>.<hmac-hex>`; the workspace is signed over but not
/// carried, since the verifier already knows which tenant it is serving.
pub fn sign_with(secret: &[u8], workspace_id: i32, attachment_id: i32) -> String {
    let sig = hmac_hex(secret, &body_of(workspace_id, attachment_id));
    format!("{attachment_id}.{sig}")
}

/// Recover the attachment id `token` claims in `workspace_id`, or `None` if the
/// signature does not match: forged, malformed, or minted for another tenant.
pub fn verify_with(secret: &[u8], workspace_id: i32, token: &str) -> Option<i32> {
    let (id_part, sig) = token.split_once('.')?;
    let attachment_id: i32 = id_part.parse().ok()?;
    let expected = hmac_hex(secret, &body_of(workspace_id, attachment_id));
    if !constant_time_eq::constant_time_eq(expected.as_bytes(), sig.as_bytes()) {
        return None;
    }
    Some(attachment_id)
}

/// [`sign_with`] keyed by the derived claim key. `None` only when `JWT_SECRET`
/// is unset, which never happens in a configured deployment.
pub fn sign(workspace_id: i32, attachment_id: i32) -> Option<String> {
    claim_key().map(|k| sign_with(k, workspace_id, attachment_id))
}

/// [`verify_with`] keyed by the derived claim key.
pub fn verify(workspace_id: i32, token: &str) -> Option<i32> {
    verify_with(claim_key()?, workspace_id, token)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &[u8] = b"guest-claim-test-secret";

    #[test]
    fn round_trips_the_attachment_id() {
        let token = sign_with(SECRET, 7, 4242);
        assert_eq!(verify_with(SECRET, 7, &token), Some(4242));
    }

    /// The point of the change: knowing a neighbouring id is no longer enough,
    /// because the attacker cannot produce a signature for it.
    #[test]
    fn a_guessed_neighbouring_id_does_not_verify() {
        let token = sign_with(SECRET, 7, 4242);
        let sig = token.split_once('.').expect("sig").1;
        for guess in [4241, 4243, 1] {
            assert_eq!(
                verify_with(SECRET, 7, &format!("{guess}.{sig}")),
                None,
                "reusing a signature under a different id must fail"
            );
        }
    }

    #[test]
    fn a_token_from_another_workspace_does_not_verify() {
        let token = sign_with(SECRET, 7, 4242);
        assert_eq!(
            verify_with(SECRET, 8, &token),
            None,
            "a token is worthless outside the tenant it was minted for"
        );
    }

    #[test]
    fn rejects_a_tampered_signature() {
        let token = sign_with(SECRET, 7, 4242);
        let mut chars: Vec<char> = token.chars().collect();
        let last = chars.len() - 1;
        chars[last] = if chars[last] == 'a' { 'b' } else { 'a' };
        let tampered: String = chars.into_iter().collect();
        assert_eq!(verify_with(SECRET, 7, &tampered), None);
    }

    #[test]
    fn rejects_malformed_tokens() {
        for bad in ["", ".", "abc", "abc.def", "4242", "4242."] {
            assert_eq!(verify_with(SECRET, 7, bad), None, "{bad:?} must not verify");
        }
    }
}
