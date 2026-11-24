//! What of an asset reaches someone who isn't staff. A kind marks an
//! attribute `staffOnly` (a purchase cost on a workspace that shows
//! requesters its assets); every place an asset leaves the server for a
//! viewer who isn't staff passes it through [`for_viewer`]: the REST routes,
//! the sync snapshot, delta and live stream, and the export.
//!
//! The keys are read from the kinds per request or batch rather than cached:
//! the kinds are a handful of rows, and a read can't go stale on another
//! server after a kind is saved.

use std::collections::{HashMap, HashSet};

use diesel::QueryResult;
use serde_json::Value;

use crate::db::DbConnection;
use crate::models::Asset;
use crate::repository::asset_kinds as kind_repo;

/// The attribute keys each kind keeps from viewers who aren't staff.
#[derive(Debug, Clone, Default)]
pub struct StaffOnlyKeys {
    by_kind: HashMap<String, HashSet<String>>,
    /// The kinds couldn't be read: every attribute is kept back.
    all: bool,
}

impl StaffOnlyKeys {
    /// Nothing kept back: for staff.
    pub fn none() -> Self {
        Self::default()
    }

    /// Every attribute kept back, for when the kinds couldn't be read.
    pub fn fail_closed() -> Self {
        Self {
            by_kind: HashMap::new(),
            all: true,
        }
    }

    /// From `(kind slug, attribute schema)` pairs.
    pub fn from_schemas<'a>(kinds: impl IntoIterator<Item = (&'a str, &'a Value)>) -> Self {
        let by_kind = kinds
            .into_iter()
            .filter_map(|(slug, schema)| {
                let keys = staff_only_properties(schema);
                (!keys.is_empty()).then(|| (slug.to_string(), keys))
            })
            .collect();
        Self {
            by_kind,
            all: false,
        }
    }

    /// The pinned workspace's kinds.
    pub fn load(conn: &mut DbConnection) -> QueryResult<Self> {
        let kinds = kind_repo::list_kinds(conn)?;
        Ok(Self::from_schemas(
            kinds.iter().map(|k| (k.slug.as_str(), &k.attribute_schema)),
        ))
    }

    /// What a viewer keeps back: nothing for staff, the workspace's
    /// staff-only keys for anyone else, every attribute when they can't be
    /// read.
    pub fn for_viewer(conn: &mut DbConnection, is_staff: bool) -> Self {
        if is_staff {
            return Self::none();
        }
        Self::load(conn).unwrap_or_else(|e| {
            tracing::error!(error = %e, "staff-only attributes unreadable; keeping every attribute back");
            Self::fail_closed()
        })
    }

    /// Whether `key` is staff only on any kind.
    pub fn any_kind_has(&self, key: &str) -> bool {
        self.all || self.by_kind.values().any(|keys| keys.contains(key))
    }

    /// `attributes` of an asset of `kind` without the keys kept back.
    pub fn strip(&self, kind: &str, attributes: &mut Value) {
        let Some(map) = attributes.as_object_mut() else {
            return;
        };
        if self.all {
            map.clear();
            return;
        }
        if let Some(keys) = self.by_kind.get(kind) {
            map.retain(|key, _| !keys.contains(key));
        }
    }
}

/// The properties of a schema marked `staffOnly`.
pub fn staff_only_properties(schema: &Value) -> HashSet<String> {
    schema
        .get("properties")
        .and_then(Value::as_object)
        .map(|props| {
            props
                .iter()
                .filter(|(_, prop)| prop.get("staffOnly").and_then(Value::as_bool) == Some(true))
                .map(|(key, _)| key.clone())
                .collect()
        })
        .unwrap_or_default()
}

/// An asset as JSON (a REST response or a sync row, which carry `kind` and
/// `attributes`) without what `keys` keeps back.
pub fn for_viewer(asset: &mut Value, keys: &StaffOnlyKeys) {
    let kind = asset
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    if let Some(attributes) = asset.get_mut("attributes") {
        keys.strip(&kind, attributes);
    }
}

/// An asset row without what `keys` keeps back.
pub fn row_for_viewer(asset: &mut Asset, keys: &StaffOnlyKeys) {
    keys.strip(&asset.kind, &mut asset.attributes);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn keys() -> StaffOnlyKeys {
        let laptop = json!({
            "type": "object",
            "properties": {
                "cost": { "type": "object", "format": "money", "staffOnly": true },
                "colour": { "type": "string" },
                "po": { "type": "string", "staffOnly": false }
            }
        });
        let van = json!({ "type": "object", "properties": { "rego": { "type": "string" } } });
        StaffOnlyKeys::from_schemas([("laptop", &laptop), ("van", &van)])
    }

    #[test]
    fn strips_a_kinds_staff_only_keys_and_nothing_else() {
        let mut asset = json!({
            "kind": "laptop",
            "attributes": { "cost": { "amount": "10.00", "currency": "AUD" }, "colour": "red", "po": "P1" }
        });
        for_viewer(&mut asset, &keys());
        assert_eq!(asset["attributes"], json!({ "colour": "red", "po": "P1" }));

        // Another kind's key of the same name is its own.
        let mut van = json!({ "kind": "van", "attributes": { "cost": "x", "rego": "ABC" } });
        for_viewer(&mut van, &keys());
        assert_eq!(van["attributes"], json!({ "cost": "x", "rego": "ABC" }));
        assert!(keys().any_kind_has("cost"));
        assert!(!keys().any_kind_has("rego"));
    }

    #[test]
    fn staff_keep_everything_and_a_failed_read_keeps_back_everything() {
        let attributes = json!({ "cost": 1, "colour": "red" });
        let mut asset = json!({ "kind": "laptop", "attributes": attributes });
        for_viewer(&mut asset, &StaffOnlyKeys::none());
        assert_eq!(asset["attributes"], attributes);
        for_viewer(&mut asset, &StaffOnlyKeys::fail_closed());
        assert_eq!(asset["attributes"], json!({}));
    }
}
