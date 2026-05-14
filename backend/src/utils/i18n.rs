//! Fluent message catalogue loader.
//!
//! Wraps a `FluentBundle` per supported locale, built once on first
//! access from the shared `.ftl` files under `i18n/locales/`. The
//! files are baked into the binary via `include_str!` so the
//! container ships its own translations and a stripped-down deploy
//! can't accidentally serve missing keys.
//!
//! Lookups go through `tr` / `tr_args`, which try the requested
//! locale first and fall back to `DEFAULT_LOCALE` if the key isn't
//! present. The caller never sees a missing-key panic; instead the
//! key itself is returned bracketed (e.g. `{missing-key}`) so the
//! gap is obvious in any rendered output without crashing the
//! response.

use std::collections::HashMap;
use std::sync::OnceLock;

use fluent_bundle::{concurrent::FluentBundle, FluentArgs, FluentResource, FluentValue};
use unic_langid::LanguageIdentifier;

use crate::utils::locale::DEFAULT_LOCALE;
#[cfg(test)]
use crate::utils::locale::SUPPORTED_LOCALES;

/// Embedded Fluent resources, one entry per supported locale.
/// Adding a new locale = add the path here + create the .ftl file.
const FTL_SOURCES: &[(&str, &str)] = &[
    ("en-US", include_str!("../../../i18n/locales/en-US/main.ftl")),
    ("en-GB", include_str!("../../../i18n/locales/en-GB/main.ftl")),
    ("en-AU", include_str!("../../../i18n/locales/en-AU/main.ftl")),
    ("fr-FR", include_str!("../../../i18n/locales/fr-FR/main.ftl")),
    ("nl-NL", include_str!("../../../i18n/locales/nl-NL/main.ftl")),
];

type Bundle = FluentBundle<FluentResource>;

fn bundles() -> &'static HashMap<String, Bundle> {
    static CELL: OnceLock<HashMap<String, Bundle>> = OnceLock::new();
    CELL.get_or_init(build_bundles)
}

fn build_bundles() -> HashMap<String, Bundle> {
    let mut map = HashMap::with_capacity(FTL_SOURCES.len());
    for (tag, source) in FTL_SOURCES {
        let langid: LanguageIdentifier = tag
            .parse()
            .unwrap_or_else(|_| panic!("supported locale {tag} must parse as BCP-47"));
        let resource = FluentResource::try_new(source.to_string())
            .unwrap_or_else(|(_, errors)| {
                panic!("malformed FTL for {tag}: {errors:?}");
            });
        let mut bundle = FluentBundle::new_concurrent(vec![langid]);
        // Fluent injects U+2068/U+2069 directional isolates around
        // interpolated values so RTL works correctly in mixed content.
        // Disable for now: our locale set is LTR-only and the isolate
        // characters leak into snapshot tests as confusing whitespace.
        bundle.set_use_isolating(false);
        bundle
            .add_resource(resource)
            .unwrap_or_else(|errors| panic!("duplicate keys in {tag}: {errors:?}"));
        map.insert(tag.to_string(), bundle);
    }
    map
}

/// Resolve a message key against the requested locale, with
/// fallback to `DEFAULT_LOCALE`. The key is returned bracketed if
/// no catalogue has it (loud-but-non-fatal).
pub fn tr(locale: &LanguageIdentifier, key: &str) -> String {
    tr_args(locale, key, None)
}

/// Resolve a message key with interpolation arguments. Pass
/// `args` as a slice of `(name, value)` tuples; values can be any
/// `Into<FluentValue<'static>>` (string, number, bool).
pub fn tr_with(
    locale: &LanguageIdentifier,
    key: &str,
    args: &[(&str, FluentValue<'static>)],
) -> String {
    let mut fluent_args = FluentArgs::new();
    for (name, value) in args {
        fluent_args.set(*name, value.clone());
    }
    tr_args(locale, key, Some(&fluent_args))
}

fn tr_args(locale: &LanguageIdentifier, key: &str, args: Option<&FluentArgs>) -> String {
    let tag = locale.to_string();
    let primary = bundles().get(&tag);
    let fallback = bundles().get(DEFAULT_LOCALE);

    for bundle in primary.iter().chain(fallback.iter()) {
        if let Some(msg) = bundle.get_message(key) {
            if let Some(pattern) = msg.value() {
                let mut errors = Vec::new();
                let formatted = bundle.format_pattern(pattern, args, &mut errors);
                // Fluent surfaces formatting errors (missing var,
                // bad type) by returning the partial string + an
                // error list. We log nothing here; call sites that
                // care can pre-validate. KISS for now.
                let _ = errors;
                return formatted.into_owned();
            }
        }
    }

    format!("{{{key}}}")
}

/// Compile-time check: every supported locale must have an FTL
/// entry. Called from a test rather than at runtime so a missing
/// catalogue is caught in CI instead of on first request.
#[cfg(test)]
fn supported_have_catalogues() -> bool {
    SUPPORTED_LOCALES
        .iter()
        .all(|tag| FTL_SOURCES.iter().any(|(t, _)| t == tag))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    fn lang(tag: &str) -> LanguageIdentifier {
        LanguageIdentifier::from_str(tag).unwrap()
    }

    #[test]
    fn every_supported_locale_ships_an_ftl_file() {
        assert!(
            supported_have_catalogues(),
            "SUPPORTED_LOCALES drifted from FTL_SOURCES; add the include_str! entry"
        );
    }

    #[test]
    fn bundles_load_without_panicking() {
        let map = bundles();
        for tag in SUPPORTED_LOCALES {
            assert!(map.contains_key(*tag), "bundle missing for {tag}");
        }
    }

    #[test]
    fn tr_returns_locale_specific_string() {
        let aus = tr_with(&lang("en-AU"), "greeting", &[("name", "Kyle".into())]);
        assert!(aus.contains("G'day"), "got: {aus}");
        assert!(aus.contains("Kyle"));

        let us = tr_with(&lang("en-US"), "greeting", &[("name", "Kyle".into())]);
        assert!(us.contains("Hello"), "got: {us}");
    }

    #[test]
    fn missing_key_returns_bracketed_key() {
        let out = tr(&lang("en-US"), "nope-no-such-key");
        assert_eq!(out, "{nope-no-such-key}");
    }

    #[test]
    fn unknown_locale_falls_back_to_default() {
        // No catalogue for de-DE; lookup should land on en-US.
        let out = tr_with(&lang("de-DE"), "greeting", &[("name", "Kyle".into())]);
        assert!(out.contains("Hello"), "expected en-US fallback, got: {out}");
    }

    #[test]
    fn plural_selectors_resolve() {
        let zero = tr_with(&lang("en-US"), "unread-count", &[("count", 0_i64.into())]);
        let one = tr_with(&lang("en-US"), "unread-count", &[("count", 1_i64.into())]);
        let many = tr_with(&lang("en-US"), "unread-count", &[("count", 5_i64.into())]);
        assert!(zero.contains("No new"), "got: {zero}");
        assert!(one.contains("One new"), "got: {one}");
        assert!(many.contains("5 new"), "got: {many}");
    }

    #[test]
    fn interpolation_substitutes_named_args() {
        let out = tr_with(&lang("en-US"), "greeting", &[("name", "Mira".into())]);
        assert!(out.contains("Mira"), "got: {out}");
    }
}
