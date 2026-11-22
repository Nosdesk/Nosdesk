//! The built-in asset record types, as a catalogue a workspace adds from.
//!
//! Nothing is seeded: an admin picks entries when setting up record types,
//! or someone picks one the first time they add a record, and
//! [`crate::repository::asset_record_types::add_from_catalogue`] writes it to
//! the workspace. An added entry keeps its slug, is built-in, and has no
//! label, field titles or outcome labels of its own, so clients show the
//! catalogue names (`asset-record-type-<slug>`,
//! `asset-record-field-<slug>-<key>`, `asset-record-outcome-<slug>-<key>`) in
//! the reader's language until someone renames them.

use serde_json::{json, Map, Value};

use crate::models::{AssetStatus, CoreFieldMode, OutcomeCategory};

/// A field of a catalogue type: its key and JSON Schema property.
pub struct CatalogueField {
    pub key: &'static str,
    pub schema: fn() -> Value,
}

/// One catalogue entry.
pub struct CatalogueType {
    pub slug: &'static str,
    pub icon: &'static str,
    pub sort_order: i32,
    /// The status whose transition opens a record of this type.
    pub opens_with_status: Option<AssetStatus>,
    /// The type's own fields, in display order.
    pub fields: &'static [CatalogueField],
    /// Shared fields the type doesn't leave optional.
    pub core_fields: &'static [(&'static str, CoreFieldMode)],
    pub outcomes: &'static [(&'static str, OutcomeCategory)],
}

impl CatalogueType {
    /// The type's `field_schema`.
    pub fn field_schema(&self) -> Value {
        let properties: Map<String, Value> = self
            .fields
            .iter()
            .map(|f| (f.key.to_string(), (f.schema)()))
            .collect();
        json!({ "type": "object", "properties": properties })
    }

    /// The type's `field_order`.
    pub fn field_order(&self) -> Vec<String> {
        self.fields.iter().map(|f| f.key.to_string()).collect()
    }

    /// The type's `core_fields`.
    pub fn core_fields(&self) -> Value {
        let map: Map<String, Value> = self
            .core_fields
            .iter()
            .map(|(field, mode)| (field.to_string(), json!(mode)))
            .collect();
        Value::Object(map)
    }

    /// The type's `outcomes`, without labels.
    pub fn outcomes(&self) -> Value {
        Value::Array(
            self.outcomes
                .iter()
                .map(|(key, category)| json!({ "key": key, "category": category }))
                .collect(),
        )
    }
}

fn text() -> Value {
    json!({ "type": "string" })
}

fn flag() -> Value {
    json!({ "type": "boolean" })
}

fn date() -> Value {
    json!({ "type": "string", "format": "date" })
}

fn text_list() -> Value {
    json!({ "type": "array", "items": { "type": "string" } })
}

use CoreFieldMode::Hidden;
use OutcomeCategory::{Done, NoFault, NotDone, Replaced};

/// The catalogue, in the order it is offered.
pub static CATALOGUE: &[CatalogueType] = &[
    CatalogueType {
        slug: "repair",
        icon: "settings",
        sort_order: 10,
        opens_with_status: Some(AssetStatus::InRepair),
        // `offsite` and `expected_return` are the keys an `in_repair`
        // transition's metadata carries.
        fields: &[
            CatalogueField {
                key: "fault",
                schema: text,
            },
            CatalogueField {
                key: "offsite",
                schema: flag,
            },
            CatalogueField {
                key: "expected_return",
                schema: date,
            },
            CatalogueField {
                key: "serial_received",
                schema: text,
            },
        ],
        core_fields: &[],
        outcomes: &[
            ("repaired", Done),
            ("replaced", Replaced),
            ("beyond_repair", NotDone),
            ("no_fault_found", NoFault),
        ],
    },
    CatalogueType {
        slug: "inspection",
        icon: "check",
        sort_order: 20,
        opens_with_status: None,
        fields: &[
            CatalogueField {
                key: "inspector_certificate",
                schema: text,
            },
            CatalogueField {
                key: "next_due",
                schema: date,
            },
        ],
        core_fields: &[],
        outcomes: &[
            ("passed", Done),
            ("passed_with_notes", Done),
            ("failed", NotDone),
        ],
    },
    CatalogueType {
        slug: "preventative_maintenance",
        icon: "calendar",
        sort_order: 30,
        opens_with_status: None,
        fields: &[CatalogueField {
            key: "tasks_done",
            schema: text_list,
        }],
        core_fields: &[],
        outcomes: &[("completed", Done), ("partially_completed", NotDone)],
    },
    CatalogueType {
        slug: "firmware_update",
        icon: "upload",
        sort_order: 40,
        opens_with_status: None,
        fields: &[
            CatalogueField {
                key: "from_version",
                schema: text,
            },
            CatalogueField {
                key: "to_version",
                schema: text,
            },
        ],
        core_fields: &[("cost", Hidden)],
        outcomes: &[
            ("updated", Done),
            ("failed", NotDone),
            ("rolled_back", NotDone),
        ],
    },
];

/// The catalogue entry with slug `slug`.
pub fn entry(slug: &str) -> Option<&'static CatalogueType> {
    CATALOGUE.iter().find(|t| t.slug == slug)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::custom_fields::schema::validate_schema;

    #[test]
    fn every_entry_is_a_valid_type() {
        for entry in CATALOGUE {
            validate_schema(&entry.field_schema()).expect(entry.slug);
            assert_eq!(
                entry.field_order().len(),
                entry.fields.len(),
                "{}: field keys are unique",
                entry.slug
            );
            let outcomes: std::collections::HashSet<_> =
                entry.outcomes.iter().map(|(k, _)| *k).collect();
            assert_eq!(outcomes.len(), entry.outcomes.len(), "{}", entry.slug);
            for (field, _) in entry.core_fields {
                assert!(crate::models::CORE_FIELDS.contains(field), "{field}");
            }
        }
    }

    /// Every catalogue name a client renders has an en-US string.
    #[test]
    fn every_name_is_in_the_catalogue_strings() {
        let en = include_str!("../../../../i18n/locales/en-US/main.ftl");
        let has = |key: String| {
            assert!(
                en.lines().any(|l| l.starts_with(&format!("{key} ="))),
                "{key} is missing from en-US"
            );
        };
        for entry in CATALOGUE {
            has(format!("asset-record-type-{}", entry.slug));
            for field in entry.fields {
                has(format!("asset-record-field-{}-{}", entry.slug, field.key));
            }
            for (outcome, _) in entry.outcomes {
                has(format!("asset-record-outcome-{}-{}", entry.slug, outcome));
            }
        }
    }
}
