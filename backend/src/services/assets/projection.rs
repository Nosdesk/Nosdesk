//! What of an asset reaches someone who isn't staff. A kind marks an
//! attribute `staffOnly` (a purchase cost on a workspace that shows
//! requesters its assets); every place an asset leaves the server for a
//! viewer who isn't staff passes it through [`for_viewer`]: the REST routes,
//! the sync snapshot, delta and live stream, and the export.
//!
//! The rule is an allowlist: a viewer who isn't staff gets only the keys
//! their asset's kind declares and doesn't mark staff only. A value the kind
//! no longer declares (a field removed with its values left in place), an
//! asset whose kind is gone, and kinds that couldn't be read all give
//! nothing.
//!
//! The kinds are read per request or batch rather than cached: they are a
//! handful of rows, and a read can't go stale on another server after a kind
//! is saved.

use std::collections::{HashMap, HashSet};

use diesel::QueryResult;
use serde_json::Value;

use crate::db::DbConnection;
use crate::models::{Asset, AssetKind};
use crate::repository::asset_kinds as kind_repo;

/// The attribute keys a viewer gets of each kind's assets.
#[derive(Debug, Clone)]
pub enum VisibleAttributes {
    /// Every attribute: staff.
    Everything,
    /// None: the kinds couldn't be read.
    Nothing,
    /// Per kind slug, the keys it declares and doesn't mark staff only,
    /// beside every key some kind marks staff only.
    Declared {
        by_kind: HashMap<String, HashSet<String>>,
        staff_only: HashSet<String>,
    },
}

impl VisibleAttributes {
    /// Every attribute: for staff.
    pub fn everything() -> Self {
        Self::Everything
    }

    /// No attribute, for when the kinds couldn't be read.
    pub fn nothing() -> Self {
        Self::Nothing
    }

    /// From `(kind slug, attribute schema)` pairs.
    pub fn from_schemas<'a>(kinds: impl IntoIterator<Item = (&'a str, &'a Value)>) -> Self {
        let mut by_kind = HashMap::new();
        let mut staff_only = HashSet::new();
        for (slug, schema) in kinds {
            let mut shown = HashSet::new();
            if let Some(props) = schema.get("properties").and_then(Value::as_object) {
                for (key, prop) in props {
                    if is_staff_only(prop) {
                        staff_only.insert(key.clone());
                    } else {
                        shown.insert(key.clone());
                    }
                }
            }
            by_kind.insert(slug.to_string(), shown);
        }
        Self::Declared {
            by_kind,
            staff_only,
        }
    }

    /// From a workspace's kinds.
    pub fn from_kinds(kinds: &[AssetKind]) -> Self {
        Self::from_schemas(kinds.iter().map(|k| (k.slug.as_str(), &k.attribute_schema)))
    }

    /// The pinned workspace's kinds.
    pub fn load(conn: &mut DbConnection) -> QueryResult<Self> {
        Ok(Self::from_kinds(&kind_repo::list_kinds(conn)?))
    }

    /// What a viewer gets: every attribute for staff, the declared ones that
    /// aren't staff only for anyone else, none when the kinds can't be read.
    pub fn for_viewer(conn: &mut DbConnection, is_staff: bool) -> Self {
        if is_staff {
            return Self::everything();
        }
        Self::load(conn).unwrap_or_else(|e| {
            tracing::error!(error = %e, "asset kinds unreadable; keeping every attribute back");
            Self::nothing()
        })
    }

    /// Whether a viewer gets `key` of an asset of `kind`.
    pub fn shows(&self, kind: &str, key: &str) -> bool {
        match self {
            Self::Everything => true,
            Self::Nothing => false,
            Self::Declared { by_kind, .. } => by_kind.get(kind).is_some_and(|k| k.contains(key)),
        }
    }

    /// Whether this viewer may order or filter assets by `key`: only when
    /// every kind declares it and none keeps it for staff. Otherwise an order
    /// or a filter would say what a value they don't get holds (a staff-only
    /// one, one a kind no longer declares).
    pub fn may_order_by(&self, key: &str) -> bool {
        match self {
            Self::Everything => true,
            Self::Nothing => false,
            Self::Declared {
                by_kind,
                staff_only,
            } => {
                !staff_only.contains(key)
                    && !by_kind.is_empty()
                    && by_kind.values().all(|shown| shown.contains(key))
            }
        }
    }

    /// `attributes` of an asset of `kind` with only what the viewer gets.
    pub fn strip(&self, kind: &str, attributes: &mut Value) {
        let Some(map) = attributes.as_object_mut() else {
            return;
        };
        match self {
            Self::Everything => {}
            Self::Nothing => map.clear(),
            Self::Declared { by_kind, .. } => match by_kind.get(kind) {
                Some(shown) => map.retain(|key, _| shown.contains(key)),
                None => map.clear(),
            },
        }
    }
}

fn is_staff_only(prop: &Value) -> bool {
    prop.get("staffOnly").and_then(Value::as_bool) == Some(true)
}

/// The properties of a schema marked `staffOnly`.
pub fn staff_only_properties(schema: &Value) -> HashSet<String> {
    schema
        .get("properties")
        .and_then(Value::as_object)
        .map(|props| {
            props
                .iter()
                .filter(|(_, prop)| is_staff_only(prop))
                .map(|(key, _)| key.clone())
                .collect()
        })
        .unwrap_or_default()
}

/// An asset as JSON (a REST response or a sync row, which carry `kind` and
/// `attributes`) with only the attributes `visible` gives.
pub fn for_viewer(asset: &mut Value, visible: &VisibleAttributes) {
    let kind = asset
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    if let Some(attributes) = asset.get_mut("attributes") {
        visible.strip(&kind, attributes);
    }
}

/// An asset row with only the attributes `visible` gives.
pub fn row_for_viewer(asset: &mut Asset, visible: &VisibleAttributes) {
    visible.strip(&asset.kind, &mut asset.attributes);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn visible() -> VisibleAttributes {
        let laptop = json!({
            "type": "object",
            "properties": {
                "cost": { "type": "object", "format": "money", "staffOnly": true },
                "colour": { "type": "string" },
                "po": { "type": "string", "staffOnly": false }
            }
        });
        let van = json!({ "type": "object", "properties": { "rego": { "type": "string" } } });
        VisibleAttributes::from_schemas([("laptop", &laptop), ("van", &van)])
    }

    #[test]
    fn a_viewer_gets_only_what_the_kind_declares_and_shows() {
        let mut asset = json!({
            "kind": "laptop",
            "attributes": {
                "cost": { "amount": "10.00", "currency": "AUD" },
                "colour": "red",
                "po": "P1",
                "removed_field": "left behind"
            }
        });
        for_viewer(&mut asset, &visible());
        assert_eq!(asset["attributes"], json!({ "colour": "red", "po": "P1" }));

        // Another kind's key of the same name is its own.
        let mut van = json!({ "kind": "van", "attributes": { "cost": "x", "rego": "ABC" } });
        for_viewer(&mut van, &visible());
        assert_eq!(van["attributes"], json!({ "rego": "ABC" }));
        assert!(!visible().may_order_by("cost"));
        // Only one kind declares it.
        assert!(!visible().may_order_by("rego"));
        let both = json!({ "type": "object", "properties": { "rego": { "type": "string" } } });
        let declared = VisibleAttributes::from_schemas([("van", &both), ("ute", &both)]);
        assert!(declared.may_order_by("rego"));
        assert!(VisibleAttributes::everything().may_order_by("anything"));
        assert!(visible().shows("laptop", "colour"));
        assert!(!visible().shows("laptop", "cost"));
    }

    #[test]
    fn an_unknown_kind_or_no_kinds_give_nothing() {
        let mut gone = json!({ "kind": "deleted", "attributes": { "colour": "red" } });
        for_viewer(&mut gone, &visible());
        assert_eq!(gone["attributes"], json!({}));

        let none_read = VisibleAttributes::from_schemas([]);
        let mut asset = json!({ "kind": "laptop", "attributes": { "colour": "red" } });
        for_viewer(&mut asset, &none_read);
        assert_eq!(asset["attributes"], json!({}));
    }

    #[test]
    fn staff_get_everything_and_a_failed_read_gives_nothing() {
        let attributes = json!({ "cost": 1, "colour": "red", "stray": true });
        let mut asset = json!({ "kind": "deleted", "attributes": attributes });
        for_viewer(&mut asset, &VisibleAttributes::everything());
        assert_eq!(asset["attributes"], attributes);
        for_viewer(&mut asset, &VisibleAttributes::nothing());
        assert_eq!(asset["attributes"], json!({}));
        assert!(!VisibleAttributes::nothing().may_order_by("colour"));
    }
}
