//! VERP (Variable Envelope Return Path) tokens for bounce correlation (B1).
//!
//! An outbound message can be sent with a dedicated SMTP envelope-from
//! (`MAIL FROM` / Return-Path) distinct from the `From` header, carrying a
//! token that names the originating `outbound_emails` row. A bounce DSN is
//! addressed back to that Return-Path, so the inbound bounce handler can link
//! it to the row directly instead of relying on the remote MTA echoing the
//! original `Message-ID` (which many don't). Keeping the Return-Path on the
//! sending mailbox's own domain also aligns SPF.
//!
//! The token is `nbx-<row_id>-<hmac8>` appended to the base mailbox's local
//! part via plus-addressing: `support@acme.com` + row 42 ->
//! `support+nbx-42-1a2b3c4d@acme.com`. The bounce still lands in the base
//! mailbox (plus-addressing delivers to the base), so it arrives wherever
//! ordinary replies already do. The truncated HMAC stops a third party forging
//! a Return-Path that would mark an arbitrary row bounced.
//!
//! Opt-in: with no `SMTP_VERP_SECRET` configured, no Return-Path is built and
//! the transport falls back to lettre's default (From-derived) envelope, so
//! existing deployments are unchanged until an operator enables and
//! deliverability-tests it.

use ring::hmac;

/// Local-part tag prefix, short and distinctive so the inbound decoder can
/// recognise our VERP addresses among ordinary plus-addressed mail.
const TAG_PREFIX: &str = "nbx";
/// HMAC truncation: 8 hex chars (32 bits). Enough to make forging a valid
/// `(row_id, tag)` pair impractical; the tag only routes a bounce to a row, it
/// grants no access.
const HMAC_HEX_LEN: usize = 8;

/// Hex HMAC-SHA256 of the row id under `secret`, truncated to [`HMAC_HEX_LEN`].
fn tag_hmac(secret: &[u8], row_id: i64) -> String {
    let key = hmac::Key::new(hmac::HMAC_SHA256, secret);
    let sig = hmac::sign(&key, row_id.to_string().as_bytes());
    let mut hex = String::with_capacity(HMAC_HEX_LEN);
    for b in sig.as_ref().iter().take(HMAC_HEX_LEN / 2) {
        hex.push_str(&format!("{b:02x}"));
    }
    hex
}

/// Build the VERP Return-Path for `row_id` by plus-tagging `base` (the polled
/// mailbox a bounce should land in, e.g. the channel's IMAP address). Returns
/// `None` when `base` isn't a bare `local@domain` address, or its local part is
/// already plus-addressed (the decoder splits on the first `+`, so a second
/// would be ambiguous) — the caller then leaves the default envelope in place
/// rather than send a malformed `MAIL FROM`.
pub fn tagged_return_path(base: &str, row_id: i64, secret: &[u8]) -> Option<String> {
    let (local, domain) = base.split_once('@')?;
    if local.is_empty() || domain.is_empty() || domain.contains('@') || local.contains('+') {
        return None;
    }
    let tag = tag_hmac(secret, row_id);
    Some(format!("{local}+{TAG_PREFIX}-{row_id}-{tag}@{domain}"))
}

/// Recover the `outbound_emails` row id from a VERP Return-Path produced by
/// [`tagged_return_path`], verifying the HMAC under `secret`. Returns `None`
/// for any address that isn't one of ours (ordinary mail, a forged tag, or a
/// tag signed with a different secret). Used by the inbound bounce handler.
pub fn row_id_from_address(address: &str, secret: &[u8]) -> Option<i64> {
    let (local, _domain) = address.split_once('@')?;
    let (_base, tag) = local.split_once('+')?;
    // tag == "nbx-<row_id>-<hmac8>"
    let rest = tag.strip_prefix(TAG_PREFIX)?.strip_prefix('-')?;
    let (row_str, hmac_str) = rest.rsplit_once('-')?;
    let row_id: i64 = row_str.parse().ok()?;
    let expected = tag_hmac(secret, row_id);
    // Constant-time compare so a timing side channel can't be used to forge the
    // tag byte-by-byte (low stakes here, but free given the helper exists).
    constant_time_eq::constant_time_eq(expected.as_bytes(), hmac_str.as_bytes()).then_some(row_id)
}

/// The configured VERP signing secret (`SMTP_VERP_SECRET`, hex-encoded), or
/// `None` when unset / blank / not valid hex — which disables VERP entirely.
/// Read fresh (not memoised) so it stays operationally toggleable and testable.
pub fn configured_secret() -> Option<Vec<u8>> {
    let hex = std::env::var("SMTP_VERP_SECRET").ok()?;
    let hex = hex.trim();
    if hex.is_empty() {
        return None;
    }
    // Accept hex only; reject anything else rather than silently signing with a
    // misconfigured key that would never verify on the inbound side.
    hex_decode(hex)
}

fn hex_decode(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &[u8] = b"verp-test-secret";

    #[test]
    fn round_trips_row_id() {
        let rp = tagged_return_path("support@acme.com", 42, SECRET).unwrap();
        assert!(rp.starts_with("support+nbx-42-"));
        assert!(rp.ends_with("@acme.com"));
        assert_eq!(row_id_from_address(&rp, SECRET), Some(42));
    }

    #[test]
    fn rejects_tampered_tag() {
        let rp = tagged_return_path("support@acme.com", 42, SECRET).unwrap();
        // Flip the last hex char of the HMAC.
        let mut chars: Vec<char> = rp.chars().collect();
        let at = rp.find('@').unwrap();
        chars[at - 1] = if chars[at - 1] == 'a' { 'b' } else { 'a' };
        let tampered: String = chars.into_iter().collect();
        assert_eq!(row_id_from_address(&tampered, SECRET), None);
    }

    #[test]
    fn rejects_wrong_secret() {
        let rp = tagged_return_path("support@acme.com", 42, SECRET).unwrap();
        assert_eq!(row_id_from_address(&rp, b"different-secret"), None);
    }

    #[test]
    fn ignores_non_verp_addresses() {
        assert_eq!(row_id_from_address("support@acme.com", SECRET), None);
        assert_eq!(row_id_from_address("support+other@acme.com", SECRET), None);
        assert_eq!(row_id_from_address("not-an-address", SECRET), None);
    }

    #[test]
    fn refuses_bad_base() {
        assert_eq!(tagged_return_path("not-an-address", 1, SECRET), None);
        assert_eq!(tagged_return_path("@acme.com", 1, SECRET), None);
        assert_eq!(tagged_return_path("support@", 1, SECRET), None);
        // Already plus-addressed: refuse to stack a second tag.
        assert_eq!(tagged_return_path("support+x@acme.com", 1, SECRET), None);
    }

    #[test]
    fn hex_decode_validates() {
        assert_eq!(hex_decode("00ff"), Some(vec![0x00, 0xff]));
        assert_eq!(hex_decode("abc"), None); // odd length
        assert_eq!(hex_decode("zz"), None); // non-hex
    }
}
