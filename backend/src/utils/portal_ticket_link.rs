//! Signed "View request" links for requester emails.
//!
//! Every email a requester gets about their ticket carries a link that opens
//! it in the portal already signed in, so an email-only requester never meets
//! a sign-in wall. The token names the requester and the ticket, expires after
//! [`LINK_TTL_DAYS`], and stays usable until then (a requester re-opens old
//! emails). It is stateless: revocation is the membership check at use, the
//! same gate as the magic link.
//!
//! The workspace is signed over but not carried, like
//! [`super::guest_attachment_token`]: the verifier knows which tenant it is
//! serving, so a link minted for one workspace is worthless at another.

use std::sync::OnceLock;

use ring::hmac;
use uuid::Uuid;

/// How long a link in an email stays good.
pub const LINK_TTL_DAYS: i64 = 7;

/// Domain-separation label for the key. Bumping the suffix invalidates every
/// outstanding link.
const KEY_LABEL: &[u8] = b"nosdesk-portal-ticket-link-v1";

/// A key derived from `JWT_SECRET` (see `guest_attachment_token::claim_key`),
/// so no other token verifies as a link and the secret itself isn't exposed.
fn key() -> Option<&'static Vec<u8>> {
    static KEY: OnceLock<Option<Vec<u8>>> = OnceLock::new();
    KEY.get_or_init(|| {
        let secret = std::env::var("JWT_SECRET").ok().filter(|s| !s.is_empty())?;
        let k = hmac::Key::new(hmac::HMAC_SHA256, secret.as_bytes());
        Some(hmac::sign(&k, KEY_LABEL).as_ref().to_vec())
    })
    .as_ref()
}

fn hmac_hex(secret: &[u8], body: &str) -> String {
    let key = hmac::Key::new(hmac::HMAC_SHA256, secret);
    hmac::sign(&key, body.as_bytes())
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn body_of(workspace_id: i32, user: Uuid, ticket_id: i32, expires: i64) -> String {
    format!("{workspace_id}.{user}.{ticket_id}.{expires}")
}

/// A link token for `user` to open `ticket_id` in `workspace_id` until
/// `expires` (unix seconds). Format `<user>.<ticket>.<expires>.<hmac-hex>`.
pub fn sign_with(
    secret: &[u8],
    workspace_id: i32,
    user: Uuid,
    ticket_id: i32,
    expires: i64,
) -> String {
    let sig = hmac_hex(secret, &body_of(workspace_id, user, ticket_id, expires));
    format!("{user}.{ticket_id}.{expires}.{sig}")
}

/// The requester and ticket `token` names in `workspace_id`, if the signature
/// matches and it hasn't expired at `now` (unix seconds).
pub fn verify_with(secret: &[u8], workspace_id: i32, token: &str, now: i64) -> Option<(Uuid, i32)> {
    let mut parts = token.splitn(4, '.');
    let user: Uuid = parts.next()?.parse().ok()?;
    let ticket_id: i32 = parts.next()?.parse().ok()?;
    let expires: i64 = parts.next()?.parse().ok()?;
    let sig = parts.next()?;
    let expected = hmac_hex(secret, &body_of(workspace_id, user, ticket_id, expires));
    if !constant_time_eq::constant_time_eq(expected.as_bytes(), sig.as_bytes()) {
        return None;
    }
    (now < expires).then_some((user, ticket_id))
}

/// [`sign_with`] under the derived key, expiring [`LINK_TTL_DAYS`] from now.
/// `None` when `JWT_SECRET` is unset.
pub fn sign(workspace_id: i32, user: Uuid, ticket_id: i32) -> Option<String> {
    let expires = (chrono::Utc::now() + chrono::Duration::days(LINK_TTL_DAYS)).timestamp();
    Some(sign_with(key()?, workspace_id, user, ticket_id, expires))
}

/// [`verify_with`] under the derived key, now.
pub fn verify(workspace_id: i32, token: &str) -> Option<(Uuid, i32)> {
    verify_with(key()?, workspace_id, token, chrono::Utc::now().timestamp())
}

/// Where a requester email's "View request" link points. Hosted: a signed
/// portal link on the workspace's own origin that signs them in. Self-hosted
/// (no portal yet): the ticket in the app. `None` when no link base is known.
pub fn view_request_url(
    conn: &mut crate::db::DbConnection,
    workspace_id: i32,
    requester: Uuid,
    ticket_id: i32,
) -> Option<String> {
    let workspace = crate::repository::workspaces::find_by_id(conn, workspace_id).ok()??;
    if crate::middleware::workspace_context::is_hosted() {
        let host = crate::utils::tenant_origin::canonical_host_for(
            &workspace.slug,
            workspace.custom_domain.as_deref(),
            crate::utils::tenant_origin::tenant_domain().as_deref(),
        )?;
        let token = sign(workspace_id, requester, ticket_id)?;
        return Some(format!("https://{host}/api/portal/auth/ticket?t={token}"));
    }
    let base = crate::utils::tenant_origin::email_link_base(None)?;
    Some(format!(
        "{}/tickets/{ticket_id}",
        base.trim_end_matches('/')
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &[u8] = b"test-secret";

    #[test]
    fn a_link_opens_its_ticket_for_its_requester_until_it_expires() {
        let user = Uuid::new_v4();
        let token = sign_with(SECRET, 3, user, 51, 1_000);
        assert_eq!(verify_with(SECRET, 3, &token, 999), Some((user, 51)));
        assert_eq!(verify_with(SECRET, 3, &token, 1_000), None, "expired");
    }

    #[test]
    fn a_link_is_worthless_elsewhere_or_altered() {
        let user = Uuid::new_v4();
        let token = sign_with(SECRET, 3, user, 51, 1_000);
        assert_eq!(verify_with(SECRET, 4, &token, 0), None, "another workspace");
        let other_ticket = token.replacen(".51.", ".52.", 1);
        assert_eq!(
            verify_with(SECRET, 3, &other_ticket, 0),
            None,
            "another ticket"
        );
        let later = token.replacen(".1000.", ".9999999999.", 1);
        assert_eq!(verify_with(SECRET, 3, &later, 0), None, "extended expiry");
        assert_eq!(verify_with(b"other", 3, &token, 0), None, "another key");
        assert_eq!(verify_with(SECRET, 3, "garbage", 0), None);
    }
}
