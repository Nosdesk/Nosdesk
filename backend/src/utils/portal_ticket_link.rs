//! Signed "View request" links for requester emails.
//!
//! Every email a requester gets about their ticket carries a link that opens
//! it in the portal already signed in, so an email-only requester never meets
//! a sign-in wall. The token names the requester and the ticket, expires after
//! [`LINK_TTL_DAYS`], and stays usable until then (a requester re-opens old
//! emails, and mail scanners open every link on delivery). It is stateless,
//! and revoked two ways at use: the membership check, the same gate as the
//! magic link, and the account's credential epoch, which it signs and which
//! signing out everywhere raises ([`crate::repository::credential_epochs`]).
//! Only [`redeem`] and [`redeem_approval`] accept a link, so no caller can
//! skip the epoch.
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

/// Approval links ("Review request" to an approver) sign under their own key:
/// a view link can never open an approval, or the reverse.
const APPROVAL_KEY_LABEL: &[u8] = b"nosdesk-portal-approval-link-v1";

/// How long an approval link stays good. Longer than a view link: an approver
/// may be away for a week.
pub const APPROVAL_TTL_DAYS: i64 = 14;

fn derive(label: &[u8]) -> Option<Vec<u8>> {
    let secret = std::env::var("JWT_SECRET").ok().filter(|s| !s.is_empty())?;
    let k = hmac::Key::new(hmac::HMAC_SHA256, secret.as_bytes());
    Some(hmac::sign(&k, label).as_ref().to_vec())
}

/// A key derived from `JWT_SECRET` (see `guest_attachment_token::claim_key`),
/// so no other token verifies as a link and the secret itself isn't exposed.
fn key() -> Option<&'static Vec<u8>> {
    static KEY: OnceLock<Option<Vec<u8>>> = OnceLock::new();
    KEY.get_or_init(|| derive(KEY_LABEL)).as_ref()
}

fn approval_key() -> Option<&'static Vec<u8>> {
    static KEY: OnceLock<Option<Vec<u8>>> = OnceLock::new();
    KEY.get_or_init(|| derive(APPROVAL_KEY_LABEL)).as_ref()
}

/// The approver and ticket an approval link names, if it is valid now and
/// was issued since they were last signed out everywhere.
pub fn redeem_approval(
    conn: &mut crate::db::DbConnection,
    workspace_id: i32,
    token: &str,
) -> diesel::QueryResult<Option<(Uuid, i32)>> {
    match approval_key() {
        Some(key) => redeem_with(conn, key, workspace_id, token),
        None => Ok(None),
    }
}

/// The requester and ticket a "View request" link names, if it is valid now
/// and was issued since they were last signed out everywhere.
pub fn redeem(
    conn: &mut crate::db::DbConnection,
    workspace_id: i32,
    token: &str,
) -> diesel::QueryResult<Option<(Uuid, i32)>> {
    match key() {
        Some(key) => redeem_with(conn, key, workspace_id, token),
        None => Ok(None),
    }
}

fn redeem_with(
    conn: &mut crate::db::DbConnection,
    secret: &[u8],
    workspace_id: i32,
    token: &str,
) -> diesel::QueryResult<Option<(Uuid, i32)>> {
    let Some((user, ticket_id, epoch)) =
        verify_with(secret, workspace_id, token, chrono::Utc::now().timestamp())
    else {
        return Ok(None);
    };
    let current = crate::repository::credential_epochs::current(conn, &user)?;
    Ok((epoch == current).then_some((user, ticket_id)))
}

/// Where an approval email's "Review request" link points: a signed portal
/// link that signs the approver in and opens the approval page.
pub fn approval_url(
    conn: &mut crate::db::DbConnection,
    workspace_id: i32,
    approver: Uuid,
    ticket_id: i32,
) -> Option<String> {
    let origin = portal_origin(conn, workspace_id)?;
    let expires = (chrono::Utc::now() + chrono::Duration::days(APPROVAL_TTL_DAYS)).timestamp();
    let epoch = epoch_for_link(conn, approver)?;
    let token = sign_with(
        approval_key()?,
        workspace_id,
        approver,
        ticket_id,
        expires,
        epoch,
    );
    Some(format!("{origin}/api/portal/auth/approval?t={token}"))
}

fn hmac_hex(secret: &[u8], body: &str) -> String {
    let key = hmac::Key::new(hmac::HMAC_SHA256, secret);
    hmac::sign(&key, body.as_bytes())
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The epoch a new link for `user` is signed under. On a read failure the
/// email goes without its link, so say so.
fn epoch_for_link(conn: &mut crate::db::DbConnection, user: Uuid) -> Option<i32> {
    match crate::repository::credential_epochs::current(conn, &user) {
        Ok(epoch) => Some(epoch),
        Err(e) => {
            tracing::warn!(error = %e, user_uuid = %user, "portal link: credential epoch unreadable; sending no link");
            None
        }
    }
}

/// What the signature covers. A link sent before epochs were signed has none,
/// and reads as epoch 0.
fn body_of(
    workspace_id: i32,
    user: Uuid,
    ticket_id: i32,
    expires: i64,
    epoch: Option<i32>,
) -> String {
    match epoch {
        Some(epoch) => format!("{workspace_id}.{user}.{ticket_id}.{expires}.{epoch}"),
        None => format!("{workspace_id}.{user}.{ticket_id}.{expires}"),
    }
}

/// A link token for `user` to open `ticket_id` in `workspace_id` until
/// `expires` (unix seconds), issued under credential `epoch`. Format
/// `<user>.<ticket>.<expires>.<epoch>.<hmac-hex>`.
pub fn sign_with(
    secret: &[u8],
    workspace_id: i32,
    user: Uuid,
    ticket_id: i32,
    expires: i64,
    epoch: i32,
) -> String {
    let sig = hmac_hex(
        secret,
        &body_of(workspace_id, user, ticket_id, expires, Some(epoch)),
    );
    format!("{user}.{ticket_id}.{expires}.{epoch}.{sig}")
}

/// The requester, ticket and epoch `token` names in `workspace_id`, if the
/// signature matches and it hasn't expired at `now` (unix seconds). A link
/// sent before epochs were signed (`<user>.<ticket>.<expires>.<hmac-hex>`)
/// reads as epoch 0.
fn verify_with(
    secret: &[u8],
    workspace_id: i32,
    token: &str,
    now: i64,
) -> Option<(Uuid, i32, i32)> {
    let parts: Vec<&str> = token.split('.').collect();
    let (user, ticket_id, expires, epoch, sig) = match parts.as_slice() {
        [user, ticket, expires, sig] => (*user, *ticket, *expires, None, *sig),
        [user, ticket, expires, epoch, sig] => (
            *user,
            *ticket,
            *expires,
            Some(epoch.parse::<i32>().ok()?),
            *sig,
        ),
        _ => return None,
    };
    let user: Uuid = user.parse().ok()?;
    let ticket_id: i32 = ticket_id.parse().ok()?;
    let expires: i64 = expires.parse().ok()?;
    let expected = hmac_hex(
        secret,
        &body_of(workspace_id, user, ticket_id, expires, epoch),
    );
    if !constant_time_eq::constant_time_eq(expected.as_bytes(), sig.as_bytes()) {
        return None;
    }
    (now < expires).then_some((user, ticket_id, epoch.unwrap_or(0)))
}

/// [`sign_with`] under the derived key and the user's current epoch, expiring
/// [`LINK_TTL_DAYS`] from now. `None` when `JWT_SECRET` is unset or the epoch
/// can't be read.
pub fn sign(
    conn: &mut crate::db::DbConnection,
    workspace_id: i32,
    user: Uuid,
    ticket_id: i32,
) -> Option<String> {
    let expires = (chrono::Utc::now() + chrono::Duration::days(LINK_TTL_DAYS)).timestamp();
    let epoch = epoch_for_link(conn, user)?;
    Some(sign_with(
        key()?,
        workspace_id,
        user,
        ticket_id,
        expires,
        epoch,
    ))
}

/// The origin requesters reach this workspace's portal API on: its own host
/// on hosted (`https://acme.nosdesk.app`), the one instance origin on
/// self-hosted. `None` when no link base is known.
pub fn portal_origin(conn: &mut crate::db::DbConnection, workspace_id: i32) -> Option<String> {
    if crate::middleware::workspace_context::is_hosted() {
        let workspace = crate::repository::workspaces::find_by_id(conn, workspace_id).ok()??;
        let host = crate::utils::tenant_origin::canonical_host_for(
            &workspace.slug,
            workspace.custom_domain.as_deref(),
            crate::utils::tenant_origin::tenant_domain().as_deref(),
        )?;
        return Some(format!("https://{host}"));
    }
    crate::utils::tenant_origin::email_link_base(None).map(|b| b.trim_end_matches('/').to_string())
}

/// Where a requester email's "View request" link points: a signed portal link
/// that signs them in and opens the ticket. `None` when no link base is known.
pub fn view_request_url(
    conn: &mut crate::db::DbConnection,
    workspace_id: i32,
    requester: Uuid,
    ticket_id: i32,
) -> Option<String> {
    let origin = portal_origin(conn, workspace_id)?;
    let token = sign(conn, workspace_id, requester, ticket_id)?;
    Some(format!("{origin}/api/portal/auth/ticket?t={token}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &[u8] = b"test-secret";

    #[test]
    fn a_link_opens_its_ticket_for_its_requester_until_it_expires() {
        let user = Uuid::new_v4();
        let token = sign_with(SECRET, 3, user, 51, 1_000, 2);
        assert_eq!(verify_with(SECRET, 3, &token, 999), Some((user, 51, 2)));
        assert_eq!(verify_with(SECRET, 3, &token, 1_000), None, "expired");
    }

    #[test]
    fn a_link_is_worthless_elsewhere_or_altered() {
        let user = Uuid::new_v4();
        let token = sign_with(SECRET, 3, user, 51, 1_000, 2);
        assert_eq!(verify_with(SECRET, 4, &token, 0), None, "another workspace");
        let other_ticket = token.replacen(".51.", ".52.", 1);
        assert_eq!(
            verify_with(SECRET, 3, &other_ticket, 0),
            None,
            "another ticket"
        );
        let later = token.replacen(".1000.", ".9999999999.", 1);
        assert_eq!(verify_with(SECRET, 3, &later, 0), None, "extended expiry");
        let older = token.replacen(".1000.2.", ".1000.1.", 1);
        assert_eq!(verify_with(SECRET, 3, &older, 0), None, "another epoch");
        assert_eq!(verify_with(b"other", 3, &token, 0), None, "another key");
        assert_eq!(verify_with(SECRET, 3, "garbage", 0), None);
    }

    #[test]
    fn a_link_sent_before_epochs_reads_as_epoch_zero() {
        let user = Uuid::new_v4();
        let sig = hmac_hex(SECRET, &body_of(3, user, 51, 1_000, None));
        let legacy = format!("{user}.51.1000.{sig}");
        assert_eq!(verify_with(SECRET, 3, &legacy, 0), Some((user, 51, 0)));
        // Its signature can't be reused under the new format.
        assert_eq!(
            verify_with(SECRET, 3, &format!("{user}.51.1000.0.{sig}"), 0),
            None
        );
    }
}
