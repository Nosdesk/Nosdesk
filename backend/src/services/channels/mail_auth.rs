//! Reading inbound mail-authentication results (RFC 8601 `Authentication-Results`).
//!
//! Self-hosted IMAP has no provider verdict struct the way SES does, so the only
//! signal that the RFC 5322 `From` domain was authenticated is the
//! `Authentication-Results` header the *receiving* MTA stamped after it ran
//! SPF/DKIM/DMARC. We reduce that to a single boolean — did DMARC pass — because
//! a DMARC pass is exactly "an aligned SPF or DKIM for the header `From` domain,"
//! i.e. the `From` is not forged.
//!
//! Trust model: `Authentication-Results` headers are *prepended* by each hop, so
//! the topmost one in the message is the receiving server we control; any lower
//! ones were on the wire when the message arrived and may have been forged by the
//! sender. We therefore trust only the **topmost** header. When the operator sets
//! `INBOUND_TRUSTED_AUTHSERV_ID`, that header's authserv-id must also match it —
//! otherwise a boundary that never authenticated (and so stamped nothing) would
//! let the sender's own forged top header through.

use super::SenderAuth;

/// The DMARC verdict for the message's `From`, read from the topmost
/// `Authentication-Results` header. `values` are the header values in message
/// order (top to bottom); `trusted_authserv_id`, when `Some`, is required to
/// match the topmost header's authserv-id. Returns [`SenderAuth::Unknown`] for
/// no header, no `dmarc=` result, an authserv-id mismatch, or a non-pass/fail
/// result (`none`, `temperror`, ...); only an explicit `dmarc=fail` yields
/// [`SenderAuth::Fail`].
pub fn sender_auth(values: &[String], trusted_authserv_id: Option<&str>) -> SenderAuth {
    let Some(top) = values.first() else {
        return SenderAuth::Unknown;
    };
    header_dmarc(top, trusted_authserv_id)
}

fn header_dmarc(value: &str, trusted_authserv_id: Option<&str>) -> SenderAuth {
    // "<authserv-id>[ version] ; method=result props ; method=result props ..."
    let (authserv, rest) = match value.split_once(';') {
        Some((a, r)) => (a.trim(), r),
        // No ';' means no result section: authserv-id plus an implicit "none".
        None => return SenderAuth::Unknown,
    };
    // authserv-id is the first token; an optional version digit may follow.
    let authserv_id = authserv.split_whitespace().next().unwrap_or("");
    if let Some(trusted) = trusted_authserv_id {
        if !authserv_id.eq_ignore_ascii_case(trusted) {
            return SenderAuth::Unknown;
        }
    }

    for chunk in split_methods(rest) {
        if let Some(result) = method_result(&chunk, "dmarc") {
            return if result.eq_ignore_ascii_case("pass") {
                SenderAuth::Pass
            } else if result.eq_ignore_ascii_case("fail") {
                SenderAuth::Fail
            } else {
                // none / temperror / permerror: no determination.
                SenderAuth::Unknown
            };
        }
    }
    SenderAuth::Unknown
}

/// Split a resinfo string into `method=result props` chunks on `;`, but never
/// inside a parenthesised comment (RFC 8601 allows `;` inside `(...)`).
fn split_methods(rest: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth: i32 = 0;
    let mut cur = String::new();
    for c in rest.chars() {
        match c {
            '(' => {
                depth += 1;
                cur.push(c);
            }
            ')' => {
                depth = depth.saturating_sub(1);
                cur.push(c);
            }
            ';' if depth == 0 => {
                out.push(std::mem::take(&mut cur));
            }
            _ => cur.push(c),
        }
    }
    if !cur.trim().is_empty() {
        out.push(cur);
    }
    out
}

/// If `chunk` is a `method=result ...` spec for `method`, return `result` (the
/// first token after `=`, stopping at whitespace or a `(` comment). Matches the
/// method name at a token boundary so `x-dmarc` never matches `dmarc`.
fn method_result(chunk: &str, method: &str) -> Option<String> {
    let (key, val) = chunk.split_once('=')?;
    if !key.trim().eq_ignore_ascii_case(method) {
        return None;
    }
    let val = val.trim_start();
    let end = val
        .find(|c: char| c.is_whitespace() || c == '(')
        .unwrap_or(val.len());
    let result = val[..end].trim();
    (!result.is_empty()).then(|| result.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Vec<String> {
        vec![s.to_string()]
    }

    #[test]
    fn dmarc_pass_is_pass() {
        let h = "mx.google.com; dkim=pass header.d=co.com; spf=pass smtp.mailfrom=co.com; \
                 dmarc=pass (p=REJECT sp=REJECT dis=NONE) header.from=co.com";
        assert_eq!(sender_auth(&v(h), None), SenderAuth::Pass);
    }

    #[test]
    fn dmarc_fail_is_fail() {
        assert_eq!(
            sender_auth(&v("mx.example.net; dmarc=fail header.from=co.com"), None),
            SenderAuth::Fail
        );
    }

    #[test]
    fn dmarc_none_or_absent_result_is_unknown() {
        // Explicit `none` (no policy), and spf/dkim pass without a dmarc result,
        // are both "no determination" — trusted-as-before, not a spoofing signal.
        assert_eq!(
            sender_auth(&v("mx.example.net; dmarc=none"), None),
            SenderAuth::Unknown
        );
        assert_eq!(
            sender_auth(
                &v("mx.example.net; spf=pass smtp.mailfrom=co.com; dkim=pass header.d=co.com"),
                None
            ),
            SenderAuth::Unknown
        );
        assert_eq!(
            sender_auth(&v("mx.net; dmarc=temperror header.from=co.com"), None),
            SenderAuth::Unknown
        );
    }

    #[test]
    fn no_or_empty_header_is_unknown() {
        assert_eq!(sender_auth(&[], None), SenderAuth::Unknown);
        assert_eq!(sender_auth(&v("mx.example.net"), None), SenderAuth::Unknown);
        assert_eq!(
            sender_auth(&v("mx.example.net; none"), None),
            SenderAuth::Unknown
        );
    }

    #[test]
    fn only_the_topmost_header_is_trusted() {
        // A forged lower header claiming pass must not win over the real top one.
        let headers = vec![
            "mx.ourserver.net; dmarc=fail header.from=co.com".to_string(),
            "attacker.example; dmarc=pass header.from=co.com".to_string(),
        ];
        assert_eq!(sender_auth(&headers, None), SenderAuth::Fail);
    }

    #[test]
    fn authserv_id_must_match_when_configured() {
        let h = v("mx.google.com; dmarc=pass header.from=co.com");
        assert_eq!(sender_auth(&h, Some("mx.google.com")), SenderAuth::Pass);
        assert_eq!(sender_auth(&h, Some("MX.GOOGLE.COM")), SenderAuth::Pass);
        // A pass from an unexpected authserv-id is ignored (untrusted boundary).
        assert_eq!(
            sender_auth(&h, Some("mx.ourserver.net")),
            SenderAuth::Unknown
        );
        // authserv-id with a trailing version token still matches.
        assert_eq!(
            sender_auth(
                &v("mx.google.com 1; dmarc=pass header.from=co.com"),
                Some("mx.google.com")
            ),
            SenderAuth::Pass
        );
    }

    #[test]
    fn semicolons_inside_comments_do_not_split_methods() {
        let h = v("mx.net; dmarc=pass (policy: p=none; sp=none) header.from=co.com");
        assert_eq!(sender_auth(&h, None), SenderAuth::Pass);
    }

    #[test]
    fn method_prefix_is_matched_at_a_boundary() {
        // `x-dmarc` must not be read as the dmarc result.
        assert_eq!(
            sender_auth(&v("mx.net; x-dmarc=pass header.from=co.com"), None),
            SenderAuth::Unknown
        );
    }

    #[test]
    fn result_case_insensitive() {
        assert_eq!(
            sender_auth(&v("mx.net; DMARC=Pass header.from=co.com"), None),
            SenderAuth::Pass
        );
    }
}
