//! Rules engine starter catalog (Phase 1 / Wave 8).
//!
//! Ships ~half a dozen ready-made manual rules baked into the
//! repo as a JSON file (`backend/data/starter-rules.json`). The
//! admin Settings → Rules page lets admins browse the catalog and
//! copy any entry as a new Draft rule in their workspace; per
//! decision 35 in the plan, we never bulk-insert these on
//! workspace creation. The catalog is in-memory, loaded once at
//! startup; the JSON is checked into the repo so changes go
//! through PR review like any other config.

use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// One catalog entry. Names + descriptions are pre-translated for
/// every supported locale; the rest is locale-independent (actions
/// reference template tokens via the renderer, not localised
/// strings).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StarterRule {
    pub id: String,
    pub name_en: String,
    pub name_fr: String,
    pub name_nl: String,
    pub description_en: String,
    pub description_fr: String,
    pub description_nl: String,
    pub trigger_kind: String,
    pub conditions: Value,
    pub actions: Value,
}

impl StarterRule {
    /// Returns the localised name for `locale` (BCP-47 prefix
    /// match, falls back to en).
    pub fn name_for(&self, locale: &str) -> &str {
        match locale_prefix(locale) {
            "fr" => &self.name_fr,
            "nl" => &self.name_nl,
            _ => &self.name_en,
        }
    }

    /// Returns the localised description for `locale` (BCP-47
    /// prefix match, falls back to en).
    pub fn description_for(&self, locale: &str) -> &str {
        match locale_prefix(locale) {
            "fr" => &self.description_fr,
            "nl" => &self.description_nl,
            _ => &self.description_en,
        }
    }
}

fn locale_prefix(locale: &str) -> &str {
    locale.split('-').next().unwrap_or(locale)
}

/// Embedded at compile time so the binary has the catalog with
/// no filesystem dependency at runtime. `include_str!` walks up
/// from `services/starter_catalog.rs` to `data/starter-rules.json`.
static RAW_CATALOG: &str = include_str!("../../data/starter-rules.json");

/// Parsed-once view of the catalog. Lazy so a malformed file
/// surfaces with a clear panic at first access (test or boot)
/// rather than blowing up serde inside every endpoint call.
static CATALOG: Lazy<Vec<StarterRule>> = Lazy::new(|| {
    serde_json::from_str(RAW_CATALOG).expect(
        "backend/data/starter-rules.json failed to parse; \
         the catalog ships in the binary and a malformed file \
         is a build-time bug, not a runtime one",
    )
});

/// Return every catalog entry. The admin browse endpoint maps
/// them through `StarterRule::name_for` / `description_for` for
/// the caller's locale before sending.
pub fn list() -> &'static [StarterRule] {
    CATALOG.as_slice()
}

/// Find a catalog entry by id. Used by the
/// `POST /api/rules?from_catalog_id=X` copy-as-new-rule path.
pub fn find(id: &str) -> Option<&'static StarterRule> {
    CATALOG.iter().find(|r| r.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_parses_at_startup() {
        // Touching CATALOG forces the Lazy to evaluate; a
        // malformed file panics here rather than the first
        // production hit.
        let entries = list();
        assert!(!entries.is_empty(), "catalog ships with at least one entry");
    }

    #[test]
    fn every_entry_carries_required_localisations() {
        for rule in list() {
            assert!(!rule.id.is_empty(), "id missing on {:?}", rule);
            assert!(!rule.name_en.is_empty(), "name_en missing on {}", rule.id);
            assert!(!rule.name_fr.is_empty(), "name_fr missing on {}", rule.id);
            assert!(!rule.name_nl.is_empty(), "name_nl missing on {}", rule.id);
        }
    }

    #[test]
    fn find_returns_expected_entry() {
        let rule = find("ack-and-acknowledge").expect("entry exists");
        assert_eq!(rule.trigger_kind, "manual");
    }

    #[test]
    fn locale_picker_falls_back_to_english() {
        let rule = list().first().expect("catalog has at least one entry");
        assert_eq!(rule.name_for("en-US"), rule.name_en);
        assert_eq!(rule.name_for("en-GB"), rule.name_en);
        assert_eq!(rule.name_for("fr-FR"), rule.name_fr);
        assert_eq!(rule.name_for("nl-NL"), rule.name_nl);
        // Unknown locale falls back to English.
        assert_eq!(rule.name_for("de-DE"), rule.name_en);
    }
}
