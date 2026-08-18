//! Session lifetime and concurrency policy.
//!
//! Single source for the three numbers that govern `active_sessions`, which
//! were previously either hardcoded at five call sites (the 7-day TTL) or
//! absent entirely (the ceiling and the cap).
//!
//! Two independent clocks, both enforced server-side:
//!
//! - [`idle_ttl`] is the sliding window. `expires_at` moves forward by this
//!   much on every refresh, so a session dies after a week of silence.
//! - [`max_lifetime`] is the ceiling. Measured from `created_at`, which never
//!   moves, so no amount of activity extends a session past it. Without this a
//!   session used weekly lives forever (OWASP ASVS 5.0 requirement 7.3.2).
//!
//! [`max_sessions_per_user`] bounds how many sessions one account can hold at
//! once. Reaching it evicts the least-recently-active session rather than
//! refusing the login: a user whose old devices are stale should not be locked
//! out of the device in their hand (ASVS 7.1.2 asks for the limit and its
//! behaviour to be documented, not for any particular choice).

use chrono::Duration;

/// Default ceiling on a session's total life. NIST SP 800-63B says
/// reauthentication at AAL1 should happen at least every 30 days.
const DEFAULT_MAX_LIFETIME_DAYS: i64 = 30;

/// Range the operator override is clamped to. The floor is a day because
/// anything shorter is the idle timeout's job; the cap is 90 days because a
/// quarter-long session is already past what 800-63B contemplates.
pub const MIN_MAX_LIFETIME_DAYS: i64 = 1;
pub const MAX_MAX_LIFETIME_DAYS: i64 = 90;

/// Concurrent sessions per user before the oldest is evicted.
const MAX_SESSIONS_PER_USER: i64 = 10;

lazy_static::lazy_static! {
    /// Resolved once at first use. `Config::from_source` validates the same
    /// variable at boot, so a bad value is reported there rather than silently
    /// clamped here at first login.
    static ref MAX_LIFETIME_DAYS: i64 =
        max_lifetime_days_from(std::env::var("NOSDESK_SESSION_MAX_LIFETIME_DAYS").ok().as_deref());
}

/// Parse and clamp the override. Shared with `Config` so boot-time validation
/// and runtime resolution cannot disagree.
pub fn max_lifetime_days_from(raw: Option<&str>) -> i64 {
    raw.and_then(|v| v.trim().parse::<i64>().ok())
        .unwrap_or(DEFAULT_MAX_LIFETIME_DAYS)
        .clamp(MIN_MAX_LIFETIME_DAYS, MAX_MAX_LIFETIME_DAYS)
}

/// The sliding inactivity window applied to `expires_at` on login and refresh.
pub fn idle_ttl() -> Duration {
    Duration::days(7)
}

/// The absolute ceiling, measured from `created_at`.
pub fn max_lifetime() -> Duration {
    Duration::days(*MAX_LIFETIME_DAYS)
}

/// Sessions one user may hold before the least-recently-active is evicted.
pub fn max_sessions_per_user() -> i64 {
    MAX_SESSIONS_PER_USER
}

/// The moment a session created at `created_at` must stop working no matter
/// how recently it was used.
pub fn absolute_deadline(created_at: chrono::NaiveDateTime) -> chrono::NaiveDateTime {
    created_at + max_lifetime()
}

/// The `expires_at` to store for a session, given when it was created. The
/// sliding window never reaches past the ceiling, so an expiry check alone is
/// enough for callers that already load the row.
pub fn next_expiry(created_at: chrono::NaiveDateTime) -> chrono::NaiveDateTime {
    let sliding = chrono::Utc::now().naive_utc() + idle_ttl();
    sliding.min(absolute_deadline(created_at))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn override_parses_and_clamps() {
        assert_eq!(max_lifetime_days_from(Some("14")), 14);
        assert_eq!(max_lifetime_days_from(Some(" 14 ")), 14);
        // Out of range in both directions, and unparseable, fall back to the
        // clamp bounds / the default rather than to zero.
        assert_eq!(max_lifetime_days_from(Some("0")), MIN_MAX_LIFETIME_DAYS);
        assert_eq!(max_lifetime_days_from(Some("365")), MAX_MAX_LIFETIME_DAYS);
        assert_eq!(
            max_lifetime_days_from(Some("soon")),
            DEFAULT_MAX_LIFETIME_DAYS
        );
        assert_eq!(max_lifetime_days_from(None), DEFAULT_MAX_LIFETIME_DAYS);
    }

    #[test]
    fn expiry_is_capped_by_the_ceiling() {
        let now = chrono::Utc::now().naive_utc();

        // A young session slides the full idle window.
        let fresh = next_expiry(now);
        assert!(fresh > now + Duration::days(6));

        // One created just under the ceiling ago gets what's left of it, not
        // another seven days.
        let old = now - max_lifetime() + Duration::hours(1);
        let capped = next_expiry(old);
        assert_eq!(capped, absolute_deadline(old));
        assert!(capped < now + Duration::days(1));
    }
}
