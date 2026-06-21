//! Asset domain services. Today the JSON-Schema-subset
//! validator that gates per-kind attribute writes lives here,
//! plus typed accessors for IT-flavoured attribute keys that
//! moved into the `attributes` JSONB blob in Pass B. Future
//! asset-level concerns (consumable usage tally, quantity
//! guards) belong alongside.

pub mod kinds;

use chrono::NaiveDate;
use diesel::result::Error as DieselError;
use serde_json::Value;

use crate::db::DbConnection;
use crate::repository::asset_kinds as repo;

/// Attribute keys owned by the Microsoft Graph (Intune / Entra) sync.
/// Mirrors the frontend `SYNC_OWNED_ATTRIBUTE_KEYS`. These are written by
/// the sync on a synced asset, never typed by a human, so: a model's
/// default specs must never set them, and a synced asset's manual edit
/// can never change them.
pub const SYNC_OWNED_ATTRIBUTE_KEYS: &[&str] = &[
    "hostname",
    "is_managed",
    "os_version",
    "operating_system",
    "last_sync_time",
    "enrollment_date",
    "entra_device_id",
    "compliance_state",
    "intune_device_id",
    "microsoft_device_id",
];

/// Return a copy of `attributes` with every sync-owned key removed. Used
/// to keep sync-owned keys out of a model's `default_attributes`.
pub fn strip_sync_owned_keys(attributes: &Value) -> Value {
    let mut obj = attributes.as_object().cloned().unwrap_or_default();
    for key in SYNC_OWNED_ATTRIBUTE_KEYS {
        obj.remove(*key);
    }
    Value::Object(obj)
}

#[cfg(test)]
mod sync_owned_tests {
    use super::strip_sync_owned_keys;
    use serde_json::json;

    #[test]
    fn strip_removes_sync_keys_keeps_user_keys() {
        let attrs = json!({
            "intune_device_id": "abc",
            "hostname": "PC-1",
            "warranty_status": "Active",
            "cpu": "M3",
        });
        let stripped = strip_sync_owned_keys(&attrs);
        assert!(stripped.get("intune_device_id").is_none());
        assert!(stripped.get("hostname").is_none());
        assert_eq!(stripped["warranty_status"], "Active");
        assert_eq!(stripped["cpu"], "M3");
    }
}

/// Errors surfaced by `validate_for_kind`.
#[derive(Debug, thiserror::Error)]
pub enum AssetValidationError {
    #[error("unknown asset kind `{0}`")]
    UnknownKind(String),
    #[error("database error: {0}")]
    Database(#[from] DieselError),
    #[error("invalid attributes: {0}")]
    Attributes(#[from] kinds::AttributeError),
}

/// Look up the kind by slug and validate `attributes` against
/// the kind's stored `attribute_schema`. Run this at every
/// asset write site so the IT-desk path and any future
/// kind-specific paths share one validation chokepoint.
pub fn validate_for_kind(
    conn: &mut DbConnection,
    kind_slug: &str,
    attributes: &Value,
) -> Result<(), AssetValidationError> {
    let kind = match repo::get_kind_by_slug(conn, kind_slug) {
        Ok(k) => k,
        Err(DieselError::NotFound) => {
            return Err(AssetValidationError::UnknownKind(kind_slug.to_string()))
        }
        Err(e) => return Err(AssetValidationError::Database(e)),
    };
    kinds::validate_attributes(&kind.attribute_schema, attributes)?;
    Ok(())
}

/// Typed accessor for the IT-flavoured attribute keys that used
/// to live as top-level columns on `assets`. The migration in
/// 2026-05-20-150000 backfilled them into the JSONB blob; this
/// helper centralises the read shape so handlers don't reach
/// into `Value` indexing directly.
pub mod it_attrs {
    use super::*;

    pub fn str<'a>(attrs: &'a Value, key: &str) -> Option<&'a str> {
        attrs.get(key).and_then(Value::as_str)
    }

    pub fn bool(attrs: &Value, key: &str) -> Option<bool> {
        attrs.get(key).and_then(Value::as_bool)
    }

    /// Date format matches `format: date` from the schema subset
    /// validator (YYYY-MM-DD). None on parse failure rather than
    /// surface noise; the caller decides whether to log.
    pub fn date(attrs: &Value, key: &str) -> Option<NaiveDate> {
        str(attrs, key).and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
    }

    pub fn hostname(attrs: &Value) -> Option<&str> {
        str(attrs, "hostname")
    }
    pub fn operating_system(attrs: &Value) -> Option<&str> {
        str(attrs, "operating_system")
    }
    pub fn os_version(attrs: &Value) -> Option<&str> {
        str(attrs, "os_version")
    }
    pub fn warranty_status(attrs: &Value) -> Option<&str> {
        str(attrs, "warranty_status")
    }
    pub fn warranty_end_date(attrs: &Value) -> Option<NaiveDate> {
        date(attrs, "warranty_end_date")
    }
    pub fn compliance_state(attrs: &Value) -> Option<&str> {
        str(attrs, "compliance_state")
    }
}
