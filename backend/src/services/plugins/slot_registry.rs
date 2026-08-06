//! Plugin UI slot allowlist.
//!
//! Read from `plugin_slots.generated.json`, which is generated from the single
//! source of truth in `packages/core/src/types/pluginSlots.ts` by
//! `pnpm --filter @nosdesk/core build:slots` and drift-checked in CI. There is
//! no hand-maintained slot list here — a manifest edit on the TS side flows
//! through the generated JSON, so the frontend taxonomy and this validator can
//! never silently disagree.

use std::sync::LazyLock;

use serde::Deserialize;

/// One slot definition, mirroring the TS `SlotDef`. Only the fields the backend
/// needs to enforce are read; the rest (order, description, mechanism) are
/// deserialized for completeness / future use.
#[derive(Debug, Clone, Deserialize)]
pub struct SlotDef {
    /// Canonical dotted identifier.
    pub name: String,
    pub mechanism: String,
    pub context: String,
    pub cardinality: String,
    /// Host chrome drawn around a contribution ("card" / "none"), unless the
    /// manifest component overrides it. Defaulted so an older generated JSON
    /// (or a hand-rolled fixture) still parses.
    #[serde(default = "default_chrome")]
    pub chrome: String,
    pub order: i64,
    pub status: String,
    /// Legacy flat names still accepted at validation time.
    #[serde(default)]
    pub aliases: Vec<String>,
    pub description: String,
}

fn default_chrome() -> String {
    "none".to_string()
}

static RAW: &str = include_str!("plugin_slots.generated.json");

/// The parsed registry. Panics at first access if the generated JSON is
/// malformed, which only happens if the committed artifact was hand-edited or
/// the generator regressed — both caught in CI before merge.
pub static SLOT_REGISTRY: LazyLock<Vec<SlotDef>> = LazyLock::new(|| {
    serde_json::from_str(RAW).expect("plugin_slots.generated.json must be valid SlotDef JSON")
});

/// True if `name` matches a canonical slot name or any of its aliases.
pub fn is_known_slot(name: &str) -> bool {
    get_slot(name).is_some()
}

/// Resolve a canonical-or-alias slot name to its definition.
pub fn get_slot(name: &str) -> Option<&'static SlotDef> {
    SLOT_REGISTRY
        .iter()
        .find(|s| s.name == name || s.aliases.iter().any(|a| a == name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_json_parses_and_is_nonempty() {
        assert!(!SLOT_REGISTRY.is_empty());
    }

    #[test]
    fn known_mechanisms_and_statuses() {
        for s in SLOT_REGISTRY.iter() {
            assert!(
                matches!(s.mechanism.as_str(), "panel" | "action"),
                "unknown mechanism {} on {}",
                s.mechanism,
                s.name
            );
            assert!(
                matches!(s.status.as_str(), "stable" | "reserved" | "experimental"),
                "unknown status {} on {}",
                s.status,
                s.name
            );
            assert!(
                matches!(
                    s.context.as_str(),
                    "ticket" | "asset" | "user" | "address" | "documentationPage" | "none"
                ),
                "unknown context {} on {}",
                s.context,
                s.name
            );
            assert!(matches!(s.cardinality.as_str(), "one" | "many"));
            assert!(s.order >= 0);
            assert!(!s.description.is_empty());
        }
    }

    #[test]
    fn chrome_is_known_and_action_slots_carry_none() {
        for s in SLOT_REGISTRY.iter() {
            assert!(
                matches!(s.chrome.as_str(), "card" | "none"),
                "unknown chrome {} on {}",
                s.chrome,
                s.name
            );
            // An action slot is rendered as native host chrome (menu item, nav
            // link), so there is no iframe to wrap. Anything but "none" here
            // would mean the generator and the taxonomy have diverged.
            if s.mechanism == "action" {
                assert_eq!(
                    s.chrome, "none",
                    "action slot {} must not draw a card",
                    s.name
                );
            }
        }
    }

    #[test]
    fn live_slots_and_aliases_resolve() {
        // The two mounted slots plus their legacy aliases must validate.
        assert!(is_known_slot("ticket.sidebar.panel"));
        assert!(is_known_slot("ticket-sidebar"));
        assert!(is_known_slot("asset.info.panel"));
        assert!(is_known_slot("asset-info-panels"));
        assert!(!is_known_slot("nope.not.a.slot"));
    }
}
