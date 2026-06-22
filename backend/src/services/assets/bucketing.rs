//! Fleet bucketing for the inventory planning lenses (OS family,
//! warranty window). The inventory list groups assets by these derived
//! buckets so an agent can see "what's about to fall out of warranty" or
//! "what still runs Windows" across the whole fleet. Bucketing lives here
//! so the derivation is computed once, server-side, and the client groups
//! by the resulting key string rather than re-deriving the heuristics.

use chrono::NaiveDate;

/// Bucket an OS string into a coarse family. Matches on substrings of the
/// reported `operating_system` because vendors spell it many ways
/// ("Windows 11 Pro", "macOS 14.2", "Ubuntu 22.04").
pub fn classify_os(raw: Option<&str>) -> &'static str {
    let s = raw.unwrap_or("").to_lowercase();
    if s.contains("windows") {
        return "windows";
    }
    if s.contains("mac") || s.contains("os x") || s.contains("darwin") {
        return "macos";
    }
    if s.contains("linux") || s.contains("ubuntu") || s.contains("fedora") || s.contains("debian") {
        return "linux";
    }
    if s.contains("ios") || s.contains("iphone") || s.contains("ipad") {
        return "ios";
    }
    if s.contains("android") {
        return "android";
    }
    "other"
}

/// Bucket a warranty end date relative to `today` into a planning window.
/// `None` end date is `unknown` rather than guessed, so a missing date
/// never reads as "in warranty".
pub fn classify_warranty_window(end: Option<NaiveDate>, today: NaiveDate) -> &'static str {
    let Some(end) = end else {
        return "unknown";
    };
    let days = (end - today).num_days();
    if days < 0 {
        "expired"
    } else if days <= 30 {
        "expiring_30d"
    } else if days <= 90 {
        "expiring_90d"
    } else {
        "active"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_os_buckets_known_families() {
        assert_eq!(classify_os(Some("Windows 11 Pro")), "windows");
        assert_eq!(classify_os(Some("macOS 14.2 Sonoma")), "macos");
        assert_eq!(classify_os(Some("Ubuntu 22.04 LTS")), "linux");
        assert_eq!(classify_os(Some("iPadOS 17")), "ios");
        assert_eq!(classify_os(Some("Android 14")), "android");
        assert_eq!(classify_os(Some("FreeBSD")), "other");
        assert_eq!(classify_os(None), "other");
    }

    #[test]
    fn classify_warranty_window_buckets_by_horizon() {
        let today = NaiveDate::from_ymd_opt(2026, 6, 21).unwrap();
        assert_eq!(classify_warranty_window(None, today), "unknown");
        assert_eq!(
            classify_warranty_window(NaiveDate::from_ymd_opt(2026, 6, 20), today),
            "expired"
        );
        assert_eq!(
            classify_warranty_window(NaiveDate::from_ymd_opt(2026, 7, 10), today),
            "expiring_30d"
        );
        assert_eq!(
            classify_warranty_window(NaiveDate::from_ymd_opt(2026, 9, 1), today),
            "expiring_90d"
        );
        assert_eq!(
            classify_warranty_window(NaiveDate::from_ymd_opt(2027, 6, 21), today),
            "active"
        );
    }
}
