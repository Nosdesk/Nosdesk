//! Stamping a catalog model onto an asset: the model's manufacturer and
//! name as text, its kind, its default attributes where the asset has no
//! value of its own, and the link. Copy at assignment, so a later edit to
//! the model never rewrites the asset. `POST /assets/{id}/model` and the
//! asset importer both stamp through here.

use serde_json::Value;

use crate::models::{AssetModel, AssetUpdate};

/// What stamping `model` onto an asset writes.
#[derive(Debug, Clone, PartialEq)]
pub struct Stamp {
    pub manufacturer: String,
    pub model: String,
    pub kind: String,
    /// The asset's attributes with the model's defaults filled in.
    pub attributes: Value,
    pub model_id: i32,
}

/// Stamp `model`, made by `manufacturer`, onto an asset holding
/// `attributes`.
pub fn stamp(attributes: &Value, model: &AssetModel, manufacturer: &str) -> Stamp {
    Stamp {
        manufacturer: manufacturer.to_string(),
        model: model.name.clone(),
        kind: model.kind.clone(),
        attributes: stamp_default_attributes(attributes, &model.default_attributes),
        model_id: model.id,
    }
}

impl Stamp {
    /// The update that writes the stamp and nothing else.
    pub fn into_update(self) -> AssetUpdate {
        AssetUpdate {
            manufacturer: Some(self.manufacturer),
            model: Some(self.model),
            kind: Some(self.kind),
            attributes: Some(self.attributes),
            model_id: Some(Some(self.model_id)),
            updated_at: Some(chrono::Utc::now().naive_utc()),
            ..Default::default()
        }
    }
}

/// Merge a model's `default_attributes` onto an asset's attributes
/// without clobbering values the asset already carries (per-unit data
/// wins over a model default). A key counts as empty when it is absent,
/// null, or an empty string.
pub fn stamp_default_attributes(existing: &Value, defaults: &Value) -> Value {
    let mut merged = existing.as_object().cloned().unwrap_or_default();
    if let Some(obj) = defaults.as_object() {
        for (key, value) in obj {
            let occupied = merged
                .get(key)
                .map(|v| !v.is_null() && v.as_str() != Some(""))
                .unwrap_or(false);
            if !occupied {
                merged.insert(key.clone(), value.clone());
            }
        }
    }
    Value::Object(merged)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn stamp_fills_empty_keys_without_clobbering() {
        // Asset already has a hostname and a blank warranty; the model
        // defaults a warranty and an OS.
        let existing = json!({
            "hostname": "LAPTOP-1",
            "warranty_status": "",
            "os_version": null,
        });
        let defaults = json!({
            "hostname": "MODEL-DEFAULT",   // occupied -> keep the asset's
            "warranty_status": "Active",   // empty string -> fill
            "os_version": "14.0",          // null -> fill
            "operating_system": "macOS",   // absent -> fill
        });

        let merged = stamp_default_attributes(&existing, &defaults);

        assert_eq!(merged["hostname"], "LAPTOP-1");
        assert_eq!(merged["warranty_status"], "Active");
        assert_eq!(merged["os_version"], "14.0");
        assert_eq!(merged["operating_system"], "macOS");
    }

    #[test]
    fn the_stamp_carries_the_model_and_its_maker() {
        let model = AssetModel {
            id: 7,
            manufacturer_id: 3,
            name: "FX6".into(),
            kind: "camera".into(),
            part_number: None,
            default_attributes: json!({ "mount": "E" }),
            notes: None,
            workspace_id: 1,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            created_by: None,
        };
        let stamped = stamp(&json!({ "mount": "PL", "lens": "24mm" }), &model, "Sony");
        assert_eq!(stamped.manufacturer, "Sony");
        assert_eq!(stamped.model, "FX6");
        assert_eq!(stamped.kind, "camera");
        assert_eq!(stamped.attributes, json!({ "mount": "PL", "lens": "24mm" }));
        let update = stamped.into_update();
        assert_eq!(update.model_id, Some(Some(7)));
        assert_eq!(update.kind.as_deref(), Some("camera"));
    }
}
