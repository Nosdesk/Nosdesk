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
