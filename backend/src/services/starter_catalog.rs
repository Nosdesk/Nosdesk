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
//!
//! The JSON can't hold workspace ids, so a starter's steps may name
//! what they need instead, and [`WorkspaceRefs::resolve_actions`]
//! looks it up in the caller's workspace:
//!
//! - `set_status` with `{"category": "done"}`: the workspace's first
//!   state in that category, as `workflow_state_id`. Adding a starter
//!   whose category has no state in the workspace is refused.
//! - `add_tags` with `{"tag_names": {"en": [...], "fr": [...], "nl": [...]}}`:
//!   the workspace's tags with those names (any case), as `tag_ids`.

use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::db::DbConnection;
use crate::models::{NewTag, Tag, WorkflowStateCategory};
use crate::repository::{tags, workflow_states};

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

/// The status category a `set_status` step names instead of a state.
pub fn status_category(config: &Value) -> Option<WorkflowStateCategory> {
    serde_json::from_value(config.get("category")?.clone()).ok()
}

/// The tag names an `add_tags` step names instead of tag ids, in
/// `locale` (BCP-47 prefix match, falls back to en).
pub fn tag_names(config: &Value, locale: &str) -> Option<Vec<String>> {
    let by_locale = config.get("tag_names")?;
    let names = match locale_prefix(locale) {
        prefix @ ("fr" | "nl") => by_locale.get(prefix),
        _ => None,
    }
    .or_else(|| by_locale.get("en"))?
    .as_array()?;
    Some(
        names
            .iter()
            .filter_map(Value::as_str)
            .map(|n| n.trim().to_string())
            .filter(|n| !n.is_empty())
            .collect(),
    )
}

/// What [`WorkspaceRefs::resolve_actions`] resolves a starter for, which
/// decides what happens to a named status or tag the workspace doesn't
/// have.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    /// The browse list. A missing tag is left out of `tag_ids`, and the
    /// step keeps `tag_names` (every name, in the request locale) so the
    /// list can name it. A missing status leaves the step without one.
    Browse,
    /// Adding the starter to the workspace. A missing tag is created. A
    /// missing status refuses the starter before anything is written.
    Add,
}

/// Why [`WorkspaceRefs::resolve_actions`] couldn't resolve a starter.
#[derive(Debug)]
pub enum ResolveError {
    /// A status step names a category the workspace has no state in
    /// (none, or all archived). Only when adding.
    NoStatusIn(WorkflowStateCategory),
    Db(diesel::result::Error),
}

impl From<diesel::result::Error> for ResolveError {
    fn from(e: diesel::result::Error) -> Self {
        Self::Db(e)
    }
}

/// Looks up a starter's named statuses and tags in the pinned
/// workspace. Loads the workspace's tags once, so one instance can
/// resolve the whole catalog.
#[derive(Default)]
pub struct WorkspaceRefs {
    tags: Option<Vec<Tag>>,
}

impl WorkspaceRefs {
    /// `rule`'s steps with named statuses and tags replaced by ids. What
    /// happens to a status or tag the workspace doesn't have depends on
    /// `purpose`.
    pub fn resolve_actions(
        &mut self,
        conn: &mut DbConnection,
        rule: &StarterRule,
        locale: &str,
        purpose: Purpose,
    ) -> Result<Value, ResolveError> {
        let Some(actions) = rule.actions.as_array() else {
            return Ok(rule.actions.clone());
        };
        // Statuses first, so a starter refused for a missing one has
        // created no tags.
        let mut statuses = Vec::with_capacity(actions.len());
        for action in actions {
            let status = match action.get("kind").and_then(Value::as_str) {
                Some("set_status") => action
                    .get("config")
                    .and_then(status_category)
                    .map(|category| self.status_config(conn, category, purpose))
                    .transpose()?,
                _ => None,
            };
            statuses.push(status);
        }
        let mut resolved = Vec::with_capacity(actions.len());
        for (action, status) in actions.iter().zip(statuses) {
            let kind = action.get("kind").and_then(Value::as_str);
            let config = action.get("config").unwrap_or(&Value::Null);
            let config = match (kind, status) {
                (_, Some(status)) => status,
                (Some("add_tags"), None) => match tag_names(config, locale) {
                    Some(names) => self.tags_config(conn, names, purpose)?,
                    None => config.clone(),
                },
                (Some("set_status"), None) => config.clone(),
                _ => {
                    resolved.push(action.clone());
                    continue;
                }
            };
            let mut action = action.clone();
            action["config"] = config;
            resolved.push(action);
        }
        Ok(Value::Array(resolved))
    }

    fn status_config(
        &mut self,
        conn: &mut DbConnection,
        category: WorkflowStateCategory,
        purpose: Purpose,
    ) -> Result<Value, ResolveError> {
        match workflow_states::first_in_category(conn, category) {
            Ok(state) => Ok(json!({ "workflow_state_id": state.id })),
            Err(diesel::result::Error::NotFound) => match purpose {
                Purpose::Browse => Ok(json!({})),
                Purpose::Add => Err(ResolveError::NoStatusIn(category)),
            },
            Err(e) => Err(e.into()),
        }
    }

    fn tags_config(
        &mut self,
        conn: &mut DbConnection,
        names: Vec<String>,
        purpose: Purpose,
    ) -> diesel::QueryResult<Value> {
        if self.tags.is_none() {
            self.tags = Some(tags::list_tags(conn, true)?);
        }
        let known = self.tags.get_or_insert_with(Vec::new);
        let mut ids: Vec<i32> = Vec::new();
        for name in &names {
            let id = match find_tag(known, name) {
                Some(id) => Some(id),
                None if purpose == Purpose::Add => {
                    let tag = tags::create_tag(
                        conn,
                        NewTag {
                            name: name.clone(),
                            color: None,
                            description: None,
                        },
                    )?;
                    let id = tag.id;
                    known.push(tag);
                    Some(id)
                }
                None => None,
            };
            if let Some(id) = id.filter(|id| !ids.contains(id)) {
                ids.push(id);
            }
        }
        Ok(match purpose {
            Purpose::Browse => json!({ "tag_ids": ids, "tag_names": names }),
            Purpose::Add => json!({ "tag_ids": ids }),
        })
    }
}

/// The tag called `name`, ignoring case. A tag in use wins over an
/// archived one, and an exact match over one that differs in case. An
/// archived tag still counts: its name is taken, so it can't be created
/// again.
fn find_tag(tags: &[Tag], name: &str) -> Option<i32> {
    let lower = name.to_lowercase();
    tags.iter()
        .filter(|t| t.name.to_lowercase() == lower)
        .min_by_key(|t| (t.archived_at.is_some(), t.name != name))
        .map(|t| t.id)
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

/// Find a catalog entry by id. Used by
/// `POST /api/admin/rule-starters/{id}`, which adds it as a draft.
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
            for (field, text) in [
                ("name_en", &rule.name_en),
                ("name_fr", &rule.name_fr),
                ("name_nl", &rule.name_nl),
                ("description_en", &rule.description_en),
                ("description_fr", &rule.description_fr),
                ("description_nl", &rule.description_nl),
            ] {
                assert!(!text.trim().is_empty(), "{field} missing on {}", rule.id);
            }
        }
    }

    /// The catalog can't hold workspace ids, so a status or tag step must
    /// name what it needs in a form `WorkspaceRefs` resolves: a known
    /// category, or tag names in every locale.
    #[test]
    fn every_named_status_and_tag_is_well_formed() {
        for rule in list() {
            let actions = rule.actions.as_array().expect("actions array");
            assert!(!actions.is_empty(), "{} has no steps", rule.id);
            for action in actions {
                let config = action.get("config").unwrap_or(&Value::Null);
                match action.get("kind").and_then(Value::as_str) {
                    Some("set_status") => {
                        assert!(
                            status_category(config).is_some(),
                            "{}: set_status needs a known category, got {config}",
                            rule.id
                        );
                    }
                    Some("add_tags" | "remove_tags") => {
                        let by_locale = config
                            .get("tag_names")
                            .unwrap_or_else(|| panic!("{}: tag step needs tag_names", rule.id));
                        for locale in ["en", "fr", "nl"] {
                            let names = by_locale[locale].as_array().unwrap_or_else(|| {
                                panic!("{}: tag_names.{locale} missing", rule.id)
                            });
                            assert!(!names.is_empty(), "{}: tag_names.{locale} empty", rule.id);
                            assert!(
                                names
                                    .iter()
                                    .all(|n| n.as_str().is_some_and(|n| !n.trim().is_empty())),
                                "{}: tag_names.{locale} has a blank name",
                                rule.id
                            );
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    #[test]
    fn tag_names_follow_the_locale() {
        let config = serde_json::json!({
            "tag_names": { "en": ["escalated"], "fr": ["escaladé"], "nl": ["geëscaleerd"] }
        });
        assert_eq!(
            tag_names(&config, "fr-FR"),
            Some(vec!["escaladé".to_string()])
        );
        assert_eq!(
            tag_names(&config, "nl"),
            Some(vec!["geëscaleerd".to_string()])
        );
        assert_eq!(
            tag_names(&config, "en-AU"),
            Some(vec!["escalated".to_string()])
        );
        assert_eq!(
            tag_names(&config, "de-DE"),
            Some(vec!["escalated".to_string()])
        );
        assert_eq!(
            tag_names(&serde_json::json!({ "tag_ids": [1] }), "en"),
            None
        );
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
