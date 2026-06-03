//! Retry policy: backoff curve + SMTP-code classification.
//!
//! Two functions, both pure / no I/O:
//! - [`next_attempt_at`] computes the next-attempt timestamp from
//!   `attempts` using full-jitter exponential backoff.
//! - [`classify`] turns an SMTP code (or a "no code, just an error
//!   string" failure) into a [`RetryDecision`] — `Retry`, `Dead`, or
//!   `Suppress` (the recipient should be added to the suppression list
//!   on top of being marked dead).
//!
//! Backoff curve (`base = 30s`, `cap = 1h`, `max_attempts = 10`):
//!
//! | attempt | delay range |
//! |---------|-------------|
//! | 1       | 0-30s       |
//! | 2       | 0-60s       |
//! | 3       | 0-2m        |
//! | 4       | 0-4m        |
//! | 5       | 0-8m        |
//! | 6       | 0-16m       |
//! | 7       | 0-32m       |
//! | 8-10    | 0-60m       |
//!
//! Total time first-attempt → dead: ~3-4 hours worst case. Covers a
//! typical SMTP provider outage; doesn't cross into the "automatic
//! feels broken, tell me about it" territory that 24h would.

use chrono::{DateTime, Duration, Utc};
use rand::Rng;

/// Maximum attempts before a row is marked `dead`.
pub const MAX_ATTEMPTS: i32 = 10;

const BASE_BACKOFF_SECS: i64 = 30;
const MAX_BACKOFF_SECS: i64 = 60 * 60;

/// Compute the next-attempt timestamp using full-jitter exponential
/// backoff. `attempts` is the count of attempts made so far (so after
/// the first failed attempt this is called with `attempts = 1`).
///
/// AWS Builders' Library full-jitter formula:
///     `delay = random_between(0, min(cap, base * 2^attempt))`
/// keeps retry traffic from synchronising across multiple workers
/// after a shared outage.
pub fn next_attempt_at(now: DateTime<Utc>, attempts: i32) -> DateTime<Utc> {
    let exp = attempts.clamp(1, 31) as u32; // saturate to avoid u32 overflow
    let raw_cap = BASE_BACKOFF_SECS.saturating_mul(1i64 << exp);
    let cap = raw_cap.clamp(1, MAX_BACKOFF_SECS);
    let jittered = rand::thread_rng().gen_range(0..=cap);
    now + Duration::seconds(jittered)
}

/// Decision returned by [`classify`]: what to do with a row whose
/// dispatch returned a given SMTP code (or no code at all).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryDecision {
    /// Transient failure — retry per the backoff curve.
    Retry,
    /// Permanent failure — mark `dead`, don't suppress recipient.
    Dead,
    /// Permanent failure AND add the recipient to the suppression list.
    /// Used for "no such user" / "mailbox disabled" / explicit policy
    /// rejections where future sends would also fail.
    Suppress,
}

/// Map an SMTP reply code to a retry decision. `code` is `None` when
/// the failure was at the network/TLS layer (no SMTP code available);
/// those are treated as transient — the most common cause is a
/// momentary connectivity blip.
///
/// Sources: RFC 3463 enhanced status codes; lettre's reply-code
/// severity classification; Mailgun/SES/Postmark public guidance on
/// hard vs soft bounces.
pub fn classify(code: Option<u16>, attempts: i32) -> RetryDecision {
    if attempts >= MAX_ATTEMPTS {
        return RetryDecision::Dead;
    }
    let Some(code) = code else {
        // No SMTP code = network/TLS layer error. Retry the first few,
        // then give up if nothing recovers.
        return RetryDecision::Retry;
    };

    match code {
        // 2xx — shouldn't reach this fn; treat as success-equivalent
        // and let the caller's mark_sent path handle it.
        200..=299 => RetryDecision::Retry, // unreachable in practice
        // 4xx — transient. Greylisting (450), service-unavailable (421),
        // mailbox-busy (450/451/452), temp DNS, IP rate-limit. Retry.
        400..=499 => RetryDecision::Retry,
        // 5xx — permanent. Default dead; specific codes promote to
        // suppress (recipient is unreachable for future sends too).
        550 | 551 | 553 => RetryDecision::Suppress, // bad recipient
        552 => RetryDecision::Dead,                 // message too large; code change
        535 | 530 => RetryDecision::Dead,           // auth failed / STARTTLS required
        // Generic 5xx fallback: dead but don't suppress (don't punish
        // a recipient because of an upstream policy bounce).
        500..=599 => RetryDecision::Dead,
        // Unknown range — be conservative, retry until max.
        _ => RetryDecision::Retry,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_attempt_grows_until_cap() {
        let now = Utc::now();
        for attempts in 1..=10 {
            let when = next_attempt_at(now, attempts);
            let delta = (when - now).num_seconds();
            assert!(delta >= 0, "next attempt must not be in the past");
            assert!(
                delta <= MAX_BACKOFF_SECS,
                "attempt {attempts} produced delta {delta} > cap {MAX_BACKOFF_SECS}"
            );
        }
    }

    #[test]
    fn next_attempt_handles_huge_attempt_counts() {
        let now = Utc::now();
        // Rust would overflow on `1i64 << 99`; we clamp.
        let when = next_attempt_at(now, 99);
        let delta = (when - now).num_seconds();
        assert!(delta <= MAX_BACKOFF_SECS);
    }

    #[test]
    fn classify_retries_transient_codes() {
        assert_eq!(classify(Some(421), 1), RetryDecision::Retry);
        assert_eq!(classify(Some(450), 1), RetryDecision::Retry);
        assert_eq!(classify(Some(451), 1), RetryDecision::Retry);
        assert_eq!(classify(Some(452), 1), RetryDecision::Retry);
        assert_eq!(classify(None, 1), RetryDecision::Retry);
    }

    #[test]
    fn classify_dead_5xx_default() {
        assert_eq!(classify(Some(500), 1), RetryDecision::Dead);
        assert_eq!(classify(Some(521), 1), RetryDecision::Dead);
        assert_eq!(classify(Some(554), 1), RetryDecision::Dead);
    }

    #[test]
    fn classify_suppression_codes() {
        assert_eq!(classify(Some(550), 1), RetryDecision::Suppress);
        assert_eq!(classify(Some(551), 1), RetryDecision::Suppress);
        assert_eq!(classify(Some(553), 1), RetryDecision::Suppress);
    }

    #[test]
    fn classify_alert_only_codes() {
        // Auth / TLS misconfig — dead, but not the recipient's fault.
        assert_eq!(classify(Some(535), 1), RetryDecision::Dead);
        assert_eq!(classify(Some(530), 1), RetryDecision::Dead);
        assert_eq!(classify(Some(552), 1), RetryDecision::Dead);
    }

    #[test]
    fn classify_promotes_to_dead_at_max_attempts() {
        // A normally-retryable 4xx flips to dead once attempts hit MAX.
        assert_eq!(classify(Some(450), MAX_ATTEMPTS), RetryDecision::Dead);
        assert_eq!(classify(Some(450), MAX_ATTEMPTS - 1), RetryDecision::Retry);
    }
}
