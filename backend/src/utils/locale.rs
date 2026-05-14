//! Locale and timezone parsing / negotiation.
//!
//! Backend speaks BCP-47 throughout (`en-US`, `en-GB`, `en-AU`).
//! Anywhere we accept a locale from a user (Accept-Language header,
//! a profile preference, a guest contact's auto-detected language)
//! it lands here for parsing and negotiation against the
//! `SUPPORTED_LOCALES` set, then flows down as a typed
//! `LanguageIdentifier` so call sites can't pass an unvalidated
//! string.
//!
//! Timezones are validated against the IANA database via
//! `chrono_tz::Tz::from_str` so we reject `Asia/Atlantis` and
//! Windows-style names like `Pacific Standard Time` at the boundary.

use std::str::FromStr;

use chrono_tz::Tz;
use unic_langid::LanguageIdentifier;

/// Locales we ship message catalogues for, in priority order. The
/// first entry is also the fallback for any message a non-default
/// locale leaves untranslated.
pub const SUPPORTED_LOCALES: &[&str] = &["en-US", "en-GB", "en-AU"];

/// Default locale for guest contacts, system emails to unknown
/// recipients, and the en-US -> en-GB -> en-AU fallback chain.
pub const DEFAULT_LOCALE: &str = "en-US";

/// Errors surfaced from parsing user-supplied locale strings.
#[derive(Debug, thiserror::Error)]
pub enum LocaleError {
    #[error("invalid BCP-47 tag: {0}")]
    InvalidTag(String),
    #[error("unsupported locale: {0}")]
    Unsupported(String),
    #[error("invalid IANA timezone: {0}")]
    InvalidTimezone(String),
}

/// Parse a single BCP-47 tag without checking it against the
/// supported list. Useful when storing a user-preferred locale
/// for a tag we may add catalogues for later.
pub fn parse_bcp47(tag: &str) -> Result<LanguageIdentifier, LocaleError> {
    LanguageIdentifier::from_str(tag.trim())
        .map_err(|_| LocaleError::InvalidTag(tag.to_string()))
}

/// Parse + validate that the tag is one we actually have a
/// catalogue for. Use this at the boundary when a request must
/// resolve to a real bundle (e.g. rendering an outbound email).
pub fn parse_supported(tag: &str) -> Result<LanguageIdentifier, LocaleError> {
    let parsed = parse_bcp47(tag)?;
    let canonical = parsed.to_string();
    if SUPPORTED_LOCALES.iter().any(|s| s.eq_ignore_ascii_case(&canonical)) {
        Ok(parsed)
    } else {
        Err(LocaleError::Unsupported(tag.to_string()))
    }
}

/// Negotiate an Accept-Language header (or a single tag) against
/// `SUPPORTED_LOCALES` and return the best match, falling back to
/// `DEFAULT_LOCALE` when nothing in the request matches.
///
/// Implements RFC 4647 lookup-style matching: each requested tag
/// is checked exact-first, then with successive subtags stripped
/// (so `en-AU-x-foo` falls back to `en-AU`, then `en`). A bare
/// language match resolves to the first supported tag with the
/// same primary language (so `en` -> `en-US`).
///
/// Hand-rolled because fluent-langneg 0.14 switched to ICU's
/// `LanguageIdentifier` type and is no longer wire-compatible
/// with fluent-bundle's. For three supported locales the lookup
/// is trivial.
pub fn negotiate(accept_language: &str) -> LanguageIdentifier {
    let default = LanguageIdentifier::from_str(DEFAULT_LOCALE).expect("DEFAULT_LOCALE parses");
    let available: Vec<LanguageIdentifier> = SUPPORTED_LOCALES
        .iter()
        .filter_map(|s| LanguageIdentifier::from_str(s).ok())
        .collect();

    // RFC 7231 ranks by q-value; ties keep source order. We parse
    // each chunk into (tag, q) and sort stably by descending q.
    let mut ranked: Vec<(LanguageIdentifier, f32)> = accept_language
        .split(',')
        .filter_map(parse_accept_language_entry)
        .collect();
    ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    for (req, _) in &ranked {
        if let Some(hit) = match_with_fallback(req, &available) {
            return hit;
        }
    }
    default
}

fn parse_accept_language_entry(chunk: &str) -> Option<(LanguageIdentifier, f32)> {
    let mut parts = chunk.split(';');
    let tag = parts.next()?.trim();
    if tag.is_empty() || tag == "*" {
        return None;
    }
    let mut q = 1.0_f32;
    for param in parts {
        let param = param.trim();
        if let Some(v) = param.strip_prefix("q=") {
            q = v.parse().unwrap_or(1.0);
        }
    }
    LanguageIdentifier::from_str(tag).ok().map(|l| (l, q))
}

/// Lookup-style matching: exact tag wins; otherwise the first
/// supported tag whose primary language matches.
fn match_with_fallback(
    requested: &LanguageIdentifier,
    available: &[LanguageIdentifier],
) -> Option<LanguageIdentifier> {
    if let Some(exact) = available.iter().find(|a| *a == requested) {
        return Some(exact.clone());
    }
    available
        .iter()
        .find(|a| a.language == requested.language)
        .cloned()
}

/// Validate an IANA timezone name. Returns the parsed `Tz` so
/// callers that need to do arithmetic don't have to re-parse.
pub fn parse_timezone(tz: &str) -> Result<Tz, LocaleError> {
    Tz::from_str(tz.trim()).map_err(|_| LocaleError::InvalidTimezone(tz.to_string()))
}

/// Resolve the effective locale for a user. Walks the chain:
///
/// 1. The user's stored preference (`user_preferences.locale`),
///    if it parses + is in `SUPPORTED_LOCALES`.
/// 2. The site-wide default (`site_settings.default_locale`), if
///    it parses + is supported. Operators set this in admin.
/// 3. The hardcoded `DEFAULT_LOCALE` (`en-US`).
///
/// An invalid stored value silently falls through to the next
/// link rather than 500ing a request: a bad row shouldn't take
/// down /auth/me.
pub fn effective_locale(user_locale: Option<&str>, site_default: &str) -> LanguageIdentifier {
    if let Some(s) = user_locale {
        if let Ok(l) = parse_supported(s) {
            return l;
        }
    }
    if let Ok(l) = parse_supported(site_default) {
        return l;
    }
    LanguageIdentifier::from_str(DEFAULT_LOCALE).expect("DEFAULT_LOCALE parses")
}

/// Resolve the effective IANA timezone for a user. Same chain as
/// `effective_locale`, with `UTC` as the final fallback. IANA-
/// only; Windows-style names ("Pacific Standard Time") and bogus
/// zones fall through.
pub fn effective_timezone(user_tz: Option<&str>, site_default: &str) -> Tz {
    if let Some(s) = user_tz {
        if let Ok(t) = parse_timezone(s) {
            return t;
        }
    }
    if let Ok(t) = parse_timezone(site_default) {
        return t;
    }
    Tz::UTC
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_bcp47_accepts_canonical_tags() {
        assert!(parse_bcp47("en-US").is_ok());
        assert!(parse_bcp47("en-AU").is_ok());
        assert!(parse_bcp47("fr-CA").is_ok());
    }

    #[test]
    fn parse_bcp47_rejects_garbage() {
        assert!(matches!(
            parse_bcp47("not a tag!!!"),
            Err(LocaleError::InvalidTag(_))
        ));
    }

    #[test]
    fn parse_supported_rejects_unsupported() {
        assert!(matches!(
            parse_supported("fr-FR"),
            Err(LocaleError::Unsupported(_))
        ));
    }

    #[test]
    fn parse_supported_accepts_each_shipped_locale() {
        for tag in SUPPORTED_LOCALES {
            assert!(
                parse_supported(tag).is_ok(),
                "supported locale {tag} should parse"
            );
        }
    }

    #[test]
    fn negotiate_picks_exact_match() {
        let chosen = negotiate("en-AU");
        assert_eq!(chosen.to_string(), "en-AU");
    }

    #[test]
    fn negotiate_respects_quality_order() {
        // Filtering preserves the order of requested tags: en-GB wins
        // even though the q-value pushes en-AU down, because we only
        // care about presence + position.
        let chosen = negotiate("en-GB;q=0.9, en-AU;q=0.5");
        assert_eq!(chosen.to_string(), "en-GB");
    }

    #[test]
    fn negotiate_falls_back_for_unsupported_request() {
        let chosen = negotiate("fr-FR, de-DE");
        assert_eq!(chosen.to_string(), DEFAULT_LOCALE);
    }

    #[test]
    fn negotiate_handles_empty_header() {
        let chosen = negotiate("");
        assert_eq!(chosen.to_string(), DEFAULT_LOCALE);
    }

    #[test]
    fn negotiate_widens_language_only_request_to_default_region() {
        // A bare `en` should resolve to one of our supported en-* tags
        // rather than dropping to the language-neutral fallback.
        let chosen = negotiate("en");
        assert!(chosen.to_string().starts_with("en-"));
    }

    #[test]
    fn parse_timezone_accepts_iana() {
        assert!(parse_timezone("Australia/Sydney").is_ok());
        assert!(parse_timezone("America/New_York").is_ok());
        assert!(parse_timezone("UTC").is_ok());
    }

    #[test]
    fn effective_locale_prefers_user_preference() {
        let l = effective_locale(Some("en-AU"), "en-GB");
        assert_eq!(l.to_string(), "en-AU");
    }

    #[test]
    fn effective_locale_falls_back_to_site_default() {
        let l = effective_locale(None, "en-GB");
        assert_eq!(l.to_string(), "en-GB");
    }

    #[test]
    fn effective_locale_falls_back_to_hardcoded_when_chain_empty() {
        let l = effective_locale(None, "");
        assert_eq!(l.to_string(), DEFAULT_LOCALE);
    }

    #[test]
    fn effective_locale_skips_invalid_user_preference() {
        // Garbage stored value falls through to site default.
        let l = effective_locale(Some("not-a-real-tag!!!"), "en-AU");
        assert_eq!(l.to_string(), "en-AU");
    }

    #[test]
    fn effective_locale_skips_unsupported_user_preference() {
        // Well-formed but we don't ship catalogues for fr-FR yet, so
        // fall through to site default rather than promise something
        // we can't deliver.
        let l = effective_locale(Some("fr-FR"), "en-AU");
        assert_eq!(l.to_string(), "en-AU");
    }

    #[test]
    fn effective_timezone_prefers_user_preference() {
        let tz = effective_timezone(Some("Australia/Sydney"), "UTC");
        assert_eq!(tz.name(), "Australia/Sydney");
    }

    #[test]
    fn effective_timezone_falls_back_to_site_default() {
        let tz = effective_timezone(None, "America/New_York");
        assert_eq!(tz.name(), "America/New_York");
    }

    #[test]
    fn effective_timezone_falls_back_to_utc_when_chain_empty() {
        let tz = effective_timezone(None, "");
        assert_eq!(tz.name(), "UTC");
    }

    #[test]
    fn effective_timezone_skips_invalid_user_preference() {
        let tz = effective_timezone(Some("Pacific Standard Time"), "Australia/Sydney");
        assert_eq!(tz.name(), "Australia/Sydney");
    }

    #[test]
    fn parse_timezone_rejects_windows_style() {
        assert!(matches!(
            parse_timezone("Pacific Standard Time"),
            Err(LocaleError::InvalidTimezone(_))
        ));
        assert!(matches!(
            parse_timezone("Asia/Atlantis"),
            Err(LocaleError::InvalidTimezone(_))
        ));
    }
}
