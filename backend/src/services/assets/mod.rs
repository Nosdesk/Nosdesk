//! Asset domain services. Today only the JSON-Schema-subset
//! validator that gates per-kind attribute writes lives here;
//! future asset-level concerns (consumable usage tally,
//! quantity guards) belong alongside.

pub mod kinds;

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
