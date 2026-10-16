//! The workspace's asset catalog as an asset import matches it:
//! manufacturers, models, native asset groups and location spellings.
//!
//! Names match trimmed, ignoring case and runs of spaces
//! ([`values::normalize_value`]). The catalog's own uniqueness is case
//! sensitive, so two entries can differ by case alone; a value naming both
//! is ambiguous rather than linked to either. Directory groups (synced from
//! Microsoft) are never linked: they are listed so Match values can say why.

use std::collections::HashMap;

use crate::db::DbConnection;
use crate::models::{AssetGroup, AssetModel, Manufacturer};
use crate::repository::{
    asset_groups as group_repo, asset_models as model_repo, assets as asset_repo,
    groups as directory_repo, manufacturers as manufacturer_repo,
};

use super::values::{normalize_value, Choice, Lookup, LookupContext, LookupKind};

/// Why a directory group can't be picked for an asset.
pub const DIRECTORY_GROUP_NOTE: &str = "Directory groups are set by directory sync";

pub struct Catalog {
    pub manufacturers: Vec<Manufacturer>,
    pub models: Vec<AssetModel>,
    /// Groups that aren't archived.
    pub groups: Vec<AssetGroup>,
    /// Directory group names.
    pub directory_groups: Vec<String>,
    /// Each location spelling assets hold, by its normalized form.
    locations: HashMap<String, Vec<String>>,
}

impl Catalog {
    pub fn load(conn: &mut DbConnection) -> diesel::QueryResult<Self> {
        let mut locations: HashMap<String, Vec<String>> = HashMap::new();
        for spelling in asset_repo::distinct_locations(conn)? {
            locations
                .entry(normalize_value(&spelling))
                .or_default()
                .push(spelling);
        }
        Ok(Self {
            manufacturers: manufacturer_repo::list(conn)?,
            models: model_repo::list(conn)?,
            groups: group_repo::active_groups(conn)?,
            directory_groups: directory_repo::group_names(conn)?,
            locations,
        })
    }

    pub fn manufacturer(&self, id: i32) -> Option<&Manufacturer> {
        self.manufacturers.iter().find(|m| m.id == id)
    }

    pub fn model(&self, id: i32) -> Option<&AssetModel> {
        self.models.iter().find(|m| m.id == id)
    }

    pub fn group(&self, id: i32) -> Option<&AssetGroup> {
        self.groups.iter().find(|g| g.id == id)
    }

    /// The name of a model's manufacturer.
    pub fn maker_of(&self, model: &AssetModel) -> &str {
        self.manufacturer(model.manufacturer_id)
            .map_or("", |m| m.name.as_str())
    }

    /// Manufacturers named `key` (a normalized value).
    pub fn manufacturers_named(&self, key: &str) -> Vec<&Manufacturer> {
        self.manufacturers
            .iter()
            .filter(|m| normalize_value(&m.name) == key)
            .collect()
    }

    /// The models a row whose manufacturer context is `context` links to:
    /// the models of the manufacturers it names, or every model when it is
    /// empty (the row has no manufacturer).
    pub fn models_in(&self, context: &str) -> Vec<&AssetModel> {
        if context.is_empty() {
            return self.models.iter().collect();
        }
        let makers: Vec<i32> = self
            .manufacturers_named(context)
            .iter()
            .map(|m| m.id)
            .collect();
        self.models
            .iter()
            .filter(|m| makers.contains(&m.manufacturer_id))
            .collect()
    }

    /// Models in `context` named `key`.
    pub fn models_named(&self, context: &str, key: &str) -> Vec<&AssetModel> {
        self.models_in(context)
            .into_iter()
            .filter(|m| normalize_value(&m.name) == key)
            .collect()
    }

    /// The active group named `key`; the database keeps active names unique
    /// ignoring case.
    pub fn group_named(&self, key: &str) -> Option<&AssetGroup> {
        self.groups.iter().find(|g| normalize_value(&g.name) == key)
    }

    /// The existing spelling a location differs from only by case or
    /// spacing, when exactly one does. `None` when the location is spelled
    /// as stored, is new, or could be one of several spellings.
    pub fn snap_location(&self, location: &str) -> Option<&str> {
        let spellings = self.locations.get(&normalize_value(location))?;
        match spellings.as_slice() {
            [only] if only != location => Some(only.as_str()),
            _ => None,
        }
    }

    /// The entries a lookup field links to in each of `contexts`.
    pub fn lookups(&self, kind: LookupKind, contexts: &[String]) -> Vec<LookupContext> {
        match kind {
            LookupKind::Manufacturer => vec![LookupContext {
                key: String::new(),
                label: None,
                lookup: Lookup {
                    kind,
                    choices: self
                        .manufacturers
                        .iter()
                        .map(|m| Choice::new(m.id.to_string(), m.name.clone()))
                        .collect(),
                },
            }],
            LookupKind::Group => {
                let mut choices: Vec<Choice> = self
                    .groups
                    .iter()
                    .map(|g| Choice::new(g.id.to_string(), g.name.clone()))
                    .collect();
                for name in &self.directory_groups {
                    let mut choice = Choice::new(format!("directory:{name}"), name.clone());
                    choice.disabled = Some(DIRECTORY_GROUP_NOTE.to_string());
                    choices.push(choice);
                }
                vec![LookupContext {
                    key: String::new(),
                    label: None,
                    lookup: Lookup { kind, choices },
                }]
            }
            LookupKind::Model => contexts
                .iter()
                .map(|context| {
                    let choices = self
                        .models_in(context)
                        .into_iter()
                        .map(|m| {
                            let mut choice = Choice::new(m.id.to_string(), m.name.clone());
                            choice.detail = Some(self.maker_of(m).to_string());
                            choice
                        })
                        .collect();
                    let label = (!context.is_empty()).then(|| {
                        self.manufacturers_named(context)
                            .first()
                            .map_or_else(|| context.clone(), |m| m.name.clone())
                    });
                    LookupContext {
                        key: context.clone(),
                        label,
                        lookup: Lookup { kind, choices },
                    }
                })
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog(locations: &[&str]) -> Catalog {
        let mut by_key: HashMap<String, Vec<String>> = HashMap::new();
        for l in locations {
            by_key
                .entry(normalize_value(l))
                .or_default()
                .push(l.to_string());
        }
        Catalog {
            manufacturers: Vec::new(),
            models: Vec::new(),
            groups: Vec::new(),
            directory_groups: Vec::new(),
            locations: by_key,
        }
    }

    #[test]
    fn locations_snap_to_the_one_spelling_they_differ_from_by_case_or_spacing() {
        let c = catalog(&["Studio A", "Field kit 2", "OB van", "ob VAN"]);
        assert_eq!(c.snap_location("studio a"), Some("Studio A"));
        assert_eq!(c.snap_location("Studio  A"), Some("Studio A"));
        assert_eq!(c.snap_location("Studio A"), None);
        assert_eq!(c.snap_location("Studio B"), None);
        // Two spellings already: the import can't say which.
        assert_eq!(c.snap_location("Ob Van"), None);
    }
}
