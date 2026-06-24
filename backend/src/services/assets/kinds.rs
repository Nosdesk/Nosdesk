//! Asset-kind helpers over the shared custom-field schema validator.
//!
//! The validator (a constrained JSON-Schema subset) now lives in
//! `services::custom_fields::schema` and is shared with users. This module
//! re-exports it so existing `kinds::validate_schema` / `kinds::validate_attributes`
//! callers keep working, and adds the asset-only `count_invalid_assets_for_kind`
//! pre-flight used when an admin changes a kind's schema.

use diesel::prelude::*;
use diesel::result::Error as DieselError;
use serde_json::{json, Value};

use crate::db::DbConnection;

pub use crate::services::custom_fields::schema::{
    validate_attributes, validate_schema, AttributeError, AttributeSchemaError,
};

/// Count existing assets of `kind_slug` whose `attributes` would
/// no longer validate against `new_schema`, returning the total
/// count plus a sample of up to `sample_limit` failures with the
/// asset id, display name, and the validation error string.
///
/// The handler uses this as a pre-flight before applying a kind
/// schema change so admins see what would break before they
/// commit the edit (and can opt in via `?force=true` to apply
/// anyway and surface the failed rows in the admin UI).
///
/// The asset scan is fully in-process; for the asset volumes
/// this project targets (single-tenant, thousands of rows per
/// kind), running validate_attributes per row is well under a
/// second. A streaming row iterator would be a future
/// optimisation if a kind ever holds 100k+ rows.
pub fn count_invalid_assets_for_kind(
    conn: &mut DbConnection,
    kind_slug: &str,
    new_schema: &Value,
    sample_limit: usize,
) -> Result<(usize, Vec<Value>), DieselError> {
    use crate::schema::assets;
    let rows: Vec<(i32, String, Value)> = assets::table
        .filter(assets::kind.eq(kind_slug))
        .select((assets::id, assets::name, assets::attributes))
        .load(conn)?;

    let mut invalid_count = 0usize;
    let mut samples: Vec<Value> = Vec::with_capacity(sample_limit.min(8));
    for (id, name, attrs) in rows {
        if let Err(err) = validate_attributes(new_schema, &attrs) {
            invalid_count += 1;
            if samples.len() < sample_limit {
                samples.push(json!({
                    "id": id,
                    "name": name,
                    "error": err.to_string(),
                }));
            }
        }
    }
    Ok((invalid_count, samples))
}
